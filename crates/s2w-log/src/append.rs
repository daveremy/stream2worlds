//! Duplicate and collision classification shared by the SQLite and in-memory logs.

use rusqlite::params;

use crate::error::map_sqlite;
use crate::{AppendOutcome, LogError, LogPosition};

/// The loud error for a 64-bit content-hash collision between two distinct payloads.
///
/// Names the source, hash and stored position so a human can inspect the two events: the same
/// event is redelivered on every restart, so this error repeats until someone acts on it.
pub(crate) fn collision(source: &str, hash: i64, position: LogPosition) -> LogError {
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
pub(crate) fn resolve_duplicate(
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
