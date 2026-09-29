//! Append-only System 2 proposals and decisions, with opaque payloads and attributed grades.
//! See decision 0019. Storage records policy routing; it never applies proposals itself.
//! Agent decisions record an agent's opinion and never count as routing or grading signals.

use std::collections::BTreeMap;

use crate::{LogError, LogPosition, check_payload_size, content_hash};

mod grading;
mod sqlite;

pub use grading::{ActorClassGrade, Tally, grade};
pub use sqlite::{PROPOSAL_DATABASE_FILE, ReadOnlySqliteProposalStore, SqliteProposalStore};

/// The author of a proposal; model versions have independent grading denominators.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Actor {
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

/// The source of a decision, distinct from the proposal's author.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Decider {
    /// Automatic routing, never an accuracy signal.
    Policy,
    /// Human review.
    Human,
    /// Confirmation or refutation by evidence.
    Evidence,
    /// An agent's opinion recorded through a tool: not policy routing, not human review and
    /// not evidence. It never counts toward routing or grading tallies other than its own.
    Agent,
}

/// A decision's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// Accepted or confirmed.
    Accept,
    /// Rejected or refuted.
    Reject,
}

/// Caller-supplied proposal; the store computes its payload hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProposal {
    /// Stable identity, reused on crash-retry.
    pub id: String,
    /// Opaque proposal class tag.
    pub class: String,
    /// Author of this proposal.
    pub actor: Actor,
    /// Snapshot position, checked against the event log by the consumer.
    pub snapshot_offset: LogPosition,
    /// Encoded proposal, opaque to this crate.
    pub payload: Vec<u8>,
    /// Caller-supplied Unix milliseconds; ignored on an otherwise identical retry.
    pub proposed_at_ms: i64,
}

/// A durable proposal in write order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProposal {
    /// Store-local write sequence (independent of decision sequences).
    pub seq: i64,
    /// Stable proposal identity.
    pub id: String,
    /// Opaque proposal class tag.
    pub class: String,
    /// Original author, including the model version.
    pub actor: Actor,
    /// Snapshot position the proposal was made against.
    pub snapshot_offset: LogPosition,
    /// Store-computed FNV-1a hash, verified on read; not a security hash.
    pub payload_hash: i64,
    /// Original encoded proposal.
    pub payload: Vec<u8>,
    /// Original caller-supplied Unix milliseconds.
    pub proposed_at_ms: i64,
}

/// A proposal without its payload bytes, for listing and grading.
///
/// Read through [`ProposalStore::proposal_summaries`], which does not recompute the payload
/// hash: `payload_hash` is the stored value, unverified. Only [`ProposalStore::proposals`]
/// verifies payload integrity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalSummary {
    /// Store-local write sequence (independent of decision sequences).
    pub seq: i64,
    /// Stable proposal identity.
    pub id: String,
    /// Opaque proposal class tag.
    pub class: String,
    /// Original author, including the model version.
    pub actor: Actor,
    /// Snapshot position the proposal was made against.
    pub snapshot_offset: LogPosition,
    /// Stored FNV-1a payload hash; not recomputed when read as a summary.
    pub payload_hash: i64,
    /// Original caller-supplied Unix milliseconds.
    pub proposed_at_ms: i64,
}

impl StoredProposal {
    /// Returns this proposal's fields without the payload bytes.
    #[must_use]
    pub fn summary(&self) -> ProposalSummary {
        ProposalSummary {
            seq: self.seq,
            id: self.id.clone(),
            class: self.class.clone(),
            actor: self.actor.clone(),
            snapshot_offset: self.snapshot_offset,
            payload_hash: self.payload_hash,
            proposed_at_ms: self.proposed_at_ms,
        }
    }
}

/// A new routing or grading decision. Corrections append another row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDecision {
    /// Identity of an existing proposal.
    pub proposal_id: String,
    /// Source of this decision.
    pub decider: Decider,
    /// Accepted or rejected.
    pub outcome: Outcome,
    /// Opaque, non-empty policy identity, reviewer or evidence reference.
    pub basis: String,
    /// Caller-supplied Unix milliseconds, possibly before the proposal's timestamp.
    pub decided_at_ms: i64,
}

/// A durable decision; largest sequence wins per (proposal, decider) when grading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDecision {
    /// Store-local decision write sequence.
    pub seq: i64,
    /// Identity of the proposal judged.
    pub proposal_id: String,
    /// Source of this decision.
    pub decider: Decider,
    /// Accepted or rejected.
    pub outcome: Outcome,
    /// Opaque policy identity, reviewer or evidence reference.
    pub basis: String,
    /// Original caller-supplied Unix milliseconds.
    pub decided_at_ms: i64,
}

/// The append-only proposal and decision seam, independent of System 2's payload format.
pub trait ProposalStore {
    /// Appends a proposal or returns an identical prior row, ignoring the retry's timestamp.
    ///
    /// # Errors
    /// Invalid metadata or conflicting identity is `Corrupt`; oversized payload is `TooLarge`.
    /// Storage errors leave no new row.
    fn append_proposal(&mut self, proposal: &NewProposal) -> Result<StoredProposal, LogError>;

    /// Appends a decision, including repeated decisions and corrections.
    ///
    /// # Errors
    /// Invalid metadata or an unknown proposal is `Corrupt`. Storage failures store nothing.
    fn append_decision(&mut self, decision: &NewDecision) -> Result<StoredDecision, LogError>;

    /// Returns proposals in sequence order, checking payload integrity.
    ///
    /// # Errors
    /// Returns storage errors or `Corrupt` for malformed rows or a payload hash mismatch.
    fn proposals(&self) -> Result<Vec<StoredProposal>, LogError>;

    /// Returns proposals in sequence order without payload bytes.
    ///
    /// The payload hash is NOT recomputed: a summary's `payload_hash` is the stored value,
    /// unverified. Only [`ProposalStore::proposals`] verifies payload integrity. Actor shape is
    /// still validated.
    ///
    /// # Errors
    /// Returns storage errors or `Corrupt` for malformed rows.
    fn proposal_summaries(&self) -> Result<Vec<ProposalSummary>, LogError>;

    /// Returns all decisions in sequence order.
    ///
    /// # Errors
    /// Returns storage errors or `Corrupt` for unknown protocol strings.
    fn decisions(&self) -> Result<Vec<StoredDecision>, LogError>;
}

fn non_empty(value: &str, field: &str) -> Result<(), LogError> {
    if value.is_empty() {
        return Err(LogError::Corrupt(format!("empty proposal record {field}")));
    }
    Ok(())
}

fn validate_proposal(proposal: &NewProposal) -> Result<(), LogError> {
    non_empty(&proposal.id, "id")?;
    non_empty(&proposal.class, "class")?;
    match &proposal.actor {
        Actor::Human { id } => non_empty(id, "human id")?,
        Actor::Agent { model, version } => {
            non_empty(model, "model")?;
            non_empty(version, "model version")?;
        }
    }
    proposal.snapshot_offset.to_sql()?;
    check_payload_size(proposal.payload.len())
}

fn validate_decision(decision: &NewDecision) -> Result<(), LogError> {
    non_empty(&decision.proposal_id, "proposal id")?;
    non_empty(&decision.basis, "basis")
}

fn check_integrity(proposal: &StoredProposal) -> Result<(), LogError> {
    if content_hash(&proposal.payload) != proposal.payload_hash {
        return Err(LogError::Corrupt(format!(
            "proposal {} payload hash mismatch",
            proposal.id
        )));
    }
    Ok(())
}

fn retry_proposal(stored: StoredProposal, new: &NewProposal) -> Result<StoredProposal, LogError> {
    check_integrity(&stored)?;
    if stored.class != new.class
        || stored.actor != new.actor
        || stored.snapshot_offset != new.snapshot_offset
        || stored.payload != new.payload
    {
        return Err(LogError::Corrupt(format!(
            "conflicting proposal id {}",
            new.id
        )));
    }
    Ok(stored)
}

fn stored_proposal(new: &NewProposal, seq: i64) -> StoredProposal {
    StoredProposal {
        seq,
        id: new.id.clone(),
        class: new.class.clone(),
        actor: new.actor.clone(),
        snapshot_offset: new.snapshot_offset,
        payload_hash: content_hash(&new.payload),
        payload: new.payload.clone(),
        proposed_at_ms: new.proposed_at_ms,
    }
}

fn stored_decision(new: &NewDecision, seq: i64) -> StoredDecision {
    StoredDecision {
        seq,
        proposal_id: new.proposal_id.clone(),
        decider: new.decider,
        outcome: new.outcome,
        basis: new.basis.clone(),
        decided_at_ms: new.decided_at_ms,
    }
}

fn next_sequence(len: usize) -> Result<i64, LogError> {
    i64::try_from(len)
        .ok()
        .and_then(|seq| seq.checked_add(1))
        .ok_or_else(|| LogError::Corrupt("proposal store sequence exhausted".into()))
}

/// An in-memory implementation with the same validation and retry semantics as SQLite.
#[derive(Debug, Default)]
pub struct InMemoryProposalStore {
    proposals: Vec<StoredProposal>,
    decisions: Vec<StoredDecision>,
    ids: BTreeMap<String, usize>,
}

impl InMemoryProposalStore {
    /// Constructs an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProposalStore for InMemoryProposalStore {
    fn append_proposal(&mut self, proposal: &NewProposal) -> Result<StoredProposal, LogError> {
        validate_proposal(proposal)?;
        if let Some(&index) = self.ids.get(&proposal.id) {
            return retry_proposal(self.proposals[index].clone(), proposal);
        }
        let stored = stored_proposal(proposal, next_sequence(self.proposals.len())?);
        self.ids.insert(stored.id.clone(), self.proposals.len());
        self.proposals.push(stored.clone());
        Ok(stored)
    }

    fn append_decision(&mut self, decision: &NewDecision) -> Result<StoredDecision, LogError> {
        validate_decision(decision)?;
        if !self.ids.contains_key(&decision.proposal_id) {
            return Err(LogError::Corrupt(format!(
                "unknown proposal {}",
                decision.proposal_id
            )));
        }
        let stored = stored_decision(decision, next_sequence(self.decisions.len())?);
        self.decisions.push(stored.clone());
        Ok(stored)
    }

    fn proposals(&self) -> Result<Vec<StoredProposal>, LogError> {
        for proposal in &self.proposals {
            check_integrity(proposal)?;
        }
        Ok(self.proposals.clone())
    }

    fn proposal_summaries(&self) -> Result<Vec<ProposalSummary>, LogError> {
        Ok(self.proposals.iter().map(StoredProposal::summary).collect())
    }

    fn decisions(&self) -> Result<Vec<StoredDecision>, LogError> {
        Ok(self.decisions.clone())
    }
}

#[cfg(test)]
mod tests;
