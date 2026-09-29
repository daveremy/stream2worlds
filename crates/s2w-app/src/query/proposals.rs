//! The proposals view (decision 0020): stored proposals, decisions and their grades, projected
//! from rows only. The pure half is [`proposals_view`]; `QueryState::proposals` reads the rows
//! fresh from the log directory on every call.

use s2w_log::{Actor, ActorClassGrade, ProposalSummary, StoredDecision, Tally, grade};
use serde::Serialize;

/// Everything the proposal store holds, minus payload bytes, plus the grades of those rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProposalsView {
    /// Proposals in store sequence order.
    pub proposals: Vec<ProposalDto>,
    /// Decisions in store sequence order, corrections included.
    pub decisions: Vec<DecisionDto>,
    /// `s2w_log::grade` over exactly these rows, ordered by (class, actor).
    pub grades: Vec<GradeDto>,
}

/// A proposal's author.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActorDto {
    /// A human author.
    Human {
        /// Caller-assigned identity.
        id: String,
    },
    /// A model author.
    Agent {
        /// Model identity.
        model: String,
        /// Model version when the proposal was produced.
        version: String,
    },
}

impl From<&Actor> for ActorDto {
    fn from(actor: &Actor) -> Self {
        match actor {
            Actor::Human { id } => Self::Human { id: id.clone() },
            Actor::Agent { model, version } => Self::Agent {
                model: model.clone(),
                version: version.clone(),
            },
        }
    }
}

/// One stored proposal without its payload.
///
/// `payload_hash` is the stored value: summaries do not recompute it, so it is not
/// re-verified against the payload here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalDto {
    /// Store-local write sequence.
    pub seq: i64,
    /// Stable proposal identity.
    pub id: String,
    /// Opaque proposal class tag.
    pub class: String,
    /// The proposal's author.
    pub actor: ActorDto,
    /// The event-log position the proposal was made against.
    pub snapshot_offset: u64,
    /// Stored payload hash as 16 lowercase hex digits (the unsigned 64-bit
    /// value), a string because JSON numbers above 2^53 lose precision in JS.
    /// Not re-verified by summaries.
    pub payload_hash: String,
    /// Caller-supplied Unix milliseconds.
    pub proposed_at_ms: i64,
}

impl From<&ProposalSummary> for ProposalDto {
    fn from(summary: &ProposalSummary) -> Self {
        Self {
            seq: summary.seq,
            id: summary.id.clone(),
            class: summary.class.clone(),
            actor: ActorDto::from(&summary.actor),
            snapshot_offset: summary.snapshot_offset.as_u64(),
            payload_hash: format!(
                "{:016x}",
                u64::from_ne_bytes(summary.payload_hash.to_ne_bytes())
            ),
            proposed_at_ms: summary.proposed_at_ms,
        }
    }
}

/// One stored decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecisionDto {
    /// Store-local decision sequence.
    pub seq: i64,
    /// The proposal decided on.
    pub proposal_id: String,
    /// `policy`, `human`, `evidence` or `agent`.
    pub decider: &'static str,
    /// `accept` or `reject`.
    pub outcome: &'static str,
    /// Opaque policy identity, reviewer or evidence reference.
    pub basis: String,
    /// Caller-supplied Unix milliseconds.
    pub decided_at_ms: i64,
}

impl From<&StoredDecision> for DecisionDto {
    fn from(decision: &StoredDecision) -> Self {
        Self {
            seq: decision.seq,
            proposal_id: decision.proposal_id.clone(),
            decider: decision.decider.as_str(),
            outcome: decision.outcome.as_str(),
            basis: decision.basis.clone(),
            decided_at_ms: decision.decided_at_ms,
        }
    }
}

/// Accepted and rejected counts with their fraction `[accepted, accepted + rejected]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TallyDto {
    /// Accepted proposals.
    pub accepted: u64,
    /// Rejected proposals.
    pub rejected: u64,
    /// `[accepted, accepted + rejected]`; `[0, 0]` means no observations.
    pub fraction: [u64; 2],
}

impl From<Tally> for TallyDto {
    fn from(tally: Tally) -> Self {
        let (numerator, denominator) = tally.fraction();
        Self {
            accepted: tally.accepted,
            rejected: tally.rejected,
            fraction: [numerator, denominator],
        }
    }
}

/// Grades for one class and actor; see `s2w_log::ActorClassGrade` for each field's meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GradeDto {
    /// Opaque proposal class tag.
    pub class: String,
    /// The graded author.
    pub actor: ActorDto,
    /// Every proposal by this actor in this class.
    pub proposed: u64,
    /// Proposals with no human or evidence decision.
    pub ungraded: u64,
    /// Latest policy accepts: routing counts, not accuracy.
    pub policy_accepted: u64,
    /// Latest policy rejects: routing counts, not accuracy.
    pub policy_rejected: u64,
    /// Latest human review per proposal.
    pub human: TallyDto,
    /// Latest evidence decision per proposal.
    pub evidence: TallyDto,
    /// Latest agent decision per proposal; never a routing or grading signal.
    pub agent: TallyDto,
    /// Policy-accepted proposals graded by human or evidence decisions.
    pub policy_applied: TallyDto,
    /// Policy-accepted proposals with neither human nor evidence decisions.
    pub policy_applied_ungraded: u64,
}

impl From<&ActorClassGrade> for GradeDto {
    fn from(grade: &ActorClassGrade) -> Self {
        Self {
            class: grade.class.clone(),
            actor: ActorDto::from(&grade.actor),
            proposed: grade.proposed,
            ungraded: grade.ungraded,
            policy_accepted: grade.policy_accepted,
            policy_rejected: grade.policy_rejected,
            human: grade.human.into(),
            evidence: grade.evidence.into(),
            agent: grade.agent.into(),
            policy_applied: grade.policy_applied.into(),
            policy_applied_ungraded: grade.policy_applied_ungraded,
        }
    }
}

/// Projects stored rows into the view; grades are exactly `s2w_log::grade(proposals, decisions)`.
#[must_use]
pub fn proposals_view(
    proposals: &[ProposalSummary],
    decisions: &[StoredDecision],
) -> ProposalsView {
    ProposalsView {
        proposals: proposals.iter().map(ProposalDto::from).collect(),
        decisions: decisions.iter().map(DecisionDto::from).collect(),
        grades: grade(proposals, decisions)
            .iter()
            .map(GradeDto::from)
            .collect(),
    }
}
