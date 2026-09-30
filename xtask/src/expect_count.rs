//! Check 18, the `#[expect]` count (s2w#156): every `#[expect]` attribute in every workspace
//! package's `*.rs` files (tests, benches and `build.rs` included), counted per lint and held
//! shrink-only against `xtask/expect-baseline.toml`.
//!
//! Source is read as `syn` ASTs, so an `expect(` inside a string or a doc comment is not an
//! attribute. One blind spot follows from that: `syn` does not parse the bodies of
//! `macro_rules!`, so an `#[expect]` written inside a macro definition is not counted. None
//! exists today. The human cross-check is `git grep -nE '#!?\[expect\(' -- '*.rs'`, whose total
//! must equal this check's summary line while every attribute names one lint. Do not replace the
//! AST walk with that text search.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, Meta, Token};

use crate::module_size::{git, trailer};

const BASELINE: &str = "xtask/expect-baseline.toml";
const HEADER: &str = "# Shrink-only (s2w#156): cargo xtask check fails if any count rises above or falls below this.
# Lower it with `cargo xtask check --tighten-baseline`; raising it needs a Baseline-growth: s2w#<N> trailer.
";

/// Every counted lint with the `path:line` of each attribute naming it.
type Counts = BTreeMap<String, Vec<String>>;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Baseline {
    #[serde(default)]
    lints: BTreeMap<String, usize>,
}

/// Runs check 18; `tighten` lowers the baseline to the actual counts when nothing else fails.
pub(super) fn check(root: &Path, meta: &super::Metadata, tighten: bool) -> Vec<String> {
    let mut problems = Vec::new();
    let path = root.join(BASELINE);
    let baseline = match super::read_toml::<Baseline>(&path) {
        Ok(b) => Some(b),
        Err(e) => {
            problems.extend(e);
            None
        }
    };
    let counts = scan(root, meta, &mut problems);
    let Some(baseline) = baseline else {
        return problems;
    };
    // Check growth BEFORE tightening so the repair command cannot conceal a raised baseline.
    problems.extend(baseline_growth(root, &baseline));
    let (over, under) = judge(&baseline.lints, &counts);
    if !tighten {
        problems.extend(over);
        problems.extend(under);
        return problems;
    }
    if !over.is_empty() || !problems.is_empty() {
        problems.extend(over);
        problems.push(format!("cannot tighten {BASELINE} over unreadable files, unauthorized baseline growth or counts above the baseline; resolve them and retry"));
        return problems;
    }
    let tightened = tighten_to(&baseline.lints, &actuals(&counts));
    if tightened == baseline.lints {
        return problems;
    }
    if let Err(e) = fs::write(&path, render(&tightened)) {
        problems.push(format!(
            "cannot tighten {}: {e}; check the file is writable and retry",
            path.display()
        ));
    }
    problems
}

/// Counts every package's `*.rs` files and prints the summary line.
fn scan(root: &Path, meta: &super::Metadata, problems: &mut Vec<String>) -> Counts {
    let dirs: BTreeSet<PathBuf> = meta
        .packages
        .iter()
        .map(|p| super::crate_dir(&p.manifest_path))
        .collect();
    let mut files = BTreeSet::new();
    for dir in &dirs {
        if let Err(e) = rust_files(dir, &mut files) {
            problems.push(e);
        }
    }
    let counts = count_files(root, &files, problems);
    let total: usize = counts.values().map(Vec::len).sum();
    let per_lint: Vec<String> = counts
        .iter()
        .map(|(l, s)| format!("{l} {}", s.len()))
        .collect();
    println!("expects: {total} ({})", per_lint.join(", "));
    counts
}

/// Rule 3 against `origin/main`'s baseline; a base that cannot be read (the file is new, or git
/// fails) counts every entry as growth.
fn baseline_growth(root: &Path, baseline: &Baseline) -> Option<String> {
    let base = git(root, &["show", &format!("origin/main:{BASELINE}")])
        .and_then(|text| toml::from_str::<Baseline>(&text).map_err(|e| e.to_string()))
        .map_err(|e| {
            println!("[expect baseline] no base on origin/main ({e}); every entry counts as growth")
        })
        .unwrap_or_default();
    let grew = grown(&base.lints, &baseline.lints);
    if grew.is_empty() {
        return None;
    }
    growth_finding(
        &grew,
        git(root, &["log", "origin/main..HEAD", "--pretty=%B"]),
    )
}

/// Collects every `*.rs` file under `dir`, skipping `target`, `node_modules` and hidden
/// directories.
fn rust_files(dir: &Path, out: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let kind = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_dir() {
            if name != "target" && name != "node_modules" && !name.starts_with('.') {
                rust_files(&path, out)?;
            }
        } else if kind.is_file() && name.ends_with(".rs") {
            out.insert(path);
        }
    }
    Ok(())
}

/// Counts every file; an unreadable or unparsable file is a problem naming it.
fn count_files(root: &Path, files: &BTreeSet<PathBuf>, problems: &mut Vec<String>) -> Counts {
    let mut counts = Counts::new();
    for file in files {
        let shown = file
            .strip_prefix(root)
            .unwrap_or(file)
            .display()
            .to_string();
        let found = fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|src| count_source(&src));
        match found {
            Ok(found) => {
                for (lint, line) in found {
                    counts.entry(lint).or_default().push(format!("{shown}:{line}"));
                }
            }
            Err(e) => problems.push(format!(
                "{shown}: cannot count its #[expect] attributes: {e}; a measurement that cannot be read is a failure, so fix the file"
            )),
        }
    }
    counts
}

/// Every lint named by an `#[expect]` (or a `cfg_attr(.., expect(..))`) in `src`, with its line.
fn count_source(src: &str) -> Result<Vec<(String, usize)>, String> {
    let file = syn::parse_file(src).map_err(|e| e.to_string())?;
    let mut visitor = Expects::default();
    visitor.visit_file(&file);
    match visitor.error {
        Some(e) => Err(e),
        None => Ok(visitor.found),
    }
}

#[derive(Default)]
struct Expects {
    found: Vec<(String, usize)>,
    error: Option<String>,
}

impl Expects {
    fn meta(&mut self, meta: &Meta, line: usize) {
        let Meta::List(list) = meta else { return };
        let nested = || list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated);
        if list.path.is_ident("expect") {
            match nested() {
                Ok(lints) => self.found.extend(
                    lints
                        .iter()
                        .filter(|m| !m.path().is_ident("reason"))
                        .map(|m| (path_text(m.path()), line)),
                ),
                Err(e) => {
                    self.error
                        .get_or_insert(format!("line {line}: unreadable #[expect]: {e}"));
                }
            }
        } else if list.path.is_ident("cfg_attr") {
            match nested() {
                // The first entry is the predicate; the rest are attributes.
                Ok(attrs) => attrs.iter().skip(1).for_each(|m| self.meta(m, line)),
                Err(e) => {
                    self.error
                        .get_or_insert(format!("line {line}: unreadable #[cfg_attr]: {e}"));
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Expects {
    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        self.meta(&attr.meta, attr.span().start().line);
    }
}

fn path_text(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn actuals(counts: &Counts) -> BTreeMap<String, usize> {
    counts.iter().map(|(l, s)| (l.clone(), s.len())).collect()
}

/// Rules 1 and 2: counts above their baseline (a lint absent from the baseline has 0), then
/// counts below it.
fn judge(baseline: &BTreeMap<String, usize>, counts: &Counts) -> (Vec<String>, Vec<String>) {
    let mut over = Vec::new();
    for (lint, sites) in counts {
        let base = baseline.get(lint).copied().unwrap_or(0);
        if sites.len() > base {
            over.push(format!(
                "{} #[expect] naming {lint} vs baseline {base}: fix the lint at {}, or raise the baseline in {BASELINE} with a Baseline-growth: s2w#<N> trailer",
                sites.len(),
                sites.join(", ")
            ));
        }
    }
    let mut under = Vec::new();
    for (lint, &base) in baseline {
        let now = counts.get(lint).map_or(0, Vec::len);
        if now < base {
            under.push(format!(
                "{lint} is at {now}, baseline {base}: run cargo xtask check --tighten-baseline and commit {BASELINE}"
            ));
        }
    }
    (over, under)
}

/// Rule 3's input: lints whose baseline is new or higher than on the base.
fn grown(base: &BTreeMap<String, usize>, current: &BTreeMap<String, usize>) -> Vec<String> {
    current
        .iter()
        .filter(|(lint, n)| base.get(*lint).is_none_or(|b| b < *n))
        .map(|(lint, _)| lint.clone())
        .collect()
}

/// Rule 3: baseline growth needs a `Baseline-growth: s2w#<N>` trailer in `origin/main..HEAD`.
fn growth_finding(grew: &[String], messages: Result<String, String>) -> Option<String> {
    match messages {
        Ok(m) if trailer(&m) => None,
        result => Some(format!(
            "expect baseline grew ({}): add a Baseline-growth: s2w#<N> trailer to a commit in origin/main..HEAD, or undo the growth in {BASELINE}{}",
            grew.join(", "),
            result
                .err()
                .map(|e| format!("; cannot read commit range: {e}"))
                .unwrap_or_default()
        )),
    }
}

/// The tightened baseline: each entry lowered to its actual count, never raised; entries whose
/// count reached 0 are dropped.
fn tighten_to(
    baseline: &BTreeMap<String, usize>,
    actual: &BTreeMap<String, usize>,
) -> BTreeMap<String, usize> {
    baseline
        .iter()
        .map(|(lint, &b)| (lint.clone(), b.min(actual.get(lint).copied().unwrap_or(0))))
        .filter(|(_, n)| *n > 0)
        .collect()
}

/// The baseline file's text: the fixed header, then one sorted line per lint.
fn render(lints: &BTreeMap<String, usize>) -> String {
    let rows: String = lints
        .iter()
        .map(|(lint, n)| format!("\"{lint}\" = {n}\n"))
        .collect();
    format!("{HEADER}[lints]\n{rows}")
}

#[cfg(test)]
mod tests;
