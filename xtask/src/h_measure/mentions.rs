//! Executors: a key spec or a stream mapping, applied to stored payloads, gives mentions placed
//! in clusters (contract B3 "Executing a mapping"). A mention is `(record index, path id)`; the
//! path id is [`s2w_discover::rule_id`], so distinct paths never share one.
//!
//! The mapping executor decodes and builds keys with `s2w_system1::decode`, the functions
//! `MappingEngine` runs, so a predicted cluster is byte for byte the natural key `serve` folds.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use s2w_core::{EntityId, World, fold_one};
use s2w_discover::rule_id;
use s2w_model::{FieldPath, NaturalKey, StreamMapping, WorldEvent};
use s2w_system1::decode::{decode_path, entity_key, key_part, lookup, natural_key};
use serde_json::Value;

use super::key::KeySpec;

/// One mention: the record's index in the corpus and the path's id.
pub(crate) type Mention = (usize, String);

/// One unique typed, directed edge between two clusters (contract B3 "Relationships"). On the
/// key side the type is the row's `type`; on the mapping side it is the rule's `kind`, which
/// the scorer reads together with the endpoint clusters' types.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Edge {
    /// The edge type (key) or relationship kind (mapping).
    pub label: String,
    /// The source endpoint's cluster.
    pub from: String,
    /// The target endpoint's cluster.
    pub to: String,
}

/// Mentions placed in clusters. The cluster id is opaque text; only equality matters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Partition {
    pub cluster: BTreeMap<Mention, String>,
}

/// The payload with every decode step applied, or `None` when a step holds a non-string or
/// invalid JSON: `MappingEngine` abstains `Unparseable` on that payload, so it mentions nothing.
/// An absent decode path is skipped, as in the engine.
fn decoded(payload: &Value, decode: &[FieldPath]) -> Option<Value> {
    let mut value = payload.clone();
    for path in decode {
        decode_path(&mut value, path).ok()?;
    }
    Some(value)
}

/// A corpus with one list of decode steps applied, built once and shared by every executor
/// whose key spec or mapping names the same steps (the key, its oracle and a discovered
/// mapping usually do), so a corpus-scale run parses each payload's JSON text once.
pub(crate) struct Decoded {
    steps: Vec<FieldPath>,
    /// One entry per record: the decoded payload, or `None` when it cannot be decoded.
    records: Vec<Option<Value>>,
}

impl Decoded {
    /// Applies `steps` to every payload.
    pub(crate) fn new(payloads: &[Value], steps: &[FieldPath]) -> Self {
        Self {
            steps: steps.to_vec(),
            records: payloads.iter().map(|p| decoded(p, steps)).collect(),
        }
    }

    /// The decode steps these records were built with.
    pub(crate) fn steps(&self) -> &[FieldPath] {
        &self.steps
    }

    /// Records whose decode failed; both executors see no mention in them.
    pub(crate) fn undecodable(&self) -> usize {
        self.records.iter().filter(|r| r.is_none()).count()
    }

    /// The records, if they were decoded with `steps`; an executor given records decoded
    /// another way would read the wrong values.
    fn records(&self, steps: &[FieldPath]) -> Result<&[Option<Value>], String> {
        if self.steps == steps {
            Ok(&self.records)
        } else {
            Err(format!(
                "the corpus was decoded with {:?}, not {steps:?}",
                self.steps
            ))
        }
    }
}

/// What the key executor found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyMentions {
    /// The gold partition.
    pub partition: Partition,
    /// Abstained paths: per mention path id, the records whose mention path held a key part
    /// but some identity path did not, so the key places no mention there. Reported beside the
    /// score, never scored.
    pub abstained: BTreeMap<String, usize>,
    /// Excluded mentions: the mention path held one of its rule's `no_identity` values, so the
    /// key places no mention there (neither a singleton nor a merge). Reported, never scored.
    pub excluded: BTreeSet<Mention>,
    /// The gold edges (format 3): per record and observable relationship row whose two paths
    /// both hold a gold mention, `(type, from cluster, to cluster)`, unique over the corpus. An
    /// abstained or excluded endpoint is no mention, so it places no edge.
    pub edges: BTreeSet<Edge>,
    /// The key's unobservable relationship rows: declared, placing no edge, never scored.
    pub unobservable: usize,
}

/// What the mapping executor found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MappingMentions {
    /// The predicted partition.
    pub partition: Partition,
    /// The predicted edges: per record and relationship rule whose two endpoint rules matched
    /// (the engine's `RelationshipObserved` claim), `(kind, from cluster, to cluster)` with each
    /// endpoint resolved through the fold as its mention is, unique over the corpus.
    pub edges: BTreeSet<Edge>,
}

impl KeyMentions {
    /// Excluded mentions per mention path id.
    pub(crate) fn excluded_per_path(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for (_, path) in &self.excluded {
            *counts.entry(path.clone()).or_default() += 1;
        }
        counts
    }
}

/// Applies a key spec, after validating it: a record mentions an entity at a rule's path when
/// that path and every identity path hold a key part and the path's value is not one of the
/// rule's `no_identity` values (an excluded mention). The gold cluster is the type and the
/// identity parts, encoded as a natural key; its type is the key's label part. A record holds
/// an observable relationship row's edge when both of the row's paths hold a gold mention.
pub(crate) fn key_mentions(spec: &KeySpec, corpus: &Decoded) -> Result<KeyMentions, String> {
    spec.validate()?;
    let mut found = KeyMentions {
        unobservable: spec
            .relationships
            .iter()
            .filter(|r| !r.observable())
            .count(),
        ..KeyMentions::default()
    };
    // Each observable row's type and its two mention path ids, computed once.
    let rows: Vec<(&str, String, String)> = spec
        .relationships
        .iter()
        .filter(|row| row.observable())
        .map(|row| (row.label.as_str(), rule_id(&row.from), rule_id(&row.to)))
        .collect();
    for (record, value) in corpus.records(&spec.decode)?.iter().enumerate() {
        let Some(value) = value else {
            continue;
        };
        // This record's gold mentions, by path id, for its edges.
        let mut here: BTreeMap<String, String> = BTreeMap::new();
        for kind in &spec.types {
            for rule in &kind.mentions {
                let Some(part) = lookup(value, &rule.path).and_then(key_part) else {
                    continue;
                };
                let id = rule_id(&rule.path);
                if rule.excludes(&part) {
                    found.excluded.insert((record, id));
                    continue;
                }
                // A validated spec has one rule per mention id, so this insert never replaces
                // a mention; relaxing that rule would need the mapping executor's conflict error.
                match natural_key(value, &kind.label, &rule.identity) {
                    Some(gold) => {
                        here.insert(id, gold.as_str().to_owned());
                    }
                    None => *found.abstained.entry(id).or_default() += 1,
                }
            }
        }
        for (label, from, to) in &rows {
            if let (Some(from), Some(to)) = (here.get(from), here.get(to)) {
                found.edges.insert(Edge {
                    label: (*label).to_owned(),
                    from: from.clone(),
                    to: to.clone(),
                });
            }
        }
        found.partition.cluster.extend(
            here.into_iter()
                .map(|(id, cluster)| ((record, id), cluster)),
        );
    }
    Ok(found)
}

/// Applies a stream mapping. Each matching entity rule mentions its entity at the rule's last
/// key path: a composite key lists its context parts first (a site id, then the object id
/// within it), and the context usually keys a type of its own, so scoring every key path would
/// put one `(record, path)` in two clusters. Two rules that place one mention in different keys
/// are an error naming both, never a silent choice.
///
/// The cluster is the entity the fold resolves the mention's key to after the whole corpus
/// (s2w#245): per record the executor folds the claims `MappingEngine` makes, `EntityObserved`
/// per matching rule and then `EntitiesMerged` per link whose two rules matched with different
/// keys, into an [`s2w_core::World`], so the scorer joins aliases with the fold's own merge
/// rule (first merge wins per absorbed key), not a replica of it. The cluster text is the
/// natural key the resolved entity was minted from, so a mapping without links clusters by its
/// natural keys exactly as before.
///
/// Per record each relationship rule whose two endpoint rules matched (the engine's
/// `RelationshipObserved` claim) gives an edge; after the corpus each endpoint key resolves
/// through the same fold to its cluster, so links join edge endpoints as they join mentions.
pub(crate) fn mapping_mentions(
    mapping: &StreamMapping,
    corpus: &Decoded,
) -> Result<MappingMentions, String> {
    mapping
        .validate()
        .map_err(|e| format!("the mapping is not valid: {e}"))?;
    // Each mention's key and the rule that placed it there, for the conflict message.
    let mut placed: BTreeMap<Mention, (NaturalKey, &str)> = BTreeMap::new();
    let mut world = World::default();
    // Each relationship claim's kind and endpoint keys, unique before resolution.
    let mut claimed: BTreeSet<(&str, NaturalKey, NaturalKey)> = BTreeSet::new();
    for (record, value) in corpus.records(&mapping.decode)?.iter().enumerate() {
        let Some(value) = value else {
            continue;
        };
        let keys: Vec<Option<NaturalKey>> = mapping
            .entities
            .iter()
            .map(|rule| entity_key(value, rule))
            .collect();
        for (rule, key) in mapping.entities.iter().zip(&keys) {
            let (Some(key), Some(last)) = (key, rule.key.last()) else {
                continue;
            };
            world = fold_one(
                world,
                &WorldEvent::EntityObserved {
                    key: key.clone(),
                    entity_type: rule.type_label.clone(),
                    attrs: BTreeMap::new(),
                },
            );
            match placed.entry((record, rule_id(last))) {
                Entry::Vacant(slot) => {
                    slot.insert((key.clone(), &rule.id));
                }
                Entry::Occupied(slot) if slot.get().0 != *key => {
                    return Err(format!(
                        "record {record}: rules {:?} and {:?} place the mention at {:?} under different keys",
                        slot.get().1,
                        rule.id,
                        slot.key().1
                    ));
                }
                Entry::Occupied(_) => {}
            }
        }
        world = merged(world, mapping, &keys);
        for rel in &mapping.relationships {
            if let (Some(from), Some(to)) = (
                matched(mapping, &keys, &rel.from),
                matched(mapping, &keys, &rel.to),
            ) {
                claimed.insert((&rel.kind, from, to));
            }
        }
    }
    resolved(&world, &placed, claimed)
}

/// `world` with one record's `EntitiesMerged` claims folded in: per link whose two rules matched
/// with different keys, as `MappingEngine` claims them.
fn merged(mut world: World, mapping: &StreamMapping, keys: &[Option<NaturalKey>]) -> World {
    for link in &mapping.links {
        if let (Some(survivor), Some(absorbed)) = (
            matched(mapping, keys, &link.survivor),
            matched(mapping, keys, &link.absorbed),
        ) && survivor != absorbed
        {
            world = fold_one(world, &WorldEvent::EntitiesMerged { survivor, absorbed });
        }
    }
    world
}

/// Each mention's cluster, and each claimed edge's: the natural key the fold minted the entity
/// a key resolves to from.
fn resolved(
    world: &World,
    placed: &BTreeMap<Mention, (NaturalKey, &str)>,
    claimed: BTreeSet<(&str, NaturalKey, NaturalKey)>,
) -> Result<MappingMentions, String> {
    // Ids are minted one per key, and every placed key was observed, so each has an id.
    let minted: BTreeMap<EntityId, &NaturalKey> = placed
        .values()
        .filter_map(|(key, _)| world.id_of(key).map(|id| (id, key)))
        .collect();
    // An edge endpoint is a matched rule's key, so it was observed and placed too.
    let root = |key: &NaturalKey| {
        world
            .id_of(key)
            .map(|id| world.resolve(id))
            .and_then(|id| minted.get(&id))
            .map(|root| root.as_str().to_owned())
            .ok_or_else(|| format!("the fold has no entity for the key {:?}", key.as_str()))
    };
    let mut found = MappingMentions::default();
    for (mention, (key, _)) in placed {
        found.partition.cluster.insert(mention.clone(), root(key)?);
    }
    for (label, from, to) in claimed {
        found.edges.insert(Edge {
            label: label.to_owned(),
            from: root(&from)?,
            to: root(&to)?,
        });
    }
    Ok(found)
}

/// The key a rule id matched in this record, if it matched: the engine's own lookup.
fn matched(mapping: &StreamMapping, keys: &[Option<NaturalKey>], id: &str) -> Option<NaturalKey> {
    let index = mapping.entities.iter().position(|rule| rule.id == id)?;
    keys.get(index)?.clone()
}
