//! Applying check 11's maps: to a raw payload, to the mapping, and to pass A's claims; plus the
//! non-vacuity checks.

use std::collections::{BTreeMap, BTreeSet};

use s2w_model::{AttrValue, FieldPath, KeyPart, NaturalKey, StreamMapping, WorldEvent};
use s2w_system1::decode::{decode_path, lookup_mut};
use serde_json::Value;

use super::Maps;

/// Decodes `payload` at each `decode` path in order with the engine's own [`decode_path`]: an
/// absent path is skipped, a non-string or invalid JSON is an error. Returns the tree and the
/// paths decoded.
pub(super) fn decode_all(
    payload: &Value,
    decode: &[FieldPath],
) -> Result<(Value, Vec<FieldPath>), String> {
    let mut tree = payload.clone();
    let mut applied = Vec::new();
    for path in decode {
        if decode_path(&mut tree, path)? {
            applied.push(path.clone());
        }
    }
    Ok((tree, applied))
}

impl Maps {
    /// The renamed payload: decoded at the mapping's decode paths (when `descend`), renamed,
    /// then re-encoded at the renamed paths in reverse order.
    pub(super) fn payload(
        &self,
        payload: &Value,
        decode: &[FieldPath],
        descend: bool,
    ) -> Result<Value, String> {
        let (tree, applied) = if descend {
            decode_all(payload, decode)?
        } else {
            (payload.clone(), Vec::new())
        };
        let mut renamed = self.rename(&tree);
        for path in applied.iter().rev() {
            let target = self.path(path)?;
            let slot = lookup_mut(&mut renamed, &target).ok_or_else(|| {
                "raw obfuscation replay: a renamed decode path no longer resolves".to_owned()
            })?;
            *slot = Value::String(serde_json::to_string(slot).map_err(|e| e.to_string())?);
        }
        Ok(renamed)
    }

    /// The renamed mapping: key segments and attribute names via the key map, type labels and
    /// kinds via the value map; rule ids and array indexes unchanged.
    pub(super) fn mapping(&self, mapping: &StreamMapping) -> Result<StreamMapping, String> {
        let mut renamed = mapping.clone();
        for path in &mut renamed.decode {
            *path = self.path(path)?;
        }
        for rule in &mut renamed.entities {
            rule.type_label = self.value(&rule.type_label)?;
            for path in &mut rule.key {
                *path = self.path(path)?;
            }
            for attr in &mut rule.attrs {
                attr.name = self.key(&attr.name)?;
                attr.path = self.path(&attr.path)?;
            }
        }
        for rel in &mut renamed.relationships {
            rel.kind = self.value(&rel.kind)?;
        }
        Ok(renamed)
    }

    /// One pass-A claim as pass B should produce it, mapped by typed role.
    pub(super) fn claim(&self, claim: &WorldEvent) -> Result<WorldEvent, String> {
        match claim {
            WorldEvent::EntityObserved {
                key,
                entity_type,
                attrs,
            } => {
                let mut renamed = BTreeMap::new();
                for (name, value) in attrs {
                    let value = match value {
                        AttrValue::Str(text) => AttrValue::Str(self.value(text)?),
                        other => other.clone(),
                    };
                    renamed.insert(self.key(name)?, value);
                }
                Ok(WorldEvent::EntityObserved {
                    key: self.natural_key(key)?,
                    entity_type: self.value(entity_type)?,
                    attrs: renamed,
                })
            }
            WorldEvent::RelationshipObserved { from, to, kind } => {
                Ok(WorldEvent::RelationshipObserved {
                    from: self.natural_key(from)?,
                    to: self.natural_key(to)?,
                    kind: self.value(kind)?,
                })
            }
            other => Err(format!(
                "raw obfuscation replay: a mapping engine proposed an unexpected claim {other:?}"
            )),
        }
    }

    /// Reads a key with [`NaturalKey::parts`]: the label and each string part via the value
    /// map, integer and boolean parts unchanged, rebuilt with [`NaturalKey::from_parts`].
    fn natural_key(&self, key: &NaturalKey) -> Result<NaturalKey, String> {
        let (label, parts) = read_key(key)?;
        let parts = parts
            .into_iter()
            .map(|part| match part {
                KeyPart::Str(text) => self.value(&text).map(KeyPart::Str),
                other => Ok(other),
            })
            .collect::<Result<Vec<_>, _>>()?;
        NaturalKey::from_parts(&self.value(label)?, &parts)
            .map_err(|e| format!("raw obfuscation replay: renamed key {key:?}: {e}"))
    }
}

/// A key's label and parts; a key that does not read is a check failure, never skipped.
fn read_key(key: &NaturalKey) -> Result<(&str, Vec<KeyPart>), String> {
    key.parts()
        .map_err(|e| format!("raw obfuscation replay: key {key:?}: {e}"))
}

/// What pass A must exercise for the replay to mean anything.
pub(super) fn non_vacuity(claims: &[WorldEvent], expected: &[WorldEvent]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut types = BTreeSet::new();
    let (mut rels, mut multi, mut int_part, mut str_attr) = (0, false, false, false);
    for claim in claims {
        match claim {
            WorldEvent::EntityObserved {
                key,
                entity_type,
                attrs,
            } => {
                types.insert(entity_type.as_str());
                match read_key(key) {
                    Ok((_, parts)) => {
                        multi |= parts.len() >= 2;
                        int_part |= parts.iter().any(|p| matches!(p, KeyPart::Int(_)));
                    }
                    Err(problem) => problems.push(problem),
                }
                str_attr |= attrs.values().any(|v| matches!(v, AttrValue::Str(_)));
            }
            WorldEvent::RelationshipObserved { .. } => rels += 1,
            _ => {}
        }
    }
    [
        (types.len() >= 2, "two entity types"),
        (rels >= 1, "one relationship"),
        (multi, "one multi-part key"),
        (int_part, "one integer key part"),
        (str_attr, "one string attribute"),
        (claims != expected, "one claim the maps change"),
    ]
    .into_iter()
    .filter(|(held, _)| !held)
    .map(|(_, what)| {
        format!("raw obfuscation replay is vacuous: pass A must yield at least {what}. Extend the fixture or the mapping")
    })
    .chain(problems)
    .collect()
}

/// Every string pass B carries that is an original raw string leaf: the obfuscation missed it.
pub(super) fn leaked_leaves(claims: &[WorldEvent], raw_leaves: &BTreeSet<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut problems = Vec::new();
    for claim in claims {
        let (keys, labels): (Vec<&NaturalKey>, Vec<&str>) = match claim {
            WorldEvent::EntityObserved {
                key,
                entity_type,
                attrs,
            } => {
                let mut labels = vec![entity_type.as_str()];
                labels.extend(attrs.values().filter_map(|v| match v {
                    AttrValue::Str(text) => Some(text.as_str()),
                    _ => None,
                }));
                (vec![key], labels)
            }
            WorldEvent::RelationshipObserved { from, to, kind } => {
                (vec![from, to], vec![kind.as_str()])
            }
            _ => (vec![], vec![]),
        };
        seen.extend(labels.into_iter().map(str::to_owned));
        for key in keys {
            match key_strings(key) {
                Ok(strings) => seen.extend(strings),
                Err(problem) => problems.push(problem),
            }
        }
    }
    seen.intersection(raw_leaves)
        .map(|leaf| format!("raw obfuscation replay: pass B still carries the raw string {leaf:?}; the obfuscation missed a field"))
        .chain(problems)
        .collect()
}

/// A key's label and its string parts, unquoted.
fn key_strings(key: &NaturalKey) -> Result<Vec<String>, String> {
    let (label, parts) = read_key(key)?;
    Ok(std::iter::once(label.to_owned())
        .chain(parts.into_iter().filter_map(|part| match part {
            KeyPart::Str(text) => Some(text),
            _ => None,
        }))
        .collect())
}
