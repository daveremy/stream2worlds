//! Executors: a key spec or a stream mapping, applied to stored payloads, gives mentions placed
//! in clusters (contract B3 "Executing a mapping"). A mention is `(record index, path id)`; the
//! path id is [`s2w_discover::rule_id`], so distinct paths never share one.
//!
//! The mapping executor decodes and builds keys with `s2w_system1::decode`, the functions
//! `MappingEngine` runs, so a predicted cluster is byte for byte the natural key `serve` folds.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use s2w_discover::rule_id;
use s2w_model::{FieldPath, StreamMapping};
use s2w_system1::decode::{decode_path, entity_key, key_part, lookup, natural_key};
use serde_json::Value;

use super::key::KeySpec;

/// One mention: the record's index in the corpus and the path's id.
pub(crate) type Mention = (usize, String);

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

/// Applies a key spec, after validating it: a record mentions an entity at a rule's path when
/// that path and every identity path hold a key part. The gold cluster is the type and the
/// identity parts, encoded as a natural key; its type is the key's label part.
pub(crate) fn key_mentions(spec: &KeySpec, payloads: &[Value]) -> Result<Partition, String> {
    spec.validate()?;
    let mut partition = Partition::default();
    for (record, payload) in payloads.iter().enumerate() {
        let Some(value) = decoded(payload, &spec.decode) else {
            continue;
        };
        for kind in &spec.types {
            for rule in &kind.mentions {
                if lookup(&value, &rule.path).and_then(key_part).is_none() {
                    continue;
                }
                // A validated spec has one rule per mention id, so this insert never replaces
                // a mention; relaxing that rule would need the mapping executor's conflict error.
                if let Some(gold) = natural_key(&value, &kind.label, &rule.identity) {
                    partition
                        .cluster
                        .insert((record, rule_id(&rule.path)), gold.as_str().to_owned());
                }
            }
        }
    }
    Ok(partition)
}

/// Applies a stream mapping. Each matching entity rule mentions its entity at the rule's last
/// key path: a composite key lists its context parts first (a site id, then the object id
/// within it), and the context usually keys a type of its own, so scoring every key path would
/// put one `(record, path)` in two clusters. Two rules that place one mention in different
/// clusters are an error naming both, never a silent choice.
pub(crate) fn mapping_mentions(
    mapping: &StreamMapping,
    payloads: &[Value],
) -> Result<Partition, String> {
    mapping
        .validate()
        .map_err(|e| format!("the mapping is not valid: {e}"))?;
    // Each mention's cluster and the rule that placed it there, for the conflict message.
    let mut placed: BTreeMap<Mention, (String, &str)> = BTreeMap::new();
    for (record, payload) in payloads.iter().enumerate() {
        let Some(value) = decoded(payload, &mapping.decode) else {
            continue;
        };
        for rule in &mapping.entities {
            let (Some(cluster), Some(last)) = (entity_key(&value, rule), rule.key.last()) else {
                continue;
            };
            let cluster = cluster.as_str().to_owned();
            match placed.entry((record, rule_id(last))) {
                Entry::Vacant(slot) => {
                    slot.insert((cluster, &rule.id));
                }
                Entry::Occupied(slot) if slot.get().0 != cluster => {
                    return Err(format!(
                        "record {record}: rules {:?} and {:?} place the mention at {:?} in different clusters",
                        slot.get().1,
                        rule.id,
                        slot.key().1
                    ));
                }
                Entry::Occupied(_) => {}
            }
        }
    }
    let cluster = placed
        .into_iter()
        .map(|(mention, (cluster, _))| (mention, cluster))
        .collect();
    Ok(Partition { cluster })
}
