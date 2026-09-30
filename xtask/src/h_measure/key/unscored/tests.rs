//! Key format 2's prefix form for `unscored` (s2w#224), and the exact form it keeps.

use s2w_model::StreamMapping;
use serde_json::{Value, json};

use super::super::super::grade::{Grade, grade};
use super::super::KeySpec;

fn spec(value: &Value) -> KeySpec {
    serde_json::from_value(value.clone()).expect("the spec deserializes")
}

fn rejects(value: &Value, needle: &str) {
    let problem = spec(value).validate().expect_err("the spec is invalid");
    assert!(problem.contains(needle), "{problem:?} lacks {needle:?}");
}

/// A key of format `version`: type `T` at `data.a`, with `unscored` as given.
fn key(version: u32, unscored: &Value) -> Value {
    json!({
        "version": version,
        "types": [{ "type": "T", "mentions": [
            { "path": ["data", "a"], "identity": [["data", "a"]] }
        ] }],
        "unscored": unscored
    })
}

/// A mapping that keys `T` at `data.a` and mints a second type at each of `extra`, the way a
/// discovered mapping mints an entity at a path the key meant to exclude.
fn mapping(extra: &[Value]) -> StreamMapping {
    let mut entities =
        vec![json!({ "id": "a", "type_label": "T", "key": [["data", "a"]], "attrs": [] })];
    for (at, path) in extra.iter().enumerate() {
        entities
            .push(json!({ "id": format!("x{at}"), "type_label": "X", "key": [path], "attrs": [] }));
    }
    serde_json::from_value(json!({
        "version": 1, "decode": [], "entities": entities, "relationships": []
    }))
    .expect("the mapping deserializes")
}

/// The development corpus holds `data.p` and `data.p.seen`; the held-out corpus also holds
/// `data.p.new`, a path under `data.p` that the development corpus never showed.
fn heldout() -> Vec<Value> {
    vec![
        json!({ "data": { "a": "x", "p": { "seen": 1, "new": 2 } } }),
        json!({ "data": { "a": "y", "p": { "seen": 3, "new": 4 } } }),
    ]
}

fn graded(key: &Value, extra: &[Value]) -> Grade {
    let key = spec(key);
    key.validate().expect("valid key");
    grade(&key, &mapping(extra), &heldout()).expect("grades")
}

#[test]
fn a_prefix_excludes_a_path_under_it_that_only_a_heldout_corpus_holds() {
    let extra = [json!(["data", "p", "new"])];
    let prefix = graded(&key(2, &json!([{ "prefix": ["data", "p"] }])), &extra);
    assert!(prefix.mapping.spurious.is_empty(), "{:?}", prefix.mapping);
    assert_eq!(prefix.mapping.micro.precision, Some(1.0));
    // Listing the paths the development corpus showed, as v0 must, still scores the new one.
    let exact = graded(
        &key(0, &json!([["data", "p"], ["data", "p", "seen"]])),
        &extra,
    );
    assert_eq!(
        exact.mapping.spurious,
        [("data.p.new".to_owned(), 2)].into()
    );
    assert_eq!(exact.mapping.micro.precision, Some(0.5));
}

#[test]
fn a_prefix_covers_itself_an_index_and_nested_keys_only() {
    let unscored = spec(&key(
        2,
        &json!([{ "prefix": ["data", "p"] }, ["data", "z"]]),
    ))
    .unscored();
    for id in ["data.p", "data.p.seen", "data.p.0", "data.p.q.r", "data.z"] {
        assert!(unscored.covers(id), "{id} is not covered");
    }
    for id in [
        "data",
        "data.px",
        "data.p\\.x",
        "data.z.child",
        "p",
        "data.a",
    ] {
        assert!(!unscored.covers(id), "{id} is covered");
    }
}

#[test]
fn a_prefix_does_not_cover_a_sibling_key_that_shares_its_text() {
    let payloads = [json!({ "data": { "a": "x", "px": 1, "p.x": 2 } })];
    let key = spec(&key(2, &json!([{ "prefix": ["data", "p"] }])));
    let extra = [json!(["data", "px"]), json!(["data", "p.x"])];
    let got = grade(&key, &mapping(&extra), &payloads).expect("grades");
    // `p.x` is one key holding a dot (id `data.p\.x`), not `x` under `p`.
    assert_eq!(
        got.mapping.spurious,
        [("data.p\\.x".to_owned(), 1), ("data.px".to_owned(), 1)].into()
    );
}

#[test]
fn an_exact_entry_in_format_2_scores_as_in_format_0() {
    // Exact entries cover only themselves in every format: `data.p.seen` stays scored.
    let exact = json!([["data", "p"]]);
    let extra = [json!(["data", "p"]), json!(["data", "p", "seen"])];
    let v0 = graded(&key(0, &exact), &extra);
    let v2 = graded(&key(2, &exact), &extra);
    assert_eq!(v0, v2);
    assert_eq!(v2.mapping.spurious, [("data.p.seen".to_owned(), 2)].into());
    assert_eq!(v2.mapping.micro.precision, Some(0.5));
}

#[test]
fn a_format_0_key_scores_as_before() {
    let unscored = json!([["data", "p", "seen"]]);
    let got = graded(&key(0, &unscored), &[json!(["data", "p", "seen"])]);
    assert!(got.mapping.spurious.is_empty(), "{:?}", got.mapping);
    assert_eq!(got.mapping.micro.f1, Some(1.0));
    assert_eq!(got.ceiling.micro.f1, Some(1.0));
    // The path under it is not covered: v0 matches exactly, as it always did.
    let under = graded(&key(0, &unscored), &[json!(["data", "p", "new"])]);
    assert_eq!(
        under.mapping.spurious,
        [("data.p.new".to_owned(), 2)].into()
    );
}

#[test]
fn a_prefix_needs_format_2() {
    for version in [0, 1] {
        rejects(
            &key(version, &json!([{ "prefix": ["data", "p"] }])),
            "needs key format 2",
        );
    }
}

#[test]
fn a_prefix_listed_twice_or_covering_another_entry_is_rejected() {
    let prefix = json!({ "prefix": ["data", "p"] });
    rejects(&key(2, &json!([prefix, prefix])), "listed twice");
    rejects(&key(2, &json!([prefix, ["data", "p"]])), "listed twice");
    rejects(
        &key(2, &json!([prefix, ["data", "p", "seen"]])),
        "is under unscored prefix",
    );
    rejects(
        &key(2, &json!([{ "prefix": ["data", "p", "q"] }, prefix])),
        "is under unscored prefix",
    );
    // A sibling that shares the prefix's text is not under it.
    spec(&key(2, &json!([prefix, ["data", "px"]])))
        .validate()
        .expect("valid");
}

#[test]
fn a_mention_path_under_a_prefix_is_rejected() {
    rejects(
        &key(2, &json!([{ "prefix": ["data"] }])),
        "is also unscored",
    );
}

#[test]
fn an_empty_or_malformed_prefix_is_rejected() {
    rejects(
        &key(2, &json!([{ "prefix": [] }])),
        "unscored path is empty",
    );
    for wrong in [
        json!({ "prefix": ["data", "p"], "exact": true }),
        json!({ "prefix": "data.p" }),
        json!({}),
    ] {
        let parsed = serde_json::from_value::<KeySpec>(key(2, &json!([wrong])));
        assert!(parsed.is_err(), "{wrong} parsed");
    }
}
