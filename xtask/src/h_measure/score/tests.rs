//! Scorer fixtures (contract B3 "Reference scorer and fixtures", identity subset).

use s2w_model::StreamMapping;
use serde_json::{Value, json};

use super::super::grade::grade;
use super::super::key::{KeySpec, Unscored};
use super::super::mentions::Partition;
use super::{Score, frozen_fixtures, score};

/// A partition from `(record, path, cluster)` triples.
fn part(mentions: &[(usize, &str, &str)]) -> Partition {
    Partition {
        cluster: mentions
            .iter()
            .map(|(record, path, cluster)| ((*record, (*path).to_owned()), (*cluster).to_owned()))
            .collect(),
    }
}

fn put(partition: &mut Partition, record: usize, path: &str, cluster: &str) {
    partition
        .cluster
        .insert((record, path.to_owned()), cluster.to_owned());
}

fn graded(key: &Partition, predicted: &Partition) -> Score {
    score(key, predicted, &Unscored::default())
}

/// `got` is `num / den`, checked by cross-multiplying.
fn exactly(got: Option<f64>, num: f64, den: f64) {
    let got = got.expect("defined");
    assert!((got * den - num).abs() < 1e-12, "{got} is not {num}/{den}");
}

/// Two entities of type `T` in one record: `{a, b}` and `{c, d}`. Clusters are natural-key
/// shaped (`label` U+001F part), as the key executor writes them.
fn two_entities() -> Partition {
    part(&[
        (0, "a", "T\u{1f}1"),
        (0, "b", "T\u{1f}1"),
        (0, "c", "T\u{1f}2"),
        (0, "d", "T\u{1f}2"),
    ])
}

#[test]
fn the_four_ninths_case() {
    let [(_, key, prediction), _] = frozen_fixtures();
    let got = graded(&key, &prediction);
    exactly(got.micro.precision, 4.0, 9.0);
    exactly(got.micro.recall, 4.0, 9.0);
    exactly(got.micro.f1, 4.0, 9.0);
    exactly(got.false_merge, 5.0, 9.0);
    assert_eq!(got.recovery, Some(0.0));
    assert_eq!(got.per_path["c"].predicted, 0);
    assert_eq!(got.spurious, [("d".to_owned(), 1)].into());
}

#[test]
fn the_frozen_all_singletons_case_recovers_nothing() {
    let [_, (_, key, prediction)] = frozen_fixtures();
    let got = graded(&key, &prediction);
    assert_eq!(got.recovery, Some(0.0));
    assert_eq!(got.micro.precision, Some(1.0));
    exactly(got.micro.recall, 1.0, 3.0);
}

#[test]
fn all_singletons_recover_nothing() {
    let got = graded(
        &two_entities(),
        &part(&[(0, "a", "1"), (0, "b", "2"), (0, "c", "3"), (0, "d", "4")]),
    );
    assert_eq!(got.recovery, Some(0.0));
    assert_eq!(got.repeated, 2);
    assert_eq!(got.micro.precision, Some(1.0));
    exactly(got.micro.recall, 1.0, 2.0);
}

#[test]
fn a_perfect_prediction_scores_one_whatever_its_cluster_ids() {
    let got = graded(
        &two_entities(),
        &part(&[(0, "a", "q"), (0, "b", "q"), (0, "c", "p"), (0, "d", "p")]),
    );
    assert_eq!(got.micro.f1, Some(1.0));
    assert_eq!(got.recovery, Some(1.0));
    assert_eq!(got.false_merge, Some(0.0));
    assert_eq!(got.per_type["T"].f1, Some(1.0));
}

#[test]
fn a_spurious_type_lowers_precision_only() {
    let mut predicted = two_entities();
    put(&mut predicted, 0, "z", "S");
    let got = graded(&two_entities(), &predicted);
    exactly(got.micro.precision, 4.0, 5.0);
    assert_eq!(got.micro.recall, Some(1.0));
    assert_eq!(got.per_type["T"].precision, Some(1.0));
}

#[test]
fn an_omitted_type_lowers_recall_only() {
    let mut key = two_entities();
    put(&mut key, 0, "u", "U\u{1f}1");
    put(&mut key, 1, "u", "U\u{1f}1");
    let got = graded(&key, &two_entities());
    assert_eq!(got.micro.precision, Some(1.0));
    exactly(got.micro.recall, 4.0, 6.0);
    assert_eq!(got.per_type["U"].recall, Some(0.0));
    assert_eq!(got.per_type["U"].precision, None);
    exactly(got.recovery, 2.0, 3.0);
    assert_eq!(got.per_path["u"].predicted, 0);
}

#[test]
fn a_false_merge_lowers_precision_whatever_the_cluster_is_called() {
    for name in ["T\u{1f}1", "T\u{1f}2", "anything"] {
        let merged = part(&[
            (0, "a", name),
            (0, "b", name),
            (0, "c", name),
            (0, "d", name),
        ]);
        let got = graded(&two_entities(), &merged);
        exactly(got.micro.precision, 1.0, 2.0);
        assert_eq!(got.micro.recall, Some(1.0));
        exactly(got.false_merge, 1.0, 2.0);
        assert_eq!(got.recovery, Some(0.0), "purity 50% is not a recovery");
    }
}

/// An entity of `size` mentions, a cluster holding `held` of them plus `extra` others.
fn recovered(size: usize, held: usize, extra: usize) -> Option<f64> {
    let (mut key, mut predicted) = (Partition::default(), Partition::default());
    for i in 0..size {
        put(&mut key, i, "a", "E");
        let cluster = if i < held {
            "C".to_owned()
        } else {
            format!("rest{i}")
        };
        put(&mut predicted, i, "a", &cluster);
    }
    for i in size..size + extra {
        put(&mut key, i, "a", &format!("F{i}"));
        put(&mut predicted, i, "a", "C");
    }
    graded(&key, &predicted).recovery
}

#[test]
fn recovery_needs_ninety_percent_both_ways() {
    // 9 of 10 held, pure cluster: recovered. 89 of 100: not.
    assert_eq!(recovered(10, 9, 0), Some(1.0));
    assert_eq!(recovered(100, 89, 0), Some(0.0));
    // 90 held, cluster 90/100 pure: recovered; 89/100 pure (11 extra): not.
    assert_eq!(recovered(90, 90, 10), Some(1.0));
    assert_eq!(recovered(89, 89, 11), Some(0.0));
}

#[test]
fn an_empty_prediction_has_undefined_precision_and_zero_recall() {
    let got = graded(&two_entities(), &Partition::default());
    assert_eq!(got.micro.precision, None);
    assert_eq!(got.micro.recall, Some(0.0));
    assert_eq!(got.micro.f1, None);
    assert_eq!(got.false_merge, None);
    assert_eq!(got.recovery, Some(0.0));
}

#[test]
fn an_empty_key_leaves_recall_and_recovery_undefined() {
    let got = graded(&Partition::default(), &two_entities());
    assert_eq!(got.micro.recall, None);
    assert_eq!(got.micro.precision, Some(0.0));
    assert_eq!(got.recovery, None);
}

#[test]
fn a_singleton_only_type_gets_its_own_row() {
    let mut key = two_entities();
    put(&mut key, 0, "id", "I\u{1f}1");
    put(&mut key, 1, "id", "I\u{1f}2");
    let got = graded(&key, &two_entities());
    assert_eq!(got.singleton_types, ["I".to_owned()].into());
    exactly(got.micro.recall, 4.0, 6.0);
    assert_eq!(got.without_singleton_types.f1, Some(1.0));
}

#[test]
fn an_unscored_path_is_dropped_from_the_prediction() {
    let mut predicted = two_entities();
    put(&mut predicted, 0, "z", "S");
    let got = score(
        &two_entities(),
        &predicted,
        &Unscored::exact(["z".to_owned()]),
    );
    assert_eq!(got.micro.precision, Some(1.0));
    assert!(got.spurious.is_empty());
}

fn spec(value: &Value) -> KeySpec {
    serde_json::from_value(value.clone()).expect("the spec deserializes")
}

fn mapping(entities: &Value) -> StreamMapping {
    serde_json::from_value(json!({
        "version": 1, "decode": [], "entities": entities, "relationships": []
    }))
    .expect("the mapping deserializes")
}

#[test]
fn a_prediction_whose_key_twin_abstained_is_spurious() {
    let key = spec(
        &json!({ "version": 0, "types": [{ "type": "T", "mentions": [
        { "path": ["a"], "identity": [["ctx"], ["a"]] }
    ] }] }),
    );
    let rules = mapping(&json!([{ "id": "r", "type_label": "T", "key": [["a"]], "attrs": [] }]));
    let payloads = [
        json!({ "a": "x", "ctx": {} }),
        json!({ "a": "x", "ctx": "c" }),
    ];
    let got = grade(&key, &rules, &payloads).expect("grades");
    assert_eq!(got.abstained, [("a".to_owned(), 1)].into());
    assert_eq!(got.mapping.spurious, [("a".to_owned(), 1)].into());
    exactly(got.mapping.micro.precision, 1.0, 4.0);
}

#[test]
fn the_oracle_of_a_key_whose_mentions_have_their_own_identities_scores_one() {
    let key = spec(&json!({ "version": 0, "types": [
        { "type": "S", "mentions": [{ "path": ["ctx"], "identity": [["ctx"]] }] },
        { "type": "O", "mentions": [
            { "path": ["id"], "identity": [["id"], ["ctx"]] },
            { "path": ["old"], "identity": [["old"]] },
            { "path": ["new"], "identity": [["new"]] }
        ] }
    ] }));
    let payloads = [
        json!({ "ctx": "c", "id": 1, "old": 5, "new": 6 }),
        json!({ "ctx": "c", "id": 1, "old": 6, "new": 7 }),
        json!({ "ctx": "d", "id": 1 }),
    ];
    let empty = mapping(&json!([]));
    let got = grade(&key, &empty, &payloads).expect("grades");
    assert_eq!(got.ceiling.micro.f1, Some(1.0), "{:?}", got.ceiling);
    assert_eq!(got.ceiling.recovery, Some(1.0));
    assert_eq!(got.mapping.micro.precision, None);
}

#[test]
fn the_oracle_cannot_join_four_aliases() {
    let alias = |path: &str| json!({ "path": [path], "identity": [["w"]] });
    let key = spec(
        &json!({ "version": 0, "types": [{ "type": "W", "mentions": [
        { "path": ["w"], "identity": [["w"]] }, alias("x"), alias("y"), alias("z")
    ] }] }),
    );
    let payloads = [json!({ "w": "en", "x": "en.w", "y": "enwiki", "z": "https://en" })];
    let got = grade(&key, &mapping(&json!([])), &payloads).expect("grades");
    exactly(got.ceiling.micro.recall, 1.0, 16.0);
    assert_eq!(got.ceiling.micro.precision, Some(1.0));
    assert_eq!(got.ceiling.recovery, Some(0.0));
}

#[test]
fn the_oracle_splits_two_mentions_of_one_multi_path_identity() {
    let key = spec(
        &json!({ "version": 0, "types": [{ "type": "T", "mentions": [
        { "path": ["a"], "identity": [["a"], ["b"]] },
        { "path": ["b"], "identity": [["a"], ["b"]] }
    ] }] }),
    );
    let got = grade(&key, &mapping(&json!([])), &[json!({ "a": "x", "b": "y" })]).expect("grades");
    exactly(got.ceiling.micro.recall, 1.0, 2.0);
    assert_eq!(got.ceiling.recovery, Some(0.0));
}

#[test]
fn excluded_mentions_are_dropped_from_the_mapping_and_the_oracle_alike() {
    let key = spec(
        &json!({ "version": 1, "types": [{ "type": "T", "mentions": [
        { "path": ["n"], "identity": [["ctx"], ["n"]], "no_identity": [0] }
    ] }] }),
    );
    let payloads = [
        json!({ "ctx": "c", "n": 0 }),
        json!({ "ctx": "c", "n": 0 }),
        json!({ "ctx": "c", "n": 7 }),
        json!({ "ctx": "c", "n": 7 }),
    ];
    // The v0 format cannot exclude a value, so this mapping mints the sentinel entity.
    let naive = mapping(&json!([
        { "id": "t", "type_label": "T", "key": [["ctx"], ["n"]], "attrs": [] }
    ]));
    let got = grade(&key, &naive, &payloads).expect("grades");
    assert_eq!(got.excluded, [("n".to_owned(), 2)].into());
    // The key has no mention at the sentinel records, so both predictions drop them: neither
    // spurious nor abstained, and the mapping reaches the ceiling.
    for score in [&got.ceiling, &got.mapping] {
        assert_eq!(score.micro.f1, Some(1.0), "{score:?}");
        assert!(score.spurious.is_empty(), "{score:?}");
    }
    assert_eq!(got.mapping, got.ceiling);
}
