//! The proposal store's read and decision-write service (decisions 0019, 0020, 0023): the one
//! implementation both write surfaces call. MCP `decision_record` records an agent decision;
//! `s2w proposals decide` records a human one. Policy and evidence decisions are never written
//! here: they belong to producers, not to a surface.
//!
//! Every call opens the store afresh. A read goes through the lockless reader and never creates
//! the store; a write checks the proposal through that reader first, so an unknown id never
//! opens (and so never creates) the writable store, then holds the writer lock for one append.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use s2w_log::{
    Actor, Decider, LogError, LogPosition, NewDecision, NewProposal, Outcome, ProposalStore,
    SqliteProposalStore, StoredProposal,
};
use s2w_model::{SourceId, StreamMapping};
use serde::Serialize;

use crate::query::{
    DASHBOARD_MANIFEST_CLASS, DecisionDto, ProposalDto, QueryError, decode_dashboard_envelope,
    open_proposal_reader,
};
use crate::routes::{self, ENVELOPE_FORMAT, MappingEnvelope, STREAM_MAPPING_CLASS};

pub use crate::query::read_view;

/// The decider a surface may write, with what it records about the one deciding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seat {
    /// An agent's opinion (MCP `decision_record`); the basis is stored verbatim.
    Agent,
    /// Human review (`s2w proposals decide`); the basis is stored as `reviewer=<id>; <basis>`.
    Human {
        /// The reviewer's identity, checked by [`check_reviewer`].
        reviewer: String,
    },
}

/// A stored decision and, for a `stream-mapping` proposal, the source it may reroute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The appended row.
    pub decision: DecisionDto,
    /// The source the decided `stream-mapping` proposal names; `None` for any other class, or
    /// for a mapping proposal whose payload does not decode (it never routes). Pass it to
    /// [`route_after`] to see what the source runs now. That read-back is separate because the
    /// decision is already stored: a failed read must not look like a failed write.
    pub mapping_source: Option<SourceId>,
}

/// One source's route, read back from the store after a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteAfter {
    /// The source the decided mapping proposal names.
    pub source: String,
    /// The mapping identity the source now runs; `None` when it is unrouted.
    pub mapping: Option<String>,
    /// The proposal that mapping comes from; `None` when the source is unrouted.
    pub proposal_id: Option<String>,
}

/// Parses `accept` or `reject`.
///
/// # Errors
/// [`QueryError::BadParameter`] naming `outcome` for anything else.
pub fn parse_outcome(raw: &str) -> Result<Outcome, QueryError> {
    match raw {
        "accept" => Ok(Outcome::Accept),
        "reject" => Ok(Outcome::Reject),
        other => Err(QueryError::BadParameter {
            name: "outcome",
            reason: format!("'{other}' is not one of accept, reject"),
        }),
    }
}

/// Checks a human reviewer identity: non-empty, with no whitespace, control character or `;`,
/// so `reviewer=<id>; ` stays a prefix that splits at its first `; `.
///
/// # Errors
/// [`QueryError::BadParameter`] naming `reviewer`.
pub fn check_reviewer(reviewer: &str) -> Result<(), QueryError> {
    check_identity("reviewer", reviewer)
}

/// Checks a human proposal author's identity by the same rule as [`check_reviewer`], so one
/// person has one spelling on both sides of a proposal.
///
/// # Errors
/// [`QueryError::BadParameter`] naming `author`.
pub fn check_author(author: &str) -> Result<(), QueryError> {
    check_identity("author", author)
}

fn check_identity(name: &'static str, value: &str) -> Result<(), QueryError> {
    if value.is_empty() {
        return Err(QueryError::BadParameter {
            name,
            reason: "must not be empty".to_owned(),
        });
    }
    if value
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control() || ch == ';')
    {
        return Err(QueryError::BadParameter {
            name,
            reason: format!("'{value}' must not contain whitespace, control characters or ';'"),
        });
    }
    Ok(())
}

fn non_blank(name: &'static str, value: &str) -> Result<(), QueryError> {
    if value.trim().is_empty() {
        return Err(QueryError::BadParameter {
            name,
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(())
}

fn now_ms() -> Result<i64, QueryError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| QueryError::Storage(format!("system clock before epoch: {error}")))?;
    i64::try_from(elapsed.as_millis())
        .map_err(|error| QueryError::Storage(format!("system clock out of range: {error}")))
}

/// Appends one decision by `seat` on an existing proposal in `log_dir`'s store.
///
/// An `accept` (by either seat) on a `stream-mapping` proposal is refused unless its payload decodes as a
/// valid envelope: routing excludes such a row whatever is decided, so the accept could never
/// take effect. A reject is always allowed, so a bad row can still be revoked.
///
/// # Errors
/// [`QueryError::BadParameter`] for a blank `proposal_id` or `basis`, an invalid reviewer, or an
/// accept on an invalid mapping envelope; [`QueryError::UnknownProposal`] when the store or the
/// id does not exist (no file is created); [`QueryError::StoreLocked`] when another writer holds
/// the store; [`QueryError::Storage`] otherwise.
pub fn record_decision(
    log_dir: &Path,
    seat: &Seat,
    proposal_id: &str,
    outcome: Outcome,
    basis: &str,
) -> Result<Recorded, QueryError> {
    non_blank("proposal_id", proposal_id)?;
    non_blank("basis", basis)?;
    let (decider, stored_basis) = match seat {
        Seat::Agent => (Decider::Agent, basis.to_owned()),
        Seat::Human { reviewer } => {
            // The CLI checks this at parse time for a usage exit; the service checks again so no
            // caller can store a reviewer the `reviewer=<id>; ` prefix cannot split.
            check_reviewer(reviewer)?;
            (Decider::Human, format!("reviewer={reviewer}; {basis}"))
        }
    };
    let unknown = || QueryError::UnknownProposal {
        id: proposal_id.to_owned(),
    };
    // One lockless read answers both "is it known" and "what does its payload say"; proposals
    // are append-only, so a known id cannot become unknown before the writer opens.
    let proposal = open_proposal_reader(log_dir)?
        .map(|reader| reader.proposal(proposal_id))
        .transpose()?
        .flatten()
        .ok_or_else(unknown)?;
    let mapping_source = mapping_source(&proposal, outcome)?;
    check_dashboard_accept(&proposal, outcome)?;
    let mut store = SqliteProposalStore::open(log_dir).map_err(|error| match error {
        LogError::Locked => QueryError::StoreLocked,
        other => other.into(),
    })?;
    let stored = store.append_decision(&NewDecision {
        proposal_id: proposal_id.to_owned(),
        decider,
        outcome,
        basis: stored_basis,
        decided_at_ms: now_ms()?,
    })?;
    drop(store);
    Ok(Recorded {
        decision: DecisionDto::from(&stored),
        mapping_source,
    })
}

/// A stored human `stream-mapping` proposal and the mapping identity it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    /// The stored row (an identical earlier row on a re-run).
    pub proposal: ProposalDto,
    /// The mapping's identity, the key routing and revocation bind to (decision 0023).
    pub identity: String,
}

/// Appends one human-authored `stream-mapping` proposal for `source` to `log_dir`'s store,
/// creating the store if needed. The mapping is `mapping_json`, a `StreamMapping` document
/// (decision 0021). Nothing is decided: routing needs a separate human accept
/// ([`record_decision`]), exactly as for a producer's proposal.
///
/// The id is [`routes::proposal_id`] over the human actor, the source and the mapping
/// identity, so the same author proposing the same mapping for the same source gets the same
/// id, and a re-run is an identical retry that returns the stored row. A hand-authored mapping
/// is not derived from a log window, so `snapshot_offset` is the log's first position; routing
/// never reads it (decision 0023).
///
/// # Errors
/// [`QueryError::BadParameter`] for an invalid author, source or mapping;
/// [`QueryError::StoreLocked`] when another writer holds the store; [`QueryError::Storage`]
/// otherwise.
pub fn record_mapping_proposal(
    log_dir: &Path,
    author: &str,
    source: &str,
    mapping_json: &[u8],
) -> Result<Proposed, QueryError> {
    check_author(author)?;
    let source = SourceId::new(source).map_err(|error| QueryError::BadParameter {
        name: "source",
        reason: error.to_string(),
    })?;
    let bad_mapping = |reason: String| QueryError::BadParameter {
        name: "mapping",
        reason,
    };
    let mapping: StreamMapping =
        serde_json::from_slice(mapping_json).map_err(|error| bad_mapping(error.to_string()))?;
    let payload = serde_json::to_vec(&MappingEnvelope {
        format: ENVELOPE_FORMAT,
        source: source.as_str().to_owned(),
        mapping,
    })
    .map_err(|error| bad_mapping(error.to_string()))?;
    // The one decoder routing uses: a row it would exclude is never stored.
    let (_, _, identity) = routes::decode_envelope(&payload).map_err(bad_mapping)?;
    let actor = Actor::Human {
        id: author.to_owned(),
    };
    let first = LogPosition::from_u64(1)
        .ok_or_else(|| QueryError::Storage("log position 1 is out of range".to_owned()))?;
    let id = routes::proposal_id(&actor, &source, first, first, &identity);
    let mut store = SqliteProposalStore::open(log_dir).map_err(|error| match error {
        LogError::Locked => QueryError::StoreLocked,
        other => other.into(),
    })?;
    let stored = store
        .append_proposal(&NewProposal {
            id: id.clone(),
            class: STREAM_MAPPING_CLASS.to_owned(),
            actor,
            snapshot_offset: first,
            payload,
            proposed_at_ms: now_ms()?,
        })
        .map_err(|error| match error {
            LogError::Locked => QueryError::StoreLocked,
            other => other.into(),
        })?;
    drop(store);
    Ok(Proposed {
        proposal: ProposalDto::from(&stored.summary()),
        identity,
    })
}

/// The source a `stream-mapping` proposal names, or `None` for any other class and for a mapping
/// whose payload does not decode.
///
/// # Errors
/// [`QueryError::BadParameter`] naming `proposal` for an accept on an undecodable envelope.
fn mapping_source(
    proposal: &StoredProposal,
    outcome: Outcome,
) -> Result<Option<SourceId>, QueryError> {
    if proposal.class != STREAM_MAPPING_CLASS {
        return Ok(None);
    }
    match routes::decode_envelope(&proposal.payload) {
        Ok((source, _, _)) => Ok(Some(source)),
        Err(reason) if outcome == Outcome::Accept => Err(QueryError::BadParameter {
            name: "proposal",
            reason: format!(
                "{} is not a valid {STREAM_MAPPING_CLASS} envelope, so routing excludes it and \
                 an accept could never take effect ({reason}); reject it instead",
                proposal.id
            ),
        }),
        Err(_) => Ok(None),
    }
}

/// Refuses an accept on a `dashboard-manifest` proposal that resolution excludes: an
/// undecodable envelope or a null manifest (decision 0029). Any other class or outcome passes.
///
/// # Errors
/// [`QueryError::BadParameter`] naming `proposal`.
fn check_dashboard_accept(proposal: &StoredProposal, outcome: Outcome) -> Result<(), QueryError> {
    if proposal.class != DASHBOARD_MANIFEST_CLASS || outcome != Outcome::Accept {
        return Ok(());
    }
    decode_dashboard_envelope(&proposal.payload)
        .map(drop)
        .map_err(|reason| QueryError::BadParameter {
            name: "proposal",
            reason: format!(
                "{} is not a usable {DASHBOARD_MANIFEST_CLASS} envelope, so resolution excludes \
                 it and an accept could never take effect ({reason}); reject it instead",
                proposal.id
            ),
        })
}

/// What `source` runs now, resolved from `log_dir`'s stored mappings and decisions.
///
/// # Errors
/// [`QueryError::Storage`] if the store cannot be read.
pub fn route_after(log_dir: &Path, source: &SourceId) -> Result<RouteAfter, QueryError> {
    let resolution =
        routes::load(log_dir).map_err(|error| QueryError::Storage(error.to_string()))?;
    let resolved = resolution.routes.get(source);
    Ok(RouteAfter {
        source: source.as_str().to_owned(),
        mapping: resolved.map(|resolved| resolved.identity.clone()),
        proposal_id: resolved.map(|resolved| resolved.proposal_id.clone()),
    })
}
