//! Deterministic per-event engines proposing claims without seeing the folded world.

pub mod decode;
mod engines;
mod verdict;

pub use engines::{JsonClaimsEngine, MappingEngine, MappingEngineError};
pub use verdict::{AbstainReason, Confidence, ConfidenceError, Verdict};

use s2w_model::RawEvent;

/// A payload-only mapping. Verdicts are persisted (decision 0012); engines must still be
/// deterministic so a version bump has a well-defined meaning and replay is exact.
pub trait Engine: Send + Sync {
    /// Stable identifier persisted with each verdict in the verdict log (decision 0012).
    fn name(&self) -> &'static str;
    /// Code version; bump whenever this engine's payload-to-verdict code changes. An engine
    /// that runs a data mapping (`MappingEngine`) names that mapping in [`Self::provenance`],
    /// not here (decision 0021).
    fn version(&self) -> u32;
    /// Total: unsupported or malformed inputs abstain, never panic or error.
    fn evaluate(&self, event: &RawEvent) -> Verdict;
    /// Reserved provenance bytes (decision 0012), encoded by the caller. `None` (the default)
    /// for engines with no identity beyond their version.
    fn provenance(&self) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
fn raw(payload: &[u8]) -> Result<RawEvent, s2w_model::ModelError> {
    Ok(RawEvent {
        source: s2w_model::SourceId::new("test")?,
        cursor: s2w_model::Cursor::new(vec![1])?,
        received_at: s2w_model::Timestamp::from_millis(0),
        payload: payload.to_vec(),
    })
}
