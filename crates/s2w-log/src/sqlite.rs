//! The durable SQLite event log and its lockless read-only handle.

use std::collections::VecDeque;
use std::fmt;
use std::fs::File;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};

use crate::error::map_sqlite;
use crate::{
    AppendOutcome, DATABASE_FILE, EventLog, LOCK_FILE, LogError, LogPosition, LogReader,
    REPLAY_PAGE_SIZE, SCHEMA_VERSION, StoredEvent, check_payload_size, initialize_schema,
    insert_in, open_sqlite_store, open_sqlite_store_read_only,
};

/// A durable SQLite-backed event log.
pub struct SqliteEventLog {
    pub(crate) connection: Connection,
    _lock: File,
}

/// A lockless, read-only handle to an initialized SQLite event log.
pub struct ReadOnlySqliteEventLog {
    pub(crate) connection: Connection,
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

    fn read_head(&self) -> Result<Option<LogPosition>, LogError> {
        head_of(&self.connection)
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

    fn head(&self) -> Result<Option<LogPosition>, LogError> {
        head_of(&self.connection)
    }
}

/// The largest stored position: one `max(position)` query, `None` for an empty log.
fn head_of(connection: &Connection) -> Result<Option<LogPosition>, LogError> {
    let head: Option<i64> = connection
        .query_row("SELECT MAX(position) FROM events", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    head.map(LogPosition::from_sql).transpose()
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
