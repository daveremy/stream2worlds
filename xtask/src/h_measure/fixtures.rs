//! The contract's eight frozen scorer fixtures (B3 "Reference scorer and fixtures"), in the
//! contract's order, each with the property it must show. `cargo xtask h-measure selftest`
//! scores and checks every one and prints the table ([`table`]).

use std::collections::BTreeSet;

use super::edges::{EdgeScore, GoldEdges, score_edges};
use super::key::Unscored;
use super::mentions::{Edge, Partition};
use super::score::{Score, score, shown};

/// One fixture: a key and a prediction, their edges (empty for the five identity fixtures), and
/// the property the scores must hold.
pub(crate) struct Fixture {
    /// The contract's name for it.
    pub name: &'static str,
    /// The key partition.
    pub key: Partition,
    /// The predicted partition.
    pub predicted: Partition,
    /// Key edges; non-empty marks an edge fixture.
    pub key_edges: BTreeSet<Edge>,
    /// Predicted edges.
    pub predicted_edges: BTreeSet<Edge>,
    /// Whether the scores show the fixture's property.
    pub holds: fn(&Score, &EdgeScore) -> bool,
}

impl Fixture {
    /// The identity score and the edge score.
    pub(crate) fn scored(&self) -> (Score, EdgeScore) {
        let none = Unscored::default();
        let gold = GoldEdges {
            partition: &self.key,
            edges: &self.key_edges,
            declared: BTreeSet::new(),
            blind: BTreeSet::new(),
            unobservable: 0,
        };
        let edges = score_edges(&gold, &self.predicted, &self.predicted_edges, &none);
        (score(&self.key, &self.predicted, &none), edges)
    }
}

/// Mentions at record 0, `(path, cluster)`.
pub(crate) fn partition(pairs: &[(&str, &str)]) -> Partition {
    Partition {
        cluster: pairs
            .iter()
            .map(|(path, cluster)| ((0, (*path).to_owned()), (*cluster).to_owned()))
            .collect(),
    }
}

/// Edges `(label, from, to)`.
pub(crate) fn edges(rows: &[(&str, &str, &str)]) -> BTreeSet<Edge> {
    rows.iter()
        .map(|(label, from, to)| Edge {
            label: (*label).to_owned(),
            from: (*from).to_owned(),
            to: (*to).to_owned(),
        })
        .collect()
}

fn near(got: Option<f64>, want: f64) -> bool {
    got.is_some_and(|x| (x - want).abs() < 1e-12)
}

fn identity(
    name: &'static str,
    key: Partition,
    predicted: Partition,
    holds: fn(&Score, &EdgeScore) -> bool,
) -> Fixture {
    Fixture {
        name,
        key,
        predicted,
        key_edges: BTreeSet::new(),
        predicted_edges: BTreeSet::new(),
        holds,
    }
}

/// Two entities of type T, `{a, b}` and `{c, d}`.
fn two_entities() -> Partition {
    partition(&[
        ("a", "T\u{1f}1"),
        ("b", "T\u{1f}1"),
        ("c", "T\u{1f}2"),
        ("d", "T\u{1f}2"),
    ])
}

fn identity_fixtures() -> [Fixture; 5] {
    let mut spurious = two_entities();
    spurious
        .cluster
        .insert((0, "z".to_owned()), "S\u{1f}1".to_owned());
    let mut omitted = two_entities();
    omitted
        .cluster
        .insert((0, "u".to_owned()), "U\u{1f}1".to_owned());
    omitted
        .cluster
        .insert((1, "u".to_owned()), "U\u{1f}1".to_owned());
    let merged = partition(&[("a", "X"), ("b", "X"), ("c", "X"), ("d", "X")]);
    let abc = partition(&[("a", "E"), ("b", "E"), ("c", "E")]);
    [
        identity("spurious type", two_entities(), spurious, |s, _| {
            near(s.micro.precision, 0.8) && s.micro.recall == Some(1.0)
        }),
        identity("omitted type", omitted, two_entities(), |s, _| {
            s.micro.precision == Some(1.0) && near(s.micro.recall, 4.0 / 6.0)
        }),
        identity("false merge", two_entities(), merged, |s, _| {
            near(s.micro.precision, 0.5) && s.micro.recall == Some(1.0)
        }),
        identity(
            "all-singletons",
            abc.clone(),
            partition(&[("a", "1"), ("b", "2"), ("c", "3")]),
            |s, _| s.recovery == Some(0.0),
        ),
        identity(
            "4/9",
            abc,
            partition(&[("a", "X"), ("b", "X"), ("d", "X")]),
            |s, _| {
                let ninths = |got: Option<f64>, n: f64| near(got, n / 9.0);
                let b = s.micro;
                ninths(b.precision, 4.0) && ninths(b.recall, 4.0) && ninths(b.f1, 4.0)
            },
        ),
    ]
}

fn edge_fixtures() -> [Fixture; 3] {
    // Key `{a, b}` ∪ `{c, d}`, predicted as one cluster split 2–2: no strict majority.
    let mut tied_key = two_entities();
    tied_key
        .cluster
        .insert((0, "t".to_owned()), "U\u{1f}1".to_owned());
    let tied = partition(&[
        ("a", "T\u{1f}7"),
        ("b", "T\u{1f}7"),
        ("c", "T\u{1f}7"),
        ("d", "T\u{1f}7"),
        ("t", "U\u{1f}8"),
    ]);
    // Nine mentions of T1 and one of T2 in one predicted cluster.
    let paths: Vec<String> = (0..10).map(|i| format!("m{i}")).collect();
    let mut nine_key = partition(&[("t", "U\u{1f}1")]);
    let mut nine = partition(&[("t", "U\u{1f}8")]);
    for (i, path) in paths.iter().enumerate() {
        let gold = if i < 9 { "T\u{1f}1" } else { "T\u{1f}2" };
        nine_key.cluster.insert((0, path.clone()), gold.to_owned());
        nine.cluster
            .insert((0, path.clone()), "T\u{1f}7".to_owned());
    }
    // One entity split across two clusters, each with an edge to the same target.
    let split_key = partition(&[("a", "T\u{1f}1"), ("b", "T\u{1f}1"), ("t", "U\u{1f}1")]);
    let split = partition(&[("a", "T\u{1f}7"), ("b", "T\u{1f}9"), ("t", "U\u{1f}8")]);
    let key_edge = edges(&[("r", "T\u{1f}1", "U\u{1f}1")]);
    [
        Fixture {
            name: "edge, no majority",
            key: tied_key,
            predicted: tied,
            key_edges: key_edge.clone(),
            predicted_edges: edges(&[("r", "T\u{1f}7", "U\u{1f}8")]),
            holds: |_, e| e.micro.precision == Some(0.0) && e.no_majority == 1,
        },
        Fixture {
            name: "edge, 9-to-1",
            key: nine_key,
            predicted: nine,
            key_edges: key_edge.clone(),
            predicted_edges: edges(&[("r", "T\u{1f}7", "U\u{1f}8")]),
            holds: |_, e| e.tp == 1 && e.micro.precision == Some(1.0),
        },
        Fixture {
            name: "edge, split",
            key: split_key,
            predicted: split,
            key_edges: key_edge,
            predicted_edges: edges(&[("r", "T\u{1f}7", "U\u{1f}8"), ("r", "T\u{1f}9", "U\u{1f}8")]),
            holds: |_, e| {
                (e.tp, e.fp) == (1, 1)
                    && near(e.micro.precision, 0.5)
                    && e.micro.recall == Some(1.0)
            },
        },
    ]
}

/// The contract's eight fixtures, in its order.
pub(crate) fn frozen_fixtures() -> Vec<Fixture> {
    identity_fixtures()
        .into_iter()
        .chain(edge_fixtures())
        .collect()
}

/// Every fixture scored and checked, as a printed table; an error names the first that fails.
pub(crate) fn table() -> Result<String, String> {
    let mut table =
        "fixture            P       R       F1      false-merge  recovery  edge P   edge R"
            .to_owned();
    for fixture in frozen_fixtures() {
        let (row, edge) = fixture.scored();
        if !(fixture.holds)(&row, &edge) {
            return Err(format!(
                "scorer: the {} fixture scores {row:?} and edges {edge:?}",
                fixture.name
            ));
        }
        let b = row.micro;
        table.push_str(&format!(
            "\n{:<18} {:<7} {:<7} {:<7} {:<12} {:<9} {:<8} {}",
            fixture.name,
            shown(b.precision),
            shown(b.recall),
            shown(b.f1),
            shown(row.false_merge),
            shown(row.recovery),
            shown(edge.micro.precision),
            shown(edge.micro.recall)
        ));
    }
    Ok(table)
}
