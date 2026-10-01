//! Edge scorer fixtures (contract B3 "Relationships"). The contract's three edge fixtures are in
//! [`crate::h_measure::fixtures`]; these pin the rest of the rule.

use std::collections::BTreeSet;

use s2w_model::StreamMapping;
use serde_json::{Value, json};

use super::{EdgeScore, GoldEdges, score_edges};
use crate::h_measure::fixtures::{edges, frozen_fixtures, partition};
use crate::h_measure::grade::grade;
use crate::h_measure::key::KeySpec;
use crate::h_measure::key::Unscored;
use crate::h_measure::mentions::{Edge, Partition};

/// Two commits (paths `a`, `b`) and an issue (path `i`): commit 2's parent is commit 1, and
/// commit 1 fixes the issue.
fn key() -> (Partition, BTreeSet<Edge>) {
    (
        partition(&[("a", "C\u{1f}1"), ("b", "C\u{1f}2"), ("i", "I\u{1f}1")]),
        edges(&[
            ("parent", "C\u{1f}2", "C\u{1f}1"),
            ("fixes", "C\u{1f}1", "I\u{1f}1"),
        ]),
    )
}

/// The same entities under other names.
fn renamed() -> Partition {
    partition(&[
        ("a", "commit\u{1f}7"),
        ("b", "commit\u{1f}8"),
        ("i", "ticket\u{1f}9"),
    ])
}

fn graded(predicted: &Partition, predicted_edges: &BTreeSet<Edge>) -> EdgeScore {
    let (partition, key_edges) = key();
    let gold = GoldEdges {
        partition: &partition,
        edges: &key_edges,
        declared: BTreeSet::new(),
        blind: BTreeSet::new(),
        unobservable: 0,
    };
    score_edges(&gold, predicted, predicted_edges, &Unscored::default())
}

#[test]
fn the_contract_edge_fixtures_hold() {
    let names: Vec<&str> = frozen_fixtures().iter().map(|f| f.name).collect();
    assert_eq!(
        names,
        [
            "spurious type",
            "omitted type",
            "false merge",
            "all-singletons",
            "4/9",
            "edge, no majority",
            "edge, 9-to-1",
            "edge, split",
        ]
    );
    for fixture in frozen_fixtures() {
        let (row, edge) = fixture.scored();
        assert!((fixture.holds)(&row, &edge), "{}: {edge:?}", fixture.name);
    }
}

#[test]
fn a_perfect_prediction_scores_one_whatever_its_cluster_and_kind_names() {
    let got = graded(
        &renamed(),
        &edges(&[
            ("p", "commit\u{1f}8", "commit\u{1f}7"),
            ("f", "commit\u{1f}7", "ticket\u{1f}9"),
        ]),
    );
    assert_eq!((got.tp, got.fp, got.missed), (2, 0, 0));
    assert_eq!(got.micro.f1, Some(1.0));
    assert_eq!(
        got.per_type["parent"].aligned.as_deref(),
        Some("commit → commit, p")
    );
}

#[test]
fn an_empty_prediction_has_undefined_precision_and_zero_recall() {
    let got = graded(&renamed(), &BTreeSet::new());
    assert_eq!(got.micro.precision, None);
    assert_eq!(got.micro.recall, Some(0.0));
    assert_eq!(got.micro.f1, None);
    assert_eq!(got.missed, 2);
}

#[test]
fn a_reversed_edge_is_one_false_positive_and_one_miss() {
    let got = graded(
        &renamed(),
        &edges(&[
            ("p", "commit\u{1f}7", "commit\u{1f}8"),
            ("f", "commit\u{1f}7", "ticket\u{1f}9"),
        ]),
    );
    // "p" hits no key edge, so it aligns with nothing: one false positive, one parent missed.
    assert_eq!((got.tp, got.fp, got.missed), (1, 1, 1));
    assert_eq!(got.unaligned_predicted["commit → commit, p"], 1);
    assert_eq!(got.per_type["parent"].missed, 1);
}

#[test]
fn an_unaligned_key_type_is_all_missed_and_aligned_types_score_independently() {
    let got = graded(
        &renamed(),
        &edges(&[("f", "commit\u{1f}7", "ticket\u{1f}9")]),
    );
    assert_eq!(got.per_type["fixes"].score.recall, Some(1.0));
    assert_eq!(got.per_type["parent"].aligned, None);
    assert_eq!(got.per_type["parent"].score.recall, Some(0.0));
    assert_eq!(got.micro.precision, Some(1.0));
    assert_eq!(got.micro.recall, Some(0.5));
}

#[test]
fn one_predicted_type_aligns_with_one_key_type_only() {
    // One kind between endpoints of different types is two predicted types.
    let got = graded(
        &renamed(),
        &edges(&[
            ("any", "commit\u{1f}8", "commit\u{1f}7"),
            ("any", "commit\u{1f}7", "ticket\u{1f}9"),
        ]),
    );
    assert_eq!((got.tp, got.fp), (2, 0));
    // Between endpoints of one type it is one predicted type, aligned with one key type only,
    // so the other key type's edge is false and its key edge missed.
    let mut same = renamed();
    same.cluster
        .insert((0, "i".to_owned()), "commit\u{1f}9".to_owned());
    let got = graded(
        &same,
        &edges(&[
            ("any", "commit\u{1f}8", "commit\u{1f}7"),
            ("any", "commit\u{1f}7", "commit\u{1f}9"),
        ]),
    );
    assert_eq!((got.tp, got.fp, got.missed), (1, 1, 1));
    // A tie between the two key types: "fixes" sorts before "parent".
    assert_eq!(
        got.per_type["fixes"].aligned.as_deref(),
        Some("commit → commit, any")
    );
}

#[test]
fn an_endpoint_with_no_scored_mention_is_dropped() {
    let unscored = Unscored::exact(["i".to_owned()]);
    let (partition, key_edges) = key();
    let gold = GoldEdges {
        partition: &partition,
        edges: &key_edges,
        declared: BTreeSet::new(),
        blind: BTreeSet::new(),
        unobservable: 0,
    };
    let predicted = edges(&[("f", "commit\u{1f}7", "ticket\u{1f}9")]);
    let got = score_edges(&gold, &renamed(), &predicted, &unscored);
    assert_eq!(got.dropped_unscored, 1);
    assert_eq!(got.predicted, 0);
}

#[test]
fn an_unaligned_edge_on_an_unobservable_row_is_dropped_and_counted() {
    let (partition, key_edges) = key();
    let gold = GoldEdges {
        partition: &partition,
        edges: &key_edges,
        declared: ["parent".to_owned(), "fixes".to_owned()].into(),
        blind: [("I".to_owned(), "C".to_owned())].into(),
        unobservable: 1,
    };
    let predicted = edges(&[
        ("closed-by", "ticket\u{1f}9", "commit\u{1f}7"),
        ("other", "commit\u{1f}7", "commit\u{1f}7"),
    ]);
    let got = score_edges(&gold, &renamed(), &predicted, &Unscored::default());
    assert_eq!(got.dropped_unobservable, 1);
    assert_eq!(got.unobservable, 1);
    // The commit-to-commit edge has no unobservable row: still false.
    assert_eq!(got.fp, 1);
}

#[test]
fn a_declared_type_with_no_edge_gets_an_empty_row() {
    let (partition, key_edges) = key();
    let gold = GoldEdges {
        partition: &partition,
        edges: &key_edges,
        declared: ["reviews".to_owned()].into(),
        blind: BTreeSet::new(),
        unobservable: 0,
    };
    let got = score_edges(&gold, &renamed(), &BTreeSet::new(), &Unscored::default());
    assert_eq!(got.per_type["reviews"].key_edges, 0);
    assert_eq!(got.per_type["reviews"].score.recall, None);
}

/// Format 3: commits at `sha`, users at `user` (`"nobody"` excluded), commit `by` user.
fn authored() -> KeySpec {
    serde_json::from_value(json!({
        "version": 3,
        "types": [
            { "type": "C", "mentions": [{ "path": ["sha"], "identity": [["sha"]] }] },
            { "type": "U", "mentions": [
                { "path": ["user"], "identity": [["user"]], "no_identity": ["nobody"] }
            ] }
        ],
        "relationships": [{ "type": "by", "from": ["sha"], "to": ["user"] }]
    }))
    .expect("the spec deserializes")
}

fn payloads() -> Vec<Value> {
    vec![
        json!({ "sha": "a", "user": "x" }),
        json!({ "sha": "b", "user": "nobody" }),
        json!({ "sha": "c", "user": "x" }),
    ]
}

#[test]
fn the_oracle_scores_one_on_edges_when_an_endpoint_is_excluded() {
    let key = authored();
    let got = grade(&key, &key.oracle().expect("oracle"), &payloads()).expect("grades");
    for edges in [&got.edges, &got.ceiling_edges, &got.ceiling_links_edges] {
        let edges = edges.as_ref().expect("a format-3 key has an edge score");
        assert_eq!(
            (edges.key_edges, edges.tp, edges.fp),
            (2, 2, 0),
            "{edges:?}"
        );
        // The edge to the excluded "nobody" lands on a cluster with no scored mention.
        assert_eq!(edges.dropped_unscored, 1);
        assert_eq!(edges.micro.f1, Some(1.0));
    }
}

#[test]
fn a_mapping_with_no_relationships_scores_zero_edge_recall() {
    let mapping: StreamMapping = serde_json::from_value(json!({
        "version": 1, "decode": [], "relationships": [], "entities": [
            { "id": "c", "type_label": "C", "key": [["sha"]], "attrs": [] },
            { "id": "u", "type_label": "U", "key": [["user"]], "attrs": [] }
        ]
    }))
    .expect("the mapping deserializes");
    let got = grade(&authored(), &mapping, &payloads()).expect("grades");
    let edges = got.edges.expect("an edge score");
    assert_eq!(edges.micro.recall, Some(0.0));
    assert_eq!(edges.micro.precision, None);
    assert_eq!(
        got.ceiling_edges.expect("a ceiling").micro.recall,
        Some(1.0)
    );
}

#[test]
fn a_key_without_relationships_has_no_edge_score() {
    let mut key = authored();
    key.relationships.clear();
    let got = grade(&key, &key.oracle().expect("oracle"), &payloads()).expect("grades");
    assert_eq!(
        (got.edges, got.ceiling_edges, got.ceiling_links_edges),
        (None, None, None)
    );
}
