//! Executors: a key spec or a stream mapping, applied to stored payloads, gives mentions placed
//! in clusters (contract B3 "Executing a mapping"). A mention is `(record index, path id)`; the
//! path id is [`s2w_discover::rule_id`], so distinct paths never share one.
//!
//! The mapping executor decodes and builds keys with `s2w_system1::decode`, the functions
//! `MappingEngine` runs, so a predicted cluster is byte for byte the natural key `serve` folds.

use std::collections::BTreeMap;

use s2w_discover::rule_id;
use s2w_model::{FieldPath, NaturalKey, StreamMapping};
use s2w_system1::decode::{decode_path, entity_key, key_part, lookup};
use serde_json::Value;

use super::key::KeySpec;

/// One mention: the record's index in the corpus and the path's id.
pub(crate) type Mention = (usize, String);

/// Mentions placed in clusters. The cluster id is opaque text; only equality matters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Partition {
    pub cluster: BTreeMap<Mention, String>,
}

/// The key side: its partition and each mention's key type.
#[derive(Clone, Debug, Default)]
pub(crate) struct Key {
    pub partition: Partition,
    pub kind: BTreeMap<Mention, String>,
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

/// Applies a key spec: a record mentions an entity at a rule's path when that path and every
/// identity path hold a key part. The gold cluster is the type and the identity parts, encoded
/// as a natural key.
pub(crate) fn key_mentions(spec: &KeySpec, payloads: &[Value]) -> Key {
    let mut key = Key::default();
    for (record, payload) in payloads.iter().enumerate() {
        let Some(value) = decoded(payload, &spec.decode) else {
            continue;
        };
        for kind in &spec.types {
            for rule in &kind.mentions {
                if lookup(&value, &rule.path).and_then(key_part).is_none() {
                    continue;
                }
                let Some(parts) = rule
                    .identity
                    .iter()
                    .map(|path| lookup(&value, path).and_then(key_part))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                // A validated spec's labels never hold the separator.
                let Ok(gold) = NaturalKey::from_parts(&kind.label, &parts) else {
                    continue;
                };
                let mention = (record, rule_id(&rule.path));
                key.partition
                    .cluster
                    .insert(mention.clone(), gold.as_str().to_owned());
                key.kind.insert(mention, kind.label.clone());
            }
        }
    }
    key
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
    let mut partition = Partition::default();
    let mut owner: BTreeMap<Mention, &str> = BTreeMap::new();
    for (record, payload) in payloads.iter().enumerate() {
        let Some(value) = decoded(payload, &mapping.decode) else {
            continue;
        };
        for rule in &mapping.entities {
            let (Some(cluster), Some(last)) = (entity_key(&value, rule), rule.key.last()) else {
                continue;
            };
            let mention = (record, rule_id(last));
            let cluster = cluster.as_str().to_owned();
            match partition.cluster.get(&mention) {
                Some(existing) if *existing != cluster => {
                    return Err(format!(
                        "record {record}: rules {:?} and {:?} place the mention at {:?} in different clusters",
                        owner.get(&mention).copied().unwrap_or_default(),
                        rule.id,
                        mention.1
                    ));
                }
                Some(_) => {}
                None => {
                    owner.insert(mention.clone(), &rule.id);
                    partition.cluster.insert(mention, cluster);
                }
            }
        }
    }
    Ok(partition)
}
