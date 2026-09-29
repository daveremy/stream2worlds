use std::env;
use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::*;
use crate::tests::{TestDirectory, retry_until_unlocked};

const CRASH_CHILD_ENV: &str = "S2W_VERDICT_CRASH_CHILD_DIRECTORY";
const CRASH_BATCHES: u64 = 8;
const CRASH_DURING: u64 = 5;
const ROWS_PER_BATCH: u64 = 3;

type TestResult = Result<(), Box<dyn Error>>;

fn position(value: u64) -> LogPosition {
    LogPosition(value)
}

fn row(at: u64, engine: &str, version: u32) -> StoredVerdict {
    StoredVerdict {
        position: position(at),
        event_hash: i64::try_from(at).unwrap_or(i64::MAX).wrapping_mul(-7_919),
        engine: engine.to_owned(),
        version,
        verdict: format!("{engine}@{version}:{at}").into_bytes(),
        provenance: None,
    }
}

/// `read_range_of` over the suite's first two batches: filtered by engine name, same order; no
/// names reads nothing (decision 0023).
fn check_engine_filter<S: VerdictStore>(store: &S, with_provenance: &StoredVerdict) -> TestResult {
    assert_eq!(
        store.read_range_of(None, position(4), &["lexicon".to_owned()])?,
        vec![row(1, "lexicon", 3)]
    );
    assert_eq!(
        store.read_range_of(
            None,
            position(4),
            &["keyword".to_owned(), "absent".to_owned()]
        )?,
        vec![
            row(1, "keyword", 1),
            row(1, "keyword", 2),
            with_provenance.clone(),
            row(4, "keyword", 1),
        ]
    );
    assert!(store.read_range_of(None, position(4), &[])?.is_empty());

    Ok(())
}

/// The contract every [`VerdictStore`] meets; run over both implementations.
#[expect(
    clippy::cognitive_complexity,
    reason = "one contract suite run over both VerdictStore impls; assertions are straight-line"
)]
fn run_verdict_suite<S: VerdictStore>(mut store: S) -> TestResult {
    assert_eq!(store.cursor()?, None);
    assert!(store.read_range(None, position(100))?.is_empty());

    // Round trip, including provenance and a negative hash.
    let mut with_provenance = row(2, "keyword", 1);
    with_provenance.provenance = Some(br#"{"model_hash":"abc"}"#.to_vec());
    let first = vec![row(1, "keyword", 1), with_provenance, row(1, "lexicon", 3)];
    store.commit_batch(&first, position(3))?;
    assert_eq!(store.cursor()?, Some(position(3)));

    // A later batch adds a second version at position 1: ordering is (position, seq).
    let second = vec![row(4, "keyword", 1), row(1, "keyword", 2)];
    store.commit_batch(&second, position(4))?;
    assert_eq!(
        store.read_range(None, position(4))?,
        vec![
            row(1, "keyword", 1),
            row(1, "lexicon", 3),
            row(1, "keyword", 2),
            first[1].clone(),
            row(4, "keyword", 1),
        ]
    );
    assert_eq!(
        store.read_range(Some(position(1)), position(2))?,
        vec![first[1].clone()]
    );
    assert!(store.read_range(Some(position(4)), position(9))?.is_empty());

    check_engine_filter(&store, &first[1])?;

    // Atomicity: a duplicate key, stored or in-batch, fails the whole batch.
    for bad in [
        vec![row(5, "keyword", 1), row(1, "keyword", 1)],
        vec![row(5, "keyword", 1), row(5, "keyword", 1)],
    ] {
        assert!(matches!(
            store.commit_batch(&bad, position(9)),
            Err(LogError::Corrupt(_))
        ));
        assert_eq!(store.cursor()?, Some(position(4)));
        assert!(store.read_range(Some(position(4)), position(9))?.is_empty());
    }

    // A row after the batch end is refused.
    assert!(matches!(
        store.commit_batch(&[row(8, "keyword", 1)], position(7)),
        Err(LogError::Corrupt(_))
    ));
    assert_eq!(store.cursor()?, Some(position(4)));

    // Monotonic cursor: an empty batch below the cursor changes nothing.
    store.commit_batch(&[], position(2))?;
    assert_eq!(store.cursor()?, Some(position(4)));
    // A non-empty batch below the cursor (a new engine on old positions) stores its rows and
    // leaves the cursor where it was.
    store.commit_batch(&[row(2, "embed", 1)], position(2))?;
    assert_eq!(store.cursor()?, Some(position(4)));
    assert!(
        store
            .read_range(Some(position(1)), position(2))?
            .contains(&row(2, "embed", 1))
    );
    // An empty batch past the cursor advances it (unrouted events).
    store.commit_batch(&[], position(6))?;
    assert_eq!(store.cursor()?, Some(position(6)));
    Ok(())
}

#[test]
fn in_memory_store_meets_the_contract() -> TestResult {
    run_verdict_suite(InMemoryVerdictStore::new())
}

#[test]
fn sqlite_store_meets_the_contract() -> TestResult {
    let directory = TestDirectory::new("verdict-suite")?;
    run_verdict_suite(SqliteVerdictStore::open(directory.path())?)
}

#[test]
fn sqlite_duplicate_key_error_text_is_unchanged() -> TestResult {
    let directory = TestDirectory::new("verdict-duplicate-text")?;
    let mut store = SqliteVerdictStore::open(directory.path())?;
    store.commit_batch(&[row(1, "keyword", 1)], position(1))?;
    let expected =
        LogError::Corrupt("duplicate verdict key: position 1, engine keyword, version 1".into());
    assert_eq!(
        store.commit_batch(&[row(1, "keyword", 1)], position(1)),
        Err(expected.clone())
    );
    assert_eq!(
        InMemoryVerdictStore::default()
            .commit_batch(&[row(1, "keyword", 1), row(1, "keyword", 1)], position(1)),
        Err(expected)
    );
    Ok(())
}

#[test]
fn sqlite_store_round_trips_across_reopen() -> TestResult {
    let directory = TestDirectory::new("verdict-reopen")?;
    let mut with_provenance = row(3, "keyword", 7);
    with_provenance.provenance = Some(b"{}".to_vec());
    let rows = vec![row(2, "keyword", 7), row(1, "lexicon", 1), with_provenance];
    {
        let mut store = SqliteVerdictStore::open(directory.path())?;
        store.commit_batch(&rows, position(5))?;
    }
    let store = retry_until_unlocked(|| SqliteVerdictStore::open(directory.path()))?;
    assert_eq!(store.cursor()?, Some(position(5)));
    assert_eq!(
        store.read_range(None, position(5))?,
        vec![rows[1].clone(), rows[0].clone(), rows[2].clone()]
    );
    Ok(())
}

#[test]
fn sqlite_store_sits_beside_the_event_log() -> TestResult {
    let directory = TestDirectory::new("verdict-sibling")?;
    let _log = crate::SqliteEventLog::open(directory.path())?;
    let _store = SqliteVerdictStore::open(directory.path())?;
    assert!(directory.path().join(DATABASE_FILE).is_file());
    assert!(directory.path().join(crate::DATABASE_FILE).is_file());
    Ok(())
}

#[test]
fn sqlite_triggers_refuse_update_and_delete() -> TestResult {
    let directory = TestDirectory::new("verdict-triggers")?;
    {
        let mut store = SqliteVerdictStore::open(directory.path())?;
        store.commit_batch(&[row(1, "keyword", 1)], position(5))?;
    }
    let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
    connection.execute_batch("PRAGMA recursive_triggers = ON;")?;
    for refused in [
        "UPDATE verdicts SET verdict = X'00'",
        "DELETE FROM verdicts",
        "INSERT OR REPLACE INTO verdicts (seq, position, engine, version, event_hash, verdict)
         VALUES (1, 1, 'keyword', 1, 0, X'00')",
        "UPDATE bridge_cursor SET position = 4",
        "DELETE FROM bridge_cursor",
        "INSERT OR REPLACE INTO bridge_cursor (id, position) VALUES (1, 1)",
    ] {
        assert!(connection.execute(refused, []).is_err(), "{refused}");
    }
    // Raising the cursor is allowed; only lowering it is refused.
    connection.execute("UPDATE bridge_cursor SET position = 6", [])?;
    drop(connection);
    let store = retry_until_unlocked(|| SqliteVerdictStore::open(directory.path()))?;
    assert_eq!(store.cursor()?, Some(position(6)));
    assert_eq!(
        store.read_range(None, position(9))?,
        vec![row(1, "keyword", 1)]
    );
    Ok(())
}

#[test]
fn sqlite_second_open_fails_fast_while_lock_is_held() -> TestResult {
    let directory = TestDirectory::new("verdict-lock")?;
    let first = SqliteVerdictStore::open(directory.path())?;
    assert_eq!(
        SqliteVerdictStore::open(directory.path()).err(),
        Some(LogError::Locked)
    );
    drop(first);
    assert!(retry_until_unlocked(|| SqliteVerdictStore::open(directory.path())).is_ok());
    Ok(())
}

#[test]
fn sqlite_uses_wal_full_synchronous_and_its_own_version() -> TestResult {
    let directory = TestDirectory::new("verdict-pragmas")?;
    for _ in 0..2 {
        let store = retry_until_unlocked(|| SqliteVerdictStore::open(directory.path()))?;
        let journal: String = store
            .connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        let synchronous: i64 = store
            .connection
            .query_row("PRAGMA synchronous", [], |row| row.get(0))?;
        let version: i64 = store
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(journal.to_ascii_lowercase(), "wal");
        assert_eq!(synchronous, 2);
        assert_eq!(version, SCHEMA_VERSION);
    }
    Ok(())
}

#[test]
fn sqlite_rejects_unknown_schema_version_as_corrupt() -> TestResult {
    let directory = TestDirectory::new("verdict-version")?;
    let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
    connection.execute_batch("PRAGMA user_version = 99;")?;
    drop(connection);
    assert!(matches!(
        SqliteVerdictStore::open(directory.path()),
        Err(LogError::Corrupt(_))
    ));
    Ok(())
}

fn crash_batch(batch: u64) -> Vec<StoredVerdict> {
    (1..=ROWS_PER_BATCH)
        .map(|offset| row((batch - 1) * ROWS_PER_BATCH + offset, "keyword", 1))
        .collect()
}

/// The child half of the crash test: commits batches until told to stop, then blocks inside
/// the open transaction of batch [`CRASH_DURING`], rows and cursor written, until killed.
#[test]
fn sqlite_verdict_crash_child() -> TestResult {
    let Some(directory) = env::var_os(CRASH_CHILD_ENV) else {
        return Ok(());
    };
    let mut store = SqliteVerdictStore::open(PathBuf::from(directory))?;
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    for batch in 1..=CRASH_BATCHES {
        let through = position(batch * ROWS_PER_BATCH);
        if batch == CRASH_DURING {
            store.commit_batch_with(&crash_batch(batch), through, || {
                writeln!(output, "in transaction {batch}")
                    .and_then(|()| output.flush())
                    .and_then(|()| input.read_exact(&mut [0_u8; 1]))
                    .map_err(|error| LogError::Io(error.to_string()))
            })?;
        } else {
            store.commit_batch(&crash_batch(batch), through)?;
            writeln!(output, "committed {batch}")?;
            output.flush()?;
            input.read_exact(&mut [0_u8; 1])?;
        }
    }
    Ok(())
}

#[test]
fn sqlite_keeps_whole_batches_after_sigkill_in_transaction() -> TestResult {
    if env::var_os(CRASH_CHILD_ENV).is_some() {
        return Ok(());
    }
    let directory = TestDirectory::new("verdict-crash")?;
    let mut child = Command::new(env::current_exe()?)
        .arg("--exact")
        .arg("verdicts::tests::sqlite_verdict_crash_child")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(CRASH_CHILD_ENV, directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let child_stdout = child.stdout.take().ok_or("child stdout unavailable")?;
    let mut child_stdin = child.stdin.take().ok_or("child stdin unavailable")?;
    let mut lines = BufReader::new(child_stdout).lines();
    loop {
        let line = lines
            .next()
            .ok_or("child exited before the crash marker")??;
        if line.contains(&format!("in transaction {CRASH_DURING}")) {
            break;
        }
        if line.contains("committed ") {
            child_stdin.write_all(&[1])?;
            child_stdin.flush()?;
        }
    }
    child.kill()?;
    assert!(!child.wait()?.success());

    let store = SqliteVerdictStore::open(directory.path())?;
    let committed = CRASH_DURING - 1;
    let expected = (1..=committed).flat_map(crash_batch).collect::<Vec<_>>();
    assert_eq!(
        store.read_range(None, position(CRASH_BATCHES * ROWS_PER_BATCH))?,
        expected
    );
    assert_eq!(store.cursor()?, Some(position(committed * ROWS_PER_BATCH)));
    Ok(())
}
