//! Routes are data (decision 0023): per source, the effective `stream-mapping` proposal in the
//! proposal store decides which [`MappingEngine`] runs on it. This module holds the per-source
//! resolution (the rule itself is `query::resolve_class`) and the start-up read that builds an
//! [`EngineRegistry`] from it.
//!
//! It lives in `s2w-app` because it composes `s2w-log` rows with `s2w-model` mappings, and
//! neither lower crate may know the other (decision 0019).

use std::collections::BTreeMap;
use std::path::Path;

use s2w_log::{
    PROPOSAL_DATABASE_FILE, ReadOnlySqliteProposalStore, StoredDecision, StoredProposal,
};
use s2w_model::{SourceId, StreamMapping};
use s2w_system1::MappingEngine;

use crate::AppError;
use crate::bridge::{EngineRegistry, Route};
use crate::query::resolve_class;
// The resolution tests predate `query::resolve_class` and name these through `super::*`.
#[cfg(test)]
use s2w_log::{Decider, Outcome};

pub use crate::query::{
    ENVELOPE_FORMAT, Excluded, MappingEnvelope, STREAM_MAPPING_CLASS, decode_envelope, proposal_id,
};

/// A source's effective mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The earliest-seq accepted proposal carrying [`Self::identity`], so a same-bytes
    /// re-proposal changes neither the running engine's provenance nor a restart's report.
    pub proposal_id: String,
    /// [`StreamMapping::identity`].
    pub identity: String,
    /// The mapping.
    pub mapping: StreamMapping,
}

/// What [`resolve`] found: one effective mapping per routed source, and every unusable row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    /// Sources with an effective mapping. A source absent here is unrouted.
    pub routes: BTreeMap<SourceId, Resolved>,
    /// Rows excluded from resolution, in proposal order. Reported loudly by the caller, never
    /// fatal: one malformed row must not stop `serve`.
    pub excluded: Vec<Excluded>,
}

/// The resolution rule of decision 0023 over [`STREAM_MAPPING_CLASS`], per source; the rule
/// itself is [`resolve_class`].
///
/// 1. Only [`STREAM_MAPPING_CLASS`] proposals count; one whose payload does not decode or
///    validate is [`Excluded`].
/// 2. `human` decides per (source, mapping identity): the latest `human` decision on any
///    proposal carrying that identity for that source wins. A human reject therefore binds the
///    mapping, not only the proposal id: a later proposal of the same bytes stays rejected
///    whatever `policy` says, until a later human accept on any of them.
/// 3. With no `human` decision on that identity, a proposal is accepted when its own latest
///    `policy` decision is accept. `agent` (context, decision 0020) and `evidence` (a grading
///    signal) never count.
/// 4. A source's effective mapping is its accepted proposal with the largest proposal `seq`
///    (write order; `proposed_at_ms` is caller-supplied and never a tie-breaker).
/// 5. No accepted proposal: the source is unrouted.
#[must_use]
pub fn resolve(proposals: &[StoredProposal], decisions: &[StoredDecision]) -> Resolution {
    let resolution = resolve_class(STREAM_MAPPING_CLASS, decode_envelope, proposals, decisions);
    Resolution {
        routes: resolution
            .winners
            .into_iter()
            .map(|(source, winner)| {
                let resolved = Resolved {
                    proposal_id: winner.proposal_id,
                    identity: winner.identity,
                    mapping: winner.value,
                };
                (source, resolved)
            })
            .collect(),
        excluded: resolution.excluded,
    }
}

/// The proposal store's watermark: `None` while no store exists, else the highest proposal and
/// decision `seq`.
pub type Watermark = Option<(Option<i64>, Option<i64>)>;

/// The proposal store's watermark (s2w#184): `None` while no store exists, else the highest
/// proposal and decision `seq`. `serve` compares it on every bridge poll and resolves again
/// only when it moved. Read it before the rows ([`load`]): a row appended in between is then
/// above the recorded watermark and seen on the next poll, never missed.
///
/// # Errors
/// [`AppError::Proposals`] if the store exists but cannot be opened or read.
pub fn watermark(log_dir: &Path) -> Result<Watermark, AppError> {
    let exists = log_dir
        .join(PROPOSAL_DATABASE_FILE)
        .try_exists()
        .map_err(|error| AppError::Proposals(s2w_log::LogError::Io(error.to_string())))?;
    if !exists {
        return Ok(None);
    }
    let store = ReadOnlySqliteProposalStore::open(log_dir).map_err(AppError::Proposals)?;
    store.watermark().map(Some).map_err(AppError::Proposals)
}

/// Reads `log_dir`'s proposal store and resolves it. A missing store resolves to no routes
/// (nothing has ever been proposed); a read never creates it.
///
/// # Errors
/// [`AppError::Proposals`] if the store exists but cannot be opened or read, including a
/// payload whose hash does not match (decision 0019's integrity check).
pub fn load(log_dir: &Path) -> Result<Resolution, AppError> {
    let exists = log_dir
        .join(PROPOSAL_DATABASE_FILE)
        .try_exists()
        .map_err(|error| AppError::Proposals(s2w_log::LogError::Io(error.to_string())))?;
    if !exists {
        return Ok(Resolution::default());
    }
    let store = ReadOnlySqliteProposalStore::open(log_dir).map_err(AppError::Proposals)?;
    let proposals = store.proposals().map_err(AppError::Proposals)?;
    let decisions = store.decisions().map_err(AppError::Proposals)?;
    Ok(resolve(&proposals, &decisions))
}

/// The default routes plus `Route::Exact(source) -> MappingEngine` for every resolved source,
/// in source order, each engine named by its mapping identity and carrying its proposal id.
///
/// # Errors
/// [`AppError::BridgeStopped`] if an engine cannot be built or registered; neither happens for
/// a resolved mapping, which already validated.
pub fn registry(resolution: &Resolution) -> Result<EngineRegistry, AppError> {
    let mut registry = EngineRegistry::with_defaults();
    for (source, resolved) in &resolution.routes {
        let engine = MappingEngine::new(resolved.mapping.clone())
            .map_err(|error| AppError::BridgeStopped(error.to_string()))?
            .with_proposal_id(resolved.proposal_id.clone());
        registry
            .register(Route::Exact(source.as_str().to_owned()), Box::new(engine))
            .map_err(|error| AppError::BridgeStopped(error.to_string()))?;
    }
    Ok(registry)
}

/// One line per routed source and per excluded row, for `serve`'s start-up report.
#[must_use]
pub fn report_lines(resolution: &Resolution) -> Vec<String> {
    let mut lines: Vec<String> = resolution
        .routes
        .iter()
        .map(|(source, resolved)| {
            format!(
                "route: source '{}' runs mapping {} from proposal {}",
                source.as_str(),
                resolved.identity,
                resolved.proposal_id
            )
        })
        .collect();
    lines.extend(resolution.excluded.iter().map(|excluded| {
        format!(
            "route: stream-mapping proposal {} is excluded from routing: {}",
            excluded.proposal_id, excluded.reason
        )
    }));
    lines
}

#[cfg(test)]
mod tests;
