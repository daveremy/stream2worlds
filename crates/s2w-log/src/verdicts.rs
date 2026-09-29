//! The System 1 verdict store: every verdict an engine produced, keyed by the event it judged.
//!
//! A sibling of the event log, not a table in it: [`SqliteVerdictStore`] lives in its own
//! `verdicts.sqlite3` file in the log directory, with its own writer lock and `user_version`.
//! The event log's writer is whoever ingests; the verdict writer is the System 1 bridge, and
//! the two may become different processes (#10), each holding one writer lock.
//!
//! The store is append-only and holds opaque bytes: it never interprets a verdict, so this
//! crate does not depend on `s2w-system1`. The bridge serializes and decodes.
//!
//! The store also holds the bridge cursor, the highest log position some bridge consumed. It
//! is written in the same transaction as each batch's verdicts and never moves backwards. It
//! does NOT mean "every engine evaluated through here": a newly registered engine stores rows
//! below it.

use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    LogError, LogPosition, map_constraint, map_sqlite, open_sqlite_store,
    open_sqlite_store_read_only,
};

const DATABASE_FILE: &str = "verdicts.sqlite3";
const LOCK_FILE: &str = "VERDICTS_LOCK";
const SCHEMA_VERSION: i64 = 1;

/// One stored engine verdict for one log position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVerdict {
    /// The log position of the event the verdict judged.
    pub position: LogPosition,
    /// The judged event's [`crate::StoredEvent::content_hash`], so a verdict is bound to the
    /// exact event and a log swapped under the store is detectable.
    pub event_hash: i64,
    /// The engine's name.
    pub engine: String,
    /// The engine's version when it produced this verdict.
    pub version: u32,
    /// The encoded verdict, opaque to this crate.
    pub verdict: Vec<u8>,
    /// Optional encoded provenance (model hash, temperature, calibration), opaque to this crate.
    pub provenance: Option<Vec<u8>>,
}

/// The durable record of System 1 verdicts and the bridge cursor.
///
/// Rows are unique on `(position, engine, version)` and are never updated or deleted.
pub trait VerdictStore {
    /// The highest log position a bridge consumed, or `None` before the first batch.
    ///
    /// # Errors
    /// Returns an error if the cursor cannot be read.
    fn cursor(&self) -> Result<Option<LogPosition>, LogError>;

    /// Verdicts for positions in `(after, through]`, ordered by position and then write order.
    ///
    /// Within one position the first row for an engine is the one first written — the row the
    /// replay selection rule serves.
    ///
    /// # Errors
    /// Returns an error if the store cannot be read or a row is malformed.
    fn read_range(
        &self,
        after: Option<LogPosition>,
        through: LogPosition,
    ) -> Result<Vec<StoredVerdict>, LogError>;

    /// [`Self::read_range`] restricted to rows whose engine is one of `engines`, in the same
    /// order. Replay reads through this with the registered names only, so rows of engines no
    /// longer registered (a replaced mapping, decision 0023) are never read and discarded per
    /// position. The default filters [`Self::read_range`]'s rows; SQLite filters in the query.
    ///
    /// # Errors
    /// As [`Self::read_range`].
    fn read_range_of(
        &self,
        after: Option<LogPosition>,
        through: LogPosition,
        engines: &[String],
    ) -> Result<Vec<StoredVerdict>, LogError> {
        let mut rows = self.read_range(after, through)?;
        rows.retain(|row| engines.contains(&row.engine));
        Ok(rows)
    }

    /// Stores every row and advances the cursor to `max(cursor, through)`, all or nothing.
    ///
    /// The cursor never moves backwards, so replaying an already persisted prefix is safe.
    /// With no rows and `through` at or below the cursor this is a no-op: no transaction, no
    /// fsync. Rows may sit below the cursor (a newly registered engine on old positions).
    ///
    /// # Errors
    /// Returns [`LogError::Corrupt`] when a row repeats an existing or in-batch
    /// `(position, engine, version)` key or lies after `through`; nothing is stored then.
    /// Storage failures also store nothing.
    fn commit_batch(
        &mut self,
        rows: &[StoredVerdict],
        through: LogPosition,
    ) -> Result<(), LogError>;
}

/// Rejects a row after the batch's `through`: the cursor would then trail a stored verdict.
fn check_rows_through(rows: &[StoredVerdict], through: LogPosition) -> Result<(), LogError> {
    match rows.iter().find(|row| row.position > through) {
        Some(row) => Err(LogError::Corrupt(format!(
            "verdict at position {} lies after the batch end {}",
            row.position.as_u64(),
            through.as_u64()
        ))),
        None => Ok(()),
    }
}

fn duplicate_key(row: &StoredVerdict) -> LogError {
    LogError::Corrupt(format!(
        "duplicate verdict key: position {}, engine {}, version {}",
        row.position.as_u64(),
        row.engine,
        row.version
    ))
}

/// An append-only in-memory verdict store for composition and tests.
#[derive(Debug, Default)]
pub struct InMemoryVerdictStore {
    /// Rows in write order; the index is the seq.
    rows: Vec<StoredVerdict>,
    cursor: Option<LogPosition>,
    /// `(position, engine, version)` keys already stored, kept incrementally so `commit_batch`
    /// never re-scans `rows` (mirrors `InMemoryEventLog::seen`).
    keys: HashSet<(LogPosition, String, u32)>,
}

impl InMemoryVerdictStore {
    /// Constructs an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl VerdictStore for InMemoryVerdictStore {
    fn cursor(&self) -> Result<Option<LogPosition>, LogError> {
        Ok(self.cursor)
    }

    fn read_range(
        &self,
        after: Option<LogPosition>,
        through: LogPosition,
    ) -> Result<Vec<StoredVerdict>, LogError> {
        let mut selected: Vec<StoredVerdict> = self
            .rows
            .iter()
            .filter(|row| after.is_none_or(|after| row.position > after) && row.position <= through)
            .cloned()
            .collect();
        // Stable: rows at one position keep write order.
        selected.sort_by_key(|row| row.position);
        Ok(selected)
    }

    fn commit_batch(
        &mut self,
        rows: &[StoredVerdict],
        through: LogPosition,
    ) -> Result<(), LogError> {
        check_rows_through(rows, through)?;
        // Validate everything before mutating anything: all or nothing. `new_keys` catches a
        // duplicate WITHIN this batch; `self.keys` catches one against rows already committed.
        let mut new_keys: HashSet<(LogPosition, String, u32)> = HashSet::with_capacity(rows.len());
        for row in rows {
            let key = (row.position, row.engine.clone(), row.version);
            if self.keys.contains(&key) || !new_keys.insert(key) {
                return Err(duplicate_key(row));
            }
        }
        self.keys.extend(new_keys);
        self.rows.extend_from_slice(rows);
        self.cursor = self.cursor.max(Some(through));
        Ok(())
    }
}

/// A durable SQLite-backed verdict store in the log directory.
pub struct SqliteVerdictStore {
    connection: Connection,
    _lock: File,
}

/// A lockless, read-only handle to an initialized SQLite verdict store.
pub struct ReadOnlySqliteVerdictStore {
    connection: Connection,
}

impl fmt::Debug for ReadOnlySqliteVerdictStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadOnlySqliteVerdictStore")
            .finish_non_exhaustive()
    }
}

impl ReadOnlySqliteVerdictStore {
    /// Opens an existing verdict store without taking the writer lock or changing its schema.
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
                    "unsupported verdict store schema version {version}; expected {SCHEMA_VERSION}"
                )
            },
        )?;
        Ok(Self { connection })
    }

    /// The highest position durably consumed by the writer bridge.
    pub fn cursor(&self) -> Result<Option<LogPosition>, LogError> {
        cursor_from(&self.connection)
    }

    /// Verdict rows in `(after, through]`, ordered by position and write sequence.
    pub fn read_range(
        &self,
        after: Option<LogPosition>,
        through: LogPosition,
    ) -> Result<Vec<StoredVerdict>, LogError> {
        read_range_from(&self.connection, after, through, None)
    }
}

impl fmt::Debug for SqliteVerdictStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SqliteVerdictStore")
            .finish_non_exhaustive()
    }
}

impl SqliteVerdictStore {
    /// Opens or creates the verdict store in `directory`, next to the event log.
    ///
    /// Its own writer lock is acquired before SQLite is opened. WAL, full synchronous writes
    /// and recursive triggers are configured for every connection, as for the event log.
    ///
    /// # Errors
    /// Returns [`LogError::Locked`] when another handle owns the verdict lock, and
    /// [`LogError::Corrupt`] for an unknown schema version.
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, LogError> {
        let (connection, lock) = open_sqlite_store(
            directory.as_ref(),
            DATABASE_FILE,
            LOCK_FILE,
            "PRAGMA synchronous = FULL;
             PRAGMA recursive_triggers = ON;",
            SCHEMA_VERSION,
            |version| {
                format!(
                    "unsupported verdict store schema version {version}; expected {SCHEMA_VERSION}"
                )
            },
            initialize_schema,
        )?;
        Ok(Self {
            connection,
            _lock: lock,
        })
    }

    /// [`VerdictStore::commit_batch`] with a hook run inside the open transaction, after every
    /// write and before the commit. The crash test uses it to die mid-transaction.
    fn commit_batch_with(
        &mut self,
        rows: &[StoredVerdict],
        through: LogPosition,
        before_commit: impl FnOnce() -> Result<(), LogError>,
    ) -> Result<(), LogError> {
        check_rows_through(rows, through)?;
        if rows.is_empty() && self.cursor()?.is_some_and(|cursor| cursor >= through) {
            return Ok(());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        for row in rows {
            let inserted = transaction.execute(
                "INSERT INTO verdicts (position, engine, version, event_hash, verdict, provenance)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    row.position.to_sql()?,
                    row.engine,
                    row.version,
                    row.event_hash,
                    row.verdict,
                    row.provenance
                ],
            );
            inserted.map_err(|error| map_constraint(error, || duplicate_key(row)))?;
        }
        transaction
            .execute(
                "INSERT INTO bridge_cursor (id, position) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET position = max(position, excluded.position)",
                [through.to_sql()?],
            )
            .map_err(map_sqlite)?;
        before_commit()?;
        // Any error above returned early and dropped the transaction uncommitted.
        transaction.commit().map_err(map_sqlite)
    }
}

impl VerdictStore for SqliteVerdictStore {
    fn cursor(&self) -> Result<Option<LogPosition>, LogError> {
        cursor_from(&self.connection)
    }

    fn read_range(
        &self,
        after: Option<LogPosition>,
        through: LogPosition,
    ) -> Result<Vec<StoredVerdict>, LogError> {
        read_range_from(&self.connection, after, through, None)
    }

    fn read_range_of(
        &self,
        after: Option<LogPosition>,
        through: LogPosition,
        engines: &[String],
    ) -> Result<Vec<StoredVerdict>, LogError> {
        read_range_from(&self.connection, after, through, Some(engines))
    }

    fn commit_batch(
        &mut self,
        rows: &[StoredVerdict],
        through: LogPosition,
    ) -> Result<(), LogError> {
        self.commit_batch_with(rows, through, || Ok(()))
    }
}

fn cursor_from(connection: &Connection) -> Result<Option<LogPosition>, LogError> {
    let position: Option<i64> = connection
        .query_row(
            "SELECT position FROM bridge_cursor WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_sqlite)?;
    position.map(LogPosition::from_sql).transpose()
}

/// Rows in `(after, through]`; with `engines`, only rows of those engine names. The name list
/// travels as one JSON array parameter (`json_each`), so the statement text is fixed and
/// cached whatever the number of engines. `json_each` is SQLite's built-in JSON support
/// (always present since 3.38, and in the bundled build).
fn read_range_from(
    connection: &Connection,
    after: Option<LogPosition>,
    through: LogPosition,
    engines: Option<&[String]>,
) -> Result<Vec<StoredVerdict>, LogError> {
    let after = after.map_or(Ok(0), LogPosition::to_sql)?;
    let engines = engines
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| LogError::Io(format!("encoding engine names: {error}")))?;
    let mut statement = connection
        .prepare_cached(
            "SELECT position, event_hash, engine, version, verdict, provenance
                 FROM verdicts
                 WHERE position > ?1 AND position <= ?2
                   AND (?3 IS NULL OR engine IN (SELECT value FROM json_each(?3)))
                 ORDER BY position, seq",
        )
        .map_err(map_sqlite)?;
    let mut rows = statement
        .query(params![after, through.to_sql()?, engines])
        .map_err(map_sqlite)?;
    let mut verdicts = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        let version: i64 = row.get(3).map_err(map_sqlite)?;
        verdicts.push(StoredVerdict {
            position: LogPosition::from_sql(row.get(0).map_err(map_sqlite)?)?,
            event_hash: row.get(1).map_err(map_sqlite)?,
            engine: row.get(2).map_err(map_sqlite)?,
            version: u32::try_from(version)
                .map_err(|_| LogError::Corrupt(format!("verdict version {version}")))?,
            verdict: row.get(4).map_err(map_sqlite)?,
            provenance: row.get(5).map_err(map_sqlite)?,
        });
    }
    Ok(verdicts)
}

fn initialize_schema(connection: &mut Connection) -> Result<(), LogError> {
    let transaction = connection.transaction().map_err(map_sqlite)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS verdicts (
                seq        INTEGER PRIMARY KEY AUTOINCREMENT,
                position   INTEGER NOT NULL,
                engine     TEXT    NOT NULL,
                version    INTEGER NOT NULL,
                event_hash INTEGER NOT NULL,
                verdict    BLOB    NOT NULL,
                provenance BLOB,
                UNIQUE(position, engine, version)
            );
            CREATE TABLE IF NOT EXISTS bridge_cursor (
                id       INTEGER PRIMARY KEY CHECK (id = 1),
                position INTEGER NOT NULL
            );
            CREATE TRIGGER IF NOT EXISTS verdicts_no_update
            BEFORE UPDATE ON verdicts BEGIN
                SELECT RAISE(ABORT, 'verdicts are append-only: update refused');
            END;
            CREATE TRIGGER IF NOT EXISTS verdicts_no_delete
            BEFORE DELETE ON verdicts BEGIN
                SELECT RAISE(ABORT, 'verdicts are append-only: delete refused');
            END;
            CREATE TRIGGER IF NOT EXISTS bridge_cursor_monotonic
            BEFORE UPDATE ON bridge_cursor WHEN NEW.position < OLD.position BEGIN
                SELECT RAISE(ABORT, 'the bridge cursor never moves backwards');
            END;
            CREATE TRIGGER IF NOT EXISTS bridge_cursor_no_delete
            BEFORE DELETE ON bridge_cursor BEGIN
                SELECT RAISE(ABORT, 'the bridge cursor is never deleted');
            END;
            PRAGMA user_version = 1;",
        )
        .map_err(map_sqlite)?;
    transaction.commit().map_err(map_sqlite)
}

#[cfg(test)]
mod tests;
