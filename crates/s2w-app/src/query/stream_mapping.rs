//! The `stream-mapping` proposal payload (decision 0023): its class, envelope and decoder.
//!
//! It lives in `query` so the dashboard read can resolve the current mappings without
//! depending on `routes`; `routes` re-exports every item here, so its callers are unchanged.

use s2w_model::{SourceId, StreamMapping};
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
