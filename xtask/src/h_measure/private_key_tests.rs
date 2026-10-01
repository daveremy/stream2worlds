//! The private-stream answer key on the synthetic fixture (s2w#372): the Rust key executor and
//! `research/h-measure/private/key.ts` agree on the partition, and the fixture reproduces the
//! scoring (the sanitized fixture the contract's Publication rule asks for).

use std::collections::BTreeMap;
use std::path::PathBuf;

use s2w_model::StreamMapping;
use serde_json::{Value, json};

use super::grade::grade;
use super::key::KeySpec;
use super::mentions::{Decoded, key_mentions};

fn data() -> PathBuf {
    crate::workspace_root().join("research/h-measure")
}

/// The key through its `keys.toml` pin, so a key file edited without a new pin fails here too.
fn key(file: &str) -> KeySpec {
    let root = crate::workspace_root();
    let pins = super::pins::Pins::load(&root).expect("the pins load");
    pins.key(&root, file).expect("the key matches its pin").1
}

fn fixture() -> Vec<Value> {
    let text = std::fs::read_to_string(data().join("private/fixture/synthetic-20.sse"))
        .expect("the fixture reads");
    crate::discover_replay::envelopes(&text).expect("the fixture replays")
}

/// The partition's shape as `key.ts`'s `shape` writes it: counts per path and entity sizes
/// per type, never a value, and for a key with relationships the unique gold edges per type.
fn shape(spec: &KeySpec, payloads: &[Value]) -> Value {
    let found = key_mentions(spec, &Decoded::new(payloads, &spec.decode)).expect("valid key");
    let mut per_path: BTreeMap<&str, usize> = BTreeMap::new();
    let mut clusters: BTreeMap<&str, usize> = BTreeMap::new();
    for ((_, path), cluster) in &found.partition.cluster {
        *per_path.entry(path).or_default() += 1;
        *clusters.entry(cluster).or_default() += 1;
    }
    // A cluster id is a natural key whose first part is the type label.
    let mut sizes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (cluster, size) in &clusters {
        let label = cluster
            .split('\u{1f}')
            .next()
            .expect("a natural key has a label");
        *sizes
            .entry(label.to_owned())
            .or_default()
            .entry(size.to_string())
            .or_default() += 1;
    }
    let mut out = json!({
        "mentions": found.partition.cluster.len(),
        "entities": clusters.len(),
        "mentions_per_path": per_path,
        "entity_sizes": sizes,
        "abstained": found.abstained,
        "excluded": found.excluded_per_path(),
    });
    if !spec.relationships.is_empty() {
        let mut edges: BTreeMap<&str, usize> = BTreeMap::new();
        for edge in &found.edges {
            *edges.entry(&edge.label).or_default() += 1;
        }
        out["edges_per_type"] = json!(edges);
    }
    out
}

#[test]
fn the_rust_executor_agrees_with_key_ts_on_the_fixture() {
    let text = std::fs::read_to_string(data().join("private/fixture/synthetic-20.key-shape.json"))
        .expect("the shape file reads");
    let committed: Value = serde_json::from_str(&text).expect("the shape file parses");
    let payloads = fixture();
    // `key.ts` writes v2's shape under the variant's name and v1's under `v1.<variant>`.
    for (variant, v2, v1, v0) in [
        (
            "base",
            "private-key-v2.json",
            "private-key-v1.json",
            "private-key-v0.json",
        ),
        (
            "context-scored",
            "private-key-v2.context-scored.json",
            "private-key-v1.context-scored.json",
            "private-key-v0.context-scored.json",
        ),
    ] {
        let v1_shape = &committed[format!("v1.{variant}")];
        for (file, want) in [(v2, &committed[variant]), (v1, v1_shape)] {
            assert!(
                want.is_object(),
                "{file}: the shape file has no section for it"
            );
            assert_eq!(
                &shape(&key(file), &payloads),
                want,
                "{file}: regenerate with key.ts --write; if it still differs, the executors disagree"
            );
        }
        // v0 (format 2) is v1 without its relationship rows: the same partition, no edges.
        let mut partition = v1_shape.clone();
        partition
            .as_object_mut()
            .expect("a variant is an object")
            .remove("edges_per_type");
        assert_eq!(shape(&key(v0), &payloads), partition, "{v0}");
    }
}

fn item_mapping(key: &Value) -> StreamMapping {
    serde_json::from_value(json!({
        "version": 1,
        "decode": [["data"]],
        "entities": [{ "id": "item", "type_label": "item", "key": key, "attrs": [] }],
        "relationships": []
    }))
    .expect("the mapping deserializes")
}

/// lifeos#900 and s2w#900 share a number: a mapping that keys items by number alone merges
/// them, which the key scores as a false merge; keyed by (repo, number) it merges nothing.
#[test]
fn the_fixture_reproduces_the_scoring() {
    let spec = key("private-key-v0.json");
    let payloads = fixture();
    let by_repo = grade(
        &spec,
        &item_mapping(&json!([["data", "repo"], ["data", "number"]])),
        &payloads,
    )
    .expect("grades");
    assert_eq!(by_repo.mapping.false_merge, Some(0.0));
    assert_eq!(by_repo.mapping.micro.precision, Some(1.0));
    let by_number = grade(
        &spec,
        &item_mapping(&json!([["data", "number"]])),
        &payloads,
    )
    .expect("grades");
    let merged = by_number.mapping.false_merge.expect("a precision");
    assert!(merged > 0.0, "{merged}");
    let f1_number = by_number.mapping.micro.f1.expect("an f1");
    let f1_repo = by_repo.mapping.micro.f1.expect("an f1");
    assert!(f1_number < f1_repo, "{f1_number} < {f1_repo}");
    // The oracle joins no alias (`key`, `slot`, `file_id`), so its ceiling misses mentions
    // but never merges two entities.
    assert_eq!(by_repo.ceiling.false_merge, Some(0.0));
    let recall = by_repo.ceiling.micro.recall.expect("a ceiling recall");
    assert!(recall < 1.0, "{recall}");
    assert!(by_repo.ceiling.singleton_types.contains("seat"));
    assert!(by_repo.ceiling.singleton_types.contains("comment"));
}

/// private-key-v1 grades edges on the fixture: the ceiling with links reaches every key edge
/// (the alias endpoints `key` included), and the two `names -> refs` rows are counted
/// unobservable, never scored.
#[test]
fn the_v1_key_grades_edges_on_the_fixture() {
    let spec = key("private-key-v1.json");
    let graded = grade(
        &spec,
        &item_mapping(&json!([["data", "repo"], ["data", "number"]])),
        &fixture(),
    )
    .expect("grades");
    let linked = graded.ceiling_links_edges.expect("a v1 key scores edges");
    assert!(linked.key_edges > 0);
    assert_eq!(linked.micro.recall, Some(1.0), "{linked:?}");
    assert_eq!(linked.unobservable, 2);
    let mapping = graded.edges.expect("a v1 key scores the mapping's edges");
    assert_eq!(
        mapping.tp, 0,
        "the item-only mapping declares no relationship"
    );
}

/// private-key-v2 (s2w#395) scores `refs` as item mentions: its 16 ref rows (`names` from a PR,
/// `commit-names` from a commit) are observable, so nothing is counted unobservable, and the
/// ceiling with links still reaches every key edge, the fixture's four ref edges included. One
/// label over both endpoint type pairs would cap that recall: the scorer aligns each key edge
/// type with one predicted type.
#[test]
fn the_v2_key_scores_refs_edges_on_the_fixture() {
    let spec = key("private-key-v2.json");
    assert_eq!(
        spec.relationships
            .iter()
            .filter(|row| row.label == "names" || row.label == "commit-names")
            .count(),
        16
    );
    let graded = grade(
        &spec,
        &item_mapping(&json!([["data", "repo"], ["data", "number"]])),
        &fixture(),
    )
    .expect("grades");
    let linked = graded.ceiling_links_edges.expect("a v2 key scores edges");
    assert_eq!(linked.unobservable, 0);
    assert_eq!(linked.micro.recall, Some(1.0), "{linked:?}");
    let v1 = grade(
        &key("private-key-v1.json"),
        &item_mapping(&json!([["data", "repo"], ["data", "number"]])),
        &fixture(),
    )
    .expect("grades")
    .ceiling_links_edges
    .expect("a v1 key scores edges");
    assert_eq!(
        linked.key_edges,
        v1.key_edges + 4,
        "the fixture's 4 ref edges"
    );
}
