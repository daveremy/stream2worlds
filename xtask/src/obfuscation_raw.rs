//! Check 11, raw obfuscation replay: a data-driven engine (`s2w-system1`'s `MappingEngine`)
//! builds the same world from a recorded raw stream whether or not every field name and string
//! value in it is renamed first, given the mapping renamed the same way (decision 0021).
//!
//! Check 10 replays claims; a mapping engine reads raw payloads, so this check starts one layer
//! earlier. Two maps are built from the union of the raw fixture and the mapping fixture:
//!
//! - a **key map** (`f1`, `f2`, … in first-seen order) for every JSON object key in every raw
//!   payload, including keys inside each string the mapping decodes (its `decode` paths), plus
//!   the mapping's attribute names;
//! - a **value map** (`h` + FNV-1a/64 hex) for every string leaf in the same trees, plus the
//!   mapping's type labels and relationship kinds. A string the mapping decodes is descended
//!   into, never hashed as a leaf. Integers, booleans, floats and nulls pass through unchanged.
//!
//! Pass A runs the engine over the raw fixture. Pass B runs it over the renamed fixture with
//! the renamed mapping (key segments via the key map; array indexes and rule ids unchanged).
//! The expected world is built from pass A's claims, mapped by their typed role: types and
//! kinds via the value map, attribute names via the key map, string attribute values via the
//! value map, and each natural key read with `NaturalKey::parts` with its label and string parts
//! mapped. Decoding uses the engine's own `s2w_system1::decode`. Folding the expected claims
//! must equal folding pass B's claims (check 10's comparator). Any collision, or a mapping name a map cannot cover, fails closed.
//!
//! Non-vacuity: pass A must yield at least two entity types, one relationship, one multi-part
//! key, one integer key part and one string attribute; the expected claims must differ from
//! pass A's; and pass B must contain no original raw string leaf.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use s2w_core::{World, fold};
use s2w_model::{
    Cursor, FieldPath, RawEvent, Segment, SourceId, StreamMapping, Timestamp, WorldEvent,
    fnv1a64_hex,
};
use s2w_system1::{Engine, MappingEngine, Verdict};
use serde_json::Value;

use crate::golden::HUB_CAP;
use crate::obfuscation::compare_named;

/// The recorded raw stream: one stored payload per line.
pub(crate) const RAW: &str = "crates/s2w-system1/testdata/raw-sample.jsonl";
/// The mapping the replay runs over it.
pub(crate) const MAPPING: &str = "crates/s2w-system1/testdata/sample.mapping.json";

/// Builds the engine a pass runs.
pub(crate) type EngineFactory = fn(StreamMapping) -> Result<Box<dyn Engine>, String>;

/// The replay's moving parts. Production runs [`Harness::REAL`]; the self-tests swap one part
/// at a time to prove the check can fail.
pub(crate) struct Harness {
    /// Builds the engine for both passes.
    pub(crate) engine: EngineFactory,
    /// Whether the obfuscator descends into decoded strings.
    pub(crate) descend: bool,
    /// Edits pass B's renamed mapping before it runs.
    pub(crate) mutate_b: fn(&mut StreamMapping),
}

impl Harness {
    pub(crate) const REAL: Self = Self {
        engine: real_engine,
        descend: true,
        mutate_b: keep,
    };
}

fn real_engine(mapping: StreamMapping) -> Result<Box<dyn Engine>, String> {
    let engine = MappingEngine::new(mapping).map_err(|e| format!("{MAPPING}: {e}"))?;
    Ok(Box::new(engine))
}

fn keep(_: &mut StreamMapping) {}

pub(crate) fn check(root: &Path) -> Vec<String> {
    let raw = fs::read_to_string(root.join(RAW)).map_err(|e| format!("{RAW}: {e}"));
    let mapping = fs::read_to_string(root.join(MAPPING)).map_err(|e| format!("{MAPPING}: {e}"));
    match (raw, mapping) {
        (Ok(raw), Ok(mapping)) => replay(&raw, &mapping, &Harness::REAL),
        (raw, mapping) => [raw.err(), mapping.err()].into_iter().flatten().collect(),
    }
}

pub(crate) fn replay(raw_text: &str, mapping_text: &str, harness: &Harness) -> Vec<String> {
    replay_inner(raw_text, mapping_text, harness).unwrap_or_else(|problems| problems)
}

fn replay_inner(
    raw_text: &str,
    mapping_text: &str,
    harness: &Harness,
) -> Result<Vec<String>, Vec<String>> {
    let lines: Vec<&str> = raw_text.lines().filter(|l| !l.trim().is_empty()).collect();
    let payloads = parse_lines(&lines)?;
    let mapping: StreamMapping = serde_json::from_str(mapping_text)
        .map_err(|e| vec![format!("{MAPPING}: not a stream mapping: {e}")])?;
    let maps = Maps::build(&payloads, &mapping)?;
    let mut mapping_b = maps.mapping(&mapping).map_err(|e| vec![e])?;
    (harness.mutate_b)(&mut mapping_b);
    let mut bytes_b = Vec::new();
    for payload in &payloads {
        let renamed = maps
            .payload(payload, &mapping.decode, harness.descend)
            .map_err(|e| vec![e])?;
        bytes_b.push(serde_json::to_vec(&renamed).map_err(|e| vec![e.to_string()])?);
    }
    let bytes_a: Vec<Vec<u8>> = lines.iter().map(|l| l.as_bytes().to_vec()).collect();
    let claims_a = run(harness.engine, mapping, &bytes_a).map_err(|e| vec![e])?;
    let claims_b = run(harness.engine, mapping_b, &bytes_b).map_err(|e| vec![e])?;
    let expected = claims_a
        .iter()
        .map(|claim| maps.claim(claim))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| vec![e])?;
    let mut problems = non_vacuity(&claims_a, &expected);
    problems.extend(leaked_leaves(&claims_b, &maps.raw_leaves));
    let (world_expected, world_b) = (fold_json(&expected)?, fold_json(&claims_b)?);
    problems.extend(compare_named(RAW, &world_expected, &world_b));
    Ok(problems)
}

fn parse_lines(lines: &[&str]) -> Result<Vec<Value>, Vec<String>> {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            serde_json::from_str(line).map_err(|e| format!("{RAW}: line {}: not JSON: {e}", i + 1))
        })
        .collect::<Result<_, _>>()
        .map_err(|e| vec![e])
}

/// Runs one engine over payload bytes and collects every proposed claim, in order.
fn run(
    factory: EngineFactory,
    mapping: StreamMapping,
    payloads: &[Vec<u8>],
) -> Result<Vec<WorldEvent>, String> {
    let engine = factory(mapping)?;
    let source = SourceId::new("fixture").map_err(|e| e.to_string())?;
    let mut claims = Vec::new();
    for (i, payload) in (1u32..).zip(payloads) {
        let event = RawEvent {
            source: source.clone(),
            cursor: Cursor::new(i.to_be_bytes().to_vec()).map_err(|e| e.to_string())?,
            received_at: Timestamp::from_millis(0),
            payload: payload.clone(),
        };
        if let Verdict::Propose {
            claims: proposed, ..
        } = engine.evaluate(&event)
        {
            claims.extend(proposed);
        }
    }
    Ok(claims)
}

fn fold_json(claims: &[WorldEvent]) -> Result<Value, Vec<String>> {
    serde_json::to_value(fold(World::with_hub_cap(HUB_CAP), claims)).map_err(|e| {
        vec![format!(
            "raw obfuscation replay: world will not serialize: {e}"
        )]
    })
}

// ---------- the maps ----------

/// The key map, the value map, and the raw string leaves pass B must never contain.
pub(crate) struct Maps {
    keys: BTreeMap<String, String>,
    values: BTreeMap<String, String>,
    hashes: BTreeMap<String, String>,
    pub(crate) raw_leaves: BTreeSet<String>,
    problems: Vec<String>,
}

impl Maps {
    pub(crate) fn build(payloads: &[Value], mapping: &StreamMapping) -> Result<Self, Vec<String>> {
        let mut maps = Self {
            keys: BTreeMap::new(),
            values: BTreeMap::new(),
            hashes: BTreeMap::new(),
            raw_leaves: BTreeSet::new(),
            problems: Vec::new(),
        };
        for (i, payload) in payloads.iter().enumerate() {
            let (tree, _) = decode_all(payload, &mapping.decode)
                .map_err(|e| vec![format!("{RAW}: line {}: {e}", i + 1)])?;
            maps.note_tree(&tree);
        }
        maps.raw_leaves = maps.values.keys().cloned().collect();
        maps.note_mapping(mapping);
        if maps.problems.is_empty() {
            Ok(maps)
        } else {
            Err(maps.problems)
        }
    }

    fn note_tree(&mut self, value: &Value) {
        match value {
            Value::Object(fields) => {
                for (key, child) in fields {
                    self.note_key(key);
                    self.note_tree(child);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| self.note_tree(item)),
            Value::String(text) => self.note_value(text),
            _ => {}
        }
    }

    /// Mapping names: key segments must already be raw object keys (a segment that is not
    /// would never resolve, so the replay could not rename it); labels and attribute names
    /// join the maps.
    fn note_mapping(&mut self, mapping: &StreamMapping) {
        let rules = &mapping.entities;
        let paths = mapping.decode.iter().chain(rules.iter().flat_map(|rule| {
            rule.key
                .iter()
                .chain(rule.attrs.iter().map(|attr| &attr.path))
        }));
        for path in paths {
            for segment in &path.0 {
                if let Segment::Key(key) = segment
                    && !self.keys.contains_key(key)
                {
                    self.problems.push(format!(
                        "raw obfuscation replay: {MAPPING} path segment '{key}' is not an object key anywhere in {RAW}, so the replay cannot rename it. Fix the mapping path or record a fixture that has it"
                    ));
                }
            }
        }
        for rule in rules {
            self.note_value(&rule.type_label);
            rule.attrs.iter().for_each(|attr| self.note_key(&attr.name));
        }
        for rel in &mapping.relationships {
            self.note_value(&rel.kind);
        }
    }

    fn note_key(&mut self, key: &str) {
        if !self.keys.contains_key(key) {
            let renamed = format!("f{}", self.keys.len() + 1);
            self.keys.insert(key.to_owned(), renamed);
        }
    }

    fn note_value(&mut self, text: &str) {
        if self.values.contains_key(text) {
            return;
        }
        let hashed = format!("h{}", fnv1a64_hex(text.as_bytes()));
        if let Some(other) = self.hashes.get(&hashed) {
            self.problems.push(format!(
                "raw obfuscation replay: value-hash collision: '{text}' and '{other}' both hash to '{hashed}'"
            ));
            return;
        }
        self.hashes.insert(hashed.clone(), text.to_owned());
        self.values.insert(text.to_owned(), hashed);
    }

    fn key(&self, key: &str) -> Result<String, String> {
        self.keys
            .get(key)
            .cloned()
            .ok_or_else(|| format!("raw obfuscation replay: name '{key}' is not in the key map"))
    }

    fn value(&self, text: &str) -> Result<String, String> {
        self.values
            .get(text)
            .cloned()
            .ok_or_else(|| format!("raw obfuscation replay: '{text}' is not in the value map"))
    }

    /// Renames every object key and hashes every string leaf in `value`.
    fn rename(&self, value: &Value) -> Value {
        match value {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (self.keys.get(k).unwrap_or(k).clone(), self.rename(v)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(|v| self.rename(v)).collect()),
            Value::String(text) => Value::String(self.values.get(text).unwrap_or(text).clone()),
            other => other.clone(),
        }
    }

    pub(crate) fn path(&self, path: &FieldPath) -> Result<FieldPath, String> {
        path.0
            .iter()
            .map(|segment| match segment {
                Segment::Key(key) => self.key(key).map(Segment::Key),
                Segment::Index(index) => Ok(Segment::Index(*index)),
            })
            .collect::<Result<_, _>>()
            .map(FieldPath)
    }
}

mod rename;
#[cfg(test)]
mod tests;

use rename::{decode_all, leaked_leaves, non_vacuity};
