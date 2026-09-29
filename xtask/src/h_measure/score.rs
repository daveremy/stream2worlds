//! The identity scorer (contract B3): B-cubed over mentions (Pradhan et al. 2014 §4.2) on the
//! unmodified key and predicted partitions, the mention-weighted false-merge rate, and entity
//! recovery. Generic over streams: types are the key's labels, paths are mention path ids.
//!
//! Every ratio is built from integer counts over the contingency table (how many mentions each
//! key cluster shares with each predicted cluster), in one pass over the mentions; sums are taken in
//! `BTreeMap` order, so two runs print the same digits. A zero denominator is `None`, reported
//! as undefined (contract B3 "Degenerate outputs").

use std::collections::{BTreeMap, BTreeSet};

use s2w_model::NaturalKey;
use serde::Serialize;

use super::mentions::{Mention, Partition};

/// B-cubed precision, recall and F1 over some set of mentions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub(crate) struct Bcubed {
    /// Mean over predicted mentions of `|R(m) ∩ K(m)| / |R(m)|`; a spurious mention scores 0.
    pub precision: Option<f64>,
    /// Mean over key mentions of `|K(m) ∩ R(m)| / |K(m)|`; a missed mention scores 0.
    pub recall: Option<f64>,
    /// `2PR / (P + R)`.
    pub f1: Option<f64>,
}

/// One key mention path's row: where the mapping loses mentions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub(crate) struct PathRow {
    /// Key mentions at this path.
    pub key: usize,
    /// Of those, how many the mapping predicts at all.
    pub predicted: usize,
    /// B-cubed recall over this path's key mentions.
    pub recall: Option<f64>,
}

/// A mapping's identity score against a key.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct Score {
    /// Micro over every key and predicted mention, singletons included (the headline).
    pub micro: Bcubed,
    /// `1 − micro precision`. Mention-weighted: not a count of merged entities.
    pub false_merge: Option<f64>,
    /// The share of key entities with at least two mentions that some predicted cluster
    /// recovers (holds ≥ 90% of the entity, and the entity holds ≥ 90% of the cluster).
    pub recovery: Option<f64>,
    /// Key entities with at least two mentions: recovery's denominator.
    pub repeated: usize,
    /// Key types whose every entity has one mention. A mapping that rightly declines to mint
    /// them scores 0 recall on them, so the report shows the micro row without them too.
    pub singleton_types: BTreeSet<String>,
    /// Micro with the singleton-only types' key mentions, and the predicted mentions on them,
    /// left out. Spurious mentions still count.
    pub without_singleton_types: Bcubed,
    /// Per key type: recall over its key mentions, precision over the predicted mentions whose
    /// key twin has the type, with predicted clusters sized over the whole prediction.
    pub per_type: BTreeMap<String, Bcubed>,
    /// Per key mention path id.
    pub per_path: BTreeMap<String, PathRow>,
    /// Spurious predicted mentions (no key twin), per path id.
    pub spurious: BTreeMap<String, usize>,
}

/// Running sums for one B-cubed row.
#[derive(Clone, Copy, Debug, Default)]
struct Sums {
    precision: f64,
    predicted: usize,
    recall: f64,
    key: usize,
}

impl Sums {
    fn add_recall(&mut self, share: f64) {
        self.recall += share;
        self.key += 1;
    }

    fn add_precision(&mut self, share: f64) {
        self.precision += share;
        self.predicted += 1;
    }

    /// F1 is undefined when `P + R = 0` as well: the contract reports every zero denominator
    /// as undefined, where some coreference scorers print 0.
    fn bcubed(self) -> Bcubed {
        let precision = mean(self.precision, self.predicted);
        let recall = mean(self.recall, self.key);
        let f1 = match (precision, recall) {
            (Some(p), Some(r)) if p + r > 0.0 => Some(2.0 * p * r / (p + r)),
            _ => None,
        };
        Bcubed {
            precision,
            recall,
            f1,
        }
    }
}

fn mean(sum: f64, count: usize) -> Option<f64> {
    (count > 0).then(|| sum / count as f64)
}

/// Running sums for one per-path row.
#[derive(Clone, Copy, Debug, Default)]
struct PathSums {
    key: usize,
    predicted: usize,
    recall: f64,
}

/// The type a gold cluster belongs to: its natural key's label. A cluster that is not a
/// natural key (hand-built fixtures) is its own type.
fn type_of(cluster: &str) -> String {
    NaturalKey::new(cluster)
        .parts()
        .map_or_else(|_| cluster.to_owned(), |(label, _)| label.to_owned())
}

fn sizes(clusters: impl Iterator<Item = impl AsRef<str>>) -> BTreeMap<String, usize> {
    let mut sizes = BTreeMap::new();
    for cluster in clusters {
        *sizes.entry(cluster.as_ref().to_owned()).or_default() += 1;
    }
    sizes
}

/// The counts every row is built from, borrowing both partitions.
struct Tables<'a> {
    key: &'a Partition,
    /// The prediction without unscored paths.
    predicted: BTreeMap<&'a Mention, &'a str>,
    key_size: BTreeMap<String, usize>,
    predicted_size: BTreeMap<String, usize>,
    /// Each gold cluster's type, read once per cluster.
    kind: BTreeMap<&'a str, String>,
    /// Mentions each (key cluster, predicted cluster) pair shares.
    overlap: BTreeMap<(&'a str, &'a str), usize>,
    singleton_types: BTreeSet<String>,
}

impl<'a> Tables<'a> {
    fn new(key: &'a Partition, predicted: &'a Partition, unscored: &BTreeSet<String>) -> Self {
        let predicted: BTreeMap<&Mention, &str> = predicted
            .cluster
            .iter()
            .filter(|((_, path), _)| !unscored.contains(path))
            .map(|(mention, cluster)| (mention, cluster.as_str()))
            .collect();
        let mut overlap = BTreeMap::new();
        for (mention, gold) in &key.cluster {
            if let Some(guess) = predicted.get(mention) {
                *overlap.entry((gold.as_str(), *guess)).or_default() += 1;
            }
        }
        let key_size = sizes(key.cluster.values());
        let clusters: BTreeSet<&str> = key.cluster.values().map(String::as_str).collect();
        let kind: BTreeMap<&str, String> = clusters.into_iter().map(|c| (c, type_of(c))).collect();
        let mut singleton_types: BTreeSet<String> = kind.values().cloned().collect();
        for (cluster, size) in &key_size {
            if *size > 1 {
                singleton_types.remove(&kind[cluster.as_str()]);
            }
        }
        Self {
            key,
            predicted_size: sizes(predicted.values()),
            predicted,
            key_size,
            kind,
            overlap,
            singleton_types,
        }
    }

    /// Mentions `gold` and `guess` share.
    fn shared(&self, gold: &'a str, guess: &'a str) -> usize {
        self.overlap
            .get(&(gold, guess))
            .copied()
            .unwrap_or_default()
    }
}

/// Scores `predicted` against `key`. Predicted mentions at an `unscored` path id are dropped
/// first (the key never holds one).
pub(crate) fn score(key: &Partition, predicted: &Partition, unscored: &BTreeSet<String>) -> Score {
    let tables = Tables::new(key, predicted, unscored);
    let (mut micro, mut without) = (Sums::default(), Sums::default());
    let mut per_type: BTreeMap<&str, Sums> = BTreeMap::new();
    let mut per_path: BTreeMap<&str, PathSums> = BTreeMap::new();
    for (mention, gold) in &key.cluster {
        let guess = tables.predicted.get(mention);
        let shared = guess.map_or(0, |g| tables.shared(gold, g));
        let share = shared as f64 / tables.key_size[gold] as f64;
        let kind = tables.kind[gold.as_str()].as_str();
        let row = per_path.entry(mention.1.as_str()).or_default();
        row.key += 1;
        row.predicted += usize::from(guess.is_some());
        row.recall += share;
        micro.add_recall(share);
        per_type.entry(kind).or_default().add_recall(share);
        if !tables.singleton_types.contains(kind) {
            without.add_recall(share);
        }
    }
    let spurious = precision(&tables, &mut micro, &mut without, &mut per_type);
    let (recovered, repeated) = recovery(&tables);
    let micro = micro.bcubed();
    Score {
        micro,
        false_merge: micro.precision.map(|p| 1.0 - p),
        recovery: mean(recovered as f64, repeated),
        repeated,
        without_singleton_types: without.bcubed(),
        per_type: per_type
            .into_iter()
            .map(|(t, s)| (t.to_owned(), s.bcubed()))
            .collect(),
        per_path: per_path
            .into_iter()
            .map(|(path, row)| {
                let recall = mean(row.recall, row.key);
                let (key, predicted) = (row.key, row.predicted);
                (
                    path.to_owned(),
                    PathRow {
                        key,
                        predicted,
                        recall,
                    },
                )
            })
            .collect(),
        spurious,
        singleton_types: tables.singleton_types,
    }
}

/// Adds every predicted mention to the precision sums; returns the spurious ones per path id.
fn precision<'a>(
    tables: &'a Tables<'a>,
    micro: &mut Sums,
    without: &mut Sums,
    per_type: &mut BTreeMap<&'a str, Sums>,
) -> BTreeMap<String, usize> {
    let mut spurious: BTreeMap<String, usize> = BTreeMap::new();
    for (mention, guess) in &tables.predicted {
        let Some(gold) = tables.key.cluster.get(*mention) else {
            *spurious.entry(mention.1.clone()).or_default() += 1;
            micro.add_precision(0.0);
            without.add_precision(0.0);
            continue;
        };
        let share = tables.shared(gold, guess) as f64 / tables.predicted_size[*guess] as f64;
        let kind = tables.kind[gold.as_str()].as_str();
        micro.add_precision(share);
        per_type.entry(kind).or_default().add_precision(share);
        if !tables.singleton_types.contains(kind) {
            without.add_precision(share);
        }
    }
    spurious
}

/// `(recovered, repeated)`: key entities with at least two mentions, and how many of them some
/// predicted cluster holds at ≥ 90% both ways, compared in integers (`10·n ≥ 9·size`).
fn recovery(tables: &Tables<'_>) -> (usize, usize) {
    let recovered: BTreeSet<&str> = tables
        .overlap
        .iter()
        .filter(|((gold, guess), shared)| {
            let (entity, cluster) = (tables.key_size[*gold], tables.predicted_size[*guess]);
            entity > 1 && 10 * **shared >= 9 * entity && 10 * **shared >= 9 * cluster
        })
        .map(|((gold, _), _)| *gold)
        .collect();
    let repeated = tables.key_size.values().filter(|size| **size > 1).count();
    (recovered.len(), repeated)
}

/// The contract's frozen fixtures (B3), as `(name, key, prediction)`: the 4/9 case (key
/// `{a, b, c}`, prediction `{a, b, d}`) and an all-singletons prediction of that key.
pub(crate) fn frozen_fixtures() -> [(&'static str, Partition, Partition); 2] {
    let partition = |pairs: &[(&str, &str)]| Partition {
        cluster: pairs
            .iter()
            .map(|(path, cluster)| ((0, (*path).to_owned()), (*cluster).to_owned()))
            .collect(),
    };
    let key = partition(&[("a", "E"), ("b", "E"), ("c", "E")]);
    [
        (
            "4/9",
            key.clone(),
            partition(&[("a", "X"), ("b", "X"), ("d", "X")]),
        ),
        (
            "all-singletons",
            key,
            partition(&[("a", "1"), ("b", "2"), ("c", "3")]),
        ),
    ]
}

/// A metric as printed: four decimals, or "undefined" for a zero denominator.
pub(crate) fn shown(metric: Option<f64>) -> String {
    metric.map_or_else(|| "undefined".to_owned(), |x| format!("{x:.4}"))
}

#[cfg(test)]
mod tests;
