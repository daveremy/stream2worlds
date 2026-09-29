//! The in-run trigger against real stores in a temporary directory.

use std::collections::BTreeSet;

use s2w_log::{ReadOnlySqliteProposalStore, SqliteEventLog, SqliteProposalStore};
use s2w_model::SourceId;

use super::Seed;
use crate::discover::tests::{Notes, SOURCE, append, small, stream};
use crate::tests::TestDirectory;

fn armed(dir: &TestDirectory) -> super::InRun {
    Seed {
        log_dir: dir.path().to_path_buf(),
        cfg: small(),
        settled: BTreeSet::new(),
    }
    .arm(&[SourceId::new(SOURCE).expect("source")])
    .expect("one pending source")
}

fn proposals(dir: &TestDirectory) -> usize {
    if !dir.path().join(s2w_log::PROPOSAL_DATABASE_FILE).exists() {
        return 0;
    }
    ReadOnlySqliteProposalStore::open(dir.path())
        .expect("store")
        .proposals()
        .expect("proposals")
        .len()
}

#[test]
fn a_settled_source_arms_nothing() {
    let dir = TestDirectory::new("in-run-settled");
    let source = SourceId::new(SOURCE).expect("source");
    let seed = Seed {
        log_dir: dir.path().to_path_buf(),
        cfg: small(),
        settled: BTreeSet::from([source.clone()]),
    };
    assert!(seed.arm(&[source]).is_none());
}

#[test]
fn the_producer_runs_once_after_the_poll_that_fills_the_window() {
    let dir = TestDirectory::new("in-run-fills");
    let payloads = stream(350);
    append(dir.path(), SOURCE, payloads[..100].to_vec());
    let mut in_run = armed(&dir);
    let mut notes = Notes::default();
    in_run.after_poll(&SqliteEventLog::open(dir.path()).expect("log"), &mut notes);
    assert!(notes.0.is_empty(), "below the window: {:?}", notes.0);
    assert!(!in_run.is_done());

    append(dir.path(), SOURCE, payloads[100..].to_vec());
    in_run.after_poll(&SqliteEventLog::open(dir.path()).expect("log"), &mut notes);
    assert_eq!(proposals(&dir), 1);
    assert!(
        notes.0.len() == 1 && notes.0[0].ends_with("; takes effect at the next restart"),
        "{:?}",
        notes.0
    );
    assert!(in_run.is_done(), "a source is profiled once per process");
}

#[test]
fn a_held_writer_lock_keeps_the_source_pending_until_the_next_poll() {
    let dir = TestDirectory::new("in-run-locked");
    let mut in_run = armed(&dir);
    append(dir.path(), SOURCE, stream(300));
    let writer = SqliteProposalStore::open(dir.path()).expect("writer");
    let mut notes = Notes::default();
    in_run.after_poll(&SqliteEventLog::open(dir.path()).expect("log"), &mut notes);
    assert!(
        notes
            .0
            .iter()
            .any(|n| n.ends_with("routes unchanged, retried at the next poll")),
        "{:?}",
        notes.0
    );
    assert_eq!(proposals(&dir), 0);
    assert!(!in_run.is_done());

    drop(writer);
    in_run.after_poll(&SqliteEventLog::open(dir.path()).expect("log"), &mut notes);
    assert_eq!(proposals(&dir), 1);
    assert!(in_run.is_done());
}
