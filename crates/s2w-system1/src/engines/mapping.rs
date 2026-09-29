//! A data-driven engine: a [`StreamMapping`] says where a payload's entities, attributes and
//! relationships live, and this executor follows it (decision 0021). The executor knows JSON,
//! never what a stream is about (decision 0018).

use std::collections::BTreeMap;

use s2w_model::{
    AttrValue, EntityRule, KeyPart, MappingError, NaturalKey, RawEvent, StreamMapping, WorldEvent,
    fnv1a64_hex,
};
use serde_json::Value;

use crate::decode::{decode_path, lookup};
use crate::{AbstainReason, Confidence, Engine, Verdict};

/// Runs one [`StreamMapping`] over raw JSON payloads.
///
/// Per payload: parse JSON (else `Unparseable`), apply the mapping's decode steps (a path that
/// is absent is skipped; one that holds a non-string or invalid JSON is `Unparseable`), then
/// match each entity rule. A rule matches when every key path holds a scalar (string, `i64`
/// integer or bool); floats, out-of-range numbers, nulls, arrays and objects never match. The
/// natural key is [`NaturalKey::from_parts`]: the type label, then each key part, joined by
/// [`s2w_model::KEY_SEPARATOR`], so a string `"7"` and an integer `7` stay distinct keys and
/// two types never share a key. A relationship is claimed when both endpoint rules matched. No rule
/// matched abstains `Insufficient`; otherwise the claims are proposed as certain, entities in
/// rule order then relationships in rule order.
#[derive(Debug, Clone)]
pub struct MappingEngine {
    mapping: StreamMapping,
    provenance: Vec<u8>,
}

/// Why a [`MappingEngine`] could not be built.
#[derive(Debug, thiserror::Error)]
pub enum MappingEngineError {
    /// The mapping broke a validation rule.
    #[error(transparent)]
    Invalid(#[from] MappingError),
    /// The mapping could not be encoded to compute its digest.
    #[error("mapping could not be encoded: {0}")]
    Encode(#[from] serde_json::Error),
}

impl MappingEngine {
    /// Validates `mapping` and computes its provenance digest.
    ///
    /// # Errors
    /// [`MappingEngineError::Invalid`] for a mapping that fails [`StreamMapping::validate`].
    pub fn new(mapping: StreamMapping) -> Result<Self, MappingEngineError> {
        mapping.validate()?;
        let canonical = serde_json::to_vec(&mapping)?;
        let provenance = format!(r#"{{"mapping_hash":"{}"}}"#, fnv1a64_hex(&canonical));
        Ok(Self {
            mapping,
            provenance: provenance.into_bytes(),
        })
    }

    /// The mapping this engine runs.
    #[must_use]
    pub fn mapping(&self) -> &StreamMapping {
        &self.mapping
    }

    fn claims(&self, value: &Value) -> Vec<WorldEvent> {
        let keys: Vec<Option<NaturalKey>> = self
            .mapping
            .entities
            .iter()
            .map(|rule| entity_key(value, rule))
            .collect();
        let mut claims: Vec<WorldEvent> = self
            .mapping
            .entities
            .iter()
            .zip(&keys)
            .filter_map(|(rule, key)| {
                key.as_ref().map(|key| WorldEvent::EntityObserved {
                    key: key.clone(),
                    entity_type: rule.type_label.clone(),
                    attrs: attrs(value, rule),
                })
            })
            .collect();
        for rel in &self.mapping.relationships {
            if let (Some(from), Some(to)) =
                (self.matched(&keys, &rel.from), self.matched(&keys, &rel.to))
            {
                claims.push(WorldEvent::RelationshipObserved {
                    from,
                    to,
                    kind: rel.kind.clone(),
                });
            }
        }
        claims
    }

    /// The key a rule id matched in this payload, if it matched.
    fn matched(&self, keys: &[Option<NaturalKey>], id: &str) -> Option<NaturalKey> {
        let index = self
            .mapping
            .entities
            .iter()
            .position(|rule| rule.id == id)?;
        keys.get(index)?.clone()
    }
}

impl Engine for MappingEngine {
    fn name(&self) -> &'static str {
        "mapping"
    }
    fn version(&self) -> u32 {
        // The executor's code version. It does not identify the mapping; see `provenance`.
        1
    }
    fn evaluate(&self, event: &RawEvent) -> Verdict {
        let mut value: Value = match serde_json::from_slice(&event.payload) {
            Ok(value) => value,
            Err(error) => return abstain(AbstainReason::Unparseable(error.to_string())),
        };
        for path in &self.mapping.decode {
            if let Err(reason) = decode_path(&mut value, path) {
                return abstain(AbstainReason::Unparseable(reason));
            }
        }
        let claims = self.claims(&value);
        if claims.is_empty() {
            return abstain(AbstainReason::Insufficient(
                "no entity rule matched".to_owned(),
            ));
        }
        Verdict::Propose {
            claims,
            confidence: Confidence::CERTAIN,
        }
    }
    fn provenance(&self) -> Option<Vec<u8>> {
        Some(self.provenance.clone())
    }
}

fn abstain(reason: AbstainReason) -> Verdict {
    Verdict::Abstain { reason }
}

/// The rule's natural key, when every key path holds a scalar.
fn entity_key(value: &Value, rule: &EntityRule) -> Option<NaturalKey> {
    let parts = rule
        .key
        .iter()
        .map(|path| match lookup(value, path)? {
            Value::String(text) => Some(KeyPart::Str(text.clone())),
            Value::Number(number) => number.as_i64().map(KeyPart::Int),
            Value::Bool(flag) => Some(KeyPart::Bool(*flag)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    // A validated mapping's labels never hold the separator, so this never declines.
    NaturalKey::from_parts(&rule.type_label, &parts).ok()
}

fn attrs(value: &Value, rule: &EntityRule) -> BTreeMap<String, AttrValue> {
    rule.attrs
        .iter()
        .filter_map(|attr| {
            let found = match lookup(value, &attr.path)? {
                Value::String(text) => AttrValue::Str(text.clone()),
                Value::Number(number) => AttrValue::Int(number.as_i64()?),
                Value::Bool(flag) => AttrValue::Bool(*flag),
                _ => return None,
            };
            Some((attr.name.clone(), found))
        })
        .collect()
}

#[cfg(test)]
mod tests;
