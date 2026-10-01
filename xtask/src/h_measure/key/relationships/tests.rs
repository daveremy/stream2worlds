//! Key format 3's `relationships` (s2w#388): validation, `from_mapping`, the oracle's rules, and
//! the gold and predicted edges both executors place.

use std::collections::BTreeSet;

use s2w_model::StreamMapping;
use serde_json::{Value, json};

use super::super::super::mentions::{
    Decoded, Edge, KeyMentions, MappingMentions, key_mentions, mapping_mentions,
};
use super::super::KeySpec;

fn spec(value: &Value) -> KeySpec {
    serde_json::from_value(value.clone()).expect("the spec deserializes")
}

fn rejects(value: &Value, needle: &str) {
    let problem = spec(value).validate().expect_err("the spec is invalid");
    assert!(problem.contains(needle), "{problem:?} lacks {needle:?}");
}

fn gold(spec: &KeySpec, payloads: &[Value]) -> KeyMentions {
    key_mentions(spec, &Decoded::new(payloads, &spec.decode)).expect("the key runs")
}

fn predicted(mapping: &StreamMapping, payloads: &[Value]) -> MappingMentions {
    mapping_mentions(mapping, &Decoded::new(payloads, &mapping.decode)).expect("the mapping runs")
}

/// A format-3 key: type `C` (commit) at `sha` and `parent`, type `U` (user) at `user` (with
/// `"nobody"` excluded) and its alias `login`, type `P` at `pr` identified by (`repo`, `pr`),
/// with `relationships` as given.
fn key(version: u32, relationships: &Value) -> Value {
    json!({
        "version": version,
        "types": [
            { "type": "C", "mentions": [
                { "path": ["sha"], "identity": [["sha"]] },
                { "path": ["parent"], "identity": [["parent"]] }
            ] },
            { "type": "U", "mentions": [
                { "path": ["user"], "identity": [["user"]], "no_identity": ["nobody"] },
                { "path": ["login"], "identity": [["user"]] }
            ] },
            { "type": "P", "mentions": [
                { "path": ["pr"], "identity": [["repo"], ["pr"]] }
            ] }
        ],
        "relationships": relationships
    })
}

fn row(label: &str, from: &str, to: &str) -> Value {
    json!({ "type": label, "from": [from], "to": [to] })
}

fn edge(label: &str, from: &str, to: &str) -> Edge {
    Edge {
        label: label.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
    }
}

/// The cluster the key executor placed a mention in.
fn at(found: &KeyMentions, record: usize, path: &str) -> String {
    found
        .partition
        .cluster
        .get(&(record, path.to_owned()))
        .unwrap_or_else(|| panic!("no mention at ({record}, {path})"))
        .clone()
}

#[test]
fn a_format_3_key_with_relationships_is_valid() {
    let value = key(
        3,
        &json!([
            row("parent", "sha", "parent"),
            row("by", "sha", "user"),
            row("opened", "login", "pr")
        ]),
    );
    spec(&value).validate().expect("valid");
}

#[test]
fn relationships_before_format_3_are_refused() {
    rejects(
        &key(2, &json!([row("parent", "sha", "parent")])),
        "need key format 3",
    );
}

#[test]
fn an_empty_relationships_list_is_fine_in_any_format() {
    spec(&key(1, &json!([]))).validate().expect("valid");
}

#[test]
fn an_observable_row_must_join_two_mention_paths() {
    rejects(
        &key(3, &json!([row("x", "sha", "repo")])),
        "to path FieldPath([Key(\"repo\")]) is not a mention path",
    );
    rejects(
        &key(3, &json!([row("x", "nowhere", "sha")])),
        "from path FieldPath([Key(\"nowhere\")]) is not a mention path",
    );
}

#[test]
fn a_row_from_a_path_to_itself_is_refused() {
    rejects(&key(3, &json!([row("x", "sha", "sha")])), "one path");
}

#[test]
fn a_repeated_row_or_pair_is_refused() {
    let twice = row("parent", "sha", "parent");
    rejects(&key(3, &json!([twice, twice])), "listed twice");
    rejects(
        &key(
            3,
            &json!([
                row("parent", "sha", "parent"),
                row("child", "sha", "parent")
            ]),
        ),
        "one (from, to) pair carries one edge type",
    );
}

#[test]
fn a_bad_type_or_path_is_refused() {
    rejects(&key(3, &json!([row("", "sha", "parent")])), "is empty");
    rejects(
        &key(3, &json!([row("a\u{1f}b", "sha", "parent")])),
        "U+001F",
    );
    rejects(
        &key(3, &json!([{ "type": "x", "from": [], "to": ["sha"] }])),
        "from or to path is empty",
    );
}

#[test]
fn an_unknown_row_field_does_not_parse() {
    let value = key(
        3,
        &json!([{ "type": "x", "from": ["sha"], "to": ["user"], "when": "always" }]),
    );
    assert!(serde_json::from_value::<KeySpec>(value).is_err());
}

#[test]
fn an_unobservable_row_needs_a_reason_and_may_name_any_path() {
    let with = |reason: &str| {
        key(
            3,
            &json!([{ "type": "names", "from": ["pr"], "to": ["refs"], "unobservable": reason }]),
        )
    };
    rejects(&with("  "), "needs a reason");
    spec(&with("refs drop the repo prefix"))
        .validate()
        .expect("an unobservable row may name a path that is no mention");
}

#[test]
fn an_earlier_format_reads_and_writes_without_relationships() {
    let value = key(2, &json!([]));
    let mut bare = value.clone();
    bare.as_object_mut()
        .expect("an object")
        .remove("relationships");
    let read = spec(&bare);
    assert!(read.relationships.is_empty());
    let written = serde_json::to_value(&read).expect("serializes");
    assert!(written.get("relationships").is_none(), "{written}");
}

#[test]
fn the_key_places_one_unique_directed_edge_per_row_and_entity_pair() {
    let spec = spec(&key(
        3,
        &json!([row("parent", "sha", "parent"), row("by", "sha", "user")]),
    ));
    let payloads = [
        json!({ "sha": "c2", "parent": "c1", "user": "ann" }),
        // The same two edges again: an edge seen in many records is one edge.
        json!({ "sha": "c2", "parent": "c1", "user": "ann" }),
        // No parent: only `by`.
        json!({ "sha": "c3", "user": "bob" }),
    ];
    let found = gold(&spec, &payloads);
    let want: BTreeSet<Edge> = [
        edge("parent", &at(&found, 0, "sha"), &at(&found, 0, "parent")),
        edge("by", &at(&found, 0, "sha"), &at(&found, 0, "user")),
        edge("by", &at(&found, 2, "sha"), &at(&found, 2, "user")),
    ]
    .into_iter()
    .collect();
    assert_eq!(found.edges, want);
    // Direction is as written: no reversed edge.
    assert!(!found.edges.contains(&edge(
        "parent",
        &at(&found, 0, "parent"),
        &at(&found, 0, "sha")
    )));
    assert_eq!(found.unobservable, 0);
}

#[test]
fn an_abstained_excluded_or_unobservable_endpoint_places_no_edge() {
    let spec = spec(&key(
        3,
        &json!([
            row("by", "sha", "user"),
            row("opened", "sha", "pr"),
            { "type": "names", "from": ["sha"], "to": ["refs"], "unobservable": "no repo" }
        ]),
    ));
    let payloads = [
        // `user` excluded (`no_identity`), `pr` abstained (no `repo`), `refs` unobservable.
        json!({ "sha": "c1", "user": "nobody", "pr": 4, "refs": "#4" }),
    ];
    let found = gold(&spec, &payloads);
    assert!(found.edges.is_empty(), "{:?}", found.edges);
    assert_eq!(found.unobservable, 1);
    assert_eq!(found.excluded.len(), 1);
    assert_eq!(found.abstained.get("pr"), Some(&1));
}

#[test]
fn relationships_change_no_mention() {
    let payloads = [
        json!({ "sha": "c2", "parent": "c1", "user": "ann", "login": "ann", "repo": "r", "pr": 1 }),
        json!({ "sha": "c3", "user": "nobody", "pr": 2 }),
    ];
    let plain = gold(&spec(&key(2, &json!([]))), &payloads);
    let with = gold(
        &spec(&key(
            3,
            &json!([row("parent", "sha", "parent"), row("opened", "login", "pr")]),
        )),
        &payloads,
    );
    assert_eq!(with.partition, plain.partition);
    assert_eq!(with.abstained, plain.abstained);
    assert_eq!(with.excluded, plain.excluded);
    assert!(plain.edges.is_empty());
    assert_eq!(with.edges.len(), 2);
}

/// A mapping: `C` at `sha` and at `parent`, `U` at `user` and at `login` (`login` absorbed into
/// `user` when `links` is true), with `relationships` as given.
fn mapping(relationships: &Value, links: bool) -> StreamMapping {
    let link = if links {
        json!([{ "survivor": "user", "absorbed": "login" }])
    } else {
        json!([])
    };
    serde_json::from_value(json!({
        "version": if links { 2 } else { 1 },
        "decode": [],
        "entities": [
            { "id": "sha", "type_label": "C", "key": [["sha"]], "attrs": [] },
            { "id": "parent", "type_label": "C", "key": [["parent"]], "attrs": [] },
            { "id": "user", "type_label": "U", "key": [["user"]], "attrs": [] },
            { "id": "login", "type_label": "U", "key": [["login"]], "attrs": [] }
        ],
        "relationships": relationships,
        "links": link
    }))
    .expect("the mapping deserializes")
}

#[test]
fn from_mapping_writes_format_3_with_one_row_per_rule() {
    let rules = json!([
        { "from": "sha", "to": "parent", "kind": "parent" },
        { "from": "sha", "to": "parent", "kind": "parent" },
        { "from": "sha", "to": "user", "kind": "by" }
    ]);
    let own = KeySpec::from_mapping(&mapping(&rules, false)).expect("a key");
    assert_eq!(own.version, 3);
    let rows: Vec<(String, Value, Value)> = own
        .relationships
        .iter()
        .map(|r| {
            (
                r.label.clone(),
                serde_json::to_value(&r.from).expect("path"),
                serde_json::to_value(&r.to).expect("path"),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("parent".to_owned(), json!(["sha"]), json!(["parent"])),
            ("by".to_owned(), json!(["sha"]), json!(["user"]))
        ]
    );
}

#[test]
fn from_mapping_refuses_two_kinds_on_one_pair() {
    let rules = json!([
        { "from": "sha", "to": "parent", "kind": "parent" },
        { "from": "sha", "to": "parent", "kind": "child" }
    ]);
    let problem = KeySpec::from_mapping(&mapping(&rules, false)).expect_err("a clash");
    assert!(problem.contains("one edge type"), "{problem}");
}

#[test]
fn a_mapping_read_as_its_own_key_reproduces_its_edges() {
    let rules = json!([
        { "from": "sha", "to": "parent", "kind": "parent" },
        { "from": "user", "to": "sha", "kind": "made" }
    ]);
    let rules = mapping(&rules, false);
    let payloads = [
        json!({ "sha": "c2", "parent": "c1", "user": "ann" }),
        json!({ "sha": "c3", "parent": "c2" }),
        json!({ "sha": "c3", "user": "bob" }),
    ];
    let own = KeySpec::from_mapping(&rules).expect("a key");
    let mapped = predicted(&rules, &payloads);
    assert_eq!(mapped.edges.len(), 4);
    assert_eq!(gold(&own, &payloads).edges, mapped.edges);
    assert_eq!(
        predicted(&own.oracle().expect("an oracle"), &payloads).edges,
        mapped.edges
    );
}

#[test]
fn a_linked_alias_endpoint_resolves_to_the_survivors_cluster() {
    let rules = json!([{ "from": "sha", "to": "login", "kind": "by" }]);
    let payloads = [
        // `login` "a1" joins `user` "ann" here.
        json!({ "user": "ann", "login": "a1" }),
        // An edge to the alias alone, in a later record.
        json!({ "sha": "c1", "login": "a1" }),
    ];
    let linked = predicted(&mapping(&rules, true), &payloads);
    let ann = linked
        .partition
        .cluster
        .get(&(0, "user".to_owned()))
        .expect("a mention")
        .clone();
    let sha = linked
        .partition
        .cluster
        .get(&(1, "sha".to_owned()))
        .expect("a mention")
        .clone();
    assert_eq!(linked.edges, [edge("by", &sha, &ann)].into_iter().collect());
    // Without the link the alias is its own cluster.
    let plain = predicted(&mapping(&rules, false), &payloads);
    assert_eq!(plain.edges.len(), 1);
    assert!(!plain.edges.contains(&edge("by", &sha, &ann)));
}

#[test]
fn the_oracle_reaches_an_alias_endpoint_only_with_links() {
    let spec = spec(&key(
        3,
        &json!([
            row("parent", "sha", "parent"),
            row("opened", "login", "pr"),
            { "type": "names", "from": ["sha"], "to": ["refs"], "unobservable": "no repo" }
        ]),
    ));
    let v0 = spec.oracle().expect("an oracle");
    let kinds = |m: &StreamMapping| -> Vec<String> {
        m.relationships.iter().map(|r| r.kind.clone()).collect()
    };
    assert_eq!(kinds(&v0), ["parent"]);
    let with = spec.oracle_with_links().expect("an oracle with links");
    assert_eq!(kinds(&with), ["parent", "opened"]);
    let payloads = [
        json!({ "user": "ann", "login": "a1" }),
        // `login` is an alias of `user`: the key places it only beside a `user` value.
        json!({ "sha": "c2", "parent": "c1", "user": "ann", "login": "a1", "repo": "r", "pr": 7 }),
    ];
    let found = gold(&spec, &payloads);
    assert_eq!(found.edges.len(), 2);
    assert_eq!(predicted(&with, &payloads).edges, found.edges);
    let reached = predicted(&v0, &payloads).edges;
    assert_eq!(reached.len(), 1);
    assert!(reached.is_subset(&found.edges));
}
