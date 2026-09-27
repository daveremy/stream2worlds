//! An append-only event log with atomic source cursors.
//!
//! [`SqliteEventLog`] is the durable implementation. [`InMemoryEventLog`] provides the same
//! seam without I/O for composition and tests.

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};
use s2w_model::{Cursor, ModelError, RawEvent, SourceId, Timestamp};

const DATABASE_FILE: &str = "events.sqlite3";
const LOCK_FILE: &str = "LOCK";
const SCHEMA_VERSION: i64 = 1;
const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
const REPLAY_PAGE_SIZE: i64 = 256;

/// Rejects a payload before any write is attempted, shared by every [`EventLog`] impl.
fn check_payload_size(len: usize) -> Result<(), LogError> {
    if len > MAX_PAYLOAD_BYTES {
        return Err(LogError::TooLarge);
    }
    Ok(())
}

/// A monotonically increasing position assigned by one event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogPosition(u64);

impl LogPosition {
    /// The numeric, log-local position.
    ///
    /// There is no public constructor: a [`LogPosition`] is only ever produced by a
    /// log implementation itself (via `append`/`replay`), so it is an opaque replay
    /// token rather than a value callers assemble by hand.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// A raw event together with its position in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    /// The event's log-local position.
    pub position: LogPosition,
    /// The source event stored at that position.
    pub event: RawEvent,
}

/// Storage-neutral failures from an event log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    /// An unclassified storage or filesystem operation failed.
    Io(String),
    /// The database or its declared schema version is corrupt or unsupported.
    Corrupt(String),
    /// Another durable log handle already holds the directory's writer lock.
    Locked,
    /// An event payload exceeded the 8 MiB defensive cap.
    TooLarge,
    /// A persisted cursor failed model validation.
    InvalidCursor(ModelError),
}

impl fmt::Display for LogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "event-log I/O failed: {message}"),
            Self::Corrupt(message) => write!(formatter, "event log is corrupt: {message}"),
            Self::Locked => formatter.write_str("event log is already open by another writer"),
            Self::TooLarge => formatter.write_str("event payload exceeds the 8 MiB limit"),
            Self::InvalidCursor(error) => write!(formatter, "invalid stored cursor: {error}"),
        }
    }
}

impl Error for LogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCursor(error) => Some(error),
            Self::Io(_) | Self::Corrupt(_) | Self::Locked | Self::TooLarge => None,
        }
    }
}

/// The storage seam used by the application composition root.
pub trait EventLog {
    /// Appends one event and atomically advances its source cursor.
    ///
    /// # Errors
    /// Returns a storage error without partially storing the event or advancing the cursor.
    fn append(&mut self, event: RawEvent) -> Result<LogPosition, LogError>;

    /// Reads the most recently committed cursor for `source`.
    ///
    /// # Errors
    /// Returns an error when storage cannot be read or persisted bytes are invalid.
    fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, LogError>;

    /// Replays events strictly after `from`, or the entire log when it is `None`.
    ///
    /// # Errors
    /// Returns an error if the replay query cannot be prepared. Errors encountered while
    /// loading later pages are yielded by the returned iterator.
    fn replay(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError>;
}

/// An append-only in-memory event log.
#[derive(Debug, Default)]
pub struct InMemoryEventLog {
    events: Vec<StoredEvent>,
    cursors: HashMap<SourceId, (Cursor, LogPosition)>,
}

impl InMemoryEventLog {
    /// Constructs an empty in-memory log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl EventLog for InMemoryEventLog {
    fn append(&mut self, event: RawEvent) -> Result<LogPosition, LogError> {
        check_payload_size(event.payload.len())?;
        let next = u64::try_from(self.events.len())
            .map_err(|error| LogError::Io(error.to_string()))?
            .checked_add(1)
            .ok_or_else(|| LogError::Io("log position overflow".to_owned()))?;
        let position = LogPosition(next);
        self.cursors
            .insert(event.source.clone(), (event.cursor.clone(), position));
        self.events.push(StoredEvent { position, event });
        Ok(position)
    }

    fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, LogError> {
        Ok(self.cursors.get(source).map(|(cursor, _)| cursor.clone()))
    }

    fn replay(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        let after = from.map_or(0, LogPosition::as_u64);
        Ok(Box::new(
            self.events
                .iter()
                .filter(move |stored| stored.position.as_u64() > after)
                .cloned()
                .map(Ok),
        ))
    }
}

/// A durable SQLite-backed event log.
pub struct SqliteEventLog {
    connection: Connection,
    _lock: File,
}

impl fmt::Debug for SqliteEventLog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SqliteEventLog")
            .finish_non_exhaustive()
    }
}

impl SqliteEventLog {
    /// Opens or creates an event log in `directory`.
    ///
    /// The writer lock is acquired before SQLite is opened. WAL, full synchronous writes,
    /// foreign keys, and recursive triggers are configured for every connection.
    ///
    /// # Errors
    /// Returns [`LogError::Locked`] when another handle owns the directory lock, and a storage
    /// or corruption error when the directory or database cannot be initialized safely.
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, LogError> {
        let directory = directory.as_ref();
        fs::create_dir_all(directory).map_err(map_fs_error)?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(directory.join(LOCK_FILE))
            .map_err(map_fs_error)?;
        lock.try_lock().map_err(|_| LogError::Locked)?;

        let mut connection = Connection::open(directory.join(DATABASE_FILE)).map_err(map_sqlite)?;
        connection
            .busy_timeout(Duration::from_secs(3))
            .map_err(map_sqlite)?;

        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .map_err(map_sqlite)?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(LogError::Io(format!(
                "SQLite refused WAL mode and selected {journal_mode:?}"
            )));
        }
        connection
            .execute_batch(
                "PRAGMA synchronous = FULL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA recursive_triggers = ON;",
            )
            .map_err(map_sqlite)?;

        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(map_sqlite)?;
        if version != 0 && version != SCHEMA_VERSION {
            return Err(LogError::Corrupt(format!(
                "unsupported schema version {version}; expected {SCHEMA_VERSION}"
            )));
        }
        if version == 0 {
            initialize_schema(&mut connection)?;
        }

        Ok(Self {
            connection,
            _lock: lock,
        })
    }
}

impl EventLog for SqliteEventLog {
    fn append(&mut self, event: RawEvent) -> Result<LogPosition, LogError> {
        check_payload_size(event.payload.len())?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        transaction
            .execute(
                "INSERT INTO events (source, cursor, received_at, payload)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    event.source.as_str(),
                    event.cursor.as_bytes(),
                    event.received_at.as_millis(),
                    event.payload
                ],
            )
            .map_err(map_sqlite)?;
        let row_id = transaction.last_insert_rowid();
        transaction
            .execute(
                "INSERT INTO cursors (source, cursor, last_position)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(source) DO UPDATE SET
                    cursor = excluded.cursor,
                    last_position = excluded.last_position",
                params![event.source.as_str(), event.cursor.as_bytes(), row_id],
            )
            .map_err(map_sqlite)?;
        transaction.commit().map_err(map_sqlite)?;
        let position = u64::try_from(row_id)
            .map(LogPosition)
            .map_err(|error| LogError::Corrupt(error.to_string()))?;
        Ok(position)
    }

    fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, LogError> {
        let bytes: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT cursor FROM cursors WHERE source = ?1",
                [source.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sqlite)?;
        bytes
            .map(Cursor::new)
            .transpose()
            .map_err(LogError::InvalidCursor)
    }

    fn replay(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        let last_seen = from.map_or(0, LogPosition::as_u64);
        let mut replay = Replay {
            connection: &self.connection,
            last_seen,
            buffer: VecDeque::new(),
            finished: false,
        };
        replay.refill()?;
        Ok(Box::new(replay))
    }
}

struct Replay<'connection> {
    connection: &'connection Connection,
    last_seen: u64,
    buffer: VecDeque<StoredEvent>,
    finished: bool,
}

impl Replay<'_> {
    fn refill(&mut self) -> Result<(), LogError> {
        let last_seen =
            i64::try_from(self.last_seen).map_err(|error| LogError::Corrupt(error.to_string()))?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT position, source, cursor, received_at, payload
                 FROM events
                 WHERE position > ?1
                 ORDER BY position
                 LIMIT ?2",
            )
            .map_err(map_sqlite)?;
        let mut rows = statement
            .query(params![last_seen, REPLAY_PAGE_SIZE])
            .map_err(map_sqlite)?;
        while let Some(row) = rows.next().map_err(map_sqlite)? {
            self.buffer.push_back(decode_stored_event(row)?);
        }
        if self.buffer.is_empty() {
            self.finished = true;
        }
        Ok(())
    }
}

impl Iterator for Replay<'_> {
    type Item = Result<StoredEvent, LogError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.is_empty()
            && !self.finished
            && let Err(error) = self.refill()
        {
            self.finished = true;
            return Some(Err(error));
        }
        let event = self.buffer.pop_front()?;
        self.last_seen = event.position.as_u64();
        Some(Ok(event))
    }
}

fn initialize_schema(connection: &mut Connection) -> Result<(), LogError> {
    let transaction = connection.transaction().map_err(map_sqlite)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                position    INTEGER PRIMARY KEY AUTOINCREMENT,
                source      TEXT    NOT NULL,
                cursor      BLOB    NOT NULL,
                received_at INTEGER NOT NULL,
                payload     BLOB    NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cursors (
                source        TEXT PRIMARY KEY,
                cursor        BLOB    NOT NULL,
                last_position INTEGER NOT NULL REFERENCES events(position)
            );
            CREATE TRIGGER IF NOT EXISTS events_no_update
            BEFORE UPDATE ON events BEGIN
                SELECT RAISE(ABORT, 'events are append-only: update refused');
            END;
            CREATE TRIGGER IF NOT EXISTS events_no_delete
            BEFORE DELETE ON events BEGIN
                SELECT RAISE(ABORT, 'events are append-only: delete refused');
            END;
            PRAGMA user_version = 1;",
        )
        .map_err(map_sqlite)?;
    transaction.commit().map_err(map_sqlite)
}

fn decode_stored_event(row: &rusqlite::Row<'_>) -> Result<StoredEvent, LogError> {
    let position: i64 = row.get(0).map_err(map_sqlite)?;
    let source: String = row.get(1).map_err(map_sqlite)?;
    let cursor: Vec<u8> = row.get(2).map_err(map_sqlite)?;
    let received_at: i64 = row.get(3).map_err(map_sqlite)?;
    let payload: Vec<u8> = row.get(4).map_err(map_sqlite)?;

    let position = u64::try_from(position)
        .map(LogPosition)
        .map_err(|error| LogError::Corrupt(error.to_string()))?;
    let source = SourceId::new(source).map_err(|error| LogError::Corrupt(error.to_string()))?;
    let cursor = Cursor::new(cursor).map_err(LogError::InvalidCursor)?;

    Ok(StoredEvent {
        position,
        event: RawEvent {
            source,
            cursor,
            received_at: Timestamp::from_millis(received_at),
            payload,
        },
    })
}

fn map_sqlite(error: rusqlite::Error) -> LogError {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
            LogError::Corrupt(error.to_string())
        }
        _ => LogError::Io(error.to_string()),
    }
}

fn map_fs_error(error: std::io::Error) -> LogError {
    LogError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const CRASH_CHILD_ENV: &str = "S2W_LOG_CRASH_CHILD_DIRECTORY";
    const CRASH_EVENT_COUNT: u8 = 12;
    const CRASH_AFTER: u8 = 6;
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    type TestResult = Result<(), Box<dyn Error>>;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> std::io::Result<Self> {
            loop {
                let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir()
                    .join(format!("s2w-log-{label}-{}-{sequence}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Ok(Self(path)),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn source(name: &str) -> Result<SourceId, ModelError> {
        SourceId::new(name)
    }

    fn cursor(value: u8) -> Result<Cursor, ModelError> {
        Cursor::new(vec![value])
    }

    fn event(index: u8) -> Result<RawEvent, ModelError> {
        let source_name = if index.is_multiple_of(2) {
            "source-b"
        } else {
            "source-a"
        };
        Ok(RawEvent {
            source: source(source_name)?,
            cursor: cursor(index)?,
            received_at: Timestamp::from_millis(i64::from(index) * 1000),
            payload: format!("payload-{index}").into_bytes(),
        })
    }

    fn run_conformance_suite<L: EventLog>(mut log: L) -> TestResult {
        let source_a = source("source-a")?;
        assert_eq!(log.cursor(&source_a)?, None);

        let event_a = event(1)?;
        let position_a = log.append(event_a.clone())?;
        assert_eq!(log.cursor(&source_a)?, Some(event_a.cursor.clone()));

        let event_b = event(2)?;
        let position_b = log.append(event_b.clone())?;
        assert!(position_b > position_a);

        let event_c = event(3)?;
        let position_c = log.append(event_c.clone())?;
        assert!(position_c > position_b);
        assert_eq!(log.cursor(&source_a)?, Some(event_c.cursor.clone()));

        let after_a = log
            .replay(Some(position_a))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            after_a,
            vec![
                StoredEvent {
                    position: position_b,
                    event: event_b.clone()
                },
                StoredEvent {
                    position: position_c,
                    event: event_c.clone()
                }
            ]
        );

        let all = log.replay(None)?.collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            all,
            vec![
                StoredEvent {
                    position: position_a,
                    event: event_a
                },
                StoredEvent {
                    position: position_b,
                    event: event_b
                },
                StoredEvent {
                    position: position_c,
                    event: event_c
                }
            ]
        );
        Ok(())
    }

    #[test]
    fn in_memory_conforms_to_event_log_seam() -> TestResult {
        run_conformance_suite(InMemoryEventLog::new())
    }

    #[test]
    fn sqlite_conforms_to_event_log_seam() -> TestResult {
        let directory = TestDirectory::new("conformance")?;
        run_conformance_suite(SqliteEventLog::open(directory.path())?)
    }

    #[test]
    fn sqlite_restart_preserves_events_and_cursors_once_in_order() -> TestResult {
        let directory = TestDirectory::new("restart")?;
        let expected = vec![event(1)?, event(2)?, event(3)?];
        {
            let mut log = SqliteEventLog::open(directory.path())?;
            for raw in &expected {
                log.append(raw.clone())?;
            }
        }

        let log = SqliteEventLog::open(directory.path())?;
        let actual = log
            .replay(None)?
            .map(|result| result.map(|stored| stored.event))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(actual, expected);
        assert_eq!(log.cursor(&source("source-a")?)?, Some(cursor(3)?));
        assert_eq!(log.cursor(&source("source-b")?)?, Some(cursor(2)?));
        Ok(())
    }

    #[test]
    fn sqlite_schema_refuses_update_and_delete() -> TestResult {
        let directory = TestDirectory::new("triggers")?;
        let mut log = SqliteEventLog::open(directory.path())?;
        log.append(event(1)?)?;
        drop(log);

        let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
        assert!(
            connection
                .execute("UPDATE events SET payload = X'00' WHERE position = 1", [])
                .is_err()
        );
        assert!(
            connection
                .execute("DELETE FROM events WHERE position = 1", [])
                .is_err()
        );
        let payload: Vec<u8> =
            connection.query_row("SELECT payload FROM events WHERE position = 1", [], |row| {
                row.get(0)
            })?;
        assert_eq!(payload, b"payload-1");
        Ok(())
    }

    #[test]
    fn sqlite_recursive_trigger_refuses_replace_and_preserves_row() -> TestResult {
        let directory = TestDirectory::new("replace")?;
        let original = event(1)?;
        let mut log = SqliteEventLog::open(directory.path())?;
        log.append(original.clone())?;
        drop(log);

        let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
        connection.execute_batch("PRAGMA recursive_triggers = ON;")?;
        assert!(
            connection
                .execute(
                    "INSERT OR REPLACE INTO events
                     (position, source, cursor, received_at, payload)
                     VALUES (1, 'replacement', X'09', 9, X'09')",
                    [],
                )
                .is_err()
        );
        let unchanged: (String, Vec<u8>, i64, Vec<u8>) = connection.query_row(
            "SELECT source, cursor, received_at, payload FROM events WHERE position = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            unchanged,
            (
                original.source.as_str().to_owned(),
                original.cursor.as_bytes().to_vec(),
                original.received_at.as_millis(),
                original.payload
            )
        );
        Ok(())
    }

    #[test]
    fn sqlite_rejects_oversized_payload_before_writing() -> TestResult {
        let directory = TestDirectory::new("too-large")?;
        let mut log = SqliteEventLog::open(directory.path())?;
        let mut oversized = event(1)?;
        oversized.payload = vec![0; MAX_PAYLOAD_BYTES + 1];
        assert_eq!(log.append(oversized), Err(LogError::TooLarge));
        assert!(log.replay(None)?.next().is_none());
        assert_eq!(log.cursor(&source("source-a")?)?, None);
        Ok(())
    }

    #[test]
    fn sqlite_second_open_fails_fast_while_lock_is_held() -> TestResult {
        let directory = TestDirectory::new("lock")?;
        let first = SqliteEventLog::open(directory.path())?;
        assert_eq!(
            SqliteEventLog::open(directory.path()).err(),
            Some(LogError::Locked)
        );
        drop(first);
        assert!(SqliteEventLog::open(directory.path()).is_ok());
        Ok(())
    }

    #[test]
    fn sqlite_uses_wal_and_full_synchronous_on_every_open() -> TestResult {
        let directory = TestDirectory::new("pragmas")?;
        for _ in 0..2 {
            let log = SqliteEventLog::open(directory.path())?;
            let journal: String = log
                .connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
            let synchronous: i64 = log
                .connection
                .query_row("PRAGMA synchronous", [], |row| row.get(0))?;
            assert_eq!(journal.to_ascii_lowercase(), "wal");
            assert_eq!(synchronous, 2);
        }
        Ok(())
    }

    #[test]
    fn sqlite_rejects_unknown_schema_version_as_corrupt() -> TestResult {
        let directory = TestDirectory::new("version")?;
        let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
        connection.execute_batch("PRAGMA user_version = 99;")?;
        drop(connection);
        assert!(matches!(
            SqliteEventLog::open(directory.path()),
            Err(LogError::Corrupt(_))
        ));
        Ok(())
    }

    #[test]
    fn sqlite_crash_child() -> TestResult {
        let Some(directory) = env::var_os(CRASH_CHILD_ENV) else {
            return Ok(());
        };
        let mut log = SqliteEventLog::open(PathBuf::from(directory))?;
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        for index in 1..=CRASH_EVENT_COUNT {
            log.append(event(index)?)?;
            writeln!(output, "committed {index}")?;
            output.flush()?;
            let mut acknowledgement = [0_u8; 1];
            input.read_exact(&mut acknowledgement)?;
        }
        Ok(())
    }

    #[test]
    fn sqlite_recovers_exact_atomic_prefix_after_sigkill() -> TestResult {
        if env::var_os(CRASH_CHILD_ENV).is_some() {
            return Ok(());
        }
        let directory = TestDirectory::new("crash")?;
        let executable = env::current_exe()?;
        let mut child = Command::new(executable)
            .arg("--exact")
            .arg("tests::sqlite_crash_child")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CRASH_CHILD_ENV, directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or("child stdout pipe was not available")?;
        let mut child_stdin = child
            .stdin
            .take()
            .ok_or("child stdin pipe was not available")?;
        let mut lines = BufReader::new(child_stdout).lines();
        let mut committed = 0_u8;
        while committed < CRASH_AFTER {
            let line = lines
                .next()
                .ok_or("child exited before the crash marker")??;
            if line.contains("committed ") {
                committed = committed.saturating_add(1);
                assert!(line.ends_with(&format!("committed {committed}")));
                if committed < CRASH_AFTER {
                    child_stdin.write_all(&[1])?;
                    child_stdin.flush()?;
                }
            }
        }
        child.kill()?;
        let status = child.wait()?;
        assert!(!status.success());

        let log = SqliteEventLog::open(directory.path())?;
        let stored = log.replay(None)?.collect::<Result<Vec<_>, _>>()?;
        let expected = (1..=CRASH_AFTER)
            .map(event)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            stored
                .iter()
                .map(|item| item.event.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(stored.len(), usize::from(CRASH_AFTER));

        let source_a = source("source-a")?;
        let source_b = source("source-b")?;
        assert_eq!(log.cursor(&source_a)?, Some(cursor(5)?));
        assert_eq!(log.cursor(&source_b)?, Some(cursor(6)?));
        let cursor_rows = [
            (&source_a, 5_i64, cursor(5)?),
            (&source_b, 6_i64, cursor(6)?),
        ];
        for (source_id, expected_position, expected_cursor) in cursor_rows {
            let actual: (Vec<u8>, i64) = log.connection.query_row(
                "SELECT cursor, last_position FROM cursors WHERE source = ?1",
                [source_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            assert_eq!(
                actual,
                (expected_cursor.as_bytes().to_vec(), expected_position)
            );
        }
        Ok(())
    }
}
