//! Check 10, obfuscation replay: the fold does not branch on what a string means, only on its
//! shape (decision 0018 — code knows protocols and formats, never what a stream is about).
//!
//! Builds two maps from the golden fixture's own events, by structural role: a **key-rename
//! map** for attribute names (`attrs`' JSON object keys — the one place a claim names its own
//! fields) and a **value-hash map** for every opaque identifier/string value the fold treats as
//! data (`NaturalKey`s, `entity_type`, relationship `kind`, and `AttrValue::Str` contents).
//! Neither map ever holds a `WorldEvent`/`World` schema field name (`key`, `attrs`, `kind`,
//! `entities`, ...) — those never enter the maps because [`build_maps`] only walks the payload
//! positions decision 0018 calls "claim structure", never the envelope.
//!
//! Folds the fixture straight (pass A) and again after applying the maps to its events (pass
//! B), applies the SAME maps to pass A's folded output, and asserts the two folded worlds are
//! structurally identical. The maps are keyed by original string regardless of whether that
//! string later appears as a JSON object key or a JSON value — a `NaturalKey` is a value in the
//! event log (`{"key": "site-a", ...}`) but a key in the folded world (`"keys": {"site-a": 0}`),
//! which is exactly the role a naive by-JSON-position transform would get wrong.
//!
//! Scope: replays `s2w-core`'s fold and, via [`engine_replay`], `s2w-system1`'s engines that
//! read a claim's own shape (`JsonClaimsEngine`). An engine that needs a mapping to run
//! (`MappingEngine`) reads raw payloads, not claims, so it is replayed by check 11
//! (`obfuscation_raw.rs`) over a recorded raw fixture instead; a new engine is covered only
//! once one of these checks runs it. Neither check runs through the bridge registry
//! (`s2w-app::Bridge`/`EngineRegistry`) — tracked as [stream2worlds#135](https://github.com/daveremy/stream2worlds/issues/135).
//! `Route::Exact("stdin")` is the only default route today, so the registry carries
//! materially less domain-keying risk than the engine layer these checks cover.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use s2w_core::{World, WorldEvent, fold};
use s2w_model::{Cursor, RawEvent, SourceId, Timestamp, fnv1a64_hex};
use s2w_system1::{Engine, JsonClaimsEngine, Verdict};
use serde_json::Value;

use crate::golden::{HUB_CAP, LOG};

pub(crate) fn check(root: &Path) -> Vec<String> {
    match fs::read_to_string(root.join(LOG)) {
        Ok(text) => {
            let mut problems = replay(&text);
            problems.extend(engine_replay(&text));
            problems
        }
        Err(e) => vec![format!("{LOG}: {e}")],
    }
}

pub(crate) fn replay(log_text: &str) -> Vec<String> {
    let events_json: Value = match serde_json::from_str(log_text) {
        Ok(v) => v,
        Err(e) => return vec![format!("{LOG}: not JSON: {e}")],
    };
    let events: Vec<WorldEvent> = match serde_json::from_value(events_json.clone()) {
        Ok(v) => v,
        Err(e) => return vec![format!("{LOG}: not a JSON array of WorldEvents: {e}")],
    };
    replay_events(&events_json, &events, |events| {
        let world = fold(World::with_hub_cap(HUB_CAP), events);
        serde_json::to_value(world).map_err(|e| e.to_string())
    })
}

/// The same replay, through the `s2w-system1` engine layer instead of the bare `s2w-core`
/// fold: each golden event is wrapped in a hand-built [`RawEvent`] and run through every
/// [`Engine`] in [`engines`], and the proposed claims are folded exactly as [`replay`] folds
/// the golden events directly. The engine vec is iterated; an engine joins it only if it runs
/// without a mapping (a mapping engine is check 11's, `obfuscation_raw.rs`).
pub(crate) fn engine_replay(log_text: &str) -> Vec<String> {
    let events_json: Value = match serde_json::from_str(log_text) {
        Ok(v) => v,
        Err(e) => return vec![format!("{LOG}: not JSON: {e}")],
    };
    let events: Vec<WorldEvent> = match serde_json::from_value(events_json.clone()) {
        Ok(v) => v,
        Err(e) => return vec![format!("{LOG}: not a JSON array of WorldEvents: {e}")],
    };
    let engines = engines();
    replay_events(&events_json, &events, engine_fold_to_json(&engines))
}

fn engines() -> Vec<Box<dyn Engine>> {
    vec![Box::new(JsonClaimsEngine)]
}

/// Runs each golden `WorldEvent`, re-serialized as a bare-`WorldEvent` [`RawEvent`] payload,
/// through every engine, collects the proposed claims (abstentions contribute nothing), and
/// folds them exactly as [`replay`]'s own closure folds the golden events directly —
/// `JsonClaimsEngine` proposes exactly the event it is given back out for this bare-event
/// payload shape, so pass A and pass B stay structurally comparable.
fn engine_fold_to_json(
    engines: &[Box<dyn Engine>],
) -> impl Fn(&[WorldEvent]) -> Result<Value, String> + '_ {
    move |events| {
        let mut claims = Vec::new();
        for (i, event) in events.iter().enumerate() {
            let payload = serde_json::to_vec(event).map_err(|e| e.to_string())?;
            let raw = RawEvent {
                source: SourceId::new("golden").map_err(|e| e.to_string())?,
                cursor: Cursor::new(vec![u8::try_from(i).unwrap_or(u8::MAX)])
                    .map_err(|e| e.to_string())?,
                received_at: Timestamp::from_millis(0),
                payload,
            };
            for engine in engines {
                if let Verdict::Propose {
                    claims: proposed, ..
                } = engine.evaluate(&raw)
                {
                    claims.extend(proposed);
                }
            }
        }
        let world = fold(World::with_hub_cap(HUB_CAP), &claims);
        serde_json::to_value(world).map_err(|e| e.to_string())
    }
}

/// `replay`'s wiring — build the maps, obfuscate the event log, fold both, transform pass A's
/// output, compare — parameterized over the fold so a test can substitute a toy fold that reads
/// a domain field name instead of `s2w_core::fold`'s real one. Proves this function's own
/// plumbing surfaces a mismatch, not just [`compare`] called directly on hand-built values (as
/// the tests below already do): production (`replay`) always passes the real fold.
fn replay_events(
    events_json: &Value,
    events: &[WorldEvent],
    fold_to_json: impl Fn(&[WorldEvent]) -> Result<Value, String>,
) -> Vec<String> {
    let (key_map, value_map, mut problems) = build_maps(events_json);
    if !problems.is_empty() {
        return problems;
    }

    let obfuscated_json = transform(events_json, &key_map, &value_map);
    let obfuscated_events: Vec<WorldEvent> = match serde_json::from_value(obfuscated_json) {
        Ok(v) => v,
        Err(e) => {
            return vec![format!(
                "obfuscation replay: the transformed event log no longer deserializes as WorldEvents: {e}. The transform touched a schema field, not just claim data."
            )];
        }
    };

    let a_json = match fold_to_json(events) {
        Ok(v) => v,
        Err(e) => {
            return vec![format!(
                "obfuscation replay: pass A world will not serialize: {e}"
            )];
        }
    };
    let b_json = match fold_to_json(&obfuscated_events) {
        Ok(v) => v,
        Err(e) => {
            return vec![format!(
                "obfuscation replay: pass B world will not serialize: {e}"
            )];
        }
    };

    let transformed_a = transform(&a_json, &key_map, &value_map);
    problems.extend(compare(&transformed_a, &b_json));
    problems
}

// ---------- the two maps, built from the event log's own claim data ----------

/// `attrs` keys get a stable `f<n>`, assigned in the order each new one is first seen — the
/// event log's own claim-attribute names, never the fold's schema field names.
fn build_maps(
    events: &Value,
) -> (
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    Vec<String>,
) {
    let mut key_map = BTreeMap::new();
    let mut value_map = BTreeMap::new();
    let mut next = 1u32;
    let mut problems = Vec::new();

    let Value::Array(events) = events else {
        return (key_map, value_map, problems);
    };
    for event in events {
        let Value::Object(wrapper) = event else {
            continue;
        };
        for (variant, payload) in wrapper {
            let Value::Object(fields) = payload else {
                continue;
            };
            match variant.as_str() {
                "EntityObserved" => {
                    note_value(&mut value_map, &mut problems, fields, "key");
                    note_value(&mut value_map, &mut problems, fields, "entity_type");
                    if let Some(Value::Object(attrs)) = fields.get("attrs") {
                        for (attr_name, attr_value) in attrs {
                            note_key(&mut key_map, &mut problems, attr_name, &mut next);
                            if let Value::Object(tagged) = attr_value {
                                note_value(&mut value_map, &mut problems, tagged, "Str");
                            }
                        }
                    }
                }
                "RelationshipObserved" => {
                    for field in ["from", "to", "kind"] {
                        note_value(&mut value_map, &mut problems, fields, field);
                    }
                }
                "EntitiesMerged" | "MergeRevoked" => {
                    for field in ["survivor", "absorbed"] {
                        note_value(&mut value_map, &mut problems, fields, field);
                    }
                }
                _ => {}
            }
        }
    }
    (key_map, value_map, problems)
}

/// Records `fields[field]`, a string, in the value-hash map: a claim's opaque identifier or
/// text, hashed to a fixed-width hex string so pass B never sees the original.
fn note_value(
    map: &mut BTreeMap<String, String>,
    problems: &mut Vec<String>,
    fields: &serde_json::Map<String, Value>,
    field: &str,
) {
    let Some(Value::String(s)) = fields.get(field) else {
        return;
    };
    let hashed = format!("h{}", fnv1a64_hex(s.as_bytes()));
    match map.get(s) {
        Some(existing) if existing != &hashed => {
            // Unreachable in practice (FNV-1a/64 over this fixture's few dozen strings), but a
            // real collision must fail closed rather than silently merge two identities.
            problems.push(format!(
                "obfuscation replay: value-hash collision — '{s}' already maps to '{existing}', now computed '{hashed}'"
            ));
        }
        _ => {
            map.insert(s.clone(), hashed);
        }
    }
}

/// Records an `attrs` key in the key-rename map, `f1`, `f2`, ... in first-seen order.
fn note_key(
    map: &mut BTreeMap<String, String>,
    problems: &mut Vec<String>,
    name: &str,
    next: &mut u32,
) {
    if map.contains_key(name) {
        return;
    }
    let renamed = format!("f{next}");
    *next += 1;
    if map.values().any(|v| v == &renamed) {
        problems.push(format!(
            "obfuscation replay: key-rename collision on '{renamed}' — should be unreachable, the counter only grows"
        ));
        return;
    }
    map.insert(name.to_owned(), renamed);
}

// ---------- the transform: role-driven, not JSON-position-driven ----------

/// Renames every object key found in `key_map`, then (whichever map matched or not) hashes every
/// string found in `value_map` — by membership in the maps, not by whether the JSON position is
/// a key or a value, so the same map applies unchanged to the event log and the folded world.
/// A schema field name (`"key"`, `"kind"`, `"entities"`, ...) is in neither map, so it always
/// passes through untouched.
fn transform(
    v: &Value,
    key_map: &BTreeMap<String, String>,
    value_map: &BTreeMap<String, String>,
) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, val)| {
                    let renamed = key_map
                        .get(k)
                        .or_else(|| value_map.get(k))
                        .cloned()
                        .unwrap_or_else(|| k.clone());
                    (renamed, transform(val, key_map, value_map))
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|x| transform(x, key_map, value_map))
                .collect(),
        ),
        Value::String(s) => value_map
            .get(s)
            .map(|h| Value::String(h.clone()))
            .unwrap_or_else(|| v.clone()),
        _ => v.clone(),
    }
}

// ---------- the comparator ----------

/// Structural equality — `serde_json::Value`'s `Object` compares by content, not key order, so
/// only genuinely order-carrying JSON (an array) needs normalizing first; [`normalize`] handles
/// the one array in this shape whose order depends on a value the transform changes.
fn compare(a: &Value, b: &Value) -> Vec<String> {
    compare_named(LOG, a, b)
}

/// [`compare`] for any fixture: `fixture` names it in the problem.
pub(crate) fn compare_named(fixture: &str, a: &Value, b: &Value) -> Vec<String> {
    let (a, b) = (normalize(a.clone()), normalize(b.clone()));
    if a == b {
        Vec::new()
    } else {
        vec![format!(
            "obfuscation replay: {fixture} folds to a different world once its claim data is renamed and hashed. The fold (or something it calls) is reading a specific name or value, not just shape. transformed pass A: {a}\npass B: {b}"
        )]
    }
}

/// `relationships` serializes as a `Vec<(Relationship, count)>` (`BTreeMap` order), and
/// `Relationship`'s `Ord` includes `kind` — a value the transform hashes — so pass A's array,
/// transformed in place, is not guaranteed to land in the same order pass B's own fold produced
/// natively. Sort both sides by their own serialized text; every other array in this shape holds
/// only entity ids, which the transform never touches, so their order is unaffected either way.
fn normalize(mut v: Value) -> Value {
    if let Value::Object(map) = &mut v
        && let Some(Value::Array(rels)) = map.get_mut("relationships")
    {
        rels.sort_by_key(ToString::to_string);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG_TEXT: &str = include_str!("../../crates/s2w-core/tests/fixtures/golden-fold-v1.json");

    #[test]
    fn the_committed_fixture_replays_clean() {
        assert_eq!(replay(LOG_TEXT), Vec::<String>::new());
    }

    #[test]
    fn a_dropped_entity_in_pass_b_is_caught() {
        let events_json: Value = serde_json::from_str(LOG_TEXT).unwrap();
        let events: Vec<WorldEvent> = serde_json::from_value(events_json.clone()).unwrap();
        let (key_map, value_map, problems) = build_maps(&events_json);
        assert!(problems.is_empty(), "{problems:?}");

        let start = || World::with_hub_cap(HUB_CAP);
        let world_a = fold(start(), &events);
        let transformed_a = transform(
            &serde_json::to_value(&world_a).unwrap(),
            &key_map,
            &value_map,
        );

        // Sanity first: an untouched clone of `transformed_a`, compared against itself, must
        // read clean. Real pass B lives in the SAME renamed namespace as `transformed_a` (it is
        // folded from the obfuscated event log, not from the raw one) — comparing against
        // anything still in the raw namespace would report every renamed field as a mismatch
        // whether or not an entity was actually dropped, which is exactly how this test used to
        // pass for the wrong reason.
        let untouched_clone = transformed_a.clone();
        let problems = compare(&transformed_a, &untouched_clone);
        assert_eq!(problems.len(), 0, "{problems:?}");

        // The drop happens on a clone of `transformed_a` itself, so it is the ONLY difference
        // from `transformed_a` — dropping this `entities.remove` call would leave `corrupted_b`
        // identical to `transformed_a` and turn the assertion below red, not silently green.
        let mut corrupted_b = transformed_a.clone();
        if let Value::Object(map) = &mut corrupted_b
            && let Some(Value::Object(entities)) = map.get_mut("entities")
        {
            let key = entities.keys().next().cloned().unwrap();
            entities.remove(&key);
        }
        let problems = compare(&transformed_a, &corrupted_b);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("different world"), "{}", problems[0]);
    }

    /// A toy "engine" that reads the literal attrs key `"wiki_id"` directly off the raw event
    /// JSON — exactly the domain-keyed read this check exists to catch. It is never wired into
    /// production; it stands in for a real engine that could regress the same way.
    fn toy_domain_keyed_read(events: &Value) -> Value {
        let mut hit = false;
        if let Value::Array(events) = events {
            for event in events {
                if let Some(Value::Object(fields)) = event.get("EntityObserved")
                    && let Some(Value::Object(attrs)) = fields.get("attrs")
                {
                    hit |= attrs.contains_key("wiki_id");
                }
            }
        }
        serde_json::json!({ "hardcoded_wiki_id_seen": hit })
    }

    #[test]
    fn a_domain_keyed_read_is_caught_by_the_real_comparator() {
        let events_json: Value = serde_json::from_str(
            r#"[{"EntityObserved": {"key": "e1", "entity_type": "t", "attrs": {"wiki_id": {"Str": "123"}}}}]"#,
        )
        .unwrap();
        let (key_map, value_map, problems) = build_maps(&events_json);
        assert!(problems.is_empty(), "{problems:?}");
        // The attrs key `wiki_id` was observed, so it is in the key-rename map — this fixture
        // exists to prove that, not just assume it.
        assert!(key_map.contains_key("wiki_id"), "{key_map:?}");

        let obfuscated = transform(&events_json, &key_map, &value_map);

        let straight_result = toy_domain_keyed_read(&events_json);
        let obfuscated_result = toy_domain_keyed_read(&obfuscated);

        // The real comparator: transform the straight run's output with the same maps, then
        // require it to match the obfuscated run's output exactly as `check` does.
        let transformed_straight = transform(&straight_result, &key_map, &value_map);
        let problems = compare(&transformed_straight, &obfuscated_result);
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn replay_itself_catches_a_domain_keyed_fold_not_just_the_comparator() {
        // The test above proves `compare` catches a domain-keyed read when called directly on
        // hand-built values — but nothing yet exercised `replay`'s own plumbing (build_maps,
        // transform, running the fold twice) end to end with a fold that can actually fail.
        // `replay`/`check` always pass the real `s2w_core::fold`, which is domain-agnostic
        // today, so this substitutes a toy fold keyed on a domain field name through
        // `replay_events` — the exact function `replay` itself calls — and requires it to
        // report the mismatch through the real entry point, not a hand-assembled comparison.
        let events_json: Value = serde_json::from_str(
            r#"[{"EntityObserved": {"key": "e1", "entity_type": "t", "attrs": {"wiki_id": {"Str": "123"}}}}]"#,
        )
        .unwrap();
        let events: Vec<WorldEvent> = serde_json::from_value(events_json.clone()).unwrap();

        let problems = replay_events(&events_json, &events, |events| {
            let mut hit = false;
            for event in events {
                if let WorldEvent::EntityObserved { attrs, .. } = event
                    && attrs.contains_key("wiki_id")
                {
                    hit = true;
                }
            }
            Ok(serde_json::json!({ "hardcoded_wiki_id_seen": hit }))
        });
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("different world"), "{}", problems[0]);
    }

    #[test]
    fn transform_never_touches_schema_field_names() {
        let events_json: Value = serde_json::from_str(LOG_TEXT).unwrap();
        let (key_map, value_map) = {
            let (k, v, problems) = build_maps(&events_json);
            assert!(problems.is_empty(), "{problems:?}");
            (k, v)
        };
        for schema in [
            "EntityObserved",
            "RelationshipObserved",
            "EntitiesMerged",
            "MergeRevoked",
            "key",
            "entity_type",
            "attrs",
            "from",
            "to",
            "kind",
            "survivor",
            "absorbed",
            "Str",
            "Int",
            "Bool",
        ] {
            assert!(
                !key_map.contains_key(schema) && !value_map.contains_key(schema),
                "schema field '{schema}' leaked into a transform map"
            );
        }
    }

    // ---------- engine-layer coverage ----------

    #[test]
    fn the_committed_fixture_replays_clean_through_the_engine_layer() {
        assert_eq!(engine_replay(LOG_TEXT), Vec::<String>::new());
    }

    /// The engine-path analogue of `a_dropped_entity_in_pass_b_is_caught`: each golden event,
    /// run through `JsonClaimsEngine`, proposes exactly itself back out, so dropping one golden
    /// event before folding stands in for "one proposed claim missing". Exercises the engine
    /// layer's own claim-collection step (`engine_fold_to_json`), not just `compare` on
    /// hand-built values.
    #[test]
    fn a_dropped_claim_on_the_engine_path_is_caught() {
        let events_json: Value = serde_json::from_str(LOG_TEXT).unwrap();
        let events: Vec<WorldEvent> = serde_json::from_value(events_json.clone()).unwrap();
        let (key_map, value_map, problems) = build_maps(&events_json);
        assert!(problems.is_empty(), "{problems:?}");

        let engines = engines();
        let fold_to_json = engine_fold_to_json(&engines);

        let full_a = fold_to_json(&events).unwrap();
        let dropped_a = fold_to_json(&events[1..]).unwrap();

        let transformed_full = transform(&full_a, &key_map, &value_map);
        let transformed_dropped = transform(&dropped_a, &key_map, &value_map);

        // Sanity first: comparing the full pass against an untouched clone of itself must read
        // clean, so the assertion below is caused by the drop, not by `compare` always firing.
        let problems = compare(&transformed_full, &transformed_full.clone());
        assert_eq!(problems.len(), 0, "{problems:?}");

        let problems = compare(&transformed_full, &transformed_dropped);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("different world"), "{}", problems[0]);
    }

    /// A toy `Engine` that reads the literal attrs key `"wiki_id"` off the raw payload bytes —
    /// exactly the domain-keyed read this check exists to catch, now at the engine layer rather
    /// than the fold. Wired through `replay_events` exactly as `engine_replay` wires the real
    /// engines, mirroring `replay_itself_catches_a_domain_keyed_fold_not_just_the_comparator`:
    /// proves `replay_events`' own plumbing surfaces the mismatch, not just `compare` called
    /// directly.
    #[test]
    fn engine_replay_itself_catches_a_domain_keyed_engine_not_just_the_comparator() {
        struct ToyDomainKeyedEngine;
        impl Engine for ToyDomainKeyedEngine {
            fn name(&self) -> &'static str {
                "toy_domain_keyed"
            }
            fn version(&self) -> u32 {
                1
            }
            fn evaluate(&self, event: &RawEvent) -> Verdict {
                let hit = String::from_utf8_lossy(&event.payload).contains("wiki_id");
                Verdict::Propose {
                    claims: vec![WorldEvent::EntityObserved {
                        key: s2w_model::NaturalKey::new(if hit { "hit" } else { "e1" }),
                        entity_type: "toy".to_owned(),
                        attrs: BTreeMap::new(),
                    }],
                    confidence: s2w_system1::Confidence::CERTAIN,
                }
            }
        }

        let events_json: Value = serde_json::from_str(
            r#"[{"EntityObserved": {"key": "e1", "entity_type": "t", "attrs": {"wiki_id": {"Str": "123"}}}}]"#,
        )
        .unwrap();
        let events: Vec<WorldEvent> = serde_json::from_value(events_json.clone()).unwrap();

        let engines: Vec<Box<dyn Engine>> = vec![Box::new(ToyDomainKeyedEngine)];
        let problems = replay_events(&events_json, &events, engine_fold_to_json(&engines));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("different world"), "{}", problems[0]);
    }
}
