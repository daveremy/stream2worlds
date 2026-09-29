//! World snapshots (decision 0021): a derived, self-contained file holding a folded world at a
//! fold offset, the log position it was folded through, and what is needed to prove it still
//! matches the running fold, the engine routing and the log.
//!
//! A snapshot is never trusted: it is loaded only when every validity rule holds, and ignored
//! (never deleted) otherwise, which falls back to a full replay from offset 0. Not to be
//! confused with the golden `*.snapshot.json` fixture, which is the fold's expected output.
//!
//! This module is pure apart from [`store`]: hashing, the file format ([`codec`]) and the
//! validity rules do no I/O. The writer, its trigger and serve wiring land separately (#33).

pub mod codec;
pub mod store;

use s2w_core::{FOLD_FIXTURE_HASH, FOLD_VERSION, World};
use s2w_log::{LogPosition, LogReader};
use s2w_model::{Cursor, SourceId};
use serde::{Deserialize, Serialize};

use crate::query::BaseTime;

/// The payload format this code writes and reads. A different value is ignored, never migrated.
pub const SNAPSHOT_FORMAT: u32 = 1;

use s2w_model::Fnv64;
use s2w_model::fnv1a64;

/// Identifies the running fold for `hub_cap`: FNV-1a over [`FOLD_VERSION`], `hub_cap`,
/// [`SNAPSHOT_FORMAT`] and [`FOLD_FIXTURE_HASH`]. A snapshot written by any other fold is
/// ignored (validity rule 3).
#[must_use]
pub fn fold_hash(hub_cap: u64) -> u64 {
    Fnv64::new()
        .write(&FOLD_VERSION.to_le_bytes())
        .write(&hub_cap.to_le_bytes())
        .write(&SNAPSHOT_FORMAT.to_le_bytes())
        .write(&FOLD_FIXTURE_HASH.to_le_bytes())
        .finish()
}

/// FNV-1a over the world's postcard bytes. The world holds only ordered maps and sets and no
/// floats, so equal worlds always hash equal: the golden equivalence test compares these.
///
/// # Errors
/// [`SnapshotError::Encode`] if the world does not serialize (not expected for any world).
pub fn world_hash(world: &World) -> Result<u64, SnapshotError> {
    postcard::to_stdvec(world)
        .map(|bytes| fnv1a64(&bytes))
        .map_err(|e| SnapshotError::Encode(e.to_string()))
}

/// The snapshot payload, format 1. Field order is the wire order; `format` must stay first
/// so a reader can refuse another format before decoding the rest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotV1 {
    /// [`SNAPSHOT_FORMAT`] at write time.
    pub format: u32,
    /// [`fold_hash`] of the writing fold.
    pub fold_hash: u64,
    /// The writing registry's [`crate::bridge::EngineRegistry::feed_fingerprint`].
    pub feed_hash: u64,
    /// The world's hub in-degree cap.
    pub hub_cap: u64,
    /// The fold offset: equals `world.offset()`.
    pub offset: u64,
    /// The last log position the bridge had consumed when the world was captured.
    pub position: u64,
    /// The stored `content_hash` of the event at `position`, binding the snapshot to this log.
    pub position_event_hash: i64,
    /// Every source cursor at write time. Recorded for portability; never consulted at load.
    pub cursors: Vec<(SourceId, Cursor)>,
    /// The time index of the folded events.
    pub time: BaseTime,
    /// The folded world.
    pub world: World,
}

impl SnapshotV1 {
    /// This snapshot as a [`SnapshotRefV1`], which encodes to the same bytes.
    #[must_use]
    pub fn as_ref_v1(&self) -> SnapshotRefV1<'_> {
        SnapshotRefV1 {
            format: self.format,
            fold_hash: self.fold_hash,
            feed_hash: self.feed_hash,
            hub_cap: self.hub_cap,
            offset: self.offset,
            position: self.position,
            position_event_hash: self.position_event_hash,
            cursors: &self.cursors,
            time: self.time,
            world: &self.world,
        }
    }
}

/// [`SnapshotV1`] borrowing its cursors and world, so `serve` can encode the live head world
/// without cloning it (#179). Same fields in the same order: postcard writes a reference as the
/// value and a slice as a `Vec`, so both encode to identical bytes (pinned by a unit test).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SnapshotRefV1<'a> {
    /// See [`SnapshotV1::format`].
    pub format: u32,
    /// See [`SnapshotV1::fold_hash`].
    pub fold_hash: u64,
    /// See [`SnapshotV1::feed_hash`].
    pub feed_hash: u64,
    /// See [`SnapshotV1::hub_cap`].
    pub hub_cap: u64,
    /// See [`SnapshotV1::offset`].
    pub offset: u64,
    /// See [`SnapshotV1::position`].
    pub position: u64,
    /// See [`SnapshotV1::position_event_hash`].
    pub position_event_hash: i64,
    /// See [`SnapshotV1::cursors`].
    pub cursors: &'a [(SourceId, Cursor)],
    /// See [`SnapshotV1::time`].
    pub time: BaseTime,
    /// See [`SnapshotV1::world`].
    pub world: &'a World,
}

/// A snapshot could not be written.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// The payload did not serialize, or is too large for the format.
    #[error("snapshot encode: {0}")]
    Encode(String),
    /// The file or its directory could not be written.
    #[error("snapshot write: {0}")]
    Io(#[from] std::io::Error),
}

/// Why a snapshot file was ignored. Each is reported and falls back to an older snapshot or a
/// full replay; none deletes the file.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    /// Rule 1: the bytes are not a whole, intact snapshot file.
    #[error("not a snapshot file: {0}")]
    Corrupt(String),
    /// Rule 2: another payload format.
    #[error("snapshot format {found}, this build reads {SNAPSHOT_FORMAT}")]
    Format {
        /// The file's format.
        found: u32,
    },
    /// Rule 3: written by another fold (version, hub cap or golden fixtures differ).
    #[error("written by a different fold (fold hash {found:#x}, running {expected:#x})")]
    Fold {
        /// The file's fold hash.
        found: u64,
        /// The running fold's.
        expected: u64,
    },
    /// Rule 4: written under other engine routing.
    #[error("written under different engine routing (feed hash {found:#x}, running {expected:#x})")]
    Feed {
        /// The file's feed hash.
        found: u64,
        /// The running registry's.
        expected: u64,
    },
    /// Rule 5: the log no longer holds the event the snapshot was folded through.
    #[error("the log does not match: {0}")]
    Log(String),
    /// The file could not be read.
    #[error("unreadable: {0}")]
    Io(String),
}

/// What the running process expects of a snapshot (rules 3 and 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expected {
    /// The hub cap the process serves under.
    pub hub_cap: u64,
    /// The running registry's feed fingerprint.
    pub feed_hash: u64,
}

/// Validity rules 2 to 4, plus the payload's internal consistency. Pure.
///
/// # Errors
/// The first rule the snapshot breaks.
pub fn check_fold(snapshot: &SnapshotV1, expected: Expected) -> Result<(), Invalid> {
    if snapshot.format != SNAPSHOT_FORMAT {
        return Err(Invalid::Format {
            found: snapshot.format,
        });
    }
    let fold = fold_hash(expected.hub_cap);
    if snapshot.fold_hash != fold {
        return Err(Invalid::Fold {
            found: snapshot.fold_hash,
            expected: fold,
        });
    }
    if snapshot.feed_hash != expected.feed_hash {
        return Err(Invalid::Feed {
            found: snapshot.feed_hash,
            expected: expected.feed_hash,
        });
    }
    let world = &snapshot.world;
    if world.offset() != snapshot.offset
        || world.hub_in_degree_cap() != snapshot.hub_cap
        || snapshot.hub_cap != expected.hub_cap
        || world.fold_version() != FOLD_VERSION
    {
        return Err(Invalid::Corrupt(format!(
            "header says offset {} cap {}, world says offset {} cap {} fold {}",
            snapshot.offset,
            snapshot.hub_cap,
            world.offset(),
            world.hub_in_degree_cap(),
            world.fold_version()
        )));
    }
    Ok(())
}

/// Validity rule 5: the log still holds, at `snapshot.position`, the event the snapshot was
/// folded through, and the verdict store's bridge cursor has reached it.
///
/// `previous` is the position just before `snapshot.position` (`None` when it is the first):
/// the event is read with [`LogReader::read_after`]`(previous)`, whose first result must be
/// exactly `snapshot.position` with the recorded content hash. Reads one event.
///
/// # Errors
/// [`Invalid::Log`] on any mismatch or read failure.
pub fn check_log<R: LogReader + ?Sized>(
    snapshot: &SnapshotV1,
    reader: &R,
    previous: Option<LogPosition>,
    bridge_cursor: Option<LogPosition>,
) -> Result<(), Invalid> {
    let position = snapshot.position;
    if bridge_cursor.is_none_or(|cursor| cursor.as_u64() < position) {
        return Err(Invalid::Log(format!(
            "the verdict store's bridge cursor ({}) is behind the snapshot's position {position}",
            bridge_cursor.map_or_else(|| "none".to_owned(), |c| c.as_u64().to_string())
        )));
    }
    if previous.is_some_and(|p| p.as_u64() >= position) {
        return Err(Invalid::Log(format!(
            "read start {} is not before position {position}",
            previous.map_or(0, LogPosition::as_u64)
        )));
    }
    let first = reader
        .read_after(previous)
        .map_err(|e| Invalid::Log(e.to_string()))?
        .next();
    match first {
        None => Err(Invalid::Log(format!(
            "the log ends before position {position}"
        ))),
        Some(Err(e)) => Err(Invalid::Log(e.to_string())),
        Some(Ok(event)) if event.position.as_u64() != position => Err(Invalid::Log(format!(
            "no event at position {position} (next is {})",
            event.position.as_u64()
        ))),
        Some(Ok(event)) if event.content_hash != snapshot.position_event_hash => Err(Invalid::Log(
            format!("the event at position {position} is not the one the snapshot folded"),
        )),
        Some(Ok(_)) => Ok(()),
    }
}

#[cfg(test)]
mod tests;
