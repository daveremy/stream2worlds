//! An append-only event log with atomic source cursors.
//!
//! [`SqliteEventLog`] is the durable implementation. [`InMemoryEventLog`] provides the same
//! seam without I/O for composition and tests. The System 1 verdict store sits beside the log
//! in the same directory: see [`VerdictStore`].

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, TransactionBehavior, params};
use s2w_model::{Cursor, ModelError, RawEvent, SourceId, Timestamp};

mod manifest;
mod membership;
mod presentation;
mod reader;
mod verdicts;

pub use manifest::WorldManifest;
pub use membership::{EffectiveFrom, MembershipRow, members_at};
pub use presentation::{Palette, Typefaces, WorldPresentation, WorldPresentationInput};
pub use reader::LogReader;
pub use verdicts::{
    InMemoryVerdictStore, ReadOnlySqliteVerdictStore, SqliteVerdictStore, StoredVerdict,
    VerdictStore,
};

const DATABASE_FILE: &str = "events.sqlite3";
const LOCK_FILE: &str = "LOCK";
const SCHEMA_VERSION: i64 = 4;
const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
const REPLAY_PAGE_SIZE: i64 = 256;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// Opens (creating if needed) a SQLite-backed store in `directory`: acquires an exclusive file
/// lock, opens the database, forces WAL mode and `extra_pragmas`, then checks the stored
/// `user_version` against `schema_version` and runs `initialize_schema` on a fresh database.
///
/// Shared by [`SqliteEventLog::open`] and [`SqliteVerdictStore::open`], which differ only in
/// their filenames, extra pragmas, schema version, and how a fresh schema is created.
///
/// # Errors
/// [`LogError::Locked`] when another handle owns `lock_file`; a storage or corruption error
/// when the directory or database cannot be initialized safely.
#[expect(
    clippy::too_many_arguments,
    reason = "one argument per storage-format knob of the two SQLite stores sharing this opener; a parameter struct would only rename them"
)]
fn open_sqlite_store(
    directory: &Path,
    database_file: &str,
    lock_file: &str,
    extra_pragmas: &str,
    schema_version: i64,
    schema_mismatch: impl FnOnce(i64) -> String,
    initialize_schema: impl FnOnce(&mut Connection) -> Result<(), LogError>,
) -> Result<(Connection, File), LogError> {
    fs::create_dir_all(directory).map_err(map_fs_error)?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(directory.join(lock_file))
        .map_err(map_fs_error)?;
    lock.try_lock().map_err(map_try_lock_error)?;

    let mut connection = Connection::open(directory.join(database_file)).map_err(map_sqlite)?;
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
        .execute_batch(extra_pragmas)
        .map_err(map_sqlite)?;

    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if database_file == DATABASE_FILE && version == 2 {
        membership::migrate_v2_to_v3(&mut connection)?;
        presentation::migrate_v3_to_v4(&mut connection)?;
    } else if database_file == DATABASE_FILE && version == 3 {
        presentation::migrate_v3_to_v4(&mut connection)?;
    } else if version != 0 && version != schema_version {
        return Err(LogError::Corrupt(schema_mismatch(version)));
    }
    if version == 0 {
        initialize_schema(&mut connection)?;
    }

    Ok((connection, lock))
}

/// Opens an already-initialized SQLite store without creating a directory or taking its writer
/// lock. Read-only handles can coexist with the process that owns the append path.
fn open_sqlite_store_read_only(
    directory: &Path,
    database_file: &str,
    schema_version: i64,
    schema_mismatch: impl FnOnce(i64) -> String,
) -> Result<Connection, LogError> {
    let connection = Connection::open_with_flags(
        directory.join(database_file),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(map_sqlite)?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(map_sqlite)?;
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if version == 0 {
        return Err(LogError::Corrupt(format!(
            "no schema in {database_file}; nothing has been written yet"
        )));
    }
    if version != schema_version {
        return Err(LogError::Corrupt(schema_mismatch(version)));
    }
    Ok(connection)
}

/// Rejects a payload before any write is attempted, shared by every [`EventLog`] impl.
fn check_payload_size(len: usize) -> Result<(), LogError> {
    if len > MAX_PAYLOAD_BYTES {
        return Err(LogError::TooLarge);
    }
    Ok(())
}

/// A deterministic, dependency-free hash of a payload's bytes, used to key append dedupe.
///
/// FNV-1a over the payload, reinterpreted as a signed integer for SQLite storage. Deliberately
/// not `DefaultHasher`, whose per-process random seed would break dedupe across restarts. The
/// value is persisted, so the algorithm is pinned by a known-answer test: changing it silently
/// would change dedupe for every existing log. A 64-bit hash can collide, so the append paths
/// compare payload bytes on a hit and fail loudly instead of dropping a distinct event.
fn content_hash(payload: &[u8]) -> i64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in payload {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    i64::from_ne_bytes(hash.to_ne_bytes())
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

    /// Converts to the `i64` SQLite stores a position as. Every SQLite-backed store in this
    /// crate binds the same `INTEGER` column type, so this is the one place that conversion is
    /// written.
    ///
    /// # Errors
    /// [`LogError::Corrupt`] if the position does not fit in an `i64` (practically unreachable:
    /// it would require appending past `i64::MAX` positions first).
    pub(crate) fn to_sql(self) -> Result<i64, LogError> {
        i64::try_from(self.0).map_err(|error| LogError::Corrupt(error.to_string()))
    }

    /// The inverse of [`LogPosition::to_sql`]: reconstructs a position read back from SQLite.
    ///
    /// # Errors
    /// [`LogError::Corrupt`] if the stored value is negative — a position column should never
    /// hold one, so a negative value means the row was written by something other than this
    /// crate's own inserts.
    pub(crate) fn from_sql(position: i64) -> Result<Self, LogError> {
        u64::try_from(position)
            .map(Self)
            .map_err(|error| LogError::Corrupt(error.to_string()))
    }
}

/// A raw event together with its position in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    /// The event's log-local position.
    pub position: LogPosition,
    /// The source event stored at that position.
    pub event: RawEvent,
    /// The FNV-1a hash of `event.payload` that keys append dedupe, as stored (`i64`). Every
    /// log implementation computes it with the same function, so a verdict store can bind a
    /// verdict to the exact event it judged, not only to a position.
    pub content_hash: i64,
}

/// What one [`EventLog::append`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    /// The event was new and is now stored at this position.
    Inserted(LogPosition),
    /// This source had already stored byte-identical payload bytes at this position, so nothing
    /// was written and the source's cursor did not move.
    Duplicate(LogPosition),
    /// Membership forbids this event; its cursor is unchanged.
    Rejected,
    /// The entire batch was fetched under an obsolete membership generation.
    StaleGeneration,
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
    /// Re-adding requires a cursor until adapters can resolve a live tail.
    ReaddRequiresCursor,
    /// A presentation record failed validation before being written.
    InvalidPresentation(String),
}

impl fmt::Display for LogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "event-log I/O failed: {message}"),
            Self::Corrupt(message) => write!(formatter, "event log is corrupt: {message}"),
            Self::Locked => formatter.write_str("event log is already open by another writer"),
            Self::TooLarge => formatter.write_str("event payload exceeds the 8 MiB limit"),
            Self::ReaddRequiresCursor => formatter.write_str(
                "re-add requires an explicit cursor: adapters cannot resolve a live tail",
            ),
            Self::InvalidCursor(error) => write!(formatter, "invalid stored cursor: {error}"),
            Self::InvalidPresentation(message) => {
                write!(formatter, "invalid presentation: {message}")
            }
        }
    }
}

impl Error for LogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCursor(error) => Some(error),
            Self::Io(_)
            | Self::Corrupt(_)
            | Self::Locked
            | Self::TooLarge
            | Self::ReaddRequiresCursor
            | Self::InvalidPresentation(_) => None,
        }
    }
}

/// The storage seam used by the application composition root.
pub trait EventLog {
    /// Appends one event and atomically advances its source cursor.
    ///
    /// Dedupe is keyed on the event's identity — the source plus the payload's content hash —
    /// never on the cursor alone, since two real events can share a cursor value. A redelivery
    /// of an already-stored event returns [`AppendOutcome::Duplicate`] and changes nothing.
    ///
    /// # Errors
    /// Returns a storage error without partially storing the event or advancing the cursor.
    fn append(&mut self, event: RawEvent) -> Result<AppendOutcome, LogError>;

    /// Appends `events` in order as one group commit: one transaction, one durable sync.
    ///
    /// Returns one outcome per input event, in input order. Each event is classified exactly as
    /// [`EventLog::append`] would classify it after the events before it in the batch, so an
    /// event repeated inside one batch is [`AppendOutcome::Duplicate`] of its first copy, and each
    /// source's cursor ends at its last inserted event. An empty batch writes nothing.
    ///
    /// # Errors
    /// All or nothing: on any error — an oversized payload, a content-hash collision, a storage
    /// failure — no event of the batch is stored and no cursor moves.
    fn append_batch(&mut self, events: Vec<RawEvent>) -> Result<Vec<AppendOutcome>, LogError>;

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
    seen: HashMap<(SourceId, i64), LogPosition>,
}

impl InMemoryEventLog {
    /// Constructs an empty in-memory log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Classifies and, when new, stores one event; the shared body of both append paths.
    fn insert(&mut self, event: RawEvent) -> Result<AppendOutcome, LogError> {
        check_payload_size(event.payload.len())?;
        let key = (event.source.clone(), content_hash(&event.payload));
        if let Some(&position) = self.seen.get(&key) {
            // A hash hit is a duplicate only when the payload bytes match, mirroring the
            // SQLite path: a genuine hash collision is loud, never a silent drop.
            let stored = self.stored_payload(position).ok_or_else(|| {
                LogError::Corrupt(format!(
                    "content hash points at missing position {}",
                    position.as_u64()
                ))
            })?;
            if stored != event.payload.as_slice() {
                return Err(collision(event.source.as_str(), key.1, position));
            }
            // Duplicate: the cursor is untouched, so a redelivery cannot move it backwards.
            return Ok(AppendOutcome::Duplicate(position));
        }
        let next = u64::try_from(self.events.len())
            .map_err(|error| LogError::Io(error.to_string()))?
            .checked_add(1)
            .ok_or_else(|| LogError::Io("log position overflow".to_owned()))?;
        let position = LogPosition(next);
        self.cursors
            .insert(event.source.clone(), (event.cursor.clone(), position));
        let content_hash = key.1;
        self.events.push(StoredEvent {
            position,
            event,
            content_hash,
        });
        self.seen.insert(key, position);
        Ok(AppendOutcome::Inserted(position))
    }

    /// The stored payload at `position`, if it is still held.
    fn stored_payload(&self, position: LogPosition) -> Option<&[u8]> {
        let index = usize::try_from(position.as_u64().checked_sub(1)?).ok()?;
        self.events
            .get(index)
            .map(|stored| stored.event.payload.as_slice())
    }
}

impl EventLog for InMemoryEventLog {
    fn append(&mut self, event: RawEvent) -> Result<AppendOutcome, LogError> {
        self.insert(event)
    }

    fn append_batch(&mut self, events: Vec<RawEvent>) -> Result<Vec<AppendOutcome>, LogError> {
        for event in &events {
            check_payload_size(event.payload.len())?;
        }
        // All or nothing: apply to a copy and swap it in only when every event succeeded.
        let mut staged = InMemoryEventLog {
            events: self.events.clone(),
            cursors: self.cursors.clone(),
            seen: self.seen.clone(),
        };
        let outcomes = events
            .into_iter()
            .map(|event| staged.insert(event))
            .collect::<Result<Vec<_>, _>>()?;
        *self = staged;
        Ok(outcomes)
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

/// A lockless, read-only handle to an initialized SQLite event log.
pub struct ReadOnlySqliteEventLog {
    connection: Connection,
}

impl fmt::Debug for ReadOnlySqliteEventLog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadOnlySqliteEventLog")
            .finish_non_exhaustive()
    }
}

impl ReadOnlySqliteEventLog {
    /// Opens an existing event log without taking the writer lock or changing its schema.
    ///
    /// # Errors
    /// Returns a storage error when the database cannot be opened and corruption when its
    /// schema is absent or unsupported.
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, LogError> {
        let connection = open_sqlite_store_read_only(
            directory.as_ref(),
            DATABASE_FILE,
            SCHEMA_VERSION,
            |version| {
                format!(
                    "unsupported schema version {version}; expected {SCHEMA_VERSION}. this version cannot migrate that schema"
                )
            },
        )?;
        Ok(Self { connection })
    }
}

impl LogReader for ReadOnlySqliteEventLog {
    fn read_after(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        replay_from(&self.connection, from)
    }
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
        let (connection, lock) = open_sqlite_store(
            directory.as_ref(),
            DATABASE_FILE,
            LOCK_FILE,
            "PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             PRAGMA recursive_triggers = ON;",
            SCHEMA_VERSION,
            |version| {
                format!(
                    "unsupported schema version {version}; expected {SCHEMA_VERSION}. this version cannot migrate that schema"
                )
            },
            initialize_schema,
        )?;
        Ok(Self {
            connection,
            _lock: lock,
        })
    }
}

impl EventLog for SqliteEventLog {
    fn append(&mut self, event: RawEvent) -> Result<AppendOutcome, LogError> {
        let mut outcomes = EventLog::append_batch(self, vec![event])?;
        outcomes
            .pop()
            .ok_or_else(|| LogError::Corrupt("a one-event append produced no outcome".to_owned()))
    }

    fn append_batch(&mut self, events: Vec<RawEvent>) -> Result<Vec<AppendOutcome>, LogError> {
        for event in &events {
            check_payload_size(event.payload.len())?;
        }
        if events.is_empty() {
            return Ok(Vec::new());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        let outcomes = events
            .iter()
            .map(|event| insert_in(&transaction, event))
            .collect::<Result<Vec<_>, _>>()?;
        // Any error above returned early and dropped the transaction uncommitted: nothing of
        // the batch is stored and no cursor moved.
        transaction.commit().map_err(map_sqlite)?;
        Ok(outcomes)
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
        replay_from(&self.connection, from)
    }
}

fn replay_from(
    connection: &Connection,
    from: Option<LogPosition>,
) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
    let last_seen = from.map_or(0, LogPosition::as_u64);
    let mut replay = Replay {
        connection,
        last_seen,
        buffer: VecDeque::new(),
        finished: false,
    };
    replay.refill()?;
    Ok(Box::new(replay))
}

/// Inserts one event inside an open transaction and advances its source cursor there.
///
/// A duplicate or collision is classified by [`resolve_duplicate`] and leaves the cursor row
/// untouched. The caller owns the commit.
fn insert_in(
    transaction: &rusqlite::Transaction<'_>,
    event: &RawEvent,
) -> Result<AppendOutcome, LogError> {
    if !membership::source_state(transaction, &event.source)?.0 {
        return Ok(AppendOutcome::Rejected);
    }
    let hash = content_hash(&event.payload);
    let inserted = transaction
        .execute(
            "INSERT INTO events (source, cursor, received_at, payload, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source, content_hash) DO NOTHING",
            params![
                event.source.as_str(),
                event.cursor.as_bytes(),
                event.received_at.as_millis(),
                event.payload,
                hash
            ],
        )
        .map_err(map_sqlite)?;
    if inserted == 0 {
        return resolve_duplicate(transaction, event.source.as_str(), hash, &event.payload);
    }
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
    LogPosition::from_sql(row_id).map(AppendOutcome::Inserted)
}

/// The loud error for a 64-bit content-hash collision between two distinct payloads.
///
/// Names the source, hash and stored position so a human can inspect the two events: the same
/// event is redelivered on every restart, so this error repeats until someone acts on it.
fn collision(source: &str, hash: i64, position: LogPosition) -> LogError {
    LogError::Corrupt(format!(
        "content-hash collision between distinct payloads: source {source}, hash {hash}, stored at position {}",
        position.as_u64()
    ))
}

/// Classifies an insert that changed no rows: a byte-identical redelivery, or a hash collision.
///
/// The stored payload bytes are compared against the new event's, because `UNIQUE(source,
/// content_hash)` alone would silently drop a real event on a 64-bit hash collision between two
/// distinct payloads. Nothing was inserted, so the cursor upsert is skipped entirely:
/// `last_insert_rowid()` still reports the previous successful insert's row here and would
/// corrupt `cursors.last_position`. A collision error aborts the whole transaction, batch
/// included.
fn resolve_duplicate(
    transaction: &rusqlite::Transaction<'_>,
    source: &str,
    hash: i64,
    payload: &[u8],
) -> Result<AppendOutcome, LogError> {
    let (position, stored_payload): (i64, Vec<u8>) = transaction
        .query_row(
            "SELECT position, payload FROM events WHERE source = ?1 AND content_hash = ?2",
            params![source, hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(map_sqlite)?;
    let position = LogPosition::from_sql(position)?;
    if stored_payload != payload {
        return Err(collision(source, hash, position));
    }
    Ok(AppendOutcome::Duplicate(position))
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
                "SELECT position, source, cursor, received_at, payload, content_hash
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
                position     INTEGER PRIMARY KEY AUTOINCREMENT,
                source       TEXT    NOT NULL,
                cursor       BLOB    NOT NULL,
                received_at  INTEGER NOT NULL,
                payload      BLOB    NOT NULL,
                content_hash INTEGER NOT NULL,
                UNIQUE(source, content_hash)
            );
            CREATE TABLE IF NOT EXISTS cursors (
                source        TEXT PRIMARY KEY,
                cursor        BLOB    NOT NULL,
                last_position INTEGER REFERENCES events(position)
            );
            CREATE TRIGGER IF NOT EXISTS events_no_update
            BEFORE UPDATE ON events BEGIN
                SELECT RAISE(ABORT, 'events are append-only: update refused');
            END;
            CREATE TRIGGER IF NOT EXISTS events_no_delete
            BEFORE DELETE ON events BEGIN
                SELECT RAISE(ABORT, 'events are append-only: delete refused');
            END;
            PRAGMA user_version = 4;",
        )
        .map_err(map_sqlite)?;
    membership::initialize(&transaction)?;
    presentation::initialize(&transaction)?;
    transaction.commit().map_err(map_sqlite)
}

fn decode_stored_event(row: &rusqlite::Row<'_>) -> Result<StoredEvent, LogError> {
    let position: i64 = row.get(0).map_err(map_sqlite)?;
    let source: String = row.get(1).map_err(map_sqlite)?;
    let cursor: Vec<u8> = row.get(2).map_err(map_sqlite)?;
    let received_at: i64 = row.get(3).map_err(map_sqlite)?;
    let payload: Vec<u8> = row.get(4).map_err(map_sqlite)?;
    let content_hash: i64 = row.get(5).map_err(map_sqlite)?;

    let position = LogPosition::from_sql(position)?;
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
        content_hash,
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

/// Maps a failed [`File::try_lock`] to a [`LogError`]: only [`std::fs::TryLockError::WouldBlock`]
/// means another handle holds the lock. Any other error is a real I/O failure and must not be
/// mistaken for [`LogError::Locked`].
fn map_try_lock_error(error: std::fs::TryLockError) -> LogError {
    match error {
        std::fs::TryLockError::WouldBlock => LogError::Locked,
        std::fs::TryLockError::Error(io_error) => LogError::Io(io_error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Duration;

    use super::*;

    const CRASH_CHILD_ENV: &str = "S2W_LOG_CRASH_CHILD_DIRECTORY";
    const CRASH_EVENT_COUNT: u8 = 12;
    const CRASH_AFTER: u8 = 6;
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    type TestResult = Result<(), Box<dyn Error>>;

    /// A unique scratch directory removed on drop; shared with the verdict-store tests.
    pub(crate) struct TestDirectory(PathBuf);

    impl TestDirectory {
        pub(crate) fn new(label: &str) -> std::io::Result<Self> {
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

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    /// Retries `open` while it returns [`LogError::Locked`], bounded by a short deadline.
    ///
    /// Test-only guard for a "reopen after our own explicit drop" site (#85): closing a
    /// `std::fs::File` releases its `flock` only once every duplicate of its open-file
    /// description is gone. `Command::spawn` (used by the SIGKILL-crash tests) `fork()`s the
    /// whole test binary before `exec()`, and fork duplicates the entire fd table — so a
    /// concurrently-running unrelated test's spawn can transiently hold a duplicate of a lock
    /// file descriptor we just dropped, until the child's `O_CLOEXEC` descriptors close at
    /// `exec()`. A reopen that lands in that window sees `WouldBlock`. Never wrap a site that
    /// asserts fail-fast behavior while another handle is deliberately still held open —
    /// that would hide a real lock regression instead of this scheduling artifact.
    pub(crate) fn retry_until_unlocked<T>(
        mut open: impl FnMut() -> Result<T, LogError>,
    ) -> Result<T, LogError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            match open() {
                Err(LogError::Locked) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                result => return result,
            }
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

    fn event_with(
        source_id: SourceId,
        event_cursor: Cursor,
        payload: Vec<u8>,
        received_at: i64,
    ) -> RawEvent {
        RawEvent {
            source: source_id,
            cursor: event_cursor,
            received_at: Timestamp::from_millis(received_at),
            payload,
        }
    }

    /// Unwraps an expected [`AppendOutcome::Inserted`]; every conformance payload is unique, so
    /// a duplicate there is a bug to fix, not a test to loosen.
    fn inserted(outcome: AppendOutcome) -> Result<LogPosition, String> {
        match outcome {
            AppendOutcome::Inserted(position) => Ok(position),
            AppendOutcome::Rejected | AppendOutcome::StaleGeneration => {
                Err("unexpected membership rejection".into())
            }
            AppendOutcome::Duplicate(position) => Err(format!(
                "expected an insert, saw a duplicate at {}",
                position.as_u64()
            )),
        }
    }

    fn run_conformance_suite<L: EventLog>(mut log: L) -> TestResult {
        let source_a = source("source-a")?;
        assert_eq!(log.cursor(&source_a)?, None);

        let event_a = event(1)?;
        let position_a = inserted(log.append(event_a.clone())?)?;
        assert_eq!(log.cursor(&source_a)?, Some(event_a.cursor.clone()));

        let event_b = event(2)?;
        let position_b = inserted(log.append(event_b.clone())?)?;
        assert!(position_b > position_a);

        let event_c = event(3)?;
        let position_c = inserted(log.append(event_c.clone())?)?;
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
                    content_hash: content_hash(&event_b.payload),
                    event: event_b.clone(),
                },
                StoredEvent {
                    position: position_c,
                    content_hash: content_hash(&event_c.payload),
                    event: event_c.clone(),
                }
            ]
        );

        let all = log.replay(None)?.collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            all,
            vec![
                StoredEvent {
                    position: position_a,
                    content_hash: content_hash(&event_a.payload),
                    event: event_a,
                },
                StoredEvent {
                    position: position_b,
                    content_hash: content_hash(&event_b.payload),
                    event: event_b,
                },
                StoredEvent {
                    position: position_c,
                    content_hash: content_hash(&event_c.payload),
                    event: event_c,
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
    fn content_hash_is_pinned_fnv1a_64() {
        // The hash is persisted on disk, so these published FNV-1a vectors pin the algorithm:
        // a silent change would silently change dedupe for every existing database.
        let as_i64 = |bits: u64| i64::from_ne_bytes(bits.to_ne_bytes());
        assert_eq!(content_hash(b""), as_i64(0xcbf29ce484222325));
        assert_eq!(content_hash(b"a"), as_i64(0xaf63dc4c8601ec8c));
        assert_eq!(content_hash(b"foobar"), as_i64(0x85944171f73967e8));
        assert_ne!(content_hash(b"payload-1"), content_hash(b"payload-2"));
    }

    /// Dedupe on event identity, shared by both impls (s2w#25).
    #[expect(
        clippy::cognitive_complexity,
        reason = "one linear contract suite over both EventLog impls; splitting it would hide the shared sequence"
    )]
    fn run_dedupe_suite<L: EventLog>(mut log: L) -> TestResult {
        let source_a = source("source-a")?;
        let source_b = source("source-b")?;

        // The same (source, payload) appends once; the redelivery reports the original
        // position and leaves exactly one row behind.
        let first = event_with(source_a.clone(), cursor(1)?, b"payload-one".to_vec(), 1_000);
        let position_first = inserted(log.append(first.clone())?)?;
        assert_eq!(
            log.append(first.clone())?,
            AppendOutcome::Duplicate(position_first)
        );
        assert_eq!(log.replay(None)?.count(), 1);

        // A later event from the same source advances the cursor; a redelivery of the first
        // event after that must not regress it (the same-millisecond sibling case).
        let second = event_with(source_a.clone(), cursor(2)?, b"payload-two".to_vec(), 2_000);
        inserted(log.append(second.clone())?)?;
        assert_eq!(log.cursor(&source_a)?, Some(second.cursor.clone()));
        assert_eq!(
            log.append(first.clone())?,
            AppendOutcome::Duplicate(position_first)
        );
        assert_eq!(log.cursor(&source_a)?, Some(second.cursor.clone()));
        assert_eq!(log.replay(None)?.count(), 2);

        // Identical payload bytes from a different legitimate source identity are two events.
        let other_source = event_with(source_b.clone(), cursor(1)?, b"payload-one".to_vec(), 3_000);
        let position_other = inserted(log.append(other_source)?)?;
        assert_ne!(position_other, position_first);
        assert_eq!(log.replay(None)?.count(), 3);

        // Two different payloads that share cursor bytes are two events: dedupe is keyed on
        // payload identity, never on the cursor alone.
        let sibling_a = event_with(
            source_a.clone(),
            cursor(9)?,
            b"payload-three".to_vec(),
            4_000,
        );
        let sibling_b = event_with(
            source_a.clone(),
            cursor(9)?,
            b"payload-four".to_vec(),
            4_000,
        );
        assert!(matches!(
            log.append(sibling_a.clone())?,
            AppendOutcome::Inserted(_)
        ));
        assert!(matches!(
            log.append(sibling_b.clone())?,
            AppendOutcome::Inserted(_)
        ));
        assert_eq!(log.replay(None)?.count(), 5);
        assert_eq!(log.cursor(&source_a)?, Some(cursor(9)?));

        // The resume-boundary case (s2w#25): an inclusive timestamp seek redelivers the first
        // sibling after the second was stored. It collapses, and nothing moves.
        assert!(matches!(
            log.append(sibling_a)?,
            AppendOutcome::Duplicate(_)
        ));
        assert_eq!(log.replay(None)?.count(), 5);
        assert_eq!(log.cursor(&source_a)?, Some(cursor(9)?));
        Ok(())
    }

    #[test]
    fn in_memory_dedupes_on_source_and_payload_identity() -> TestResult {
        run_dedupe_suite(InMemoryEventLog::new())
    }

    #[test]
    fn sqlite_dedupes_on_source_and_payload_identity() -> TestResult {
        let directory = TestDirectory::new("dedupe")?;
        run_dedupe_suite(SqliteEventLog::open(directory.path())?)
    }

    #[test]
    fn sqlite_duplicate_leaves_the_stored_cursor_row_untouched() -> TestResult {
        let directory = TestDirectory::new("duplicate-cursor")?;
        let first = event(1)?;
        let second = event(3)?;
        let mut log = SqliteEventLog::open(directory.path())?;
        let position_first = inserted(log.append(first.clone())?)?;
        inserted(log.append(second.clone())?)?;

        let read_cursor_row = |log: &SqliteEventLog| -> Result<(Vec<u8>, i64), rusqlite::Error> {
            log.connection.query_row(
                "SELECT cursor, last_position FROM cursors WHERE source = ?1",
                [first.source.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        };
        let before = read_cursor_row(&log)?;
        assert_eq!(
            log.append(first.clone())?,
            AppendOutcome::Duplicate(position_first)
        );
        assert_eq!(read_cursor_row(&log)?, before);
        assert_eq!(log.cursor(&first.source)?, Some(second.cursor.clone()));
        Ok(())
    }

    #[test]
    fn in_memory_hash_collision_is_loud_and_stores_nothing() -> TestResult {
        let mut log = InMemoryEventLog::new();
        let stored = event(1)?;
        inserted(log.append(stored.clone())?)?;
        // Force a collision: keep the stored hash key but change the stored bytes, so the next
        // append of the original payload hits the key with different bytes behind it.
        match log.events.first_mut() {
            Some(first) => first.event.payload = b"different bytes".to_vec(),
            None => panic!("the log should hold the event just appended"),
        }
        match log.append(stored) {
            Err(LogError::Corrupt(message)) => assert!(message.contains("collision")),
            other => panic!("a hash collision must be Corrupt, got {other:?}"),
        }
        assert_eq!(log.replay(None)?.count(), 1);
        Ok(())
    }

    #[test]
    fn sqlite_hash_collision_is_loud_and_stores_nothing() -> TestResult {
        let directory = TestDirectory::new("collision")?;
        let incoming = event(1)?;
        let mut log = SqliteEventLog::open(directory.path())?;
        // A row whose hash is the incoming payload's but whose bytes differ: a collision.
        log.connection.execute(
            "INSERT INTO events (source, cursor, received_at, payload, content_hash)
             VALUES (?1, X'01', 1, X'01', ?2)",
            params![incoming.source.as_str(), content_hash(&incoming.payload)],
        )?;
        match log.append(incoming.clone()) {
            Err(LogError::Corrupt(message)) => assert!(message.contains("collision")),
            other => panic!("a hash collision must be Corrupt, got {other:?}"),
        }
        assert_eq!(log.replay(None)?.count(), 1);
        assert_eq!(log.cursor(&incoming.source)?, None);
        Ok(())
    }

    /// The batch seam: in-order outcomes, in-batch duplicates, cursors, all-or-nothing errors.
    fn run_batch_suite<L: EventLog>(mut log: L) -> TestResult {
        assert_eq!(log.append_batch(Vec::new())?, Vec::new());
        let outcomes =
            log.append_batch(vec![event(1)?, event(2)?, event(3)?, event(4)?, event(1)?])?;
        let positions = outcomes[..4]
            .iter()
            .map(|outcome| inserted(*outcome))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            positions.iter().map(|p| p.as_u64()).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(outcomes[4], AppendOutcome::Duplicate(positions[0]));
        assert_eq!(log.cursor(&source("source-a")?)?, Some(cursor(3)?));
        assert_eq!(log.cursor(&source("source-b")?)?, Some(cursor(4)?));

        // Same stored state as appending one by one.
        let mut sequential = InMemoryEventLog::new();
        for index in 1..=4 {
            sequential.append(event(index)?)?;
        }
        let batched = log.replay(None)?.collect::<Result<Vec<_>, _>>()?;
        let expected = sequential.replay(None)?.collect::<Result<Vec<_>, _>>()?;
        assert_eq!(batched, expected);

        // An oversized payload anywhere in the batch stores nothing and moves no cursor.
        let mut oversized = event(7)?;
        oversized.payload = vec![0; MAX_PAYLOAD_BYTES + 1];
        assert_eq!(
            log.append_batch(vec![event(5)?, oversized]),
            Err(LogError::TooLarge)
        );
        assert_eq!(log.replay(None)?.count(), 4);
        assert_eq!(log.cursor(&source("source-a")?)?, Some(cursor(3)?));
        Ok(())
    }

    #[test]
    fn in_memory_batch_append_conforms() -> TestResult {
        run_batch_suite(InMemoryEventLog::new())
    }

    #[test]
    fn sqlite_batch_append_conforms() -> TestResult {
        let directory = TestDirectory::new("batch")?;
        run_batch_suite(SqliteEventLog::open(directory.path())?)
    }

    #[test]
    fn sqlite_batch_in_batch_duplicate_keeps_cursor_on_last_insert() -> TestResult {
        let directory = TestDirectory::new("batch-dup")?;
        let mut log = SqliteEventLog::open(directory.path())?;
        let a = event(1)?;
        let b = event(3)?;
        let outcomes = log.append_batch(vec![a.clone(), a, b.clone()])?;
        let first = inserted(outcomes[0])?;
        assert_eq!(outcomes[1], AppendOutcome::Duplicate(first));
        let last = inserted(outcomes[2])?;
        let last_position: i64 = log.connection.query_row(
            "SELECT last_position FROM cursors WHERE source = ?1",
            [b.source.as_str()],
            |row| row.get(0),
        )?;
        assert_eq!(u64::try_from(last_position)?, last.as_u64());
        assert_eq!(log.cursor(&b.source)?, Some(b.cursor));
        Ok(())
    }

    #[test]
    fn sqlite_batch_with_a_collision_stores_nothing() -> TestResult {
        let directory = TestDirectory::new("batch-collision")?;
        let incoming = event(1)?;
        let mut log = SqliteEventLog::open(directory.path())?;
        log.connection.execute(
            "INSERT INTO events (source, cursor, received_at, payload, content_hash)
             VALUES (?1, X'01', 1, X'01', ?2)",
            params![incoming.source.as_str(), content_hash(&incoming.payload)],
        )?;
        match log.append_batch(vec![event(2)?, event(3)?, incoming]) {
            Err(LogError::Corrupt(message)) => assert!(message.contains("collision")),
            other => panic!("a hash collision must be Corrupt, got {other:?}"),
        }
        assert_eq!(log.replay(None)?.count(), 1);
        assert_eq!(log.cursor(&source("source-a")?)?, None);
        assert_eq!(log.cursor(&source("source-b")?)?, None);
        Ok(())
    }

    #[test]
    fn in_memory_batch_with_a_collision_stores_nothing() -> TestResult {
        let mut log = InMemoryEventLog::new();
        log.append(event(2)?)?;
        // Point event 3's identity at the stored event 2, whose bytes differ: a collision.
        let incoming = event(3)?;
        let stored = LogPosition(1);
        log.seen.insert(
            (incoming.source.clone(), content_hash(&incoming.payload)),
            stored,
        );
        match log.append_batch(vec![event(4)?, incoming]) {
            Err(LogError::Corrupt(message)) => assert!(message.contains("collision")),
            other => panic!("a hash collision must be Corrupt, got {other:?}"),
        }
        assert_eq!(log.replay(None)?.count(), 1);
        assert_eq!(log.cursor(&source("source-b")?)?, Some(cursor(2)?));
        Ok(())
    }

    #[test]
    fn sqlite_rejects_a_version_1_database_as_corrupt() -> TestResult {
        let directory = TestDirectory::new("version-1")?;
        let connection = Connection::open(directory.path().join(DATABASE_FILE))?;
        // The complete schema exactly as version 1 defined it, without `content_hash`: the only
        // shape a database written by the previous release can have. It must be refused loudly
        // rather than opened and then misbehaving on the first append.
        connection.execute_batch(
            "CREATE TABLE events (
                position    INTEGER PRIMARY KEY AUTOINCREMENT,
                source      TEXT    NOT NULL,
                cursor      BLOB    NOT NULL,
                received_at INTEGER NOT NULL,
                payload     BLOB    NOT NULL
            );
             CREATE TABLE cursors (
                source        TEXT PRIMARY KEY,
                cursor        BLOB    NOT NULL,
                last_position INTEGER NOT NULL REFERENCES events(position)
            );
             PRAGMA user_version = 1;",
        )?;
        drop(connection);
        match SqliteEventLog::open(directory.path()) {
            Err(LogError::Corrupt(message)) => {
                assert!(message.contains("unsupported schema version 1"));
            }
            Err(other) => panic!("a version-1 database should be Corrupt, got {other:?}"),
            Ok(_) => panic!("a version-1 database must not open as if it were current"),
        }
        Ok(())
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

        let log = retry_until_unlocked(|| SqliteEventLog::open(directory.path()))?;
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
                     (position, source, cursor, received_at, payload, content_hash)
                     VALUES (1, 'replacement', X'09', 9, X'09', ?1)",
                    params![content_hash(&original.payload)],
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
    fn both_logs_expose_the_same_content_hash() -> TestResult {
        let directory = TestDirectory::new("content-hash")?;
        let mut sqlite = SqliteEventLog::open(directory.path())?;
        let mut memory = InMemoryEventLog::new();
        let events = (1..=3).map(event).collect::<Result<Vec<_>, _>>()?;
        sqlite.append_batch(events.clone())?;
        memory.append_batch(events.clone())?;
        let from_sqlite = sqlite.replay(None)?.collect::<Result<Vec<_>, _>>()?;
        let from_memory = memory.replay(None)?.collect::<Result<Vec<_>, _>>()?;
        assert_eq!(from_sqlite, from_memory);
        for (stored, original) in from_sqlite.iter().zip(&events) {
            assert_eq!(stored.content_hash, content_hash(&original.payload));
        }
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
        assert!(retry_until_unlocked(|| SqliteEventLog::open(directory.path())).is_ok());
        Ok(())
    }

    #[test]
    fn read_only_sqlite_log_coexists_with_an_active_writer() -> TestResult {
        let directory = TestDirectory::new("read-only-coexistence")?;
        // Initialize the database before the reader races the append loop.
        drop(SqliteEventLog::open(directory.path())?);
        let path = directory.path().to_owned();
        let stopped = Arc::new(AtomicBool::new(false));
        let writer_stopped = Arc::clone(&stopped);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || -> Result<(), String> {
            let mut log = retry_until_unlocked(|| SqliteEventLog::open(&path))
                .map_err(|error| error.to_string())?;
            let mut index = 1_u8;
            log.append(event(index).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
            index = index.saturating_add(1);
            ready_tx.send(()).map_err(|error| error.to_string())?;
            while !writer_stopped.load(Ordering::Acquire) {
                log.append(event(index).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
                index = index.saturating_add(1);
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(())
        });
        ready_rx.recv()?;

        assert_eq!(
            SqliteEventLog::open(directory.path()).err(),
            Some(LogError::Locked)
        );
        let reader = ReadOnlySqliteEventLog::open(directory.path())?;
        let stored = reader.read_after(None)?.collect::<Result<Vec<_>, _>>()?;
        assert!(!stored.is_empty());
        for (expected, stored) in (1_u8..).zip(&stored) {
            assert_eq!(stored.event, event(expected)?);
        }

        stopped.store(true, Ordering::Release);
        writer.join().map_err(|_| "writer thread panicked")??;
        Ok(())
    }

    #[test]
    fn sqlite_uses_wal_and_full_synchronous_on_every_open() -> TestResult {
        let directory = TestDirectory::new("pragmas")?;
        for _ in 0..2 {
            let log = retry_until_unlocked(|| SqliteEventLog::open(directory.path()))?;
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
    #[expect(
        clippy::too_many_lines,
        reason = "crash-recovery test: spawn child, SIGKILL, reopen and assert form one scenario"
    )]
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

    #[test]
    fn try_lock_would_block_maps_to_locked() {
        let error = std::fs::TryLockError::WouldBlock;
        assert!(matches!(map_try_lock_error(error), LogError::Locked));
    }

    #[test]
    fn try_lock_other_error_maps_to_io() {
        let io_error = std::io::Error::other("disk gremlins");
        let error = std::fs::TryLockError::Error(io_error);
        match map_try_lock_error(error) {
            LogError::Io(message) => assert!(message.contains("disk gremlins")),
            other => panic!("expected LogError::Io, got {other:?}"),
        }
    }
}
