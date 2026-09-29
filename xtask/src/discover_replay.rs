//! Check 12, profiler obfuscation replay: `s2w-discover` proposes the same mapping for a
//! recorded raw stream whether or not every object key and string value in it is renamed and
//! hashed first, up to that renaming (decision 0022).
//!
//! The fixture is SSE text; each event's `data:` and `id:` lines become the stored envelope
//! `{"data":…,"id":…}` (the bytes need not match the adapter's, only the shape). Pass A profiles
//! the envelopes. Check 11's maps are built from them with a decode-only mapping (pass A's
//! decode steps), so keys inside the decoded strings are renamed and their string leaves hashed.
//! Pass B profiles the renamed envelopes. The expected mapping is pass A's with every path
//! renamed and its rule ids, type labels and attribute names re-derived from the renamed paths
//! by the crate's own `rule_id` and `type_labels`; both sides are sorted the same way, and must
//! be equal, as must the event-type field.
//!
//! Non-vacuity: pass A must propose a mapping with two or more types, a relationship, an
//! attribute and an alias class of two or more paths; pass B's payloads and mapping must carry
//! no original string leaf and no original key.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use s2w_discover::{Config, Discovery, Profile, discover, rule_id, type_labels};
use s2w_model::{FieldPath, MAPPING_VERSION, StreamMapping};
use serde_json::Value;

use crate::obfuscation_raw::Maps;

/// The recorded stream, through a neutral-named link so no check reads a domain name.
pub(crate) const FIXTURE: &str = "crates/s2w-discover/testdata/recorded.raw.sse";

pub(crate) type Profiler = fn(&[&[u8]]) -> (Profile, Discovery);

/// What the replay runs; the self-tests swap in broken parts to watch the check fire.
pub(crate) struct Harness {
    pub(crate) profiler: Profiler,
    /// Whether the renaming reaches inside decoded strings.
    pub(crate) descend: bool,
    pub(crate) mutate_expected: fn(&mut StreamMapping),
}

impl Harness {
    pub(crate) const REAL: Self = Self {
        profiler: real_profiler,
        descend: true,
        mutate_expected: keep,
    };
}

fn real_profiler(payloads: &[&[u8]]) -> (Profile, Discovery) {
    discover(payloads, &Config::default())
}

fn keep(_: &mut StreamMapping) {}

pub(crate) fn check(root: &Path) -> Vec<String> {
    match fs::read_to_string(root.join(FIXTURE)) {
        Ok(text) => replay(&text, &Harness::REAL),
        Err(e) => vec![format!("{FIXTURE}: {e}")],
    }
}

pub(crate) fn replay(sse: &str, harness: &Harness) -> Vec<String> {
    replay_inner(sse, harness).unwrap_or_else(|problems| problems)
}

fn replay_inner(sse: &str, harness: &Harness) -> Result<Vec<String>, Vec<String>> {
    let payloads = envelopes(sse);
    let (profile_a, a) = profile(harness.profiler, &payloads)?;
    let a = proposed("A (plain)", a)?;
    let decode_only = StreamMapping {
        version: MAPPING_VERSION,
        decode: a.decode.clone(),
        entities: Vec::new(),
        relationships: Vec::new(),
    };
    let maps = Maps::build(&payloads, &decode_only)?;
    let hidden = payloads
        .iter()
        .map(|p| maps.payload(p, &a.decode, harness.descend))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| vec![e])?;
    let (profile_b, b) = profile(harness.profiler, &hidden)?;
    let b = proposed("B (renamed)", b)?;
    let mut expected = renamed(&maps, &a).map_err(|e| vec![e])?;
    (harness.mutate_expected)(&mut expected);
    let mut problems = non_vacuity(&a);
    problems.extend(leaks(&maps, &hidden, &b));
    let event_type = profile_a
        .event_type
        .as_ref()
        .map(|p| maps.path(p))
        .transpose();
    if event_type.map_err(|e| vec![e])? != profile_b.event_type {
        problems.push(format!(
            "profiler obfuscation replay: {FIXTURE}: the event-type field changed under renaming: {:?} became {:?}",
            profile_a.event_type, profile_b.event_type
        ));
    }
    problems.extend(compare(&canonical(expected), &canonical(b)));
    Ok(problems)
}

/// The fixture's events as stored envelopes: one per blank-line-terminated event carrying both
/// a `data:` and an `id:` field (multi-line data joined by `\n`, per the SSE format).
pub(crate) fn envelopes(sse: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let (mut data, mut id): (Option<String>, Option<String>) = (None, None);
    for line in sse.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let (Some(d), Some(i)) = (data.take(), id.take()) {
                out.push(serde_json::json!({"data": d, "id": i}));
            }
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => data = Some(data.map_or_else(|| value.to_owned(), |d| d + "\n" + value)),
            "id" => id = Some(value.to_owned()),
            _ => {}
        }
    }
    out
}

fn profile(profiler: Profiler, payloads: &[Value]) -> Result<(Profile, Discovery), Vec<String>> {
    let bytes = payloads
        .iter()
        .map(serde_json::to_vec)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| vec![e.to_string()])?;
    let refs: Vec<&[u8]> = bytes.iter().map(Vec::as_slice).collect();
    Ok(profiler(&refs))
}

fn proposed(pass: &str, discovery: Discovery) -> Result<StreamMapping, Vec<String>> {
    match discovery {
        Discovery::Mapping(m) => Ok(m),
        Discovery::Abstain(reason) => Err(vec![format!(
            "profiler obfuscation replay: {FIXTURE}: pass {pass} abstained ({reason}), so the replay proves nothing. Fix the profiler or record a fixture it can map"
        )]),
    }
}

/// Pass A's mapping as pass B must propose it: renamed paths, with ids, labels and attribute
/// names re-derived from them.
fn renamed(maps: &Maps, a: &StreamMapping) -> Result<StreamMapping, String> {
    let mut classes: BTreeMap<&str, Vec<FieldPath>> = BTreeMap::new();
    let mut ids = BTreeMap::new();
    for rule in &a.entities {
        let key = rule.key.first().ok_or("an entity rule has no key")?;
        let key = maps.path(key)?;
        ids.insert(rule.id.as_str(), rule_id(&key));
        classes.entry(&rule.type_label).or_default().push(key);
    }
    let paths: Vec<Vec<FieldPath>> = classes.values().cloned().collect();
    let labels: BTreeMap<&str, String> = classes.keys().copied().zip(type_labels(&paths)).collect();
    let id = |old: &str| {
        ids.get(old)
            .cloned()
            .ok_or(format!("unknown rule id '{old}'"))
    };
    let mut out = a.clone();
    out.decode = a
        .decode
        .iter()
        .map(|p| maps.path(p))
        .collect::<Result<_, _>>()?;
    for rule in &mut out.entities {
        rule.id = id(&rule.id)?;
        rule.type_label
            .clone_from(&labels[rule.type_label.as_str()]);
        rule.key = rule
            .key
            .iter()
            .map(|p| maps.path(p))
            .collect::<Result<_, _>>()?;
        for attr in &mut rule.attrs {
            attr.path = maps.path(&attr.path)?;
            attr.name = rule_id(&attr.path);
        }
    }
    for rel in &mut out.relationships {
        rel.from = id(&rel.from)?;
        rel.to = id(&rel.to)?;
    }
    Ok(out)
}

pub(crate) fn canonical(mut m: StreamMapping) -> StreamMapping {
    m.entities.sort_by(|x, y| x.id.cmp(&y.id));
    for rule in &mut m.entities {
        rule.attrs.sort_by(|x, y| x.name.cmp(&y.name));
    }
    m.relationships
        .sort_by(|x, y| (&x.from, &x.to, &x.kind).cmp(&(&y.from, &y.to, &y.kind)));
    m
}

fn compare(expected: &StreamMapping, got: &StreamMapping) -> Vec<String> {
    if expected == got {
        return Vec::new();
    }
    let show = |m: &StreamMapping| serde_json::to_value(m).unwrap_or(Value::Null);
    let (e, g) = (show(expected), show(got));
    let first = ["decode", "entities", "relationships"]
        .into_iter()
        .find(|k| e[k] != g[k])
        .unwrap_or("version");
    vec![format!(
        "profiler obfuscation replay: {FIXTURE}: the renamed stream got a different mapping ({} entity and {} relationship rules expected, {} and {} proposed; first difference in `{first}`). A profiler decision depends on a key name or a string value: find it and make it read statistics only (decision 0022)",
        expected.entities.len(),
        expected.relationships.len(),
        got.entities.len(),
        got.relationships.len()
    )]
}

fn non_vacuity(a: &StreamMapping) -> Vec<String> {
    let mut per_label: BTreeMap<&str, usize> = BTreeMap::new();
    for rule in &a.entities {
        *per_label.entry(&rule.type_label).or_default() += 1;
    }
    let missing = [
        (per_label.len() >= 2, "two entity types"),
        (!a.relationships.is_empty(), "a relationship"),
        (
            a.entities.iter().any(|r| !r.attrs.is_empty()),
            "an attribute",
        ),
        (
            per_label.values().any(|&n| n >= 2),
            "an alias class of two paths",
        ),
    ];
    missing
        .into_iter()
        .filter(|(ok, _)| !ok)
        .map(|(_, what)| {
            format!("profiler obfuscation replay: {FIXTURE}: pass A proposed no {what}, so the replay is vacuous there")
        })
        .collect()
}

/// Original string leaves in pass B's payloads (decoded strings included), and original keys in
/// its mapping's paths: either means the renaming missed something.
fn leaks(maps: &Maps, hidden: &[Value], b: &StreamMapping) -> Vec<String> {
    let mut leaves = BTreeSet::new();
    hidden.iter().for_each(|p| collect_leaves(p, &mut leaves));
    let mut problems: Vec<String> = leaves
        .intersection(&maps.raw_leaves)
        .take(3)
        .map(|leaf| format!("profiler obfuscation replay: {FIXTURE}: an original string leaf survived renaming: '{leaf}'"))
        .collect();
    let original = b
        .entities
        .iter()
        .flat_map(|r| &r.key)
        .find(|p| maps.path(p).is_ok());
    if let Some(path) = original {
        problems.push(format!(
            "profiler obfuscation replay: {FIXTURE}: pass B's mapping names an original key path: {path:?}"
        ));
    }
    problems
}

fn collect_leaves(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(fields) => fields.values().for_each(|v| collect_leaves(v, out)),
        Value::Array(items) => items.iter().for_each(|v| collect_leaves(v, out)),
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(inner @ (Value::Object(_) | Value::Array(_))) => collect_leaves(&inner, out),
            _ => {
                out.insert(text.clone());
            }
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests;
