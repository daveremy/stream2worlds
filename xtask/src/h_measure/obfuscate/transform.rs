//! One frame in, one frame out: every object key renamed through the replicate's field table,
//! every value transformed by its path's rule (contract B2.2). With no rule, a string is hashed
//! as text unless it is an RFC 3339 date-time, which is shifted (decision 0030); numbers,
//! booleans, nulls and the event's structure are kept.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::canon;
use super::hash::Keyed;
use super::rules::{Rule, Rules};

/// The domain undeclared strings, and URL values of an unexpected shape, are hashed in.
pub(super) const TEXT: &str = "text";

/// The domain the SSE `id:` cursor is hashed in, whole.
const CURSOR: &str = "cursor";

/// Field path to output name (`f1…fN`), one table per replicate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FieldTable(pub BTreeMap<Vec<String>, String>);

impl FieldTable {
    /// The output name of `path`. A path the table does not hold is refused: a later window of a
    /// replicate may never give a field a name of its own.
    pub(super) fn name(&self, path: &[String]) -> Result<&str, String> {
        self.0.get(path).map(String::as_str).ok_or_else(|| {
            format!(
                "field path {path:?} is not in the replicate's field table; a window transformed with --meta reuse may not add a field. Re-run every window of the replicate in one invocation"
            )
        })
    }
}

/// What the run did per path, for the metadata file.
#[derive(Debug, Default)]
pub(super) struct Stats {
    /// Every leaf path and how its values were transformed.
    pub treatments: BTreeMap<Vec<String>, BTreeSet<String>>,
    /// Paths holding a number no rule declared: kept unchanged, listed for the author to re-check.
    pub undeclared_numbers: BTreeSet<Vec<String>>,
    /// Values hashed whole as text because their URL rule did not match their shape, per path.
    pub fallbacks: BTreeMap<Vec<String>, usize>,
}

/// The transformer for one replicate.
pub(super) struct Transformer<'r> {
    rules: &'r Rules,
    keyed: Keyed,
    table: FieldTable,
    shift: i64,
    pub stats: Stats,
}

/// Every object-key path in `value` under `chain` (array indexes skipped), into `out`.
pub(super) fn collect_paths(
    value: &Value,
    chain: &mut Vec<String>,
    out: &mut BTreeSet<Vec<String>>,
) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                chain.push(key.clone());
                out.insert(chain.clone());
                collect_paths(inner, chain, out);
                chain.pop();
            }
        }
        Value::Array(items) => {
            for inner in items {
                collect_paths(inner, chain, out);
            }
        }
        _ => {}
    }
}

fn lookup<'v>(record: &'v Value, path: &[String]) -> Option<&'v Value> {
    path.iter()
        .try_fold(record, |node, key| node.as_object()?.get(key))
}

/// A scalar's text: a string as is, a number in its JSON form. Booleans and nulls have none.
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

impl<'r> Transformer<'r> {
    pub(super) fn new(rules: &'r Rules, keyed: Keyed, table: FieldTable) -> Self {
        let shift = keyed.shift();
        Self {
            rules,
            keyed,
            table,
            shift,
            stats: Stats::default(),
        }
    }

    pub(super) fn shift(&self) -> i64 {
        self.shift
    }

    pub(super) fn table(&self) -> &FieldTable {
        &self.table
    }

    pub(super) fn fingerprint(&self) -> String {
        self.keyed.fingerprint()
    }

    /// One SSE frame: the `id:` cursor hashed whole, the `data:` JSON transformed.
    pub(super) fn frame(&mut self, id: &str, data: &str) -> Result<String, String> {
        let event: Value =
            serde_json::from_str(data).map_err(|e| format!("a data: line is not JSON: {e}"))?;
        let out = self.walk(&event, &mut Vec::new(), &event)?;
        let cursor = self.keyed.value(CURSOR, &[id])?;
        let text = serde_json::to_string(&out).map_err(|e| e.to_string())?;
        Ok(format!("id: {cursor}\ndata: {text}\n\n"))
    }

    fn walk(
        &mut self,
        value: &Value,
        chain: &mut Vec<String>,
        record: &Value,
    ) -> Result<Value, String> {
        match value {
            Value::Object(map) => {
                if self.rules.by_path.contains_key(chain.as_slice()) {
                    return Err(format!(
                        "the rule for {chain:?} meets an object; rules apply to scalar values"
                    ));
                }
                let mut out = Map::new();
                for (key, inner) in map {
                    chain.push(key.clone());
                    let name = self.table.name(chain)?.to_owned();
                    let renamed = self.walk(inner, chain, record)?;
                    chain.pop();
                    out.insert(name, renamed);
                }
                Ok(Value::Object(out))
            }
            Value::Array(items) => items
                .iter()
                .map(|inner| self.walk(inner, chain, record))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            scalar => self.leaf(chain, scalar, Some(record)),
        }
    }

    /// One scalar at `chain`. `record` is the plain event it sits in, for rules that read
    /// another path; `None` (a key's sentinel value) refuses such a rule.
    pub(super) fn leaf(
        &mut self,
        chain: &[String],
        value: &Value,
        record: Option<&Value>,
    ) -> Result<Value, String> {
        let rules = self.rules;
        let Some(rule) = rules.by_path.get(chain) else {
            return self.undeclared(chain, value);
        };
        if rule.unix_seconds {
            self.note(chain, "unix-seconds shifted");
            return self.unix(chain, value);
        }
        if !rule.url_query.is_empty() {
            return self.query(chain, rule, value, record);
        }
        self.hashed(chain, rule, value, record)
    }

    fn note(&mut self, chain: &[String], how: &str) {
        self.stats
            .treatments
            .entry(chain.to_vec())
            .or_default()
            .insert(how.to_owned());
    }

    fn undeclared(&mut self, chain: &[String], value: &Value) -> Result<Value, String> {
        match value {
            Value::String(text) if s2w_discover::stamp::shaped(text) => {
                self.note(chain, "date-time shifted");
                s2w_discover::stamp::shift(text, self.shift)
                    .map(Value::String)
                    .ok_or_else(|| format!(
                        "a date-time at {chain:?} cannot be shifted (a leap second, or a year outside 0000-9999); a date-time is shifted, never hashed (decision 0030)"
                    ))
            }
            Value::String(text) => {
                self.note(chain, &format!("hashed in {TEXT}"));
                self.keyed.value(TEXT, &[text]).map(Value::String)
            }
            Value::Number(_) => {
                self.note(chain, "number kept (undeclared)");
                self.stats.undeclared_numbers.insert(chain.to_vec());
                Ok(value.clone())
            }
            _ => {
                self.note(chain, "kept");
                Ok(value.clone())
            }
        }
    }

    fn unix(&self, chain: &[String], value: &Value) -> Result<Value, String> {
        if value.is_null() {
            return Ok(Value::Null);
        }
        value
            .as_i64()
            .and_then(|seconds| seconds.checked_add(self.shift))
            .map(Value::from)
            .ok_or_else(|| format!("{chain:?} is unix_seconds but holds {value}, not an integer that can be shifted"))
    }

    fn fallback(&mut self, chain: &[String], text: &str) -> Result<Value, String> {
        *self.stats.fallbacks.entry(chain.to_vec()).or_default() += 1;
        self.note(chain, &format!("hashed in {TEXT} (URL shape not matched)"));
        self.keyed.value(TEXT, &[text]).map(Value::String)
    }

    fn hashed(
        &mut self,
        chain: &[String],
        rule: &Rule,
        value: &Value,
        record: Option<&Value>,
    ) -> Result<Value, String> {
        let Some(own) = scalar_text(value) else {
            self.note(chain, "kept");
            return Ok(value.clone());
        };
        let domain = rule.domain.as_deref().unwrap_or(TEXT);
        let mut parts = folded(chain, &rule.fold, record)?;
        let canonical = if let Some(from) = &rule.from {
            Some(context(chain, from, record)?)
        } else if let Some(url) = &rule.url_path {
            let base = context(chain, &url.base, record).ok();
            base.and_then(|base| canon::url_tail(&own, &base, &url.marker, &url.replace))
        } else {
            Some(own.clone())
        };
        let Some(canonical) = canonical else {
            return self.fallback(chain, &own);
        };
        parts.push(canonical);
        self.note(chain, &format!("hashed in {domain}"));
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        self.keyed.value(domain, &refs).map(Value::String)
    }

    fn query(
        &mut self,
        chain: &[String],
        rule: &Rule,
        value: &Value,
        record: Option<&Value>,
    ) -> Result<Value, String> {
        let url = match value {
            Value::String(url) => url,
            Value::Null => return Ok(Value::Null),
            other => return Err(format!("{chain:?} has a url_query rule but holds {other}")),
        };
        let params = canon::query(url).unwrap_or_default();
        let mut hashes = Vec::new();
        for wanted in &rule.url_query {
            let Some((_, found)) = params.iter().find(|(name, _)| *name == wanted.param) else {
                continue;
            };
            let mut parts = folded(chain, &wanted.fold, record)?;
            parts.push(found.clone());
            let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
            hashes.push(self.keyed.value(&wanted.domain, &refs)?);
        }
        if hashes.is_empty() {
            return self.fallback(chain, url);
        }
        self.note(chain, "url-query hashed");
        Ok(Value::String(hashes.join("/")))
    }
}

/// The text at each fold path of `record`, in order.
fn folded(
    chain: &[String],
    fold: &[Vec<String>],
    record: Option<&Value>,
) -> Result<Vec<String>, String> {
    fold.iter().map(|path| context(chain, path, record)).collect()
}

/// The scalar text at `path` of `record`; an error when there is no record or no scalar there.
fn context(chain: &[String], path: &[String], record: Option<&Value>) -> Result<String, String> {
    let record = record.ok_or_else(|| {
        format!(
            "the rule for {chain:?} reads {path:?} from its record, and a key's sentinel value has no record; give that mention rule no no_identity, or drop the context from the rule"
        )
    })?;
    lookup(record, path).and_then(scalar_text).ok_or_else(|| {
        format!(
            "the rule for {chain:?} reads {path:?}, which holds no string or number in a record that has {chain:?}; every such record needs it"
        )
    })
}
