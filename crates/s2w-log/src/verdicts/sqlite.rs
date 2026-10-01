//! The durable SQLite verdict store and its lockless read-only handle.

use std::fmt;
use std::fs::File;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{StoredVerdict, VerdictStore, check_rows_through, duplicate_key};
use crate::{
    LogError, LogPosition, map_constraint, map_sqlite, open_sqlite_store,
    open_sqlite_store_read_only,
};

pub(super) const DATABASE_FILE: &str = "verdicts.sqlite3";
const LOCK_FILE: &str = "VERDICTS_LOCK";
pub(super) const SCHEMA_VERSION: i64 = 1;

/// A durable SQLite-backed verdict store in the log directory.
pub struct SqliteVerdictStore {
    pub(super) connection: Connection,
    _lock: File,
}

/// A lockless, read-only handle to an initialized SQLite verdict store.
pub struct ReadOnlySqliteVerdictStore {
    pub(super) connection: Connection,
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
    pub(super) fn commit_batch_with(
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
