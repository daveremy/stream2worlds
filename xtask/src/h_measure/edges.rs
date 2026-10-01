//! The relationship scorer (contract B3 "Relationships"): precision and recall over unique,
//! typed, directed edges between entities.
//!
//! A predicted cluster stands for the key entity holding strictly more than half of its scored
//! mentions ([`super::score::majority`]); a cluster with no such entity stands for nothing, so
//! an edge touching it is false. A predicted edge's type is `(type of the from cluster, type of
//! the to cluster, relationship kind)`, and predicted types are aligned one-to-one with key
//! edge types by a maximum-weight assignment ([`hungarian`]). Under that alignment a key edge
//! hit by at least one predicted edge is one true positive, each further predicted edge on it is
//! a false positive (an entity split across clusters), every other predicted edge is a false
//! positive and every key edge never hit is missed. Every set and map is a `BTree*`, so two runs
//! print the same digits; a zero denominator is `None` (undefined), as in [`super::score`].

mod hungarian;

use std::collections::{BTreeMap, BTreeSet};

use s2w_model::FieldPath;
use serde::Serialize;

use super::key::{KeySpec, Unscored};
use super::mentions::{Edge, KeyMentions, Partition};
use super::score::{majority, type_of};

/// Precision, recall and F1 over edge counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub(crate) struct Prf {
    /// `TP / (TP + FP)`.
    pub precision: Option<f64>,
    /// `TP / (TP + FN)`.
    pub recall: Option<f64>,
    /// `2PR / (P + R)`; undefined when `P + R = 0`.
    pub f1: Option<f64>,
}

impl Prf {
    fn of(tp: usize, fp: usize, missed: usize) -> Self {
        let ratio = |den: usize| (den > 0).then(|| tp as f64 / den as f64);
        let (precision, recall) = (ratio(tp + fp), ratio(tp + missed));
        let f1 = match (precision, recall) {
            (Some(p), Some(r)) if p + r > 0.0 => Some(2.0 * p * r / (p + r)),
            _ => None,
        };
        Self {
            precision,
            recall,
            f1,
        }
    }
}

/// One key edge type's row.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct TypeRow {
    /// Unique key edges of this type.
    pub key_edges: usize,
    /// The predicted type aligned with it, if any.
    pub aligned: Option<String>,
    /// Key edges of this type hit by the aligned predicted type.
    pub tp: usize,
    /// Edges of the aligned predicted type that are not true positives.
    pub fp: usize,
    /// Key edges of this type never hit.
    #[serde(rename = "fn")]
    pub missed: usize,
    /// The row's precision, recall and F1.
    pub score: Prf,
}

/// A prediction's edge score against a key with relationships.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct EdgeScore {
    /// Micro over every scored edge.
    pub micro: Prf,
    /// Unique key edges.
    pub key_edges: usize,
    /// Predicted edges scored (`TP + FP`).
    pub predicted: usize,
    /// True positives.
    pub tp: usize,
    /// False positives.
    pub fp: usize,
    /// Missed key edges.
    #[serde(rename = "fn")]
    pub missed: usize,
    /// Per key edge type, every type the key declares as observable.
    pub per_type: BTreeMap<String, TypeRow>,
    /// Predicted edges of a type aligned with no key type (all false), per predicted type.
    pub unaligned_predicted: BTreeMap<String, usize>,
    /// Predicted edges with an endpoint cluster that has no strict-majority entity (all false).
    pub no_majority: usize,
    /// Predicted edges dropped because an endpoint cluster has no scored mention (every mention
    /// unscored or excluded): neither true nor false, like an unscored mention.
    pub dropped_unscored: usize,
    /// The key's unobservable relationship rows (declared, placing no edge).
    pub unobservable: usize,
    /// Predicted edges of an unaligned type whose mapped endpoints have an unobservable row's
    /// endpoint types: dropped, not false, as an unscored path drops a mention.
    pub dropped_unobservable: usize,
}

/// The key's side of an edge grade.
pub(crate) struct GoldEdges<'a> {
    /// The gold partition.
    pub partition: &'a Partition,
    /// The gold edges.
    pub edges: &'a BTreeSet<Edge>,
    /// The observable edge types the key declares, so a type with no edge in the corpus shows.
    pub declared: BTreeSet<String>,
    /// Each unobservable row's endpoint key types `(from, to)`, when both are mention paths.
    pub blind: BTreeSet<(String, String)>,
    /// The key's unobservable rows.
    pub unobservable: usize,
}

impl<'a> GoldEdges<'a> {
    /// The key's edges and declared rows; `None` for a key without relationships (format 2 or
    /// earlier), which has no edge score at all.
    pub(crate) fn of(spec: &KeySpec, gold: &'a KeyMentions) -> Option<Self> {
        if spec.relationships.is_empty() {
            return None;
        }
        let type_at = |path: &FieldPath| {
            spec.types.iter().find_map(|kind| {
                kind.mentions
                    .iter()
                    .any(|rule| &rule.path == path)
                    .then(|| kind.label.clone())
            })
        };
        let (observable, blind): (Vec<_>, Vec<_>) =
            spec.relationships.iter().partition(|r| r.observable());
        Some(Self {
            partition: &gold.partition,
            edges: &gold.edges,
            declared: observable.iter().map(|r| r.label.clone()).collect(),
            blind: blind
                .iter()
                .filter_map(|r| Some((type_at(&r.from)?, type_at(&r.to)?)))
                .collect(),
            unobservable: gold.unobservable,
        })
    }
}

/// One predicted edge, typed, with its endpoints mapped to key entities (`None`: an endpoint
/// has no strict-majority entity).
struct Typed<'a> {
    kind: String,
    mapped: Option<(&'a str, &'a str)>,
}

/// Scores `predicted` edges (clusters of `partition`) against `gold`. Mentions at paths
/// `unscored` covers are dropped first, as in [`super::score::score`].
pub(crate) fn score_edges(
    gold: &GoldEdges<'_>,
    partition: &Partition,
    predicted: &BTreeSet<Edge>,
    unscored: &Unscored,
) -> EdgeScore {
    let owner = majority(gold.partition, partition, unscored);
    let mut found = EdgeScore {
        unobservable: gold.unobservable,
        key_edges: gold.edges.len(),
        ..EdgeScore::default()
    };
    let mut typed = Vec::new();
    for edge in predicted {
        let (Some(from), Some(to)) = (owner.get(&edge.from), owner.get(&edge.to)) else {
            found.dropped_unscored += 1;
            continue;
        };
        let mapped = from.as_deref().zip(to.as_deref());
        found.no_majority += usize::from(mapped.is_none());
        let kind = format!(
            "{} → {}, {}",
            type_of(&edge.from),
            type_of(&edge.to),
            edge.label
        );
        typed.push(Typed { kind, mapped });
    }
    let aligned = align(gold, &typed);
    count(gold, &typed, &aligned, &mut found);
    found
}

/// The predicted type aligned with each key type, by maximum-weight assignment of how many
/// distinct key edges of each key type each predicted type hits.
fn align(gold: &GoldEdges<'_>, typed: &[Typed<'_>]) -> BTreeMap<String, String> {
    let keys: Vec<&str> = key_types(gold).into_iter().collect();
    let guesses: Vec<&str> = typed
        .iter()
        .map(|t| t.kind.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut hits: BTreeSet<(usize, usize, &str, &str)> = BTreeSet::new();
    for t in typed {
        let Some((from, to)) = t.mapped else { continue };
        let Ok(p) = guesses.binary_search(&t.kind.as_str()) else {
            continue;
        };
        for (k, label) in keys.iter().enumerate() {
            if gold.edges.contains(&edge(label, from, to)) {
                hits.insert((k, p, from, to));
            }
        }
    }
    let mut weights = vec![vec![0usize; guesses.len()]; keys.len()];
    for (k, p, _, _) in hits {
        weights[k][p] += 1;
    }
    hungarian::assign(&weights)
        .into_iter()
        .map(|(k, p)| (guesses[p].to_owned(), keys[k].to_owned()))
        .collect()
}

/// Every key edge type: the declared observable ones and any a gold edge carries.
fn key_types<'a>(gold: &'a GoldEdges<'_>) -> BTreeSet<&'a str> {
    let declared = gold.declared.iter().map(String::as_str);
    declared
        .chain(gold.edges.iter().map(|e| e.label.as_str()))
        .collect()
}

fn edge(label: &str, from: &str, to: &str) -> Edge {
    Edge {
        label: label.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
    }
}

/// Counts true and false positives per aligned key type, unaligned and dropped edges, and the
/// misses, into `found`. `aligned` maps a predicted type to its key type.
fn count(
    gold: &GoldEdges<'_>,
    typed: &[Typed<'_>],
    aligned: &BTreeMap<String, String>,
    found: &mut EdgeScore,
) {
    let mut rows: BTreeMap<String, TypeRow> = key_types(gold)
        .into_iter()
        .map(|k| (k.to_owned(), TypeRow::default()))
        .collect();
    for e in gold.edges {
        rows.entry(e.label.clone()).or_default().key_edges += 1;
    }
    for (guess, key) in aligned {
        rows.entry(key.clone()).or_default().aligned = Some(guess.clone());
    }
    let mut hit: BTreeSet<Edge> = BTreeSet::new();
    for t in typed {
        let Some(key) = aligned.get(&t.kind) else {
            if t.mapped.is_some_and(|(f, to)| blind(gold, f, to)) {
                found.dropped_unobservable += 1;
            } else {
                *found.unaligned_predicted.entry(t.kind.clone()).or_default() += 1;
            }
            continue;
        };
        let row = rows.entry(key.clone()).or_default();
        let target = t.mapped.map(|(f, to)| edge(key, f, to));
        // A key edge's first hit is its true positive; a second hit (a split entity) is false.
        if target
            .filter(|e| gold.edges.contains(e))
            .is_some_and(|e| hit.insert(e))
        {
            row.tp += 1;
        } else {
            row.fp += 1;
        }
    }
    for row in rows.values_mut() {
        row.missed = row.key_edges - row.tp;
        row.score = Prf::of(row.tp, row.fp, row.missed);
        found.tp += row.tp;
        found.fp += row.fp;
        found.missed += row.missed;
    }
    found.fp += found.unaligned_predicted.values().sum::<usize>();
    found.predicted = found.tp + found.fp;
    found.micro = Prf::of(found.tp, found.fp, found.missed);
    found.per_type = rows;
}

/// Whether the key entities `from` and `to` have an unobservable row's endpoint types.
fn blind(gold: &GoldEdges<'_>, from: &str, to: &str) -> bool {
    gold.blind.contains(&(type_of(from), type_of(to)))
}

#[cfg(test)]
mod tests;
