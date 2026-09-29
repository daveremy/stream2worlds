//! Pure grading of persisted records. Policy routing is separate from accuracy.

use std::collections::BTreeMap;

use super::{Actor, Decider, Outcome, StoredDecision, StoredProposal};

/// Independent accepted/rejected counts; an empty tally has no accuracy estimate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    /// Accepted proposals.
    pub accepted: u64,
    /// Rejected proposals.
    pub rejected: u64,
}

impl Tally {
    /// Returns (accepted, accepted + rejected), with (0, 0) for no observations.
    #[must_use]
    pub const fn fraction(&self) -> (u64, u64) {
        (self.accepted, self.accepted + self.rejected)
    }

    fn count(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Accept => self.accepted += 1,
            Outcome::Reject => self.rejected += 1,
        }
    }
}

/// Grades for exactly one class and actor (including model version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorClassGrade {
    /// Caller-assigned class.
    pub class: String,
    /// Author whose proposals are counted.
    pub actor: Actor,
    /// Every proposal by this actor in this class.
    pub proposed: u64,
    /// Proposals with no human or evidence decision, including policy-only rows.
    pub ungraded: u64,
    /// Latest policy accepts: routing counts, not accuracy.
    pub policy_accepted: u64,
    /// Latest policy rejects: routing counts, not accuracy.
    pub policy_rejected: u64,
    /// Latest human review per proposal; never add to evidence's denominator.
    pub human: Tally,
    /// Latest evidence decision per proposal; no precedence over human review here.
    pub evidence: Tally,
    /// Policy-accepted proposals graded by human/evidence: any latest reject wins.
    /// This cross-tab is not time-ordered relative to policy acceptance.
    pub policy_applied: Tally,
    /// Policy-accepted proposals with neither human nor evidence decisions.
    pub policy_applied_ungraded: u64,
}

impl ActorClassGrade {
    fn new(proposal: &StoredProposal) -> Self {
        Self {
            class: proposal.class.clone(),
            actor: proposal.actor.clone(),
            proposed: 0,
            ungraded: 0,
            policy_accepted: 0,
            policy_rejected: 0,
            human: Tally::default(),
            evidence: Tally::default(),
            policy_applied: Tally::default(),
            policy_applied_ungraded: 0,
        }
    }

    fn count(&mut self, outcomes: [Option<Outcome>; 3]) {
        let [policy, human, evidence] = outcomes;
        self.proposed += 1;
        if let Some(outcome) = human {
            self.human.count(outcome);
        }
        if let Some(outcome) = evidence {
            self.evidence.count(outcome);
        }
        let graded = match (human, evidence) {
            (Some(Outcome::Reject), _) | (_, Some(Outcome::Reject)) => Some(Outcome::Reject),
            (Some(Outcome::Accept), _) | (_, Some(Outcome::Accept)) => Some(Outcome::Accept),
            (None, None) => None,
        };
        if graded.is_none() {
            self.ungraded += 1;
        }
        match policy {
            Some(Outcome::Accept) => {
                self.policy_accepted += 1;
                match graded {
                    Some(outcome) => self.policy_applied.count(outcome),
                    None => self.policy_applied_ungraded += 1,
                }
            }
            Some(Outcome::Reject) => self.policy_rejected += 1,
            None => {}
        }
    }
}

/// Grades persisted rows, ordered by (class, actor). The largest decision `seq` wins per
/// (proposal, decider), regardless of input order or caller timestamps. Policy counts route;
/// human/evidence tallies independently grade. Decisions for absent proposals are ignored.
/// Inputs are expected to be store rows with unique proposal ids and decision sequences.
/// No threshold or auto-apply logic runs here.
#[must_use]
pub fn grade(proposals: &[StoredProposal], decisions: &[StoredDecision]) -> Vec<ActorClassGrade> {
    let mut latest: BTreeMap<(&str, Decider), &StoredDecision> = BTreeMap::new();
    for decision in decisions {
        let entry = latest
            .entry((&decision.proposal_id, decision.decider))
            .or_insert(decision);
        if decision.seq > entry.seq {
            *entry = decision;
        }
    }
    let mut grades = BTreeMap::new();
    for proposal in proposals {
        let entry = grades
            .entry((proposal.class.clone(), proposal.actor.clone()))
            .or_insert_with(|| ActorClassGrade::new(proposal));
        let outcomes = [Decider::Policy, Decider::Human, Decider::Evidence].map(|decider| {
            latest
                .get(&(proposal.id.as_str(), decider))
                .map(|decision| decision.outcome)
        });
        entry.count(outcomes);
    }
    grades.into_values().collect()
}
