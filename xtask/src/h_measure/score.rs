//! The identity scorer (contract B3): B-cubed over mentions (Pradhan et al. 2014 §4.2) on the
//! unmodified key and predicted partitions, the mention-weighted false-merge rate, and entity
//! recovery. Generic over streams: types are the key's labels, paths are mention path ids.
//!
//! Every ratio is built from integer counts over the contingency table (how many mentions each
//! key cluster shares with each predicted cluster), so a run is O(mentions); sums are taken in
//! `BTreeMap` order, so two runs print the same digits. A zero denominator is `None`, reported
//! as undefined (contract B3 "Degenerate outputs").

use std::collections::{BTreeMap, BTreeSet};

use s2w_discover::rule_id;
use s2w_model::{KEY_SEPARATOR, StreamMapping};
use serde_json::Value;

use super::key::KeySpec;
use super::mentions::{Decoded, Partition, key_mentions, mapping_mentions};

/// B-cubed precision, recall and F1 over some set of mentions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Bcubed {
    /// Mean over predicted mentions of `|R(m) ∩ K(m)| / |R(m)|`; a spurious mention scores 0.
    pub precision: Option<f64>,
    /// Mean over key mentions of `|K(m) ∩ R(m)| / |K(m)|`; a missed mention scores 0.
    pub recall: Option<f64>,
    /// `2PR / (P + R)`.
    pub f1: Option<f64>,
}

/// One key mention path's row: where the mapping loses mentions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PathRow {
    /// Key mentions at this path.
    pub key: usize,
    /// Of those, how many the mapping predicts at all.
    pub predicted: usize,
    /// B-cubed recall over this path's key mentions.
    pub recall: Option<f64>,
}

/// A mapping's identity score against a key.
#[derive(Clone, Debug, Default, PartialEq)]
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

/// The type a gold cluster belongs to: its natural key's label part.
fn type_of(cluster: &str) -> &str {
    cluster.split(KEY_SEPARATOR).next().unwrap_or_default()
}

fn sizes(partition: &Partition) -> BTreeMap<&str, usize> {
    let mut sizes = BTreeMap::new();
    for cluster in partition.cluster.values() {
        *sizes.entry(cluster.as_str()).or_default() += 1;
    }
    sizes
}

/// The counts every row is built from.
struct Tables<'a> {
    key: &'a Partition,
    predicted: Partition,
    key_size: BTreeMap<&'a str, usize>,
    predicted_size: BTreeMap<String, usize>,
    /// Mentions each (key cluster, predicted cluster) pair shares.
    overlap: BTreeMap<(&'a str, String), usize>,
    singleton_types: BTreeSet<String>,
}

impl<'a> Tables<'a> {
    fn new(key: &'a Partition, predicted: &Partition, unscored: &BTreeSet<String>) -> Self {
        let predicted = Partition {
            cluster: predicted
                .cluster
                .iter()
                .filter(|((_, path), _)| !unscored.contains(path))
                .map(|(mention, cluster)| (mention.clone(), cluster.clone()))
                .collect(),
        };
        let predicted_size = sizes(&predicted)
            .into_iter()
            .map(|(cluster, size)| (cluster.to_owned(), size))
            .collect();
        let mut overlap = BTreeMap::new();
        for (mention, gold) in &key.cluster {
            if let Some(guess) = predicted.cluster.get(mention) {
                *overlap.entry((gold.as_str(), guess.clone())).or_default() += 1;
            }
        }
        let key_size = sizes(key);
        let mut singleton_types: BTreeSet<String> =
            key_size.keys().map(|c| type_of(c).to_owned()).collect();
        for (cluster, size) in &key_size {
            if *size > 1 {
                singleton_types.remove(type_of(cluster));
            }
        }
        Self {
            key,
            predicted,
            key_size,
            predicted_size,
            overlap,
            singleton_types,
        }
    }

    /// Mentions `gold` and `guess` share.
    fn shared(&self, gold: &'a str, guess: &str) -> usize {
        self.overlap
            .get(&(gold, guess.to_owned()))
            .copied()
            .unwrap_or_default()
    }
}

/// Scores `predicted` against `key`. Predicted mentions at an `unscored` path id are dropped
/// first (the key never holds one).
pub(crate) fn score(key: &Partition, predicted: &Partition, unscored: &BTreeSet<String>) -> Score {
    let tables = Tables::new(key, predicted, unscored);
    let (mut micro, mut without) = (Sums::default(), Sums::default());
    let mut per_type: BTreeMap<String, Sums> = BTreeMap::new();
    let mut per_path: BTreeMap<String, (PathRow, f64)> = BTreeMap::new();
    for (mention, gold) in &key.cluster {
        let guess = tables.predicted.cluster.get(mention);
        let shared = guess.map_or(0, |g| tables.shared(gold, g));
        let share = shared as f64 / tables.key_size[gold.as_str()] as f64;
        let kind = type_of(gold);
        let row = per_path.entry(mention.1.clone()).or_default();
        row.0.key += 1;
        row.0.predicted += usize::from(guess.is_some());
        row.1 += share;
        let mut rows = vec![&mut micro, per_type.entry(kind.to_owned()).or_default()];
        if !tables.singleton_types.contains(kind) {
            rows.push(&mut without);
        }
        for sums in rows {
            sums.recall += share;
            sums.key += 1;
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
        per_type: per_type.into_iter().map(|(t, s)| (t, s.bcubed())).collect(),
        per_path: per_path
            .into_iter()
            .map(|(path, (mut row, sum))| {
                row.recall = mean(sum, row.key);
                (path, row)
            })
            .collect(),
        spurious,
        singleton_types: tables.singleton_types,
    }
}

/// Adds every predicted mention to the precision sums; returns the spurious ones per path id.
fn precision(
    tables: &Tables<'_>,
    micro: &mut Sums,
    without: &mut Sums,
    per_type: &mut BTreeMap<String, Sums>,
) -> BTreeMap<String, usize> {
    let mut spurious: BTreeMap<String, usize> = BTreeMap::new();
    for (mention, guess) in &tables.predicted.cluster {
        let (share, kind) = match tables.key.cluster.get(mention) {
            Some(gold) => (
                tables.shared(gold, guess) as f64 / tables.predicted_size[guess] as f64,
                Some(type_of(gold)),
            ),
            None => {
                *spurious.entry(mention.1.clone()).or_default() += 1;
                (0.0, None)
            }
        };
        let mut rows = vec![&mut *micro];
        if kind.is_none_or(|k| !tables.singleton_types.contains(k)) {
            rows.push(&mut *without);
        }
        if let Some(kind) = kind {
            rows.push(per_type.entry(kind.to_owned()).or_default());
        }
        for sums in rows {
            sums.precision += share;
            sums.predicted += 1;
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
            let (entity, cluster) = (tables.key_size[gold], tables.predicted_size[guess]);
            entity > 1 && 10 * **shared >= 9 * entity && 10 * **shared >= 9 * cluster
        })
        .map(|((gold, _), _)| *gold)
        .collect();
    let repeated = tables.key_size.values().filter(|size| **size > 1).count();
    (recovered.len(), repeated)
}

/// A mapping graded against a key on one corpus.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Grade {
    /// The mapping's score.
    pub mapping: Score,
    /// The oracle v0 mapping's score ([`KeySpec::oracle`]): the ceiling to read `mapping`
    /// against, since a v0 mapping cannot join aliases with different values.
    pub ceiling: Score,
    /// Abstained paths from the key executor, per mention path id.
    pub abstained: BTreeMap<String, usize>,
    /// Records the key's decode steps could not decode.
    pub undecodable: usize,
}

/// Grades `mapping` against `spec` on `payloads`. The payloads are decoded once for the key and
/// its oracle, and again for the mapping only when its decode steps differ.
pub(crate) fn grade(
    spec: &KeySpec,
    mapping: &StreamMapping,
    payloads: &[Value],
) -> Result<Grade, String> {
    let corpus = Decoded::new(payloads, &spec.decode);
    let gold = key_mentions(spec, &corpus)?;
    let unscored: BTreeSet<String> = spec.unscored.iter().map(rule_id).collect();
    let other;
    let mapping_corpus = if mapping.decode == corpus.steps() {
        &corpus
    } else {
        other = Decoded::new(payloads, &mapping.decode);
        &other
    };
    let predicted = mapping_mentions(mapping, mapping_corpus)?;
    let oracle = mapping_mentions(&spec.oracle()?, &corpus)?;
    Ok(Grade {
        mapping: score(&gold.partition, &predicted, &unscored),
        ceiling: score(&gold.partition, &oracle, &unscored),
        abstained: gold.abstained,
        undecodable: corpus.undecodable(),
    })
}

#[cfg(test)]
mod tests;
