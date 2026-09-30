//! Unit tests for the key spec and the executors (s2w#56).

use s2w_model::StreamMapping;
use serde_json::{Value, json};

use super::key::KeySpec;
use super::mentions::{Decoded, KeyMentions, Partition};

fn key_mentions(spec: &KeySpec, payloads: &[Value]) -> Result<KeyMentions, String> {
    super::mentions::key_mentions(spec, &Decoded::new(payloads, &spec.decode))
}

fn mapping_mentions(rules: &StreamMapping, payloads: &[Value]) -> Result<Partition, String> {
    super::mentions::mapping_mentions(rules, &Decoded::new(payloads, &rules.decode))
}

fn spec(value: &Value) -> KeySpec {
    serde_json::from_value(value.clone()).expect("the spec deserializes")
}

fn mapping(entities: &Value) -> StreamMapping {
    serde_json::from_value(json!({
        "version": 1,
        "decode": [],
        "entities": entities,
        "relationships": []
    }))
    .expect("the mapping deserializes")
}

/// The plan's example spec, valid as written.
fn example() -> Value {
    json!({
        "version": 0,
        "decode": [["data"]],
        "types": [{ "type": "T", "mentions": [
            { "path": ["data", "a"], "identity": [["data", "ctx"], ["data", "a"]] }
        ] }],
        "unscored": [["data", "z"]]
    })
}

fn rejects(value: &Value, needle: &str) {
    let problem = spec(value).validate().expect_err("the spec is invalid");
    assert!(problem.contains(needle), "{problem:?} lacks {needle:?}");
}

#[test]
fn the_example_spec_is_valid() {
    spec(&example()).validate().expect("valid");
}

#[test]
fn another_version_is_rejected() {
    let mut value = example();
    value["version"] = json!(3);
    rejects(&value, "version 3");
}

#[test]
fn a_spec_without_types_is_rejected() {
    let mut value = example();
    value["types"] = json!([]);
    rejects(&value, "no types");
}

#[test]
fn a_mention_path_listed_twice_is_rejected() {
    let mut value = example();
    value["types"] = json!([
        { "type": "T", "mentions": [{ "path": ["a"], "identity": [["a"]] }] },
        { "type": "U", "mentions": [{ "path": ["a"], "identity": [["b"]] }] }
    ]);
    rejects(&value, "listed twice");
}

#[test]
fn a_scored_path_that_is_also_unscored_is_rejected() {
    let mut value = example();
    value["unscored"] = json!([["data", "a"]]);
    rejects(&value, "also unscored");
}

#[test]
fn an_empty_identity_or_path_is_rejected() {
    let mut value = example();
    value["types"][0]["mentions"][0]["identity"] = json!([]);
    rejects(&value, "empty path or identity");
    let mut value = example();
    value["types"][0]["mentions"][0]["path"] = json!([]);
    rejects(&value, "empty path or identity");
    let mut value = example();
    value["unscored"] = json!([[]]);
    rejects(&value, "unscored path is empty");
    let mut value = example();
    value["decode"] = json!([[]]);
    rejects(&value, "decode path is empty");
    let mut value = example();
    value["types"][0]["mentions"][0]["identity"] = json!([["data", ""]]);
    rejects(&value, "empty or U+001F key");
    let mut value = example();
    value["unscored"] = json!([["data", "z\u{1f}"]]);
    rejects(&value, "empty or U+001F key");
}

#[test]
fn a_repeated_unscored_path_is_rejected() {
    let mut value = example();
    value["unscored"] = json!([["data", "z"], ["data", "z"]]);
    rejects(&value, "unscored path");
}

#[test]
fn the_key_executor_refuses_an_invalid_spec() {
    let mut value = example();
    value["version"] = json!(9);
    assert!(key_mentions(&spec(&value), &[json!({})]).is_err());
}

#[test]
fn a_bad_or_repeated_label_is_rejected() {
    let mut value = example();
    value["types"][0]["type"] = json!("a\u{1f}b");
    rejects(&value, "U+001F");
    let mut value = example();
    let kind = value["types"][0].clone();
    let mut other = kind.clone();
    other["mentions"][0]["path"] = json!(["data", "b"]);
    value["types"] = json!([kind, other]);
    rejects(&value, "listed twice");
}

#[test]
fn an_index_and_a_digit_key_are_one_mention_path() {
    let mut value = example();
    value["types"] = json!([
        { "type": "T", "mentions": [{ "path": ["a", 1], "identity": [["a", 1]] }] },
        { "type": "U", "mentions": [{ "path": ["a", "1"], "identity": [["a", "1"]] }] }
    ]);
    rejects(&value, "listed twice");
}

#[test]
fn an_index_segment_reads_an_array_element() {
    let key = spec(&json!({
        "version": 0,
        "types": [{ "type": "T", "mentions": [
            { "path": ["items", 0, "id"], "identity": [["items", 0, "id"]] }
        ] }]
    }));
    let payloads = [json!({ "items": [{ "id": 3 }] }), json!({ "items": [] })];
    let got = key_mentions(&key, &payloads)
        .expect("valid")
        .partition
        .cluster;
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        [&(0, "items.0.id".to_owned())]
    );
}

#[test]
fn an_unknown_field_does_not_parse() {
    let mut value = example();
    value["extra"] = json!(true);
    assert!(serde_json::from_value::<KeySpec>(value).is_err());
}

#[test]
fn aliases_with_equal_identity_values_join_one_cluster() {
    let key = spec(&json!({
        "version": 0,
        "types": [{ "type": "T", "mentions": [
            { "path": ["name"], "identity": [["name"]] },
            { "path": ["alias"], "identity": [["alias"]] }
        ] }]
    }));
    let payloads = [
        json!({ "name": "x", "alias": "x" }),
        json!({ "alias": "y" }),
    ];
    let got = key_mentions(&key, &payloads)
        .expect("valid")
        .partition
        .cluster;
    assert_eq!(got.len(), 3);
    assert_eq!(got[&(0, "name".to_owned())], got[&(0, "alias".to_owned())]);
    assert_ne!(got[&(0, "alias".to_owned())], got[&(1, "alias".to_owned())]);
}

#[test]
fn a_non_scalar_mention_path_gives_no_mention_even_with_a_full_identity() {
    let key = spec(&json!({
        "version": 0,
        "types": [{ "type": "T", "mentions": [
            { "path": ["a"], "identity": [["id"]] }
        ] }]
    }));
    let payloads = [
        json!({ "a": { "nested": 1 }, "id": "x" }),
        json!({ "a": "y", "id": "x" }),
    ];
    let got = key_mentions(&key, &payloads)
        .expect("valid")
        .partition
        .cluster;
    assert_eq!(got.keys().collect::<Vec<_>>(), [&(1, "a".to_owned())]);
}

#[test]
fn a_non_scalar_identity_gives_no_mention() {
    let key = spec(&json!({
        "version": 0,
        "types": [{ "type": "T", "mentions": [
            { "path": ["a"], "identity": [["ctx"], ["a"]] }
        ] }]
    }));
    let payloads = [
        json!({ "a": "x", "ctx": { "nested": 1 } }),
        json!({ "a": "x", "ctx": 1.5 }),
        json!({ "a": "x" }),
        json!({ "a": null, "ctx": "c" }),
    ];
    let got = key_mentions(&key, &payloads).expect("valid");
    assert!(got.partition.cluster.is_empty(), "{got:?}");
    // Records 0-2 hold the mention but not a scalar context: abstained, not scored.
    assert_eq!(got.abstained, [("a".to_owned(), 3)].into());
}

#[test]
fn an_undecodable_payload_mentions_nothing() {
    let key = spec(&json!({
        "version": 0,
        "decode": [["data"]],
        "types": [{ "type": "T", "mentions": [
            { "path": ["data", "a"], "identity": [["data", "a"]] }
        ] }]
    }));
    let payloads = [
        json!({ "data": "{\"a\": \"x\"}" }),
        json!({ "data": "not json" }),
        json!({ "data": 7 }),
    ];
    let got = key_mentions(&key, &payloads)
        .expect("valid")
        .partition
        .cluster;
    assert_eq!(got.len(), 1);
    assert!(got.contains_key(&(0, "data.a".to_owned())), "{got:?}");
}

#[test]
fn a_composite_mapping_key_mentions_at_its_last_path() {
    let rules = mapping(&json!([
        { "id": "site", "type_label": "S", "key": [["ctx"]], "attrs": [] },
        { "id": "object", "type_label": "O", "key": [["ctx"], ["id"]], "attrs": [] }
    ]));
    let got = mapping_mentions(&rules, &[json!({ "ctx": "c", "id": 4 })])
        .expect("no conflict")
        .cluster;
    assert_eq!(got.len(), 2);
    assert_ne!(got[&(0, "ctx".to_owned())], got[&(0, "id".to_owned())]);
}

#[test]
fn two_rules_clustering_one_mention_apart_is_an_error() {
    let rules = mapping(&json!([
        { "id": "first", "type_label": "A", "key": [["id"]], "attrs": [] },
        { "id": "second", "type_label": "B", "key": [["id"]], "attrs": [] }
    ]));
    let problem = mapping_mentions(&rules, &[json!({ "id": "x" })]).expect_err("conflict");
    assert!(
        problem.contains("\"first\"") && problem.contains("\"second\""),
        "{problem}"
    );
}

#[test]
fn two_rules_agreeing_on_a_mention_are_not_an_error() {
    let rules = mapping(&json!([
        { "id": "first", "type_label": "A", "key": [["id"]], "attrs": [] },
        { "id": "second", "type_label": "A", "key": [["id"]], "attrs": [] }
    ]));
    let got = mapping_mentions(&rules, &[json!({ "id": "x" })]).expect("agree");
    assert_eq!(got.cluster.len(), 1);
}

#[test]
fn a_mapping_read_as_its_own_key_reproduces_its_mentions() {
    let rules = mapping(&json!([
        { "id": "site", "type_label": "S", "key": [["ctx"]], "attrs": [] },
        { "id": "object", "type_label": "O", "key": [["ctx"], ["id"]], "attrs": [] },
        { "id": "object-again", "type_label": "O", "key": [["ctx"], ["id"]], "attrs": [] },
        { "id": "other", "type_label": "O", "key": [["ctx"], ["other"]], "attrs": [] }
    ]));
    let payloads = [
        json!({ "ctx": "c", "id": 4, "other": true }),
        json!({ "ctx": "d", "id": 4 }),
        json!({ "id": 5 }),
    ];
    let key = KeySpec::from_mapping(&rules).expect("valid key");
    assert_eq!(key.types.len(), 2);
    assert_eq!(
        key_mentions(&key, &payloads).expect("valid").partition,
        mapping_mentions(&rules, &payloads).expect("no conflict")
    );
}

#[test]
fn a_mapping_whose_rules_clash_is_not_a_key() {
    let rules = mapping(&json!([
        { "id": "first", "type_label": "A", "key": [["id"]], "attrs": [] },
        { "id": "second", "type_label": "B", "key": [["id"]], "attrs": [] }
    ]));
    let problem = KeySpec::from_mapping(&rules).expect_err("clash");
    assert!(problem.contains("listed twice"), "{problem}");
}

#[test]
fn the_committed_sample_passes_the_selftest() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace root");
    let report = super::selftest(root).expect("parity holds");
    assert!(
        report.contains("20 records, 60 mentions, 44 clusters"),
        "{report}"
    );
}

/// Every key file `research/h-measure/keys.toml` pins parses as a key spec, validates, and
/// yields an oracle mapping, so a malformed key fails here rather than at the first score. The
/// sha256 pins are checked by `h-measure freeze` and `score` (`h_measure/pins.rs`), not here.
#[test]
fn every_pinned_key_file_is_a_valid_key() {
    #[derive(serde::Deserialize)]
    struct Pins {
        key: Vec<Pin>,
    }
    #[derive(serde::Deserialize)]
    struct Pin {
        file: String,
    }
    let dir = crate::workspace_root().join("research/h-measure");
    let text = std::fs::read_to_string(dir.join("keys.toml")).expect("keys.toml reads");
    let pins: Pins = toml::from_str(&text).expect("keys.toml parses");
    assert!(!pins.key.is_empty(), "keys.toml pins no key");
    let mut versions = std::collections::BTreeSet::new();
    for pin in pins.key {
        let text = std::fs::read_to_string(dir.join(&pin.file)).expect("the key file reads");
        let spec: KeySpec = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{}: not a key spec: {e}", pin.file));
        spec.validate()
            .unwrap_or_else(|e| panic!("{}: invalid: {e}", pin.file));
        spec.oracle()
            .unwrap_or_else(|e| panic!("{}: no oracle mapping: {e}", pin.file));
        // A file's format is part of its pin: a `-v0` file is format 0, whatever came later.
        if pin.file.starts_with("dev-key-v0") {
            assert_eq!(spec.version, 0, "{}", pin.file);
        }
        versions.insert(spec.version);
    }
    assert!(
        versions.contains(&super::key::KEY_VERSION),
        "no pinned key uses format {}",
        super::key::KEY_VERSION
    );
}

/// A format-1 spec: type `T` at `n`, identity (`ctx`, `n`), with `no_identity` as given.
fn with_no_identity(no_identity: &Value) -> Value {
    json!({
        "version": 1,
        "types": [{ "type": "T", "mentions": [
            { "path": ["n"], "identity": [["ctx"], ["n"]], "no_identity": no_identity }
        ] }]
    })
}

#[test]
fn a_format_1_spec_with_no_identity_is_valid() {
    spec(&with_no_identity(&json!([0, "none", false])))
        .validate()
        .expect("valid");
    let mut plain = example();
    plain["version"] = json!(1);
    spec(&plain)
        .validate()
        .expect("format 1 without no_identity is valid");
}

#[test]
fn no_identity_of_the_wrong_type_does_not_parse() {
    for wrong in [json!(0), json!("0"), json!({ "0": true })] {
        let parsed = serde_json::from_value::<KeySpec>(with_no_identity(&wrong));
        assert!(parsed.is_err(), "{wrong} parsed");
    }
}

#[test]
fn a_non_scalar_no_identity_value_is_rejected() {
    for wrong in [json!(null), json!(1.5), json!([0]), json!({ "a": 0 })] {
        rejects(
            &with_no_identity(&json!([wrong])),
            "is not a string, integer or boolean",
        );
    }
}

#[test]
fn a_repeated_no_identity_value_is_rejected() {
    rejects(&with_no_identity(&json!([0, 0])), "listed twice");
    // Compared as key parts: an integer and a string are two values.
    spec(&with_no_identity(&json!([0, "0"])))
        .validate()
        .expect("valid");
}

#[test]
fn no_identity_needs_format_1() {
    let mut value = with_no_identity(&json!([0]));
    value["version"] = json!(0);
    rejects(&value, "needs key format 1");
}

#[test]
fn no_identity_on_a_path_outside_its_identity_is_rejected() {
    rejects(
        &json!({ "version": 1, "types": [{ "type": "T", "mentions": [
            { "path": ["alias"], "identity": [["n"]], "no_identity": [0] }
        ] }] }),
        "is not one of its identity paths",
    );
}

#[test]
fn a_mention_holding_a_no_identity_value_is_dropped() {
    let key = spec(&with_no_identity(&json!([0])));
    let payloads = [
        json!({ "ctx": "c", "n": 0 }),
        json!({ "ctx": "c", "n": 0 }),
        json!({ "ctx": "c", "n": 7 }),
        json!({ "ctx": "c", "n": "0" }),
        json!({ "n": 0 }),
    ];
    let got = key_mentions(&key, &payloads).expect("valid");
    // Records 0, 1 and 4 hold the sentinel: no mention, not merged, not a singleton, and not
    // abstained even when the rest of the identity is missing (record 4).
    let mentioned: Vec<usize> = got.partition.cluster.keys().map(|(r, _)| *r).collect();
    assert_eq!(mentioned, [2, 3]);
    assert!(got.abstained.is_empty(), "{:?}", got.abstained);
    assert_eq!(got.excluded_per_path(), [("n".to_owned(), 3)].into());
    // The same key without the sentinel merges records 0 and 1 into one entity.
    let plain = key_mentions(&spec(&with_no_identity(&json!([]))), &payloads).expect("valid");
    assert_eq!(plain.partition.cluster.len(), 4);
    assert!(plain.excluded.is_empty());
}

#[test]
fn a_key_read_from_a_mapping_is_the_newest_format_without_exclusions() {
    let rules = mapping(&json!([
        { "id": "object", "type_label": "O", "key": [["ctx"], ["id"]], "attrs": [] }
    ]));
    let key = KeySpec::from_mapping(&rules).expect("valid key");
    assert_eq!(key.version, super::key::KEY_VERSION);
    assert!(
        key.types
            .iter()
            .flat_map(|t| &t.mentions)
            .all(|m| m.no_identity.is_empty())
    );
}

#[test]
fn an_identity_listing_a_path_twice_is_rejected() {
    let value = json!({ "version": 0, "types": [{ "type": "P", "mentions": [
        { "path": ["id"], "identity": [["wiki"], ["id"], ["wiki"]] }
    ] }] });
    rejects(&value, "lists an identity path twice");
}
