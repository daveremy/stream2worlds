//! Context collisions: the unfloored composite-key sub-metric (evaluation contract, 2026-09-29
//! note on s2w#17, defined by s2w#56 PR 2b; `research/h-measure/README.md` states it for the
//! plain stream's key). Generic over streams: the context paths are the key's own identity
//! paths, read as data.
//!
//! A row is one key type and one **context path**: an identity path of a mention rule whose
//! identity has two or more paths and includes the rule's own path, other than that path. Drop
//! the context part from each gold entity's identity parts; gold entities of the type that then
//! become equal form a **collision group**, the entities a mapping that keys the type without
//! the context would merge. The row scores B-cubed on every gold mention (alias paths included)
//! of the entities in groups of two or more, with both partitions restricted to those mentions
//! (predicted clusters are sized inside that set). No such group: every metric is undefined. Reported beside B3's
//! metrics, never a floor.

use std::collections::{BTreeMap, BTreeSet};

use s2w_discover::rule_id;
use s2w_model::{KeyPart, NaturalKey};
use serde::Serialize;

use super::key::KeySpec;
use super::mentions::{Mention, Partition};
use super::score::{Bcubed, score};

/// One context-collision row.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct ContextRow {
    /// Collision groups: sets of two or more gold entities equal without the context part.
    pub groups: usize,
    /// Gold entities in those groups.
    pub entities: usize,
    /// Every gold mention of those entities, alias paths included: the set both partitions
    /// are restricted to.
    pub mentions: usize,
    /// The graded mapping on that set.
    pub mapping: Bcubed,
    /// The oracle ceiling on that set.
    pub ceiling: Bcubed,
}

/// The collision mentions of one row, before scoring.
#[derive(Default)]
struct Collisions<'a> {
    groups: usize,
    entities: BTreeSet<&'a str>,
    mentions: BTreeSet<Mention>,
}

/// One row's name: `type @ context path id`.
fn row_name(label: &str, context: &str) -> String {
    format!("{label} @ {context}")
}

/// Per mention path id, the rows its gold mentions feed and the context part's index in the
/// identity. A row comes from a rule whose identity includes its own path; every rule of the
/// type with that same identity (an alias path included) feeds the row, so an entity seen only
/// at an alias path still joins its collision group.
fn contexts(spec: &KeySpec) -> BTreeMap<String, Vec<(String, usize)>> {
    let mut contexts: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    for kind in &spec.types {
        for rule in &kind.mentions {
            if rule.identity.len() < 2 || !rule.identity.contains(&rule.path) {
                continue;
            }
            for (at, context) in rule.identity.iter().enumerate() {
                if *context == rule.path {
                    continue;
                }
                let row = row_name(&kind.label, &rule_id(context));
                for feeder in kind.mentions.iter().filter(|m| m.identity == rule.identity) {
                    let fed = contexts.entry(rule_id(&feeder.path)).or_default();
                    if !fed.contains(&(row.clone(), at)) {
                        fed.push((row.clone(), at));
                    }
                }
            }
        }
    }
    contexts
}

/// Every row of `spec`, with the gold mentions of the entities in its collision groups. A row
/// with no group is still listed.
fn collisions<'a>(
    spec: &KeySpec,
    gold: &'a Partition,
) -> Result<BTreeMap<String, Collisions<'a>>, String> {
    let contexts = contexts(spec);
    let mut groups: BTreeMap<(&str, Vec<KeyPart>), BTreeSet<&str>> = BTreeMap::new();
    for (mention, cluster) in &gold.cluster {
        let Some(rows) = contexts.get(&mention.1) else {
            continue;
        };
        // A gold cluster is a natural key the key itself built, one part per identity path; a
        // key that does not parse, or has too few parts, is a broken invariant, never skipped.
        let (_, parts) = NaturalKey::new(cluster.as_str())
            .parts()
            .map_err(|e| format!("gold entity {cluster:?}: {e}"))?;
        for (row, at) in rows {
            let mut rest = parts.clone();
            if *at >= rest.len() {
                return Err(format!(
                    "gold entity {cluster:?} has {} identity parts; row {row:?} drops part {at}",
                    rest.len()
                ));
            }
            rest.remove(*at);
            groups
                .entry((row.as_str(), rest))
                .or_default()
                .insert(cluster.as_str());
        }
    }
    let mut rows: BTreeMap<String, Collisions<'a>> = contexts
        .values()
        .flatten()
        .map(|(row, _)| (row.clone(), Collisions::default()))
        .collect();
    for ((row, _), entities) in groups {
        if entities.len() > 1 {
            let found = rows.entry(row.to_owned()).or_default();
            found.groups += 1;
            found.entities.extend(entities);
        }
    }
    // Every gold mention of a colliding entity, at any of its type's mention paths: an alias
    // mention counts, so a mapping that separates the entities at an alias path is credited.
    for found in rows.values_mut() {
        found.mentions = gold
            .cluster
            .iter()
            .filter(|(_, cluster)| found.entities.contains(cluster.as_str()))
            .map(|(mention, _)| mention.clone())
            .collect();
    }
    Ok(rows)
}

fn restricted(partition: &Partition, to: &BTreeSet<Mention>) -> Partition {
    Partition {
        cluster: partition
            .cluster
            .iter()
            .filter(|(mention, _)| to.contains(*mention))
            .map(|(mention, cluster)| (mention.clone(), cluster.clone()))
            .collect(),
    }
}

/// The context-collision rows of `spec`, for the graded mapping and the oracle ceiling.
pub(crate) fn rows(
    spec: &KeySpec,
    gold: &Partition,
    predicted: &Partition,
    oracle: &Partition,
) -> Result<BTreeMap<String, ContextRow>, String> {
    // The restricted set holds gold mentions only, and a mention path is never also unscored
    // (`KeySpec::validate`), so no unscored path can reach this scorer.
    let unscored = BTreeSet::new();
    Ok(collisions(spec, gold)?
        .into_iter()
        .map(|(name, found)| {
            let key = restricted(gold, &found.mentions);
            let micro = |guess: &Partition| {
                score(&key, &restricted(guess, &found.mentions), &unscored).micro
            };
            let row = ContextRow {
                groups: found.groups,
                entities: found.entities.len(),
                mentions: found.mentions.len(),
                mapping: micro(predicted),
                ceiling: micro(oracle),
            };
            (name, row)
        })
        .collect())
}

#[cfg(test)]
mod tests;
