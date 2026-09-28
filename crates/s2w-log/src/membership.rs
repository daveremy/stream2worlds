//! Append-only source membership, paired with the event head under the writer transaction.
use crate::{AppendOutcome, LogError, SqliteEventLog, check_payload_size, insert_in, map_sqlite};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use s2w_model::{Cursor, RawEvent, SourceId};

/// Where a newly added source starts reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectiveFrom {
    /// First-ever membership, without a stored cursor. Re-adds require an explicit cursor.
    Now,
    /// Resume at the supplied source cursor.
    FromCursor(Cursor),
}

/// One immutable membership transition. `None` means removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MembershipRow {
    /// Total write order, also the source's membership generation.
    pub seq: i64,
    /// Source identity used by ingestion and dedupe.
    pub source: SourceId,
    /// Start cursor for an addition, or `None` for removal.
    pub effective_from: Option<EffectiveFrom>,
    /// Event-log head observed inside the write transaction.
    pub world_offset: u64,
}

pub(crate) fn initialize(connection: &Connection) -> Result<(), LogError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS membership (
        seq INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('added','removed')), effective_from BLOB,
        world_offset INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS membership_source_seq ON membership(source, seq);
        CREATE TABLE IF NOT EXISTS world_manifest (
            world TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS world_manifest_engines (world TEXT NOT NULL, engine TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS world_manifest_policies (world TEXT NOT NULL, policy TEXT NOT NULL);") .map_err(map_sqlite)?;
    for table in [
        "membership",
        "world_manifest",
        "world_manifest_engines",
        "world_manifest_policies",
    ] {
        for operation in ["UPDATE", "DELETE"] {
            connection
                .execute_batch(&format!(
                    "CREATE TRIGGER IF NOT EXISTS {table}_no_{operation}
                BEFORE {operation} ON {table} BEGIN
                SELECT RAISE(ABORT, '{table} is append-only'); END;"
                ))
                .map_err(map_sqlite)?;
        }
    }
    Ok(())
}

pub(crate) fn migrate_v2_to_v3(connection: &mut Connection) -> Result<(), LogError> {
    let tx = connection.transaction().map_err(map_sqlite)?;
    tx.execute_batch("CREATE TABLE cursors_new (
        source TEXT PRIMARY KEY, cursor BLOB NOT NULL, last_position INTEGER REFERENCES events(position));
        INSERT INTO cursors_new SELECT source, cursor, last_position FROM cursors;
        DROP TABLE cursors;
        ALTER TABLE cursors_new RENAME TO cursors;").map_err(map_sqlite)?;
    initialize(&tx)?;
    tx.execute_batch("PRAGMA user_version = 3;")
        .map_err(map_sqlite)?;
    tx.commit().map_err(map_sqlite)
}

pub(crate) fn source_state(
    connection: &Connection,
    source: &SourceId,
) -> Result<(bool, i64), LogError> {
    connection.query_row("SELECT kind = 'added', seq FROM membership WHERE source = ?1 ORDER BY seq DESC LIMIT 1",
        [source.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional().map(|state| state.unwrap_or((true, 0))).map_err(map_sqlite)
}

impl SqliteEventLog {
    /// Reads membership and generation together. No history is a safe default for legacy callers;
    /// serving sources are bootstrapped before the first poll.
    pub fn source_membership(&self, source: &SourceId) -> Result<(bool, i64), LogError> {
        source_state(&self.connection, source)
    }
    /// Whether the latest row permits ingestion.
    pub fn is_source_member(&self, source: &SourceId) -> Result<bool, LogError> {
        Ok(self.source_membership(source)?.0)
    }
    /// Latest sequence number, or zero before bootstrap.
    pub fn membership_generation(&self, source: &SourceId) -> Result<i64, LogError> {
        Ok(self.source_membership(source)?.1)
    }
    /// Adds a source and resolves its cursor atomically. `Now` on any re-add is an error.
    pub fn record_source_added(
        &mut self,
        source: &SourceId,
        from: EffectiveFrom,
    ) -> Result<(), LogError> {
        self.record_membership_row(source, Some(from), false)
    }
    /// Stops future ingestion without changing the source's durable cursor.
    pub fn record_source_removed(&mut self, source: &SourceId) -> Result<(), LogError> {
        self.record_membership_row(source, None, false)
    }
    /// Writes exactly one initial Added row, preserving an existing cursor and its position.
    pub fn bootstrap_source(&mut self, source: &SourceId) -> Result<(), LogError> {
        self.record_membership_row(source, Some(EffectiveFrom::Now), true)
    }
    /// The future admin mutation path (add/remove against a live `serve` process, filed as a
    /// follow-up issue) MUST call this function rather than inserting into `membership`
    /// directly — it is the only place that pairs a membership row with a correct-at-commit-time
    /// offset. Bootstrap reads but never changes an existing cursor.
    fn record_membership_row(
        &mut self,
        source: &SourceId,
        mut from: Option<EffectiveFrom>,
        bootstrap: bool,
    ) -> Result<(), LogError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        let exists = source_state(&tx, source)?.1 != 0;
        if bootstrap {
            if exists {
                return Ok(());
            }
            let cursor: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT cursor FROM cursors WHERE source=?1",
                    [source.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(map_sqlite)?;
            from = Some(match cursor {
                Some(bytes) => {
                    EffectiveFrom::FromCursor(Cursor::new(bytes).map_err(LogError::InvalidCursor)?)
                }
                None => EffectiveFrom::Now,
            });
        } else if exists && from == Some(EffectiveFrom::Now) {
            return Err(LogError::ReaddRequiresCursor);
        }
        let head: i64 = tx
            .query_row("SELECT COALESCE(MAX(position),0) FROM events", [], |row| {
                row.get(0)
            })
            .map_err(map_sqlite)?;
        let bytes = match &from {
            Some(EffectiveFrom::FromCursor(cursor)) => Some(cursor.as_bytes()),
            _ => None,
        };
        tx.execute("INSERT INTO membership(source, kind, effective_from, world_offset) VALUES (?1,?2,?3,?4)",
            params![source.as_str(), if from.is_some() {"added"} else {"removed"}, bytes, head]).map_err(map_sqlite)?;
        if !bootstrap {
            match &from {
                Some(EffectiveFrom::FromCursor(cursor)) => {
                    tx.execute(
                        "INSERT INTO cursors(source,cursor,last_position) VALUES (?1,?2,NULL)
                    ON CONFLICT(source) DO UPDATE SET cursor=excluded.cursor,last_position=NULL",
                        params![source.as_str(), cursor.as_bytes()],
                    )
                    .map_err(map_sqlite)?;
                }
                Some(EffectiveFrom::Now) => {
                    tx.execute("DELETE FROM cursors WHERE source=?1", [source.as_str()])
                        .map_err(map_sqlite)?;
                }
                None => {}
            }
        }
        tx.commit().map_err(map_sqlite)
    }
    /// Full immutable history in write order, suitable for a serving snapshot.
    pub fn membership_history(&self) -> Result<Vec<MembershipRow>, LogError> {
        let mut stmt = self
            .connection
            .prepare(
                "SELECT seq,source,kind,effective_from,world_offset FROM membership ORDER BY seq",
            )
            .map_err(map_sqlite)?;
        let mut rows = stmt.query([]).map_err(map_sqlite)?;
        let mut history = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite)? {
            let source: String = row.get(1).map_err(map_sqlite)?;
            let kind: String = row.get(2).map_err(map_sqlite)?;
            let bytes: Option<Vec<u8>> = row.get(3).map_err(map_sqlite)?;
            history.push(MembershipRow {
                seq: row.get(0).map_err(map_sqlite)?,
                source: SourceId::new(source).map_err(|e| LogError::Corrupt(e.to_string()))?,
                effective_from: if kind == "removed" {
                    None
                } else {
                    Some(match bytes {
                        Some(bytes) => EffectiveFrom::FromCursor(
                            Cursor::new(bytes).map_err(LogError::InvalidCursor)?,
                        ),
                        None => EffectiveFrom::Now,
                    })
                },
                world_offset: crate::LogPosition::from_sql(row.get(4).map_err(map_sqlite)?)?
                    .as_u64(),
            });
        }
        Ok(history)
    }
    /// Members at `at`, folding equal-offset rows by their sequence number.
    pub fn membership_at(&self, at: u64) -> Result<Vec<SourceId>, LogError> {
        Ok(members_at(&self.membership_history()?, at))
    }
    /// Atomically appends a batch only if its fetch generation still holds. A stale batch
    /// returns one `StaleGeneration` per input event and never advances a cursor.
    pub fn append_batch_with_generation(
        &mut self,
        events: Vec<RawEvent>,
        source: &SourceId,
        fetch_generation: i64,
    ) -> Result<Vec<AppendOutcome>, LogError> {
        self.append_batch_with_generations(events, &[(source.clone(), fetch_generation)])
    }
    /// Multi-partition equivalent: every captured source generation must still hold.
    pub fn append_batch_with_generations(
        &mut self,
        events: Vec<RawEvent>,
        generations: &[(SourceId, i64)],
    ) -> Result<Vec<AppendOutcome>, LogError> {
        for event in &events {
            check_payload_size(event.payload.len())?;
        }
        if events.is_empty() {
            return Ok(Vec::new());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        for (source, generation) in generations {
            if source_state(&tx, source)?.1 != *generation {
                return Ok(vec![AppendOutcome::StaleGeneration; events.len()]);
            }
        }
        let outcomes = events
            .iter()
            .map(|event| insert_in(&tx, event))
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit().map_err(map_sqlite)?;
        Ok(outcomes)
    }
}

/// Folds a serving snapshot, with the same ordering as the storage query.
#[must_use]
pub fn members_at(history: &[MembershipRow], at: u64) -> Vec<SourceId> {
    let mut latest = std::collections::BTreeMap::new();
    for row in history.iter().filter(|row| row.world_offset <= at) {
        let entry = latest.entry(row.source.clone()).or_insert(row);
        if row.seq > entry.seq {
            *entry = row;
        }
    }
    latest
        .into_iter()
        .filter_map(|(source, row)| row.effective_from.is_some().then_some(source))
        .collect()
}

#[cfg(test)]
mod tests;
