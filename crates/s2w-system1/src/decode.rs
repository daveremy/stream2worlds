//! JSON path lookup and in-place decode for [`s2w_model::StreamMapping`] payloads. The one
//! implementation: [`crate::MappingEngine`] runs it per event and `cargo xtask check` 11
//! (raw obfuscation replay) runs the same decode and lookups to rename payloads, so the check
//! cannot drift from the engine.

use s2w_model::{FieldPath, Segment};
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
    path.0
        .iter()
        .try_fold(value, |node, segment| match segment {
            Segment::Key(key) => node.as_object()?.get(key),
            Segment::Index(index) => node.as_array()?.get(*index),
        })
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
