//! One-shot, read-only replay of the durable world served by `s2w mcp --log-dir`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use s2w_log::{
    LogError, LogPosition, LogReader, ReadOnlySqliteEventLog, ReadOnlySqliteVerdictStore,
    StoredEvent, StoredVerdict,
};
use s2w_system1::Verdict;

use crate::bridge::BridgeConfig;
use crate::query::{QueryError, QueryState, Timeline};

/// A failure to reconstruct the read-only world from its durable event and verdict stores.
#[derive(Debug, thiserror::Error)]
pub enum ReadOnlyWorldError {
    /// One of the two database files could not be opened.
    #[error(
        "cannot open the read-only world at {}: {source} (check --log-dir names a directory \
         s2w has written, or run s2w mcp without it for an empty world)",
        path.display()
    )]
    Open {
        /// The directory supplied to `--log-dir`.
        path: PathBuf,
        /// The underlying storage failure.
        #[source]
        source: LogError,
    },
    /// A later store read failed.
    #[error("reading the read-only world: {0}")]
    Log(#[from] LogError),
    /// The two durable stores disagree or contain an undecodable verdict.
    #[error("the read-only world is corrupt: {0}")]
    Corrupt(String),
    /// Appending a decoded claim to the query timeline failed.
    #[error("building the read-only query timeline: {0}")]
    Query(#[from] QueryError),
}

/// Reconstructs one immutable query snapshot from verdicts durably committed at open time.
///
/// No writer lock is acquired and no System 1 engine is run. The verdict cursor is captured
/// once and is the inclusive upper bound, so commits after startup cannot extend this snapshot.
///
/// # Errors
/// Returns an open/read error, or a loud corruption error when log and verdict rows disagree.
pub fn read_only_world(
    log_dir: &Path,
    world: impl Into<Arc<str>>,
    hub_cap: usize,
) -> Result<QueryState, ReadOnlyWorldError> {
    let reader = ReadOnlySqliteEventLog::open(log_dir).map_err(|source| open(log_dir, source))?;
    let verdicts =
        ReadOnlySqliteVerdictStore::open(log_dir).map_err(|source| open(log_dir, source))?;
    let snapshot_end = verdicts.cursor()?;
    let state = QueryState::new(Timeline::new(hub_cap as u64)).with_world(world);
    let Some(snapshot_end) = snapshot_end else {
        return Ok(state);
    };

    let batch_size = BridgeConfig::default().batch;
    let mut last = None;
    loop {
        let items = reader.read_after(last)?;
        let mut events = Vec::new();
        for item in items.take(batch_size) {
            let event = item?;
            if event.position > snapshot_end {
                break;
            }
            events.push(event);
        }
        if events.is_empty() {
            return Err(ReadOnlyWorldError::Corrupt(format!(
                "the verdict store's cursor is {}, but the event log ends at {}",
                snapshot_end.as_u64(),
                last.map_or(0, LogPosition::as_u64)
            )));
        }

        replay_batch(&state, &verdicts, last, &events)?;
        last = events.last().map(|event| event.position);
        if last == Some(snapshot_end) {
            return Ok(state);
        }
    }
}

fn open(path: &Path, source: LogError) -> ReadOnlyWorldError {
    match source {
        LogError::Locked => ReadOnlyWorldError::Corrupt(
            "a read-only SQLite open unexpectedly tried to acquire a writer lock".to_owned(),
        ),
        source => ReadOnlyWorldError::Open {
            path: path.to_owned(),
            source,
        },
    }
}

fn replay_batch(
    state: &QueryState,
    verdicts: &ReadOnlySqliteVerdictStore,
    after: Option<LogPosition>,
    events: &[StoredEvent],
) -> Result<(), ReadOnlyWorldError> {
    let Some(through) = events.last().map(|event| event.position) else {
        return Err(ReadOnlyWorldError::Corrupt(
            "an empty event batch reached read-only replay".to_owned(),
        ));
    };
    let stored = verdicts.read_range(after, through)?;
    let mut stored = stored.as_slice();
    for event in events {
        let here = stored.partition_point(|row| row.position <= event.position);
        let (at_event, rest) = stored.split_at(here);
        replay_event(state, event, at_event)?;
        stored = rest;
    }
    Ok(())
}

fn replay_event(
    state: &QueryState,
    event: &StoredEvent,
    rows: &[StoredVerdict],
) -> Result<(), ReadOnlyWorldError> {
    let at = event.position.as_u64();
    if let Some(row) = rows.iter().find(|row| row.position != event.position) {
        return Err(ReadOnlyWorldError::Corrupt(format!(
            "a stored verdict names log position {}, which holds no event",
            row.position.as_u64()
        )));
    }
    if rows.iter().any(|row| row.event_hash != event.content_hash) {
        return Err(ReadOnlyWorldError::Corrupt(format!(
            "stored verdicts at log position {at} judged a different event than the log holds"
        )));
    }

    let mut served_engines = HashSet::new();
    for row in rows {
        if !served_engines.insert(row.engine.as_str()) {
            continue;
        }
        append_verdict(state, event, row)?;
    }
    Ok(())
}

fn append_verdict(
    state: &QueryState,
    event: &StoredEvent,
    row: &StoredVerdict,
) -> Result<(), ReadOnlyWorldError> {
    let verdict: Verdict = serde_json::from_slice(&row.verdict).map_err(|error| {
        ReadOnlyWorldError::Corrupt(format!(
            "stored verdict of engine '{}' at log position {} does not decode: {error}",
            row.engine,
            event.position.as_u64()
        ))
    })?;
    if let Verdict::Propose { claims, .. } = verdict {
        for claim in claims {
            state.append(event.event.received_at, claim)?;
        }
    }
    Ok(())
}
