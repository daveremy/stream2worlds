//! Routes are data (decision 0023): per source, the effective `stream-mapping` proposal in the
//! proposal store decides which [`MappingEngine`] runs on it. This module holds the resolution
//! rule (pure) and the start-up read that builds an [`EngineRegistry`] from it.
//!
//! It lives in `s2w-app` because it composes `s2w-log` rows with `s2w-model` mappings, and
//! neither lower crate may know the other (decision 0019).

use std::collections::BTreeMap;
use std::path::Path;

use s2w_log::{
    Decider, Outcome, PROPOSAL_DATABASE_FILE, ReadOnlySqliteProposalStore, StoredDecision,
    StoredProposal,
};
use s2w_model::{SourceId, StreamMapping};
use s2w_system1::{MappingEngine, MappingEngineError};
use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::bridge::{EngineRegistry, Route};

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

/// A `stream-mapping` proposal left out of resolution because its payload is unusable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excluded {
    /// The proposal id.
    pub proposal_id: String,
    /// Why: the decode, envelope or validation failure.
    pub reason: String,
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

/// One decodable `stream-mapping` proposal.
struct Candidate<'a> {
    seq: i64,
    id: &'a str,
    source: SourceId,
    identity: String,
    mapping: StreamMapping,
}

/// Decodes a `stream-mapping` payload into its source, mapping and identity.
///
/// # Errors
/// A message naming the failure: not JSON of the envelope's shape, another envelope format,
/// an invalid source id, a mapping that fails [`StreamMapping::validate`], or a mapping with
/// links, which [`MappingEngine`] does not execute yet (decision 0027).
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
    if !envelope.mapping.links.is_empty() {
        let error = MappingEngineError::LinksNotExecuted(envelope.mapping.links.len());
        return Err(format!("mapping: {error}"));
    }
    Ok((source, envelope.mapping, identity))
}

/// The latest decision of one decider, by decision `seq` (never by timestamp, decision 0019).
fn later<'a>(slot: &mut Option<&'a StoredDecision>, decision: &'a StoredDecision) {
    if slot.is_none_or(|current| decision.seq > current.seq) {
        *slot = Some(decision);
    }
}

/// Every [`STREAM_MAPPING_CLASS`] proposal, decoded, in ascending `seq`; the unusable ones
/// apart, in proposal order.
fn candidates(proposals: &[StoredProposal]) -> (Vec<Candidate<'_>>, Vec<Excluded>) {
    let mut excluded = Vec::new();
    let mut candidates = Vec::new();
    for proposal in proposals.iter().filter(|p| p.class == STREAM_MAPPING_CLASS) {
        match decode_envelope(&proposal.payload) {
            Ok((source, mapping, identity)) => candidates.push(Candidate {
                seq: proposal.seq,
                id: &proposal.id,
                source,
                identity,
                mapping,
            }),
            Err(reason) => excluded.push(Excluded {
                proposal_id: proposal.id.clone(),
                reason,
            }),
        }
    }
    candidates.sort_by_key(|c| c.seq);
    (candidates, excluded)
}

/// The resolution rule (decision 0023). Pure: the same rows always resolve the same way.
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
    let (candidates, excluded) = candidates(proposals);

    // Latest human and policy decision per proposal id.
    let mut latest: BTreeMap<&str, [Option<&StoredDecision>; 2]> = BTreeMap::new();
    for decision in decisions {
        let slot = match decision.decider {
            Decider::Human => 0,
            Decider::Policy => 1,
            Decider::Agent | Decider::Evidence => continue,
        };
        later(
            &mut latest.entry(decision.proposal_id.as_str()).or_default()[slot],
            decision,
        );
    }
    // Rule 2: the latest human decision per (source, identity).
    let mut human: BTreeMap<(&SourceId, &str), Option<&StoredDecision>> = BTreeMap::new();
    for candidate in &candidates {
        let slot = human
            .entry((&candidate.source, candidate.identity.as_str()))
            .or_default();
        if let Some(decision) = latest.get(candidate.id).and_then(|d| d[0]) {
            later(slot, decision);
        }
    }
    let is_accepted = |candidate: &Candidate<'_>| match human
        .get(&(&candidate.source, candidate.identity.as_str()))
        .copied()
        .flatten()
    {
        Some(decision) => decision.outcome == Outcome::Accept,
        None => latest
            .get(candidate.id)
            .and_then(|d| d[1])
            .is_some_and(|decision| decision.outcome == Outcome::Accept),
    };

    let mut routes = BTreeMap::new();
    // Ascending seq: the last accepted candidate per source is the effective one (rule 4).
    let accepted: Vec<&Candidate<'_>> = candidates.iter().filter(|c| is_accepted(c)).collect();
    for candidate in &accepted {
        routes.insert(candidate.source.clone(), *candidate);
    }
    Resolution {
        routes: routes
            .into_iter()
            .map(|(source, effective)| {
                let earliest = accepted
                    .iter()
                    .find(|c| c.source == source && c.identity == effective.identity)
                    .map_or(effective.id, |c| c.id);
                let resolved = Resolved {
                    proposal_id: earliest.to_owned(),
                    identity: effective.identity.clone(),
                    mapping: effective.mapping.clone(),
                };
                (source, resolved)
            })
            .collect(),
        excluded,
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
