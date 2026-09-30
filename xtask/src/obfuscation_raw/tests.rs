use super::*;
use s2w_model::{KEY_SEPARATOR, NaturalKey};
use s2w_system1::AbstainReason;

const RAW_TEXT: &str = include_str!("../../../crates/s2w-system1/testdata/raw-sample.jsonl");
const MAPPING_TEXT: &str = include_str!("../../../crates/s2w-system1/testdata/sample.mapping.json");
const LINKS_TEXT: &str =
    include_str!("../../../crates/s2w-system1/testdata/sample-links.mapping.json");

fn replay_with(harness: &Harness) -> Vec<String> {
    replay(RAW_TEXT, MAPPING_TEXT, harness)
}

fn assert_different_world(problems: &[String]) {
    assert!(
        problems.iter().any(|p| p.contains("different world")),
        "{problems:?}"
    );
}

#[test]
fn the_committed_fixture_replays_clean() {
    assert_eq!(replay_with(&Harness::REAL), Vec::<String>::new());
}

#[test]
fn the_check_reads_the_committed_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert_eq!(check(&root), Vec::<String>::new());
}

/// The fixture lines are byte-identical to what the SSE adapter stores: the envelope is a
/// sorted two-key JSON object `{"data":…,"id":…}` (`s2w-sources/src/sse/envelope.rs`).
#[test]
fn fixture_lines_are_the_sse_adapters_stored_bytes() {
    for line in RAW_TEXT.lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        let data = value["data"].as_str().unwrap();
        let id = value["id"].as_str().unwrap();
        let mut fields = serde_json::Map::new();
        fields.insert("data".to_owned(), data.into());
        fields.insert("id".to_owned(), id.into());
        assert_eq!(Value::Object(fields).to_string(), line);
    }
}

/// A toy engine that runs the real mapping but only when the payload spells one of the
/// fixture's raw field names: exactly the domain-keyed read this check exists to catch.
struct KeyedOnAFieldName(MappingEngine);

impl Engine for KeyedOnAFieldName {
    fn name(&self) -> &'static str {
        "toy_keyed"
    }
    fn version(&self) -> u32 {
        1
    }
    fn evaluate(&self, event: &RawEvent) -> Verdict {
        if String::from_utf8_lossy(&event.payload).contains("performer") {
            self.0.evaluate(event)
        } else {
            Verdict::Abstain {
                reason: AbstainReason::NotMine,
            }
        }
    }
}

#[test]
fn an_engine_keyed_on_a_raw_field_name_is_caught() {
    let harness = Harness {
        engine: |mapping| {
            let inner = MappingEngine::new(mapping).map_err(|e| e.to_string())?;
            Ok(Box::new(KeyedOnAFieldName(inner)))
        },
        ..Harness::REAL
    };
    assert_different_world(&replay_with(&harness));
}

/// An obfuscator that renames the envelope but never descends into the decoded string leaves
/// the stream's own field names in place; the renamed mapping then resolves nothing and pass B
/// is empty, which the check must report rather than pass.
#[test]
fn an_obfuscator_that_skips_the_decoded_string_is_caught() {
    let harness = Harness {
        descend: false,
        ..Harness::REAL
    };
    assert_different_world(&replay_with(&harness));
}

#[test]
fn a_dropped_claim_in_pass_b_is_caught() {
    let harness = Harness {
        mutate_b: |mapping| {
            mapping.relationships.pop();
        },
        ..Harness::REAL
    };
    assert_different_world(&replay_with(&harness));
}

#[test]
fn a_changed_label_in_pass_b_is_caught() {
    let harness = Harness {
        mutate_b: |mapping| mapping.entities[0].type_label = "other".to_owned(),
        ..Harness::REAL
    };
    assert_different_world(&replay_with(&harness));
}

#[test]
fn a_vacuous_mapping_is_reported() {
    let mapping = r#"{"version":1,"decode":[["data"]],"entities":[
        {"id":"only","type_label":"t","key":[["data","dt"]],"attrs":[]}],"relationships":[]}"#;
    let problems = replay(RAW_TEXT, mapping, &Harness::REAL);
    for what in [
        "two entity types",
        "one relationship",
        "one multi-part key",
        "one integer key part",
        "one string attribute",
    ] {
        assert!(
            problems.iter().any(|p| p.contains(what)),
            "{what}: {problems:?}"
        );
    }
}

#[test]
fn a_mapping_segment_absent_from_the_fixture_fails_closed() {
    let mapping = MAPPING_TEXT.replacen("\"user_text\"", "\"not_a_field\"", 1);
    let problems = replay(RAW_TEXT, &mapping, &Harness::REAL);
    assert!(
        problems.iter().any(|p| p.contains("'not_a_field'")),
        "{problems:?}"
    );
}

#[test]
fn a_raw_leaf_left_in_pass_b_is_reported() {
    let leaves = BTreeSet::from(["x".to_owned()]);
    let claim = WorldEvent::EntityObserved {
        key: NaturalKey::new(format!("t{KEY_SEPARATOR}\"x\"")),
        entity_type: "t".to_owned(),
        attrs: BTreeMap::new(),
    };
    assert_eq!(rename::leaked_leaves(&[claim], &leaves).len(), 1);
}

#[test]
fn a_key_that_does_not_read_fails_closed() {
    let claim = WorldEvent::EntityObserved {
        key: NaturalKey::new(format!("t{KEY_SEPARATOR}007")),
        entity_type: "t".to_owned(),
        attrs: BTreeMap::new(),
    };
    let leaked = rename::leaked_leaves(std::slice::from_ref(&claim), &BTreeSet::new());
    assert_eq!(leaked.len(), 1, "{leaked:?}");
    assert!(leaked[0].contains("key part 0"), "{leaked:?}");
    let vacuous = rename::non_vacuity(std::slice::from_ref(&claim), &[], false);
    assert!(
        vacuous.iter().any(|p| p.contains("key part 0")),
        "{vacuous:?}"
    );
}

#[test]
fn the_maps_cover_every_mapping_name_and_decode_into_the_data_string() {
    let payloads = parse_lines(&RAW_TEXT.lines().collect::<Vec<_>>()).unwrap();
    let mapping: StreamMapping = serde_json::from_str(MAPPING_TEXT).unwrap();
    let maps = Maps::build(&payloads, &mapping).unwrap();
    // Keys inside the decoded string are renamed, and the string itself is not a leaf.
    assert!(maps.keys.contains_key("performer"));
    assert!(!maps.raw_leaves.iter().any(|leaf| leaf.starts_with('{')));
    let renamed = maps.mapping(&mapping).unwrap();
    let text = serde_json::to_string(&renamed).unwrap();
    for name in ["data", "performer", "user", "edited", "title"] {
        assert!(!text.contains(&format!("\"{name}\"")), "{name}: {text}");
    }
    // Rule ids are not renamed.
    assert_eq!(renamed.entities[0].id, mapping.entities[0].id);
}

#[test]
fn the_committed_linked_fixture_replays_clean() {
    assert_eq!(
        replay_requiring(RAW_TEXT, LINKS_TEXT, &Harness::REAL, true),
        Vec::<String>::new()
    );
}

/// A pass B that loses its link claims no merge, so its world keeps each site's two keys
/// apart: the replay sees a different world.
#[test]
fn a_pass_b_without_its_merges_is_a_different_world() {
    let harness = Harness {
        mutate_b: |mapping| mapping.links.clear(),
        ..Harness::REAL
    };
    assert_different_world(&replay(RAW_TEXT, LINKS_TEXT, &harness));
}

/// The linked fixture must claim a merge that changes the world: without its link (the
/// version-1 fixture, required to merge) the replay is vacuous on both counts.
#[test]
fn a_linked_fixture_without_a_merge_is_reported() {
    let problems = replay_requiring(RAW_TEXT, MAPPING_TEXT, &Harness::REAL, true);
    for what in ["one merge claim", "merges must change the folded world"] {
        assert!(
            problems.iter().any(|p| p.contains(what)),
            "{what}: {problems:?}"
        );
    }
}

/// A link whose two rules always give equal keys claims no merge, and the linked fixture's
/// replay says so rather than passing.
#[test]
fn a_link_whose_keys_never_differ_is_reported() {
    // `data.performer.wiki_id` equals `data.wiki_id` on every line, so the two keys are equal
    // text and the engine claims no merge at all.
    let mapping = LINKS_TEXT.replacen(
        r#"[["data", "meta", "domain"]]"#,
        r#"[["data", "performer", "wiki_id"]]"#,
        1,
    );
    assert_ne!(mapping, LINKS_TEXT);
    let problems = replay(RAW_TEXT, &mapping, &Harness::REAL);
    assert!(
        problems.iter().any(|p| p.contains("one merge claim")),
        "{problems:?}"
    );
}
