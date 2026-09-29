//! Unit tests for the key spec and the executors (s2w#56).

use s2w_model::StreamMapping;
use serde_json::{Value, json};

use super::key::KeySpec;
use super::mentions::{key_mentions, mapping_mentions};

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
    value["version"] = json!(1);
    rejects(&value, "version 1");
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
    let got = key_mentions(&key, &payloads).expect("valid").cluster;
    assert_eq!(got.len(), 3);
    assert_eq!(got[&(0, "name".to_owned())], got[&(0, "alias".to_owned())]);
    assert_ne!(got[&(0, "alias".to_owned())], got[&(1, "alias".to_owned())]);
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
    assert!(got.cluster.is_empty(), "{got:?}");
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
    let got = key_mentions(&key, &payloads).expect("valid").cluster;
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
        key_mentions(&key, &payloads).expect("valid"),
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
    assert!(report.contains("20 records"), "{report}");
}
