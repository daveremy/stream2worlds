//! The `stream-mapping` proposal payload (decision 0023): its class, envelope and decoder.
//!
//! It lives in `query` so the dashboard read can resolve the current mappings without
//! depending on `routes`; `routes` re-exports every item here, so its callers are unchanged.

use s2w_log::{Actor, LogPosition};
use s2w_model::{SourceId, StreamMapping, fnv1a64_hex};
use serde::{Deserialize, Serialize};

/// The proposal class whose payloads are [`MappingEnvelope`]s. One class for every source, so
/// grading and revocation by class (decision 0019) see one denominator, not one per source.
pub const STREAM_MAPPING_CLASS: &str = "stream-mapping";

/// The one envelope format this build reads.
pub const ENVELOPE_FORMAT: u32 = 1;

/// A `stream-mapping` proposal's payload: which source the mapping is for, and the mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingEnvelope {
    /// Always [`ENVELOPE_FORMAT`].
    pub format: u32,
    /// The source id the mapping routes.
    pub source: String,
    /// The mapping itself (decision 0021).
    pub mapping: StreamMapping,
}

/// Decodes a `stream-mapping` payload into its source, mapping and identity.
///
/// # Errors
/// A message naming the failure: not JSON of the envelope's shape, another envelope format,
/// an invalid source id, or a mapping that fails [`StreamMapping::validate`].
pub fn decode_envelope(payload: &[u8]) -> Result<(SourceId, StreamMapping, String), String> {
    let envelope: MappingEnvelope =
        serde_json::from_slice(payload).map_err(|error| format!("payload: {error}"))?;
    if envelope.format != ENVELOPE_FORMAT {
        return Err(format!(
            "envelope format {} is not supported; this build reads format {ENVELOPE_FORMAT}",
            envelope.format
        ));
    }
    let source = SourceId::new(envelope.source).map_err(|error| format!("source: {error}"))?;
    let identity = envelope
        .mapping
        .identity()
        .map_err(|error| format!("mapping: {error}"))?;
    Ok((source, envelope.mapping, identity))
}

/// `fnv1a64_hex` over the actor, source, window bounds and mapping identity, each length-
/// prefixed so no two tuples share an encoding. The same log gives the same id; a moved window
/// gives another. Shared by every `stream-mapping` producer: discover's window proposals and a
/// human's `s2w proposals propose` (whose window is the single position 1).
#[must_use]
pub fn proposal_id(
    actor: &Actor,
    source: &SourceId,
    first: LogPosition,
    last: LogPosition,
    identity: &str,
) -> String {
    let actor = match actor {
        Actor::Human { id } => format!("human:{id}"),
        Actor::Agent { model, version } => format!("agent:{model}/{version}"),
    };
    let first = first.as_u64().to_string();
    let last = last.as_u64().to_string();
    let mut bytes = Vec::new();
    for field in [actor.as_str(), source.as_str(), &first, &last, identity] {
        bytes.extend_from_slice(&u64::try_from(field.len()).unwrap_or(u64::MAX).to_le_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
    fnv1a64_hex(&bytes)
}
