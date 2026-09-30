//! JSON path lookup and in-place decode for [`s2w_model::StreamMapping`] payloads. The one
//! implementation: [`crate::MappingEngine`] runs it per event and `cargo xtask check` 11
//! (raw obfuscation replay) runs the same decode and lookups to rename payloads, so the check
//! cannot drift from the engine. [`natural_key`] is the engine's one key builder; `cargo xtask
//! h-measure` reads mentions with it, so a scored cluster is the entity `serve` would fold.

use s2w_model::{EntityRule, FieldPath, KeyPart, NaturalKey, Segment};
use serde_json::Value;

/// Replaces the JSON text at `path` with its parsed value.
///
/// Returns `Ok(true)` when the path was decoded and `Ok(false)` when it is absent, which is not
/// an error.
///
/// # Errors
/// A message when the path holds a non-string value or text that is not JSON.
pub fn decode_path(value: &mut Value, path: &FieldPath) -> Result<bool, String> {
    let Some(slot) = lookup_mut(value, path) else {
        return Ok(false);
    };
    let Value::String(text) = slot else {
        return Err("a decode path holds a non-string value".to_owned());
    };
    let parsed: Value = serde_json::from_str(text)
        .map_err(|error| format!("a decode path holds invalid JSON: {error}"))?;
    *slot = parsed;
    Ok(true)
}

/// The value at `path`, if every segment exists.
#[must_use]
pub fn lookup<'v>(value: &'v Value, path: &FieldPath) -> Option<&'v Value> {
    path.lookup(value)
}

/// The value at `path`, mutably, if every segment exists.
#[must_use]
pub fn lookup_mut<'v>(value: &'v mut Value, path: &FieldPath) -> Option<&'v mut Value> {
    path.0
        .iter()
        .try_fold(value, |node, segment| match segment {
            Segment::Key(key) => node.as_object_mut()?.get_mut(key),
            Segment::Index(index) => node.as_array_mut()?.get_mut(*index),
        })
}

/// The key part a JSON value can be: a string, an integer that fits `i64`, or a bool. Floats,
/// larger numbers, nulls, arrays and objects are not key parts.
#[must_use]
pub fn key_part(value: &Value) -> Option<KeyPart> {
    match value {
        Value::String(text) => Some(KeyPart::Str(text.clone())),
        Value::Number(number) => number.as_i64().map(KeyPart::Int),
        Value::Bool(flag) => Some(KeyPart::Bool(*flag)),
        _ => None,
    }
}

/// The rule's natural key in a decoded payload, when every key path holds a key part.
#[must_use]
pub fn entity_key(value: &Value, rule: &EntityRule) -> Option<NaturalKey> {
    natural_key(value, &rule.type_label, &rule.key)
}

/// The natural key of `type_label` and the key parts at `paths`, when every path holds one.
/// The one key builder: [`entity_key`] and `cargo xtask h-measure`'s key executor both call it.
#[must_use]
pub fn natural_key(value: &Value, type_label: &str, paths: &[FieldPath]) -> Option<NaturalKey> {
    let parts = paths
        .iter()
        .map(|path| key_part(lookup(value, path)?))
        .collect::<Option<Vec<_>>>()?;
    // A validated mapping's or key spec's labels never hold the separator, so this never
    // declines for one.
    NaturalKey::from_parts(type_label, &parts).ok()
}
