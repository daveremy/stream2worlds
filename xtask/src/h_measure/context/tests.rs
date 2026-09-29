//! Context-collision rows (the unfloored composite-key sub-metric), graded end to end.

use s2w_model::StreamMapping;
use serde_json::{Value, json};

use super::super::key::KeySpec;
use super::super::score::{Grade, grade};
use super::ContextRow;

fn spec(value: &Value) -> KeySpec {
    serde_json::from_value(value.clone()).expect("the spec deserializes")
}

fn mapping(entities: &Value) -> StreamMapping {
    serde_json::from_value(json!({
        "version": 1, "decode": [], "entities": entities, "relationships": []
    }))
    .expect("the mapping deserializes")
}

/// Type `P` keyed on `(wiki, id)`, with `title` an alias mention of the same identity.
fn composite() -> KeySpec {
    spec(
        &json!({ "version": 0, "types": [{ "type": "P", "mentions": [
        { "path": ["id"], "identity": [["wiki"], ["id"]] },
        { "path": ["title"], "identity": [["wiki"], ["id"]] }
    ] }] }),
    )
}

/// Entities `(en, 1)` and `(de, 1)` collide without the wiki; `(en, 2)` does not.
fn colliding() -> [Value; 3] {
    [
        json!({ "wiki": "en", "id": 1, "title": "A" }),
        json!({ "wiki": "de", "id": 1, "title": "A" }),
        json!({ "wiki": "en", "id": 2 }),
    ]
}

fn only_row(got: &Grade) -> (&str, &ContextRow) {
    assert_eq!(got.contexts.len(), 1, "{:?}", got.contexts);
    let (name, row) = got.contexts.iter().next().expect("one row");
    (name.as_str(), row)
}

fn keyed_on(key: Value) -> StreamMapping {
    mapping(&json!([{ "id": "r", "type_label": "P", "key": key, "attrs": [] }]))
}

#[test]
fn a_mapping_keyed_without_the_context_merges_the_collision_group() {
    let got = grade(&composite(), &keyed_on(json!([["id"]])), &colliding()).expect("grades");
    let (name, row) = only_row(&got);
    assert!(name.starts_with("P @ ") && name.contains("wiki"), "{name}");
    assert_eq!((row.groups, row.entities), (1, 2));
    // Both entities' mentions at `id` and at the alias `title`; `(en, 2)` is outside the set.
    assert_eq!(row.mentions, 4);
    assert_eq!(row.mapping.precision, Some(0.5));
    assert_eq!(row.ceiling.precision, Some(1.0));
}

#[test]
fn a_mapping_keyed_with_the_context_scores_the_ceilings_precision() {
    let got = grade(
        &composite(),
        &keyed_on(json!([["wiki"], ["id"]])),
        &colliding(),
    )
    .expect("grades");
    let (_, row) = only_row(&got);
    assert_eq!(row.mapping.precision, Some(1.0));
    assert_eq!(row.mapping, row.ceiling);
}

#[test]
fn an_alias_mention_of_a_colliding_entity_is_in_the_set() {
    let got = grade(&composite(), &keyed_on(json!([["id"]])), &colliding()).expect("grades");
    let (_, row) = only_row(&got);
    // The oracle has no rule for the alias path, so the ceiling cannot recall every mention.
    assert!(
        row.ceiling.recall.is_some_and(|r| r < 1.0),
        "{:?}",
        row.ceiling
    );
}

#[test]
fn a_row_with_no_collision_group_is_listed_and_undefined() {
    let payloads = [
        json!({ "wiki": "en", "id": 1 }),
        json!({ "wiki": "en", "id": 2 }),
    ];
    let got = grade(&composite(), &keyed_on(json!([["id"]])), &payloads).expect("grades");
    let (_, row) = only_row(&got);
    assert_eq!((row.groups, row.entities, row.mentions), (0, 0, 0));
    assert_eq!(row.mapping.precision, None);
    assert_eq!(row.ceiling.recall, None);
}

#[test]
fn a_single_path_identity_has_no_context_row() {
    let key = spec(
        &json!({ "version": 0, "types": [{ "type": "P", "mentions": [
        { "path": ["id"], "identity": [["id"]] }
    ] }] }),
    );
    let got = grade(&key, &keyed_on(json!([["id"]])), &colliding()).expect("grades");
    assert!(got.contexts.is_empty(), "{:?}", got.contexts);
}

#[test]
fn an_excluded_mention_never_forms_a_collision_group() {
    let key = spec(
        &json!({ "version": 1, "types": [{ "type": "P", "mentions": [
        { "path": ["id"], "identity": [["wiki"], ["id"]], "no_identity": [0] }
    ] }] }),
    );
    let payloads = [
        json!({ "wiki": "en", "id": 0 }),
        json!({ "wiki": "de", "id": 0 }),
    ];
    let got = grade(&key, &keyed_on(json!([["id"]])), &payloads).expect("grades");
    let (_, row) = only_row(&got);
    assert_eq!(row.groups, 0);
    assert_eq!(row.mapping.precision, None);
}
