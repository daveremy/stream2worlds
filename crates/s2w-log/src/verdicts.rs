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

use crate::{LogError, LogPosition};

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

mod sqlite;

#[cfg(test)]
use sqlite::{DATABASE_FILE, SCHEMA_VERSION};
pub use sqlite::{ReadOnlySqliteVerdictStore, SqliteVerdictStore};

#[cfg(test)]
mod tests;
