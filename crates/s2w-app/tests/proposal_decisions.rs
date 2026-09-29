//! The proposal decision service (`s2w_app::proposals`) that `s2w proposals` and MCP
//! `decision_record` share: a human decision moves the human tally and nothing else, a write
//! never creates the store for an unknown id, a held writer lock is `store_locked`, and a human
//! reject of the effective stream mapping revokes it (decision 0023).

use std::path::PathBuf;

use s2w_app::proposals::{
    Recorded, RouteAfter, Seat, check_reviewer, read_view, record_decision, route_after,
};
use s2w_app::query::{GradeDto, QueryError};
use s2w_app::routes::{ENVELOPE_FORMAT, MappingEnvelope, STREAM_MAPPING_CLASS};
use s2w_log::{
    Actor, Decider, LogPosition, NewDecision, NewProposal, Outcome, ProposalStore,
    ReadOnlySqliteProposalStore, SqliteProposalStore,
};
use s2w_model::StreamMapping;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

const SOURCE: &str = "test.mapped";

fn human(reviewer: &str) -> Seat {
    Seat::Human {
        reviewer: reviewer.to_owned(),
    }
}

fn mapping_a() -> Fallible<StreamMapping> {
    Ok(serde_json::from_str(include_str!(
        "../../s2w-system1/testdata/sample.mapping.json"
    ))?)
}

fn mapping_b() -> Fallible<StreamMapping> {
    let mut mapping = mapping_a()?;
    let rule = mapping.entities.first_mut().ok_or("no entity rule")?;
    rule.type_label = format!("{}-b", rule.type_label);
    Ok(mapping)
}

fn propose(store: &mut SqliteProposalStore, id: &str, class: &str, payload: Vec<u8>) -> TestResult {
    store.append_proposal(&NewProposal {
        id: id.to_owned(),
        class: class.to_owned(),
        actor: Actor::Agent {
            model: "m".to_owned(),
            version: "1".to_owned(),
        },
        snapshot_offset: LogPosition::from_u64(1).ok_or("position")?,
        payload,
        proposed_at_ms: 0,
    })?;
    Ok(())
}

fn decide(store: &mut SqliteProposalStore, id: &str, decider: Decider) -> TestResult {
    store.append_decision(&NewDecision {
        proposal_id: id.to_owned(),
        decider,
        outcome: Outcome::Accept,
        basis: "seeded".to_owned(),
        decided_at_ms: 0,
    })?;
    Ok(())
}

fn envelope(mapping: StreamMapping) -> Fallible<Vec<u8>> {
    Ok(serde_json::to_vec(&MappingEnvelope {
        format: ENVELOPE_FORMAT,
        source: SOURCE.to_owned(),
        mapping,
    })?)
}

/// What the decided mapping's source runs after `recorded`'s write.
fn route_of(dir: &std::path::Path, recorded: &Recorded) -> Fallible<RouteAfter> {
    let source = recorded
        .mapping_source
        .as_ref()
        .ok_or("no mapping source")?;
    Ok(route_after(dir, source)?)
}

/// One grade row's tallies as plain numbers: (human, agent, evidence, policy_applied).
fn tallies(grade: &GradeDto) -> [(u64, u64); 4] {
    [
        grade.human,
        grade.agent,
        grade.evidence,
        grade.policy_applied,
    ]
    .map(|tally| (tally.accepted, tally.rejected))
}

#[test]
fn a_human_decision_moves_the_human_tally_and_nothing_else() -> TestResult {
    let dir = TestDirectory::new("human")?;
    let mut store = SqliteProposalStore::open(&dir.0)?;
    propose(&mut store, "p1", "class-x", b"{}".to_vec())?;
    decide(&mut store, "p1", Decider::Policy)?;
    decide(&mut store, "p1", Decider::Agent)?;
    drop(store);
    let before = read_view(&dir.0)?;
    let recorded = record_decision(&dir.0, &human("dave"), "p1", Outcome::Reject, "looked")?;
    assert_eq!(recorded.decision.decider, "human");
    assert_eq!(recorded.decision.outcome, "reject");
    assert_eq!(recorded.decision.basis, "reviewer=dave; looked");
    assert_eq!(recorded.mapping_source, None);
    let after = read_view(&dir.0)?;
    assert_eq!(after.proposals, before.proposals);
    let [human_before, agent_before, evidence_before, _] =
        tallies(before.grades.first().ok_or("grade")?);
    let [human_after, agent_after, evidence_after, applied_after] =
        tallies(after.grades.first().ok_or("grade")?);
    assert_eq!(human_before, (0, 0));
    assert_eq!(human_after, (0, 1));
    assert_eq!(agent_after, agent_before);
    assert_eq!(agent_after, (1, 0));
    assert_eq!(evidence_after, evidence_before);
    // Policy applied it and the human rejected it: the policy-applied tally now counts a miss.
    assert_eq!(applied_after, (0, 1));
    Ok(())
}

#[test]
fn an_agent_seat_stores_its_basis_verbatim() -> TestResult {
    let dir = TestDirectory::new("agent")?;
    let mut store = SqliteProposalStore::open(&dir.0)?;
    propose(&mut store, "p1", "class-x", b"{}".to_vec())?;
    drop(store);
    let recorded = record_decision(&dir.0, &Seat::Agent, "p1", Outcome::Accept, "why")?;
    assert_eq!(recorded.decision.decider, "agent");
    assert_eq!(recorded.decision.basis, "why");
    Ok(())
}

#[test]
fn an_unknown_proposal_is_refused_without_creating_a_store() -> TestResult {
    let empty = TestDirectory::new("unknown-empty")?;
    let error = record_decision(&empty.0, &human("dave"), "p9", Outcome::Reject, "why");
    assert!(matches!(error, Err(QueryError::UnknownProposal { ref id }) if id == "p9"));
    assert_eq!(
        std::fs::read_dir(&empty.0)?.count(),
        0,
        "a file was created"
    );
    assert!(read_view(&empty.0)?.proposals.is_empty());
    assert_eq!(
        std::fs::read_dir(&empty.0)?.count(),
        0,
        "a read created a file"
    );

    let seeded = TestDirectory::new("unknown-seeded")?;
    let mut store = SqliteProposalStore::open(&seeded.0)?;
    propose(&mut store, "p1", "class-x", b"{}".to_vec())?;
    drop(store);
    let error = record_decision(&seeded.0, &human("dave"), "p9", Outcome::Reject, "why");
    assert!(matches!(error, Err(QueryError::UnknownProposal { .. })));
    assert!(
        ReadOnlySqliteProposalStore::open(&seeded.0)?
            .decisions()?
            .is_empty()
    );
    Ok(())
}

#[test]
fn a_held_writer_lock_is_store_locked_and_appends_nothing() -> TestResult {
    let dir = TestDirectory::new("locked")?;
    let mut store = SqliteProposalStore::open(&dir.0)?;
    propose(&mut store, "p1", "class-x", b"{}".to_vec())?;
    let error = record_decision(&dir.0, &human("dave"), "p1", Outcome::Accept, "why");
    assert!(matches!(error, Err(QueryError::StoreLocked)), "{error:?}");
    assert!(store.decisions()?.is_empty());
    Ok(())
}

#[test]
fn bad_parameters_are_refused_before_anything_is_read() -> TestResult {
    let dir = TestDirectory::new("params")?;
    for reviewer in ["", "a b", "a;b", "a\tb"] {
        assert!(check_reviewer(reviewer).is_err(), "{reviewer:?}");
        let error = record_decision(&dir.0, &human(reviewer), "p1", Outcome::Accept, "why");
        assert!(
            matches!(
                error,
                Err(QueryError::BadParameter {
                    name: "reviewer",
                    ..
                })
            ),
            "{reviewer:?}: {error:?}"
        );
    }
    let error = record_decision(&dir.0, &human("dave"), "p1", Outcome::Accept, "  ");
    assert!(matches!(
        error,
        Err(QueryError::BadParameter { name: "basis", .. })
    ));
    assert_eq!(std::fs::read_dir(&dir.0)?.count(), 0);
    Ok(())
}

#[test]
fn an_invalid_mapping_envelope_can_be_rejected_but_not_accepted() -> TestResult {
    let dir = TestDirectory::new("envelope")?;
    let mut store = SqliteProposalStore::open(&dir.0)?;
    propose(
        &mut store,
        "bad",
        STREAM_MAPPING_CLASS,
        b"not json".to_vec(),
    )?;
    drop(store);
    let error = record_decision(&dir.0, &human("dave"), "bad", Outcome::Accept, "why");
    assert!(
        matches!(error, Err(QueryError::BadParameter { name: "proposal", ref reason }) if reason.contains("payload")),
        "{error:?}"
    );
    assert!(
        ReadOnlySqliteProposalStore::open(&dir.0)?
            .decisions()?
            .is_empty()
    );
    let recorded = record_decision(&dir.0, &human("dave"), "bad", Outcome::Reject, "why")?;
    assert_eq!(recorded.mapping_source, None);
    Ok(())
}

#[test]
fn a_human_reject_revokes_the_effective_mapping_back_then_to_unrouted() -> TestResult {
    let dir = TestDirectory::new("revoke")?;
    let mut store = SqliteProposalStore::open(&dir.0)?;
    propose(
        &mut store,
        "m1",
        STREAM_MAPPING_CLASS,
        envelope(mapping_a()?)?,
    )?;
    decide(&mut store, "m1", Decider::Policy)?;
    propose(
        &mut store,
        "m2",
        STREAM_MAPPING_CLASS,
        envelope(mapping_b()?)?,
    )?;
    decide(&mut store, "m2", Decider::Policy)?;
    drop(store);
    let identity_a = mapping_a()?.identity()?;

    // Revoking the newest goes back to the older accepted mapping, not dark.
    let back = record_decision(&dir.0, &human("dave"), "m2", Outcome::Reject, "wrong")?;
    let route = route_of(&dir.0, &back)?;
    assert_eq!(route.source, SOURCE);
    assert_eq!(route.mapping.as_deref(), Some(identity_a.as_str()));
    assert_eq!(route.proposal_id.as_deref(), Some("m1"));

    // Revoking that one too leaves the source unrouted.
    let dark = record_decision(&dir.0, &human("dave"), "m1", Outcome::Reject, "wrong")?;
    let route = route_of(&dir.0, &dark)?;
    assert_eq!(route.source, SOURCE);
    assert_eq!((route.mapping, route.proposal_id), (None, None));

    // A later human accept lifts the revoke.
    let again = record_decision(&dir.0, &human("dave"), "m1", Outcome::Accept, "fine")?;
    assert_eq!(route_of(&dir.0, &again)?.proposal_id.as_deref(), Some("m1"));
    Ok(())
}

/// A fresh directory under the system temp dir, removed on drop (even when a test panics).
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Fallible<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "s2w-app-proposal-decisions-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}
