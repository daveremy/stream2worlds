use std::error::Error;

use rusqlite::{Connection, params};

use super::*;
use crate::tests::{TestDirectory, retry_until_unlocked};

const DATABASE_FILE: &str = "proposals.sqlite3";
type TestResult = Result<(), Box<dyn Error>>;

fn proposal(id: &str) -> NewProposal {
    NewProposal {
        id: id.into(),
        class: "class-a".into(),
        actor: Actor::Agent {
            model: "model-a".into(),
            version: "1".into(),
        },
        snapshot_offset: LogPosition(7),
        payload: vec![0, 255, 42],
        proposed_at_ms: 1000,
    }
}

fn decision(id: &str, decider: Decider, outcome: Outcome) -> NewDecision {
    NewDecision {
        proposal_id: id.into(),
        decider,
        outcome,
        basis: "reference-a".into(),
        decided_at_ms: -10,
    }
}

fn run_round_trip<S: ProposalStore>(mut store: S) -> TestResult {
    assert!(store.proposals()?.is_empty());
    assert!(store.decisions()?.is_empty());
    let first = proposal("z");
    let mut second = proposal("a");
    second.actor = Actor::Human {
        id: "reviewer".into(),
    };
    second.class = "class-b".into();
    second.payload = Vec::new();
    second.proposed_at_ms = -1;
    let stored = vec![
        store.append_proposal(&first)?,
        store.append_proposal(&second)?,
    ];
    assert_eq!(
        stored,
        vec![stored_proposal(&first, 1), stored_proposal(&second, 2)]
    );
    assert_eq!(store.proposals()?, stored);
    let decisions = [
        decision("a", Decider::Policy, Outcome::Accept),
        decision("z", Decider::Human, Outcome::Reject),
        decision("a", Decider::Evidence, Outcome::Reject),
        decision("z", Decider::Human, Outcome::Accept),
    ];
    let mut expected = Vec::new();
    for (seq, new) in (1..).zip(&decisions) {
        let row = store.append_decision(new)?;
        assert_eq!(row, stored_decision(new, seq));
        expected.push(row);
    }
    assert_eq!(store.decisions()?, expected);
    let before = grade(&stored, &expected);
    store.append_decision(&decisions[3])?;
    assert_eq!(grade(&store.proposals()?, &store.decisions()?), before);
    assert_eq!(store.decisions()?.len(), 5);
    Ok(())
}

fn run_retry<S: ProposalStore>(mut store: S) -> TestResult {
    let original = proposal("retry");
    let stored = store.append_proposal(&original)?;
    let mut retry = original.clone();
    retry.proposed_at_ms += 200;
    assert_eq!(store.append_proposal(&retry)?, stored);
    let mut changed = vec![original.clone(); 6];
    changed[0].class.push('x');
    changed[1].actor = Actor::Human { id: "human".into() };
    changed[2].actor = Actor::Agent {
        model: "model-b".into(),
        version: "1".into(),
    };
    changed[3].actor = Actor::Agent {
        model: "model-a".into(),
        version: "2".into(),
    };
    changed[4].snapshot_offset = LogPosition(8);
    changed[5].payload.push(1);
    for bad in changed {
        assert!(matches!(
            store.append_proposal(&bad),
            Err(LogError::Corrupt(_))
        ));
        assert_eq!(store.proposals()?, vec![stored.clone()]);
    }
    assert!(store.decisions()?.is_empty());
    assert_eq!(store.append_proposal(&proposal("next"))?.seq, 2);
    Ok(())
}

fn invalid_proposals() -> Vec<NewProposal> {
    let mut invalid = vec![proposal("invalid"); 6];
    invalid[0].id.clear();
    invalid[1].class.clear();
    invalid[2].actor = Actor::Human { id: String::new() };
    invalid[3].actor = Actor::Agent {
        model: String::new(),
        version: "1".into(),
    };
    invalid[4].actor = Actor::Agent {
        model: "model".into(),
        version: String::new(),
    };
    invalid[5].snapshot_offset = LogPosition(u64::MAX);
    invalid
}

fn run_validation<S: ProposalStore>(mut store: S) -> TestResult {
    for bad in invalid_proposals() {
        assert!(matches!(
            store.append_proposal(&bad),
            Err(LogError::Corrupt(_))
        ));
    }
    let mut oversized = proposal("large");
    oversized.payload = vec![0; crate::MAX_PAYLOAD_BYTES + 1];
    assert_eq!(store.append_proposal(&oversized), Err(LogError::TooLarge));
    assert!(store.proposals()?.is_empty());
    let unknown = decision("missing", Decider::Human, Outcome::Accept);
    assert!(matches!(
        store.append_decision(&unknown),
        Err(LogError::Corrupt(_))
    ));
    let stored = store.append_proposal(&proposal("known"))?;
    let mut bad = decision("known", Decider::Policy, Outcome::Reject);
    bad.basis.clear();
    assert!(matches!(
        store.append_decision(&bad),
        Err(LogError::Corrupt(_))
    ));
    bad.basis = "reference".into();
    bad.proposal_id.clear();
    assert!(matches!(
        store.append_decision(&bad),
        Err(LogError::Corrupt(_))
    ));
    assert_eq!(store.proposals()?, vec![stored]);
    assert!(store.decisions()?.is_empty());
    assert_eq!(
        store
            .append_decision(&decision("known", Decider::Policy, Outcome::Reject))?
            .seq,
        1
    );
    Ok(())
}

#[test]
fn in_memory_meets_contract() -> TestResult {
    run_round_trip(InMemoryProposalStore::new())?;
    run_retry(InMemoryProposalStore::new())?;
    run_validation(InMemoryProposalStore::new())
}

#[test]
fn sqlite_meets_contract() -> TestResult {
    let round_trip = TestDirectory::new("proposal-round-trip")?;
    let retry = TestDirectory::new("proposal-retry")?;
    let validation = TestDirectory::new("proposal-validation")?;
    run_round_trip(SqliteProposalStore::open(round_trip.path())?)?;
    run_retry(SqliteProposalStore::open(retry.path())?)?;
    run_validation(SqliteProposalStore::open(validation.path())?)
}

#[test]
fn implementations_return_identical_rows_and_hashes() -> TestResult {
    let directory = TestDirectory::new("proposal-parity")?;
    let mut sqlite = SqliteProposalStore::open(directory.path())?;
    let mut memory = InMemoryProposalStore::new();
    let mut boundary = proposal("boundary");
    boundary.payload = vec![42; crate::MAX_PAYLOAD_BYTES];
    for new in [proposal("first"), boundary] {
        let a = sqlite.append_proposal(&new)?;
        let b = memory.append_proposal(&new)?;
        assert_eq!(a, b);
        assert_eq!(a.payload_hash, content_hash(&new.payload));
        let choice = decision(&new.id, Decider::Evidence, Outcome::Accept);
        assert_eq!(
            sqlite.append_decision(&choice)?,
            memory.append_decision(&choice)?
        );
    }
    assert_eq!(sqlite.proposals()?, memory.proposals()?);
    assert_eq!(sqlite.decisions()?, memory.decisions()?);
    Ok(())
}

#[test]
fn in_memory_payload_tampering_is_corrupt_on_read_and_retry() -> TestResult {
    let mut store = InMemoryProposalStore::new();
    let original = proposal("tamper");
    store.append_proposal(&original)?;
    store.proposals[0].payload.push(1);
    assert!(matches!(store.proposals(), Err(LogError::Corrupt(_))));
    assert!(matches!(
        store.append_proposal(&original),
        Err(LogError::Corrupt(_))
    ));
    Ok(())
}

fn populated<S: ProposalStore>(store: &mut S) -> TestResult {
    store.append_proposal(&proposal("p"))?;
    store.append_decision(&decision("p", Decider::Policy, Outcome::Accept))?;
    store.append_decision(&decision("p", Decider::Human, Outcome::Accept))?;
    store.append_decision(&decision("p", Decider::Evidence, Outcome::Reject))?;
    store.append_decision(&decision("p", Decider::Human, Outcome::Reject))?;
    store.append_proposal(&proposal("ungraded"))?;
    Ok(())
}

#[test]
fn restart_and_read_only_replay_preserve_rows_and_grades() -> TestResult {
    let directory = TestDirectory::new("proposal-restart")?;
    let mut writer = SqliteProposalStore::open(directory.path())?;
    populated(&mut writer)?;
    let proposals = writer.proposals()?;
    let decisions = writer.decisions()?;
    let grades = grade(&proposals, &decisions);
    drop(writer);
    let writer = retry_until_unlocked(|| SqliteProposalStore::open(directory.path()))?;
    let reader = ReadOnlySqliteProposalStore::open(directory.path())?;
    assert_eq!(writer.proposals()?, proposals);
    assert_eq!(writer.decisions()?, decisions);
    assert_eq!(reader.proposals()?, proposals);
    assert_eq!(reader.decisions()?, decisions);
    assert_eq!(grade(&writer.proposals()?, &writer.decisions()?), grades);
    assert_eq!(grade(&reader.proposals()?, &reader.decisions()?), grades);
    Ok(())
}

#[test]
fn lock_is_independent_and_read_only_coexists_with_writes() -> TestResult {
    let directory = TestDirectory::new("proposal-lock")?;
    let _events = crate::SqliteEventLog::open(directory.path())?;
    let _verdicts = crate::SqliteVerdictStore::open(directory.path())?;
    let mut writer = SqliteProposalStore::open(directory.path())?;
    assert_eq!(
        SqliteProposalStore::open(directory.path()).err(),
        Some(LogError::Locked)
    );
    let reader = ReadOnlySqliteProposalStore::open(directory.path())?;
    assert!(reader.proposals()?.is_empty());
    populated(&mut writer)?;
    assert_eq!(reader.proposals()?, writer.proposals()?);
    assert_eq!(reader.decisions()?, writer.decisions()?);
    assert!(directory.path().join("PROPOSALS_LOCK").is_file());
    drop(writer);
    assert!(retry_until_unlocked(|| SqliteProposalStore::open(directory.path())).is_ok());
    Ok(())
}

#[test]
fn unsupported_or_absent_schema_is_refused() -> TestResult {
    for version in [0, 2, 99] {
        let directory = TestDirectory::new("proposal-version")?;
        let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
        connection.execute_batch(&format!("PRAGMA user_version = {version};"))?;
        assert!(matches!(
            ReadOnlySqliteProposalStore::open(directory.path()),
            Err(LogError::Corrupt(_))
        ));
        if version != 0 {
            assert!(matches!(
                SqliteProposalStore::open(directory.path()),
                Err(LogError::Corrupt(_))
            ));
        }
    }
    Ok(())
}

#[test]
fn triggers_refuse_updates_deletes_and_replacements() -> TestResult {
    let directory = TestDirectory::new("proposal-triggers")?;
    let mut store = SqliteProposalStore::open(directory.path())?;
    populated(&mut store)?;
    let proposals = store.proposals()?;
    let decisions = store.decisions()?;
    let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
    connection.execute_batch("PRAGMA recursive_triggers = ON; PRAGMA foreign_keys = ON;")?;
    for sql in [
        "UPDATE proposals SET payload = X'00'",
        "DELETE FROM proposals",
        "UPDATE decisions SET outcome = 'accept'",
        "DELETE FROM decisions",
        "INSERT OR REPLACE INTO proposals SELECT * FROM proposals WHERE id = 'p'",
        "INSERT OR REPLACE INTO decisions SELECT * FROM decisions WHERE seq = 1",
    ] {
        assert!(connection.execute(sql, []).is_err(), "{sql}");
    }
    assert_eq!(store.proposals()?, proposals);
    assert_eq!(store.decisions()?, decisions);
    Ok(())
}

#[test]
fn schema_checks_actor_shape_protocol_values_and_foreign_key() -> TestResult {
    let directory = TestDirectory::new("proposal-checks")?;
    let mut store = SqliteProposalStore::open(directory.path())?;
    store.append_proposal(&proposal("p"))?;
    let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    for (kind, id, model, version) in [
        ("unknown", None, None, None),
        ("human", None, None, None),
        ("human", Some("id"), Some("model"), None),
        ("human", Some("id"), None, Some("version")),
        ("agent", None, None, Some("version")),
        ("agent", None, Some("model"), None),
        ("agent", Some("id"), Some("model"), Some("version")),
    ] {
        assert!(
            connection
                .execute(
                    "INSERT INTO proposals (id, class, actor_kind, actor_id, model, model_version,
             snapshot_offset, payload_hash, payload, proposed_at_ms)
             VALUES ('bad', 'class', ?1, ?2, ?3, ?4, 1, 0, X'00', 0)",
                    params![kind, id, model, version],
                )
                .is_err()
        );
    }
    for (id, decider, outcome) in [
        ("missing", "policy", "accept"),
        ("p", "unknown", "accept"),
        ("p", "human", "unknown"),
    ] {
        assert!(
            connection
                .execute(
                    "INSERT INTO decisions (proposal_id, decider, outcome, basis, decided_at_ms)
             VALUES (?1, ?2, ?3, 'basis', 0)",
                    params![id, decider, outcome],
                )
                .is_err()
        );
    }
    assert_eq!(store.proposals()?.len(), 1);
    assert!(store.decisions()?.is_empty());
    Ok(())
}

#[test]
fn payload_tampering_is_corrupt_on_read_and_retry() -> TestResult {
    let directory = TestDirectory::new("proposal-tamper")?;
    let mut store = SqliteProposalStore::open(directory.path())?;
    store.append_proposal(&proposal("p"))?;
    let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
    connection.execute_batch(
        "DROP TRIGGER proposals_no_update;
        UPDATE proposals SET payload = X'01' WHERE id = 'p';",
    )?;
    let reader = ReadOnlySqliteProposalStore::open(directory.path())?;
    assert!(matches!(store.proposals(), Err(LogError::Corrupt(_))));
    assert!(matches!(reader.proposals(), Err(LogError::Corrupt(_))));
    assert!(matches!(
        store.append_proposal(&proposal("p")),
        Err(LogError::Corrupt(_))
    ));
    Ok(())
}

#[test]
fn unknown_actor_decider_and_outcome_are_corrupt_on_read() -> TestResult {
    for (table, column) in [
        ("proposals", "actor_kind"),
        ("decisions", "decider"),
        ("decisions", "outcome"),
    ] {
        let directory = TestDirectory::new("proposal-unknown-enum")?;
        let mut store = SqliteProposalStore::open(directory.path())?;
        populated(&mut store)?;
        let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
        connection.execute_batch(&format!(
            "PRAGMA ignore_check_constraints = ON; DROP TRIGGER {table}_no_update;
             UPDATE {table} SET {column} = 'unknown';"
        ))?;
        let reader = ReadOnlySqliteProposalStore::open(directory.path())?;
        if table == "proposals" {
            assert!(matches!(store.proposals(), Err(LogError::Corrupt(_))));
            assert!(matches!(reader.proposals(), Err(LogError::Corrupt(_))));
        } else {
            assert!(matches!(store.decisions(), Err(LogError::Corrupt(_))));
            assert!(matches!(reader.decisions(), Err(LogError::Corrupt(_))));
        }
    }
    Ok(())
}

#[test]
fn latest_sequence_wins_independent_of_input_order_or_time() -> TestResult {
    let mut store = InMemoryProposalStore::new();
    populated(&mut store)?;
    let mut correction = decision("p", Decider::Human, Outcome::Accept);
    correction.decided_at_ms = i64::MIN;
    store.append_decision(&correction)?;
    let proposals = store.proposals()?;
    let mut decisions = store.decisions()?;
    decisions.reverse();
    let grades = grade(&proposals, &decisions);
    assert_eq!(grades.len(), 1);
    let entry = &grades[0];
    assert_eq!(entry.proposed, 2);
    assert_eq!(entry.ungraded, 1);
    assert_eq!(
        entry.human,
        Tally {
            accepted: 1,
            rejected: 0
        }
    );
    assert_eq!(
        entry.evidence,
        Tally {
            accepted: 0,
            rejected: 1
        }
    );
    assert_eq!(entry.policy_applied.fraction(), (0, 1));
    assert_eq!(grade(&[], &decisions), Vec::new());
    assert_eq!(grade(&proposals, &[])[0].ungraded, 2);
    Ok(())
}

#[test]
fn policy_is_routing_not_accuracy() -> TestResult {
    let mut store = InMemoryProposalStore::new();
    for id in ["accepted", "rejected", "none"] {
        store.append_proposal(&proposal(id))?;
    }
    store.append_decision(&decision("accepted", Decider::Policy, Outcome::Reject))?;
    store.append_decision(&decision("accepted", Decider::Policy, Outcome::Accept))?;
    store.append_decision(&decision("rejected", Decider::Policy, Outcome::Accept))?;
    store.append_decision(&decision("rejected", Decider::Policy, Outcome::Reject))?;
    let grades = grade(&store.proposals()?, &store.decisions()?);
    let entry = &grades[0];
    assert_eq!(entry.proposed, 3);
    assert_eq!(entry.ungraded, 3);
    assert_eq!((entry.policy_accepted, entry.policy_rejected), (1, 1));
    assert_eq!(entry.human.fraction(), (0, 0));
    assert_eq!(entry.evidence.fraction(), (0, 0));
    assert_eq!(entry.policy_applied.fraction(), (0, 0));
    assert_eq!(entry.policy_applied_ungraded, 1);
    Ok(())
}

#[test]
fn policy_cross_tab_any_reject_wins_and_has_no_time_order() -> TestResult {
    let mut store = InMemoryProposalStore::new();
    let combinations = [None, Some(Outcome::Accept), Some(Outcome::Reject)];
    for human in combinations {
        for evidence in combinations {
            let id = format!("{human:?}/{evidence:?}");
            store.append_proposal(&proposal(&id))?;
            if let Some(outcome) = human {
                store.append_decision(&decision(&id, Decider::Human, outcome))?;
            }
            if let Some(outcome) = evidence {
                store.append_decision(&decision(&id, Decider::Evidence, outcome))?;
            }
            // Human/evidence precede policy both by sequence and by caller timestamp.
            let mut policy = decision(&id, Decider::Policy, Outcome::Accept);
            policy.decided_at_ms = 2000;
            store.append_decision(&policy)?;
        }
    }
    let entry = grade(&store.proposals()?, &store.decisions()?).remove(0);
    assert_eq!(entry.proposed, 9);
    assert_eq!(entry.ungraded, 1);
    assert_eq!(entry.policy_accepted, 9);
    assert_eq!(entry.policy_applied_ungraded, 1);
    assert_eq!(
        entry.policy_applied,
        Tally {
            accepted: 3,
            rejected: 5
        }
    );
    assert_eq!(
        entry.human,
        Tally {
            accepted: 3,
            rejected: 3
        }
    );
    assert_eq!(
        entry.evidence,
        Tally {
            accepted: 3,
            rejected: 3
        }
    );
    assert_eq!(entry.policy_applied.fraction(), (3, 8));
    Ok(())
}

#[test]
fn policy_rejection_excludes_grades_from_cross_tab() -> TestResult {
    let mut store = InMemoryProposalStore::new();
    for id in ["rejected", "no-policy"] {
        store.append_proposal(&proposal(id))?;
        store.append_decision(&decision(id, Decider::Human, Outcome::Accept))?;
    }
    store.append_decision(&decision("rejected", Decider::Policy, Outcome::Accept))?;
    store.append_decision(&decision("rejected", Decider::Policy, Outcome::Reject))?;
    let entry = grade(&store.proposals()?, &store.decisions()?).remove(0);
    assert_eq!(entry.human.fraction(), (2, 2));
    assert_eq!(entry.ungraded, 0);
    assert_eq!(entry.policy_rejected, 1);
    assert_eq!(entry.policy_applied.fraction(), (0, 0));
    assert_eq!(entry.policy_applied_ungraded, 0);
    Ok(())
}

#[test]
fn grades_separate_classes_model_versions_and_humans_in_sorted_order() -> TestResult {
    let mut store = InMemoryProposalStore::new();
    let mut proposals = vec![
        proposal("v1"),
        proposal("v2"),
        proposal("human"),
        proposal("class"),
        proposal("model"),
    ];
    proposals[1].actor = Actor::Agent {
        model: "model-a".into(),
        version: "2".into(),
    };
    proposals[2].actor = Actor::Human {
        id: "model-a".into(),
    };
    proposals[3].class = "class-b".into();
    proposals[4].actor = Actor::Agent {
        model: "model-b".into(),
        version: "1".into(),
    };
    for proposal in &proposals {
        store.append_proposal(proposal)?;
    }
    store.append_decision(&decision("v1", Decider::Human, Outcome::Reject))?;
    store.append_decision(&decision("v2", Decider::Human, Outcome::Accept))?;
    store.append_decision(&decision("human", Decider::Human, Outcome::Accept))?;
    let grades = grade(&store.proposals()?, &store.decisions()?);
    assert_eq!(grades.len(), 5);
    let keys: Vec<_> = grades
        .iter()
        .map(|entry| (&entry.class, &entry.actor))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert!(grades.iter().all(|entry| entry.proposed == 1));
    assert_eq!(grades[0].actor, proposals[2].actor);
    assert_eq!(grades[0].human.fraction(), (1, 1));
    assert_eq!(grades[1].human.fraction(), (0, 1));
    assert_eq!(grades[2].human.fraction(), (1, 1));
    assert_eq!(grades[3].human.fraction(), (0, 0));
    assert_eq!(grades[4].human.fraction(), (0, 0));
    let mut reversed = store.proposals()?;
    reversed.reverse();
    assert_eq!(grade(&reversed, &store.decisions()?), grades);
    Ok(())
}
