use super::*;
use crate::{
    EventLog, WorldManifest,
    tests::{TestDirectory, retry_until_unlocked},
};
use s2w_model::Timestamp;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn source() -> Result<SourceId, s2w_model::ModelError> {
    SourceId::new("member")
}
fn cursor(n: u8) -> Result<Cursor, s2w_model::ModelError> {
    Cursor::new(vec![n])
}
fn event(n: u8) -> Result<RawEvent, s2w_model::ModelError> {
    Ok(RawEvent {
        source: source()?,
        cursor: cursor(n)?,
        received_at: Timestamp::from_millis(i64::from(n)),
        payload: vec![n],
    })
}
fn fail_cursor(log: &SqliteEventLog) -> Result<(), rusqlite::Error> {
    log.connection.execute_batch("CREATE TEMP TRIGGER fail_cursor BEFORE INSERT ON cursors BEGIN SELECT RAISE(ABORT,'forced cursor failure'); END;")
}
#[test]
fn same_transaction_atomicity() -> TestResult {
    let dir = TestDirectory::new("membership-atomic")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    fail_cursor(&log)?;
    assert!(
        log.record_source_added(&source()?, EffectiveFrom::FromCursor(cursor(9)?))
            .is_err()
    );
    assert!(log.membership_history()?.is_empty());
    assert_eq!(log.cursor(&source()?)?, None);
    log.connection.execute_batch("DROP TRIGGER fail_cursor")?;
    log.record_source_added(&source()?, EffectiveFrom::FromCursor(cursor(9)?))?;
    assert_eq!(log.membership_history()?.len(), 1);
    assert_eq!(log.cursor(&source()?)?, Some(cursor(9)?));
    log.connection.execute_batch("CREATE TEMP TRIGGER fail_remove AFTER INSERT ON membership WHEN NEW.kind='removed' BEGIN SELECT RAISE(ABORT,'forced removal failure'); END;")?;
    assert!(log.record_source_removed(&source()?).is_err());
    assert!(log.is_source_member(&source()?)?);
    assert_eq!(log.cursor(&source()?)?, Some(cursor(9)?));
    log.connection.execute_batch("DROP TRIGGER fail_remove")?;
    log.record_source_removed(&source()?)?;
    assert!(!log.is_source_member(&source()?)?);
    assert_eq!(log.cursor(&source()?)?, Some(cursor(9)?));
    Ok(())
}
#[test]
fn membership_gates_ingestion_db_enforced() -> TestResult {
    let dir = TestDirectory::new("membership-gate")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    log.append_batch(vec![event(1)?])?;
    log.record_source_removed(&source()?)?;
    let mut sibling = event(3)?;
    sibling.source = SourceId::new("sibling")?;
    let outcomes =
        log.append_batch_with_generation(vec![event(2)?, sibling.clone()], &sibling.source, 0)?;
    assert_eq!(outcomes[0], AppendOutcome::Rejected);
    assert!(matches!(outcomes[1], AppendOutcome::Inserted(_)));
    let replay = log.replay(None)?.collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        replay
            .iter()
            .map(|e| e.event.payload.clone())
            .collect::<Vec<_>>(),
        vec![vec![1], vec![3]]
    );
    assert_eq!(log.cursor(&source()?)?, Some(cursor(1)?));
    Ok(())
}
#[test]
fn readd_cursor_atomicity() -> TestResult {
    let dir = TestDirectory::new("readd")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    log.append(event(1)?)?;
    log.record_source_removed(&source()?)?;
    fail_cursor(&log)?;
    assert!(
        log.record_source_added(&source()?, EffectiveFrom::FromCursor(cursor(9)?))
            .is_err()
    );
    assert_eq!(log.membership_history()?.len(), 1);
    assert_eq!(log.cursor(&source()?)?, Some(cursor(1)?));
    log.connection.execute_batch("DROP TRIGGER fail_cursor")?;
    log.record_source_added(&source()?, EffectiveFrom::FromCursor(cursor(9)?))?;
    assert_eq!(log.membership_history()?.len(), 2);
    assert_eq!(log.cursor(&source()?)?, Some(cursor(9)?));
    let position: Option<i64> =
        log.connection
            .query_row("SELECT last_position FROM cursors", [], |r| r.get(0))?;
    assert_eq!(position, None);
    Ok(())
}
#[test]
fn bootstrap_preserves_existing_cursor() -> TestResult {
    let dir = TestDirectory::new("bootstrap")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    log.append(event(1)?)?;
    let before: (Vec<u8>, i64) =
        log.connection
            .query_row("SELECT cursor,last_position FROM cursors", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
    drop(log);
    let mut log = retry_until_unlocked(|| SqliteEventLog::open(dir.path()))?;
    log.bootstrap_source(&source()?)?;
    log.bootstrap_source(&source()?)?;
    let history = log.membership_history()?;
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].effective_from,
        Some(EffectiveFrom::FromCursor(cursor(1)?))
    );
    let after = log
        .connection
        .query_row("SELECT cursor,last_position FROM cursors", [], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
        })?;
    assert_eq!(before, after);
    log.record_source_removed(&source()?)?;
    log.bootstrap_source(&source()?)?;
    assert_eq!(log.membership_history()?.len(), 2);
    assert!(!log.is_source_member(&source()?)?);
    let fresh = SourceId::new("fresh")?;
    log.bootstrap_source(&fresh)?;
    assert_eq!(
        log.membership_history()?[2].effective_from,
        Some(EffectiveFrom::Now)
    );
    assert_eq!(log.cursor(&fresh)?, None);
    Ok(())
}
#[test]
fn append_only_enforcement() -> TestResult {
    let dir = TestDirectory::new("append-only-membership")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    log.bootstrap_source(&source()?)?;
    WorldManifest::create_if_absent(
        &mut log,
        "default",
        "Name",
        Timestamp::from_millis(0),
        &["engine".into()],
        &["policy".into()],
    )?;
    for (table, column) in [
        ("membership", "source"),
        ("world_manifest", "name"),
        ("world_manifest_engines", "engine"),
        ("world_manifest_policies", "policy"),
    ] {
        for sql in [
            format!("UPDATE {table} SET {column}='changed'"),
            format!("DELETE FROM {table}"),
        ] {
            let error = log
                .connection
                .execute(&sql, [])
                .err()
                .ok_or("mutation succeeded")?;
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::ConstraintViolation)
            );
            assert!(error.to_string().contains("append-only"));
        }
    }
    Ok(())
}
#[test]
fn readd_with_now_is_rejected() -> TestResult {
    let dir = TestDirectory::new("readd-now")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    log.record_source_removed(&source()?)?;
    assert_eq!(
        log.record_source_added(&source()?, EffectiveFrom::Now),
        Err(LogError::ReaddRequiresCursor)
    );
    assert_eq!(log.membership_history()?.len(), 1);
    assert!(!log.is_source_member(&source()?)?);
    Ok(())
}
#[test]
fn stale_batch_after_remove_readd_is_dropped() -> TestResult {
    let dir = TestDirectory::new("stale-generation")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    log.bootstrap_source(&source()?)?;
    let generation = log.membership_generation(&source()?)?;
    log.record_source_removed(&source()?)?;
    log.record_source_added(&source()?, EffectiveFrom::FromCursor(cursor(9)?))?;
    let mut sibling = event(3)?;
    sibling.source = SourceId::new("sibling")?;
    assert_eq!(
        log.append_batch_with_generation(vec![event(1)?, sibling.clone()], &source()?, generation)?,
        vec![AppendOutcome::StaleGeneration; 2]
    );
    assert_eq!(log.replay(None)?.count(), 0);
    assert_eq!(log.cursor(&source()?)?, Some(cursor(9)?));
    assert_eq!(log.cursor(&sibling.source)?, None);
    Ok(())
}
#[test]
fn offset_provenance_matches_transaction_head() -> TestResult {
    let dir = TestDirectory::new("offset-provenance")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    log.append_batch(vec![event(1)?, event(2)?])?;
    // Captures the head inside the membership writer's own transaction, not a before/after read.
    log.connection.execute_batch("CREATE TEMP TABLE observed(head INTEGER);
        CREATE TEMP TRIGGER observe_head AFTER INSERT ON membership BEGIN
        INSERT INTO observed SELECT COALESCE(MAX(position),0) FROM events;
        SELECT CASE WHEN NEW.world_offset != (SELECT MAX(position) FROM events) THEN RAISE(ABORT,'wrong head') END; END;")?;
    log.bootstrap_source(&source()?)?;
    let head: i64 = log
        .connection
        .query_row("SELECT head FROM observed", [], |r| r.get(0))?;
    assert_eq!(head, 2);
    assert_eq!(
        log.membership_history()?[0].world_offset,
        u64::try_from(head)?
    );
    Ok(())
}
#[test]
fn migrates_v2_fixture_preserving_cursor_and_events() -> TestResult {
    let dir = TestDirectory::new("v2-migration")?;
    let conn = Connection::open(dir.path().join(crate::DATABASE_FILE))?;
    conn.execute_batch("CREATE TABLE events(position INTEGER PRIMARY KEY AUTOINCREMENT,source TEXT NOT NULL,cursor BLOB NOT NULL,received_at INTEGER NOT NULL,payload BLOB NOT NULL,content_hash INTEGER NOT NULL,UNIQUE(source,content_hash));
        CREATE TABLE cursors(source TEXT PRIMARY KEY,cursor BLOB NOT NULL,last_position INTEGER NOT NULL REFERENCES events(position));
        CREATE TRIGGER events_no_update BEFORE UPDATE ON events BEGIN SELECT RAISE(ABORT,'append-only'); END;
        CREATE TRIGGER events_no_delete BEFORE DELETE ON events BEGIN SELECT RAISE(ABORT,'append-only'); END;
        PRAGMA user_version=2;")?;
    let raw = event(1)?;
    conn.execute(
        "INSERT INTO events VALUES (1,?1,?2,?3,?4,?5)",
        params![
            raw.source.as_str(),
            raw.cursor.as_bytes(),
            raw.received_at.as_millis(),
            raw.payload,
            crate::content_hash(&raw.payload)
        ],
    )?;
    conn.execute(
        "INSERT INTO cursors VALUES (?1,?2,1)",
        params![raw.source.as_str(), raw.cursor.as_bytes()],
    )?;
    drop(conn);
    let mut log = SqliteEventLog::open(dir.path())?;
    assert_eq!(
        log.connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        3
    );
    assert_eq!(log.replay(None)?.next().ok_or("missing event")??.event, raw);
    assert_eq!(log.cursor(&source()?)?, Some(cursor(1)?));
    assert_eq!(
        log.connection
            .query_row("SELECT last_position FROM cursors", [], |r| r
                .get::<_, i64>(0))?,
        1
    );
    let fresh = SourceId::new("fresh")?;
    log.record_source_added(&fresh, EffectiveFrom::FromCursor(cursor(7)?))?;
    assert_eq!(
        log.connection.query_row(
            "SELECT last_position FROM cursors WHERE source='fresh'",
            [],
            |r| r.get::<_, Option<i64>>(0)
        )?,
        None
    );
    drop(log);
    let conn = Connection::open(dir.path().join(crate::DATABASE_FILE))?;
    conn.execute_batch("PRAGMA user_version=4")?;
    drop(conn);
    assert!(matches!(
        retry_until_unlocked(|| SqliteEventLog::open(dir.path())),
        Err(LogError::Corrupt(_))
    ));
    Ok(())
}
#[test]
fn failed_migration_rolls_back_schema_and_version() -> TestResult {
    let mut conn = Connection::open_in_memory()?;
    conn.execute_batch("CREATE TABLE cursors(source TEXT PRIMARY KEY,cursor BLOB NOT NULL,last_position INTEGER NOT NULL);
        INSERT INTO cursors VALUES('old',X'01',1); CREATE VIEW membership AS SELECT 1; PRAGMA user_version=2;")?;
    assert!(migrate_v2_to_v3(&mut conn).is_err());
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        2
    );
    assert!(
        conn.execute("INSERT INTO cursors VALUES('new',X'01',NULL)", [])
            .is_err()
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM cursors", [], |r| r.get::<_, i64>(0))?,
        1
    );
    Ok(())
}
#[test]
fn manifest_is_idempotent_and_rejects_world_mismatch() -> TestResult {
    let dir = TestDirectory::new("manifest")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    let expected = WorldManifest::create_if_absent(
        &mut log,
        "world",
        "Name",
        Timestamp::from_millis(7),
        &["a|b\n\"".into()],
        &["policy".into()],
    )?;
    assert_eq!(
        WorldManifest::create_if_absent(
            &mut log,
            "world",
            "Other",
            Timestamp::from_millis(8),
            &[],
            &[]
        )?,
        expected
    );
    assert!(matches!(
        WorldManifest::load(&log, "other"),
        Err(LogError::Corrupt(_))
    ));
    Ok(())
}
