//! Stage 1 of research 0002 §6: find the decode steps, then flatten every payload into one
//! column per object-key path. Arrays are not addressable by a v0 key path, so they are skipped.

use std::collections::BTreeMap;

use s2w_model::{FieldPath, Segment};
use serde_json::{Map, Value};

use crate::Config;

/// Scalar kinds seen in a column, as bits.
pub(crate) const STR: u8 = 1;
pub(crate) const INT: u8 = 2;
pub(crate) const BOOL: u8 = 4;
/// Floats, nulls and integers outside `i64`: never part of a key.
pub(crate) const OTHER: u8 = 8;

/// One path's values in stream order.
#[derive(Default)]
pub(crate) struct Column {
    /// `(event index, value id)`; the id indexes `texts`. `None` marks a non-keyable value.
    pub(crate) cells: Vec<(usize, Option<u32>)>,
    /// Canonical JSON text of each distinct keyable value.
    pub(crate) texts: Vec<String>,
    ids: BTreeMap<String, u32>,
    pub(crate) kinds: u8,
    /// Values that were strings, and their total length in bytes (the dashboard proposer's
    /// statistics; no rule reads them).
    pub(crate) strs: usize,
    pub(crate) str_bytes: usize,
}

impl Column {
    fn push(&mut self, event: usize, value: &Value) {
        if let Value::String(text) = value {
            self.strs += 1;
            self.str_bytes = self.str_bytes.saturating_add(text.len());
        }
        let kind = match value {
            Value::String(_) => STR,
            Value::Bool(_) => BOOL,
            Value::Number(n) if n.as_i64().is_some() => INT,
            _ => OTHER,
        };
        self.kinds |= kind;
        if kind == OTHER {
            self.cells.push((event, None));
            return;
        }
        let text = value.to_string();
        let next = u32::try_from(self.texts.len()).unwrap_or(u32::MAX);
        let id = *self.ids.entry(text.clone()).or_insert(next);
        if id == next {
            self.texts.push(text);
        }
        self.cells.push((event, Some(id)));
    }

    /// Whether every value is one keyable kind: a string, an `i64` or a bool.
    pub(crate) fn keyable(&self) -> bool {
        matches!(self.kinds, STR | INT | BOOL)
    }
}

/// Every parsed payload, as columns.
#[derive(Default)]
pub(crate) struct Table {
    /// Payloads that parsed as a JSON object.
    pub(crate) events: usize,
    /// Payloads that did not.
    pub(crate) skipped: usize,
    pub(crate) decode: Vec<FieldPath>,
    pub(crate) paths: Vec<FieldPath>,
    pub(crate) columns: Vec<Column>,
    /// Per event, per path index: the value id, for keyable values only. Dense, so every
    /// lookup in the dependency and alias tests is an index.
    pub(crate) rows: Vec<Vec<Option<u32>>>,
    index: BTreeMap<FieldPath, usize>,
}

impl Table {
    /// Parses `payloads`, finds decode steps, and flattens.
    pub(crate) fn build(payloads: &[&[u8]], cfg: &Config) -> Self {
        let mut objects = Vec::with_capacity(payloads.len());
        let mut table = Self::default();
        for bytes in payloads {
            match serde_json::from_slice::<Value>(bytes) {
                Ok(Value::Object(map)) => objects.push(map),
                _ => table.skipped += 1,
            }
        }
        let decode = decode_keys(&objects, cfg);
        table.decode = decode
            .iter()
            .map(|k| FieldPath(vec![Segment::Key(k.clone())]))
            .collect();
        for mut map in objects {
            for key in &decode {
                let parsed = match map.get(key) {
                    Some(Value::String(s)) => serde_json::from_str::<Value>(s).ok(),
                    _ => None,
                };
                if let Some(value @ Value::Object(_)) = parsed {
                    map.insert(key.clone(), value);
                }
            }
            let event = table.events;
            table.events += 1;
            table.rows.push(Vec::new());
            table.walk(event, &mut Vec::new(), &map);
        }
        let width = table.paths.len();
        table
            .rows
            .iter_mut()
            .for_each(|row| row.resize(width, None));
        table
    }

    fn walk(&mut self, event: usize, prefix: &mut Vec<Segment>, map: &Map<String, Value>) {
        for (key, value) in map {
            prefix.push(Segment::Key(key.clone()));
            match value {
                Value::Object(inner) => self.walk(event, prefix, inner),
                Value::Array(_) => {}
                scalar => self.record(event, FieldPath(prefix.clone()), scalar),
            }
            prefix.pop();
        }
    }

    fn record(&mut self, event: usize, path: FieldPath, value: &Value) {
        let next = self.paths.len();
        let index = *self.index.entry(path.clone()).or_insert(next);
        if index == next {
            self.paths.push(path);
            self.columns.push(Column::default());
        }
        let column = &mut self.columns[index];
        column.push(event, value);
        if let Some(&(_, Some(id))) = column.cells.last() {
            let row = &mut self.rows[event];
            if row.len() <= index {
                row.resize(index + 1, None);
            }
            row[index] = Some(id);
        }
    }

    /// The value text of `path` in `event`, if keyable and present.
    pub(crate) fn text(&self, event: usize, path: usize) -> Option<&str> {
        let id = self.rows[event][path]?;
        self.columns[path]
            .texts
            .get(id as usize)
            .map(String::as_str)
    }
}

/// Root keys whose string value holds a JSON object in at least `decode_pct` of the events
/// that carry them, and that at least `min_support` events carry. A format step, not a meaning.
fn decode_keys(objects: &[Map<String, Value>], cfg: &Config) -> Vec<String> {
    let mut seen: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for map in objects {
        for (key, value) in map {
            if let Value::String(text) = value {
                let entry = seen.entry(key).or_default();
                entry.0 += 1;
                if matches!(serde_json::from_str::<Value>(text), Ok(Value::Object(_))) {
                    entry.1 += 1;
                }
            }
        }
    }
    seen.into_iter()
        .filter(|(_, (carried, parsed))| {
            *carried >= cfg.min_support && pct(*parsed, *carried) >= cfg.decode_pct
        })
        .map(|(key, _)| key.to_owned())
        .collect()
}

/// `part / whole` in whole percent, rounded down; 0 when `whole` is 0.
pub(crate) fn pct(part: usize, whole: usize) -> usize {
    (part * 100).checked_div(whole).unwrap_or(0)
}
