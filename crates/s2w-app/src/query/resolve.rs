//! The resolution rule of decision 0023, generic over a proposal class and its scope key: per
//! key, which accepted proposal is in effect. `routes` resolves `stream-mapping` per source
//! with it, and the dashboard read resolves `dashboard-manifest` per world (decision 0029).

use std::collections::BTreeMap;

use s2w_log::{Decider, Outcome, StoredDecision, StoredProposal};

/// A proposal left out of resolution because its payload is unusable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excluded {
    /// The proposal id.
    pub proposal_id: String,
    /// Why: the decode, envelope or validation failure.
    pub reason: String,
}

/// A scope key's effective proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Winner<T> {
    /// The earliest-seq accepted proposal carrying [`Self::identity`] for this key, so a
    /// same-bytes re-proposal changes neither the effective provenance nor a restart's report.
    pub proposal_id: String,
    /// The payload's identity.
    pub identity: String,
    /// The decoded payload.
    pub value: T,
}

/// What [`resolve_class`] found: one winner per scope key, and every unusable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassResolution<K, T> {
    /// Keys with an effective proposal. A key absent here has none.
    pub winners: BTreeMap<K, Winner<T>>,
    /// Rows excluded from resolution, in proposal order. Reported loudly by the caller, never
    /// fatal: one malformed row must not stop a read.
    pub excluded: Vec<Excluded>,
}

/// One decodable proposal of the class.
struct Candidate<'a, K, T> {
    seq: i64,
    id: &'a str,
    key: K,
    identity: String,
    value: T,
}

/// The latest decision of one decider, by decision `seq` (never by timestamp, decision 0019).
fn later<'a>(slot: &mut Option<&'a StoredDecision>, decision: &'a StoredDecision) {
    if slot.is_none_or(|current| decision.seq > current.seq) {
        *slot = Some(decision);
    }
}

/// The resolution rule (decision 0023). Pure: the same rows always resolve the same way.
///
/// 1. Only `class` proposals count; one whose payload `decode` refuses is [`Excluded`].
///    `decode` returns the payload's scope key, value and identity.
/// 2. `human` decides per (key, identity): the latest `human` decision on any proposal
///    carrying that identity for that key wins. A human reject therefore binds the payload,
///    not only the proposal id: a later proposal of the same bytes stays rejected whatever
///    `policy` says, until a later human accept on any of them.
/// 3. With no `human` decision on that identity, a proposal is accepted when its own latest
///    `policy` decision is accept. `agent` (context, decision 0020) and `evidence` (a grading
///    signal) never count.
/// 4. A key's effective proposal is its accepted proposal with the largest proposal `seq`
///    (write order; `proposed_at_ms` is caller-supplied and never a tie-breaker).
/// 5. No accepted proposal: the key has no winner.
#[must_use]
pub fn resolve_class<K, T, F>(
    class: &str,
    decode: F,
    proposals: &[StoredProposal],
    decisions: &[StoredDecision],
) -> ClassResolution<K, T>
where
    K: Ord + Clone,
    T: Clone,
    F: Fn(&[u8]) -> Result<(K, T, String), String>,
{
    let mut excluded = Vec::new();
    let mut candidates = Vec::new();
    for proposal in proposals.iter().filter(|p| p.class == class) {
        match decode(&proposal.payload) {
            Ok((key, value, identity)) => candidates.push(Candidate {
                seq: proposal.seq,
                id: &proposal.id,
                key,
                identity,
                value,
            }),
            Err(reason) => excluded.push(Excluded {
                proposal_id: proposal.id.clone(),
                reason,
            }),
        }
    }
    candidates.sort_by_key(|c| c.seq);

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
    // Rule 2: the latest human decision per (key, identity).
    let mut human: BTreeMap<(&K, &str), Option<&StoredDecision>> = BTreeMap::new();
    for candidate in &candidates {
        let slot = human
            .entry((&candidate.key, candidate.identity.as_str()))
            .or_default();
        if let Some(decision) = latest.get(candidate.id).and_then(|d| d[0]) {
            later(slot, decision);
        }
    }
    let is_accepted = |candidate: &Candidate<'_, K, T>| match human
        .get(&(&candidate.key, candidate.identity.as_str()))
        .copied()
        .flatten()
    {
        Some(decision) => decision.outcome == Outcome::Accept,
        None => latest
            .get(candidate.id)
            .and_then(|d| d[1])
            .is_some_and(|decision| decision.outcome == Outcome::Accept),
    };

    // Ascending seq: the last accepted candidate per key is the effective one (rule 4).
    let accepted: Vec<&Candidate<'_, K, T>> =
        candidates.iter().filter(|c| is_accepted(c)).collect();
    let mut effective: BTreeMap<&K, &Candidate<'_, K, T>> = BTreeMap::new();
    for candidate in &accepted {
        effective.insert(&candidate.key, candidate);
    }
    let winners = effective
        .into_iter()
        .map(|(key, winner)| {
            let earliest = accepted
                .iter()
                .find(|c| c.key == *key && c.identity == winner.identity)
                .map_or(winner.id, |c| c.id);
            let resolved = Winner {
                proposal_id: earliest.to_owned(),
                identity: winner.identity.clone(),
                value: winner.value.clone(),
            };
            (key.clone(), resolved)
        })
        .collect();
    ClassResolution { winners, excluded }
}
