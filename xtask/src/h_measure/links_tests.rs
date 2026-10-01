//! Unit tests for scoring links (s2w#245 PR 3): the mapping executor clusters by the fold's
//! resolved entity, and the key's oracle with links.

use std::collections::{BTreeMap, BTreeSet};

use s2w_core::{World, fold};
use s2w_model::{Cursor, RawEvent, SourceId, StreamMapping, Timestamp, WorldEvent};
use s2w_system1::{Engine, MappingEngine, Verdict};
use serde_json::{Value, json};

use super::key::KeySpec;
use super::mentions::{Decoded, Partition, mapping_mentions};

fn mentions(mapping: &StreamMapping, payloads: &[Value]) -> Partition {
    mapping_mentions(mapping, &Decoded::new(payloads, &mapping.decode)).expect("the mapping runs")
}

fn linked(links: &Value) -> StreamMapping {
    serde_json::from_value(json!({
        "version": 2,
        "decode": [],
        "entities": [
            { "id": "name", "type_label": "T", "key": [["name"]], "attrs": [] },
            { "id": "url", "type_label": "T", "key": [["url"]], "attrs": [] }
        ],
        "relationships": [],
        "links": links
    }))
    .expect("the mapping deserializes")
}

fn cluster<'p>(partition: &'p Partition, record: usize, path: &str) -> &'p str {
    partition
        .cluster
        .get(&(record, path.to_owned()))
        .map(String::as_str)
        .unwrap_or_else(|| panic!("no mention at ({record}, {path})"))
}

#[test]
fn a_link_joins_two_values_into_the_survivors_cluster() {
    let payloads = [
        json!({ "name": "en", "url": "https://en" }),
        json!({ "url": "https://en" }),
    ];
    let got = mentions(
        &linked(&json!([{ "survivor": "name", "absorbed": "url" }])),
        &payloads,
    );
    let survivor = cluster(&got, 0, "name");
    assert_eq!(cluster(&got, 0, "url"), survivor);
    // A later mention of the absorbed value alone resolves to the survivor too.
    assert_eq!(cluster(&got, 1, "url"), survivor);
    // Without the link the two values are two clusters, keyed by their natural keys.
    let apart = mentions(&linked(&json!([])), &payloads);
    assert_ne!(cluster(&apart, 0, "url"), cluster(&apart, 0, "name"));
}

#[test]
fn an_absorbed_value_joins_only_the_first_survivor_it_meets() {
    let payloads = [
        json!({ "name": "en", "url": "shared" }),
        json!({ "name": "de", "url": "shared" }),
    ];
    let got = mentions(
        &linked(&json!([{ "survivor": "name", "absorbed": "url" }])),
        &payloads,
    );
    assert_eq!(cluster(&got, 1, "url"), cluster(&got, 0, "name"));
    assert_ne!(cluster(&got, 1, "name"), cluster(&got, 0, "name"));
}

/// The executor's clusters are the fold's: run `MappingEngine` on the linked fixture, fold its
/// claims, and each record's entities resolve to the clusters the executor gives that record.
#[test]
fn the_executor_matches_the_engine_folded_on_the_linked_fixture() {
    let root = crate::workspace_root();
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).expect("the fixture reads");
    let payloads: Vec<Value> = read("crates/s2w-system1/testdata/raw-sample.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("a JSON line"))
        .collect();
    let mapping: StreamMapping = serde_json::from_str(&read(
        "crates/s2w-system1/testdata/sample-links.mapping.json",
    ))
    .expect("the linked fixture parses");
    let (engine_side, observed) = engine_folded(&mapping, &payloads);
    let executor_side: BTreeSet<(usize, String)> = mentions(&mapping, &payloads)
        .cluster
        .iter()
        .map(|((record, _), cluster)| (*record, cluster.clone()))
        .collect();
    assert_eq!(executor_side, engine_side);
    assert!(
        executor_side.len() < observed,
        "no record's entities were joined by a link"
    );
}

/// Each `(record, cluster)` the engine's claims give after folding them all, and how many
/// `(record, key)` entities it observed.
fn engine_folded(
    mapping: &StreamMapping,
    payloads: &[Value],
) -> (BTreeSet<(usize, String)>, usize) {
    let engine = MappingEngine::new(mapping.clone()).expect("the engine builds");
    let source = SourceId::new("fixture").expect("a source id");
    let mut claims = Vec::new();
    let mut observed: BTreeSet<(usize, String)> = BTreeSet::new();
    for (record, payload) in payloads.iter().enumerate() {
        let event = RawEvent {
            source: source.clone(),
            cursor: Cursor::new(
                u64::try_from(record)
                    .expect("a position")
                    .to_be_bytes()
                    .to_vec(),
            )
            .expect("a cursor"),
            received_at: Timestamp::from_millis(0),
            payload: serde_json::to_vec(payload).expect("the payload encodes"),
        };
        if let Verdict::Propose { claims: made, .. } = engine.evaluate(&event) {
            for claim in made {
                if let WorldEvent::EntityObserved { key, .. } = &claim {
                    observed.insert((record, key.as_str().to_owned()));
                }
                claims.push(claim);
            }
        }
    }
    assert!(
        claims
            .iter()
            .any(|c| matches!(c, WorldEvent::EntitiesMerged { .. })),
        "the fixture claims no merge, so the check is vacuous"
    );
    let world = fold(World::default(), &claims);
    // Each key's root, written as the key its root entity was minted from.
    let mut minted = BTreeMap::new();
    for (_, key) in &observed {
        let natural = s2w_model::NaturalKey::new(key.clone());
        minted.insert(world.id_of(&natural).expect("an observed key"), key.clone());
    }
    let engine_side: BTreeSet<(usize, String)> = observed
        .iter()
        .map(|(record, key)| {
            let natural = s2w_model::NaturalKey::new(key.clone());
            let root = world.resolve(world.id_of(&natural).expect("an observed key"));
            (*record, minted[&root].clone())
        })
        .collect();
    (engine_side, observed.len())
}

fn aliased_spec() -> KeySpec {
    serde_json::from_value(json!({
        "version": 0,
        "decode": [],
        "types": [{ "type": "T", "mentions": [
            { "path": ["name"], "identity": [["name"]] },
            { "path": ["url"], "identity": [["name"]] }
        ] }],
        "unscored": []
    }))
    .expect("the spec deserializes")
}

#[test]
fn the_oracle_with_links_keeps_the_v0_rules_and_links_each_alias() {
    let spec = aliased_spec();
    let v0 = spec.oracle().expect("an oracle");
    let with = spec.oracle_with_links().expect("an oracle with links");
    assert_eq!(v0.version, s2w_model::MAPPING_VERSION);
    assert_eq!(with.version, s2w_model::MAPPING_VERSION_LINKS);
    assert_eq!(with.entities[..v0.entities.len()], v0.entities[..]);
    assert_eq!(with.links.len(), 1);
    assert_eq!(with.links[0].survivor, v0.entities[0].id);
}

#[test]
fn the_oracle_with_links_scores_an_alias_the_v0_oracle_cannot() {
    let spec = aliased_spec();
    let payloads = [
        json!({ "name": "en", "url": "https://en" }),
        json!({ "name": "en", "url": "https://en" }),
    ];
    let graded = super::grade::grade(&spec, &spec.oracle().expect("an oracle"), &payloads)
        .expect("the grade runs");
    assert_eq!(graded.ceiling_links.micro.f1, Some(1.0));
    assert!(graded.ceiling.micro.recall < Some(1.0));
}

#[test]
fn a_key_without_aliases_has_the_v0_oracle_as_its_oracle_with_links() {
    let spec: KeySpec = serde_json::from_value(json!({
        "version": 0,
        "decode": [],
        "types": [{ "type": "T", "mentions": [{ "path": ["name"], "identity": [["name"]] }] }],
        "unscored": []
    }))
    .expect("the spec deserializes");
    assert_eq!(spec.oracle_with_links(), spec.oracle());
}
