//! A data-driven engine: a [`StreamMapping`] says where a payload's entities, attributes and
//! relationships live, and this executor follows it (decision 0021). The executor knows JSON,
//! never what a stream is about (decision 0018).

use std::collections::BTreeMap;

use s2w_model::{
    AttrValue, EntityRule, MappingError, NaturalKey, RawEvent, StreamMapping, WorldEvent,
    fnv1a64_hex,
};
use serde_json::Value;

use crate::decode::{decode_path, entity_key, lookup};

/// Every mapping engine's name is this prefix and the mapping identity (decision 0023).
const NAME_PREFIX: &str = "mapping-";
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
    /// `mapping-<identity>` (decision 0023).
    name: String,
    mapping_hash: String,
    /// Computed once per engine: the bridge asks for it on every evaluated event.
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
    /// The mapping has links, which this engine does not execute yet (decision 0027; s2w#245
    /// PR 2 adds the merge claims). Refused rather than run, so a link is never silently dropped.
    #[error("mapping has {0} link(s); this engine does not execute links yet (s2w#245)")]
    LinksNotExecuted(usize),
}

impl MappingEngine {
    /// Validates `mapping` and computes its identity and provenance digest.
    ///
    /// # Errors
    /// [`MappingEngineError::Invalid`] for a mapping that fails [`StreamMapping::validate`];
    /// [`MappingEngineError::LinksNotExecuted`] for a valid mapping with links.
    pub fn new(mapping: StreamMapping) -> Result<Self, MappingEngineError> {
        let name = format!("{NAME_PREFIX}{}", mapping.identity()?);
        if !mapping.links.is_empty() {
            return Err(MappingEngineError::LinksNotExecuted(mapping.links.len()));
        }
        let mapping_hash = fnv1a64_hex(&serde_json::to_vec(&mapping)?);
        let provenance = provenance(&mapping_hash, None);
        Ok(Self {
            mapping,
            name,
            mapping_hash,
            provenance,
        })
    }

    /// Records the proposal this mapping was read from in every verdict's provenance, so a
    /// stored verdict names the row it came from (decision 0023). The name does not change:
    /// the same mapping under another proposal id is the same engine.
    #[must_use]
    pub fn with_proposal_id(mut self, proposal_id: impl Into<String>) -> Self {
        let proposal_id: String = proposal_id.into();
        self.provenance = provenance(&self.mapping_hash, Some(&proposal_id));
        self
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
    fn name(&self) -> &str {
        &self.name
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

/// `{"mapping_hash":…}` plus `"proposal_id"` when known, keys in sorted order.
fn provenance(mapping_hash: &str, proposal_id: Option<&str>) -> Vec<u8> {
    let mut fields = serde_json::Map::new();
    fields.insert("mapping_hash".to_owned(), mapping_hash.into());
    if let Some(id) = proposal_id {
        fields.insert("proposal_id".to_owned(), id.into());
    }
    // Displaying a `Value` cannot fail, unlike `to_vec`, so no verdict loses its provenance.
    serde_json::Value::Object(fields).to_string().into_bytes()
}

fn abstain(reason: AbstainReason) -> Verdict {
    Verdict::Abstain { reason }
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
