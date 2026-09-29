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

use s2w_log::{Decider, LogError, NewDecision, Outcome, ProposalStore, SqliteProposalStore};
use serde::Serialize;

use crate::query::{DecisionDto, ProposalsView, QueryError, open_proposal_reader, proposals_view};
use crate::routes::{self, STREAM_MAPPING_CLASS};

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

/// A stored decision and, for a `stream-mapping` proposal, what its source runs after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recorded {
    /// The appended row.
    pub decision: DecisionDto,
    /// The route of the decided proposal's source after the write; `None` for any other class,
    /// or for a mapping proposal whose payload does not decode (it never routes).
    pub route: Option<RouteAfter>,
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
    if reviewer.is_empty() {
        return Err(QueryError::BadParameter {
            name: "reviewer",
            reason: "must not be empty".to_owned(),
        });
    }
    if reviewer
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control() || ch == ';')
    {
        return Err(QueryError::BadParameter {
            name: "reviewer",
            reason: format!("'{reviewer}' must not contain whitespace, control characters or ';'"),
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

/// The proposals view of `log_dir`'s store: every summary, decision and grade. Empty when the
/// store file does not exist; never creates it.
///
/// # Errors
/// [`QueryError::Storage`] if the store exists but cannot be opened or read.
pub fn read_view(log_dir: &Path) -> Result<ProposalsView, QueryError> {
    let Some(reader) = open_proposal_reader(log_dir)? else {
        return Ok(ProposalsView::default());
    };
    Ok(proposals_view(
        &reader.proposal_summaries()?,
        &reader.decisions()?,
    ))
}

/// Appends one decision by `seat` on an existing proposal in `log_dir`'s store.
///
/// A human `accept` on a `stream-mapping` proposal is refused unless its payload decodes as a
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
    let mapping_source = if proposal.class == STREAM_MAPPING_CLASS {
        match routes::decode_envelope(&proposal.payload) {
            Ok((source, _, _)) => Some(source),
            Err(reason) if outcome == Outcome::Accept => {
                return Err(QueryError::BadParameter {
                    name: "proposal",
                    reason: format!(
                        "{proposal_id} is not a valid {STREAM_MAPPING_CLASS} envelope, so routing \
                         excludes it and an accept could never take effect ({reason}); reject it \
                         instead"
                    ),
                });
            }
            Err(_) => None,
        }
    } else {
        None
    };
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
    let route = match mapping_source {
        Some(source) => {
            let resolution =
                routes::load(log_dir).map_err(|error| QueryError::Storage(error.to_string()))?;
            let resolved = resolution.routes.get(&source);
            Some(RouteAfter {
                source: source.as_str().to_owned(),
                mapping: resolved.map(|resolved| resolved.identity.clone()),
                proposal_id: resolved.map(|resolved| resolved.proposal_id.clone()),
            })
        }
        None => None,
    };
    Ok(Recorded {
        decision: DecisionDto::from(&stored),
        route,
    })
}
