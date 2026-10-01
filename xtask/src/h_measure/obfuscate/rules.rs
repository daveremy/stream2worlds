//! The obfuscation rules file (TOML, contract B2.2): which field paths hold identifiers, each
//! path's identifier domain, how an identifier is read out of a URL, which integer paths are unix
//! times, and which relationships the transformation destroys. The rules are data, one file per
//! stream (decision 0018); this module reads any stream's rules the same way.
//!
//! A path is a list of object keys from the event's root. Array indexes are not part of a path:
//! every element of an array is at the array's path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The rules-file format this build reads.
const RULES_VERSION: u32 = 1;

/// A rules file as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    #[serde(default)]
    rule: Vec<Rule>,
    #[serde(default)]
    unobservable: Vec<Unobservable>,
}

/// One `[[rule]]`: how the values at `path` are transformed. Exactly one of `domain`,
/// `url_query` and `unix_seconds` is set.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rule {
    pub path: Vec<String>,
    /// The identifier domain: the value is hashed with the domain, so equal values in one
    /// domain stay equal and equal values in two domains do not.
    pub domain: Option<String>,
    /// Context paths whose values are hashed in before the value, in order, so one value under
    /// two contexts gets two hashes. Each must hold a scalar in every record the rule applies to.
    #[serde(default)]
    pub fold: Vec<Vec<String>>,
    /// An alias: hash the value at this path of the same record instead of the value here, so
    /// the alias becomes byte-equal to that path's hash.
    pub from: Option<Vec<String>>,
    /// The identifier is the tail of a URL: see [`UrlPath`].
    pub url_path: Option<UrlPath>,
    /// The value is a URL whose query parameters hold identifiers: each listed parameter
    /// present is hashed into its own domain, and the output is those hashes joined by `/`.
    #[serde(default)]
    pub url_query: Vec<QueryParam>,
    /// The value is an integer count of seconds since the epoch, moved by the shift.
    #[serde(default)]
    pub unix_seconds: bool,
}

/// `url_path`: when the value is the string at `base` in the same record, then `marker`, then a
/// tail, the identifier is the tail cut at `?` or `#`, percent-decoded, with each `replace`
/// pair applied. A value of any other shape is hashed whole as text (and counted).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UrlPath {
    pub base: Vec<String>,
    pub marker: String,
    #[serde(default)]
    pub replace: Vec<(String, String)>,
}

/// One `url_query` parameter.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QueryParam {
    pub param: String,
    pub domain: String,
    #[serde(default)]
    pub fold: Vec<Vec<String>>,
}

/// One `[[unobservable]]` row: a path, or a relationship (`from`, `to`, `kind`), that the
/// obfuscated stream cannot show, with the reason. Paths are added to the renamed answer key's
/// `unscored`; relationship rows are recorded in the metadata only (no key format carries
/// relationships yet).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Unobservable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub reason: String,
}

/// A validated rules file.
#[derive(Debug)]
pub(super) struct Rules {
    pub by_path: BTreeMap<Vec<String>, Rule>,
    pub unobservable: Vec<Unobservable>,
    /// Lower-case hex sha256 of the file's bytes.
    pub sha256: String,
}

impl Rules {
    /// Reads and validates a rules file.
    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let shown = path.display();
        let bytes = fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
        let text = String::from_utf8(bytes.clone()).map_err(|e| format!("{shown}: {e}"))?;
        Self::parse(&text, crate::sha256(&bytes)).map_err(|e| format!("{shown}: {e}"))
    }

    /// Parses and validates rules-file text.
    pub(super) fn parse(text: &str, sha256: String) -> Result<Self, String> {
        let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
        if file.version != RULES_VERSION {
            return Err(format!(
                "version {} is not {RULES_VERSION}, the format this build reads",
                file.version
            ));
        }
        let mut by_path = BTreeMap::new();
        for rule in file.rule {
            check_rule(&rule)?;
            let path = rule.path.clone();
            if by_path.insert(path.clone(), rule).is_some() {
                return Err(format!("two rules for path {path:?}; a path has one rule"));
            }
        }
        let mut seen = BTreeSet::new();
        for row in &file.unobservable {
            check_unobservable(row)?;
            if !seen.insert(row) {
                return Err(format!("unobservable row {row:?} is listed twice"));
            }
        }
        Ok(Self {
            by_path,
            unobservable: file.unobservable,
            sha256,
        })
    }
}

fn check_path(what: &str, path: &[String]) -> Result<(), String> {
    if path.is_empty() || path.iter().any(String::is_empty) {
        Err(format!("{what} {path:?} is empty or has an empty key"))
    } else {
        Ok(())
    }
}

fn check_rule(rule: &Rule) -> Result<(), String> {
    let path = &rule.path;
    check_path("rule path", path)?;
    let kinds = usize::from(rule.domain.is_some())
        + usize::from(!rule.url_query.is_empty())
        + usize::from(rule.unix_seconds);
    if kinds != 1 {
        return Err(format!(
            "rule {path:?} sets {kinds} of domain, url_query, unix_seconds; set exactly one"
        ));
    }
    let hashed = rule.domain.is_some();
    if !hashed && (!rule.fold.is_empty() || rule.from.is_some() || rule.url_path.is_some()) {
        return Err(format!(
            "rule {path:?}: fold, from and url_path need a domain"
        ));
    }
    if rule.from.is_some() && rule.url_path.is_some() {
        return Err(format!("rule {path:?}: set from or url_path, not both"));
    }
    if rule.domain.as_deref() == Some("") {
        return Err(format!("rule {path:?} has an empty domain"));
    }
    rule.fold.iter().try_for_each(|p| check_path("fold path", p))?;
    if let Some(from) = &rule.from {
        check_path("from path", from)?;
    }
    if let Some(url) = &rule.url_path {
        check_path("url_path base", &url.base)?;
        if url.marker.is_empty() || url.replace.iter().any(|(from, _)| from.is_empty()) {
            return Err(format!(
                "rule {path:?}: url_path needs a marker and non-empty replace patterns"
            ));
        }
    }
    let mut names = BTreeSet::new();
    for param in &rule.url_query {
        if param.param.is_empty() || param.domain.is_empty() || !names.insert(&param.param) {
            return Err(format!(
                "rule {path:?}: every url_query entry needs a distinct param and a domain"
            ));
        }
        param.fold.iter().try_for_each(|p| check_path("fold path", p))?;
    }
    Ok(())
}

fn check_unobservable(row: &Unobservable) -> Result<(), String> {
    if row.reason.trim().is_empty() {
        return Err(format!("unobservable row {row:?} needs a reason"));
    }
    let edge = [&row.from, &row.to, &row.kind];
    match (&row.path, edge.iter().filter(|part| part.is_some()).count()) {
        (Some(path), 0) => check_path("unobservable path", path),
        (None, 3) => Ok(()),
        _ => Err(format!(
            "unobservable row {row:?}: give a path, or all of from, to and kind"
        )),
    }
}
