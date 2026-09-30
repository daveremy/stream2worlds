//! One-shot and live, read-only replay of the durable world served by `s2w mcp --log-dir`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use s2w_log::{
    LogError, LogPosition, LogReader, ReadOnlySqliteEventLog, ReadOnlySqliteVerdictStore,
    StoredEvent, StoredVerdict, WorldManifest,
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

impl ReadOnlyWorldError {
    /// True only for a transient I/O failure (e.g. `SQLITE_BUSY`/locked, see
    /// `s2w_log::map_sqlite`) — the sole class the live refresh loop retries. Every other
    /// variant, including `Corrupt`, `Open`, and a `Query` failure, is treated as structural
    /// and stops the refresh loop rather than retrying forever on a deterministic error
    /// (karpathy ruling, stream2worlds#128).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Log(LogError::Io(_)))
    }
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
    hub_cap: u64,
) -> Result<QueryState, ReadOnlyWorldError> {
    Ok(LiveReadOnlyWorld::open(log_dir, world, hub_cap)?.0)
}

/// A read-only world that can be advanced incrementally as new verdicts are committed by a
/// concurrently-running `s2w serve`/`s2w watch` process (stream2worlds#128).
///
/// Holds the same two read-only connections `read_only_world` opens once; never takes a writer
/// lock and never reopens the stores.
pub struct LiveReadOnlyWorld {
    reader: ReadOnlySqliteEventLog,
    verdicts: ReadOnlySqliteVerdictStore,
    /// The highest log position already folded into the paired `QueryState`.
    last: Option<LogPosition>,
}

impl LiveReadOnlyWorld {
    /// Opens both read-only stores and folds every verdict committed up to the cursor at open
    /// time, mirroring [`read_only_world`]'s snapshot but returning the handle needed to poll
    /// for more later.
    ///
    /// # Errors
    /// Returns an open/read error, or a loud corruption error when log and verdict rows
    /// disagree.
    pub fn open(
        log_dir: &Path,
        world: impl Into<Arc<str>>,
        hub_cap: u64,
    ) -> Result<(QueryState, Self), ReadOnlyWorldError> {
        let world: Arc<str> = world.into();
        let reader =
            ReadOnlySqliteEventLog::open(log_dir).map_err(|source| open(log_dir, source))?;
        let verdicts =
            ReadOnlySqliteVerdictStore::open(log_dir).map_err(|source| open(log_dir, source))?;
        let manifest: Option<WorldManifest> = reader.world_manifest(&world)?;
        let membership = reader.membership_history()?;
        let state = QueryState::new(Timeline::new(hub_cap))
            .with_world(Arc::clone(&world))
            .with_metadata(manifest, membership)
            .with_log_dir(log_dir);
        let mut last = None;
        if let Some(snapshot_end) = verdicts.cursor()? {
            catch_up(
                Replay {
                    state: &state,
                    reader: &reader,
                    verdicts: &verdicts,
                },
                &mut last,
                snapshot_end,
                None,
            )?;
        }
        Ok((
            state,
            Self {
                reader,
                verdicts,
                last,
            },
        ))
    }

    /// Polls the verdict cursor and folds any newly-committed verdicts into `state`.
    ///
    /// Returns `Ok(true)` if anything new was folded in, `Ok(false)` if the cursor was
    /// unchanged (the common case on a 500ms poll with no writer activity — one cheap cursor
    /// read, no further query).
    ///
    /// # Errors
    /// Returns [`ReadOnlyWorldError::Corrupt`] if the cursor moved backward or reset from
    /// `Some` to `None` (the bridge cursor is documented monotonic per-database — either shape
    /// is a loud signal something is wrong, never silently absorbed), or the same read/corrupt
    /// errors `catch_up` can return. The caller decides whether an error is retryable via
    /// [`ReadOnlyWorldError::is_retryable`].
    pub fn refresh(&mut self, state: &QueryState) -> Result<bool, ReadOnlyWorldError> {
        self.refresh_checking_stop(state, None)
    }

    /// Same as [`Self::refresh`], but checks `stop` between batches inside a large catch-up so
    /// a shutdown doesn't have to wait for the whole backlog to replay.
    pub(crate) fn refresh_checking_stop(
        &mut self,
        state: &QueryState,
        stop: Option<&AtomicBool>,
    ) -> Result<bool, ReadOnlyWorldError> {
        let new_cursor = self.verdicts.cursor()?;
        match classify_cursor_update(self.last, new_cursor) {
            CursorUpdate::Unchanged => Ok(false),
            CursorUpdate::Regressed(message) => Err(ReadOnlyWorldError::Corrupt(message)),
            CursorUpdate::Advanced(new) => {
                catch_up(
                    Replay {
                        state,
                        reader: &self.reader,
                        verdicts: &self.verdicts,
                    },
                    &mut self.last,
                    new,
                    stop,
                )?;
                Ok(true)
            }
        }
    }
}

/// What a freshly-read verdict cursor means relative to the cursor already folded into
/// `QueryState` — pulled out of [`LiveReadOnlyWorld::refresh_checking_stop`] as a pure function
/// so the cursor-regression guard (backward move, or `Some` resetting to `None`) is testable
/// without a real SQLite store (stream2worlds#128 round-2 review, both reviewers).
enum CursorUpdate {
    /// Nothing new — the common case on a poll with no writer activity.
    Unchanged,
    /// The bridge cursor is documented monotonic per-database (a DB trigger refuses lowering
    /// it), so either shape here is a loud signal something is wrong, never silently absorbed.
    Regressed(String),
    /// The cursor advanced normally; catch up to it.
    Advanced(LogPosition),
}

fn classify_cursor_update(previous: Option<LogPosition>, new: Option<LogPosition>) -> CursorUpdate {
    match (previous, new) {
        (previous, new) if previous == new => CursorUpdate::Unchanged,
        (Some(_), None) => {
            CursorUpdate::Regressed("the verdict store's cursor reset from Some to None".to_owned())
        }
        (Some(previous), Some(new)) if new < previous => CursorUpdate::Regressed(format!(
            "the verdict store's cursor moved backward from {} to {}",
            previous.as_u64(),
            new.as_u64()
        )),
        (_, Some(new)) => CursorUpdate::Advanced(new),
        (None, None) => CursorUpdate::Unchanged,
    }
}

/// The fixed inputs of every [`catch_up`] call: the state it folds into and the read-only log
/// and verdict store it reads from.
#[derive(Clone, Copy)]
struct Replay<'a> {
    state: &'a QueryState,
    reader: &'a ReadOnlySqliteEventLog,
    verdicts: &'a ReadOnlySqliteVerdictStore,
}

/// Folds every verdict in `(*from, through]` into `replay.state`, batch by batch, advancing `*from`
/// after each batch fully resolves (never partially, per stream2worlds#128's round-2 review:
/// each batch's fallible I/O — the event read and the verdict range read — is collected in
/// full before any claim is appended to `state`, so a read failure mid-batch never leaves a
/// partial append behind to duplicate on retry).
///
/// `batch_end` is always the position of the last event actually read in the batch (never a
/// synthetic `through` value — the verdict cursor always names a real event position, so the
/// final batch's last event position and `through` coincide once caught up). A batch that reads
/// zero events is `Corrupt`, not a silent no-op: the verdict cursor named a position the event
/// log has nothing at, which cannot happen on a healthy log and would otherwise spin the loop
/// rerunning an empty catch-up forever.
///
/// If `stop` is set and observed true between batches, returns early leaving `*from` at
/// whatever batch was last fully applied — safe to resume from on the next call.
fn catch_up(
    replay: Replay<'_>,
    from: &mut Option<LogPosition>,
    through: LogPosition,
    stop: Option<&AtomicBool>,
) -> Result<(), ReadOnlyWorldError> {
    let Replay {
        state,
        reader,
        verdicts,
    } = replay;
    let batch_size = BridgeConfig::default().batch;
    while *from != Some(through) {
        if stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
            return Ok(());
        }

        // Collect this batch's fallible I/O in full — the event reads and the verdict range
        // read — before appending anything to `state`. A read failure here (e.g. a transient
        // `Io`) leaves `*from` exactly where it was, with nothing appended for this batch, so a
        // retry cleanly redoes the same batch instead of duplicating a partial one.
        let items = reader.read_after(*from)?;
        let mut events = Vec::new();
        for item in items.take(batch_size) {
            let event: StoredEvent = item?;
            if event.position > through {
                break;
            }
            events.push(event);
        }
        if events.is_empty() {
            return Err(ReadOnlyWorldError::Corrupt(format!(
                "the verdict store's cursor is {}, but the event log has no more events after {}",
                through.as_u64(),
                from.map_or(0, LogPosition::as_u64)
            )));
        }
        let batch_end = events
            .last()
            .map_or(through, |event: &StoredEvent| event.position);
        let stored = verdicts.read_range(*from, batch_end)?;

        // Everything fallible above already happened; the append step itself cannot fail on
        // I/O, only on a structural decode/corruption error — never retried regardless.
        replay_batch(state, &events, &stored)?;
        // `batch_end` equals `through` exactly once this batch reaches the last event at or
        // before `through` (the common "caught up" case), matching `refresh`'s
        // `new_cursor == *from` cursor-value comparison — both name the same real event position.
        *from = Some(batch_end);
    }
    Ok(())
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
    events: &[StoredEvent],
    stored: &[StoredVerdict],
) -> Result<(), ReadOnlyWorldError> {
    let mut stored = stored;
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use s2w_core::{AttrValue, NaturalKey, WorldEvent};
    use s2w_log::{
        AppendOutcome, EffectiveFrom, EventLog, LogError, SqliteEventLog, SqliteVerdictStore,
        VerdictStore, WorldManifest,
    };
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
    use s2w_system1::{Confidence, Verdict};
    use tower::ServiceExt;

    use super::{CursorUpdate, LiveReadOnlyWorld, ReadOnlyWorldError, classify_cursor_update};
    use crate::query::{QueryState, router};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn world_json(state: &QueryState) -> String {
        serde_json::to_string(&state.world_at(None).unwrap()).unwrap()
    }

    async fn get(app: &Router, path: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            loop {
                let sequence = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path = env::temp_dir().join(format!(
                    "s2w-app-replay-{label}-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("creating test directory: {error}"),
                }
            }
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn raw(index: u8) -> RawEvent {
        RawEvent {
            source: SourceId::new("test.replay").unwrap(),
            cursor: Cursor::new(vec![index]).unwrap(),
            received_at: Timestamp::from_millis(1_000 + i64::from(index)),
            payload: vec![index],
        }
    }

    fn claim(key: &str) -> WorldEvent {
        WorldEvent::EntityObserved {
            key: NaturalKey::new(key),
            entity_type: "fixture".to_owned(),
            attrs: BTreeMap::from([("seen".to_owned(), AttrValue::Str("yes".to_owned()))]),
        }
    }

    /// Commits one event plus a matching CERTAIN verdict for it, mirroring the shape
    /// `s2w_bridge` would write: append the event, replay it back to get its content hash,
    /// commit a verdict batch of one row at that position.
    fn commit(
        log: &mut SqliteEventLog,
        verdicts: &mut SqliteVerdictStore,
        index: u8,
        key: &str,
    ) -> s2w_log::LogPosition {
        let position = match log.append(raw(index)).unwrap() {
            AppendOutcome::Inserted(position) => position,
            other => panic!("unexpected append outcome: {other:?}"),
        };
        let stored = log
            .replay(None)
            .unwrap()
            .filter_map(Result::ok)
            .find(|event| event.position == position)
            .expect("the just-inserted event replays back");
        let encoded = serde_json::to_vec(&Verdict::Propose {
            claims: vec![claim(key)],
            confidence: Confidence::CERTAIN,
        })
        .unwrap();
        verdicts
            .commit_batch(
                &[s2w_log::StoredVerdict {
                    position,
                    event_hash: stored.content_hash,
                    engine: "fixture".to_owned(),
                    version: 1,
                    verdict: encoded,
                    provenance: None,
                }],
                position,
            )
            .unwrap();
        position
    }

    #[test]
    fn read_only_world_serves_manifest_name_and_membership_from_the_writer() {
        crate::tests::run(false, async {
            let directory = TestDirectory::new("metadata");
            let mut log = SqliteEventLog::open(directory.path()).unwrap();
            let mut verdicts = SqliteVerdictStore::open(directory.path()).unwrap();
            WorldManifest::create_if_absent(
                &mut log,
                "default",
                "The display name",
                Timestamp::from_millis(900),
                &[],
                &[],
            )
            .unwrap();
            log.bootstrap_source(&SourceId::new("test.replay").unwrap())
                .unwrap();
            let head = commit(&mut log, &mut verdicts, 1, "metadata-at-open");

            let (state, _live) =
                LiveReadOnlyWorld::open(directory.path(), "default", 10_000).unwrap();
            let app = router(state);

            assert_eq!(
                get(&app, "/worlds").await,
                (
                    StatusCode::OK,
                    serde_json::json!({
                        "worlds": [{
                            "world": "default",
                            "name": "The display name",
                            "head": head.as_u64(),
                            "title": null,
                            "tagline": null
                        }]
                    })
                )
            );
            assert_eq!(
                get(
                    &app,
                    &format!("/worlds/default/sources?at={}", head.as_u64())
                )
                .await,
                // The read-only replay path runs no bridge, so the member reports zeros.
                (
                    StatusCode::OK,
                    serde_json::json!([{
                        "source": "test.replay",
                        "consumed": 0,
                        "unrouted": 0,
                        "recent_unrouted": []
                    }])
                )
            );
        });
    }

    /// `SqliteEventLog::open` eagerly creates the manifest and membership tables, so a legacy
    /// directory that never wrote either kind of metadata safely reads them as empty.
    #[test]
    fn read_only_world_has_no_manifest_when_none_was_ever_created() {
        crate::tests::run(false, async {
            let directory = TestDirectory::new("metadata-legacy");
            let mut log = SqliteEventLog::open(directory.path()).unwrap();
            let mut verdicts = SqliteVerdictStore::open(directory.path()).unwrap();
            let head = commit(&mut log, &mut verdicts, 1, "legacy-at-open");

            let (state, _live) =
                LiveReadOnlyWorld::open(directory.path(), "legacy", 10_000).unwrap();
            let app = router(state);

            assert_eq!(
                get(&app, "/worlds").await,
                (
                    StatusCode::OK,
                    serde_json::json!({
                        "worlds": [{
                            "world": "legacy",
                            "name": "legacy",
                            "head": head.as_u64(),
                            "title": null,
                            "tagline": null
                        }]
                    })
                )
            );
            assert_eq!(
                get(
                    &app,
                    &format!("/worlds/legacy/sources?at={}", head.as_u64())
                )
                .await,
                (StatusCode::OK, serde_json::json!([]))
            );
        });
    }

    #[test]
    fn read_only_metadata_is_frozen_at_open_even_as_a_writer_commits_a_new_membership_row_and_refresh_runs()
     {
        crate::tests::run(false, async {
            let directory = TestDirectory::new("metadata-frozen");
            let mut log = SqliteEventLog::open(directory.path()).unwrap();
            let mut verdicts = SqliteVerdictStore::open(directory.path()).unwrap();
            let event_source = SourceId::new("test.replay").unwrap();
            let initial_peer = SourceId::new("initial.peer").unwrap();
            log.bootstrap_source(&event_source).unwrap();
            log.bootstrap_source(&initial_peer).unwrap();
            commit(&mut log, &mut verdicts, 1, "seen-at-open");

            let (state, mut live) =
                LiveReadOnlyWorld::open(directory.path(), "default", 10_000).unwrap();

            let added_after_open = SourceId::new("third.added-after-open").unwrap();
            log.record_source_added(&added_after_open, EffectiveFrom::Now)
                .unwrap();
            let new_head = commit(&mut log, &mut verdicts, 2, "seen-after-refresh");
            assert!(live.refresh(&state).unwrap());
            assert!(world_json(&state).contains("seen-after-refresh"));

            let app = router(state);
            assert_eq!(
                get(
                    &app,
                    &format!("/worlds/default/sources?at={}", new_head.as_u64())
                )
                .await,
                (
                    StatusCode::OK,
                    serde_json::json!([
                        { "source": "initial.peer", "consumed": 0, "unrouted": 0, "recent_unrouted": [] },
                        { "source": "test.replay", "consumed": 0, "unrouted": 0, "recent_unrouted": [] },
                    ])
                )
            );
        });
    }

    #[test]
    fn refresh_folds_verdicts_committed_after_open_and_is_a_noop_when_nothing_changed() {
        let directory = TestDirectory::new("refresh-basic");
        let mut log = SqliteEventLog::open(directory.path()).unwrap();
        let mut verdicts = SqliteVerdictStore::open(directory.path()).unwrap();
        commit(&mut log, &mut verdicts, 1, "seen-at-open");

        let (state, mut live) =
            LiveReadOnlyWorld::open(directory.path(), "default", 10_000).unwrap();
        assert!(world_json(&state).contains("seen-at-open"));

        // Nothing new yet: refresh is a cheap no-op.
        assert!(!live.refresh(&state).unwrap());

        // A second process (here, the same test process) commits a new claim.
        commit(&mut log, &mut verdicts, 2, "seen-after-refresh");
        assert!(live.refresh(&state).unwrap());
        assert!(world_json(&state).contains("seen-after-refresh"));

        // Immediately calling refresh again with nothing new returns false.
        assert!(!live.refresh(&state).unwrap());
    }

    /// The cursor-regression guard is a pure classification (round-2 review, both reviewers) —
    /// tested directly against real `LogPosition`s from two commits, no store manipulation
    /// needed to fabricate a "backward" or "reset" reading.
    #[test]
    #[expect(
        clippy::cognitive_complexity,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    fn cursor_regression_guard_rejects_backward_and_reset_but_allows_forward_and_unchanged() {
        let directory = TestDirectory::new("cursor-classify");
        let mut log = SqliteEventLog::open(directory.path()).unwrap();
        let mut verdicts = SqliteVerdictStore::open(directory.path()).unwrap();
        let first = commit(&mut log, &mut verdicts, 1, "first");
        let second = commit(&mut log, &mut verdicts, 2, "second");
        assert!(first < second, "fixture assumes ascending positions");

        assert!(matches!(
            classify_cursor_update(None, None),
            CursorUpdate::Unchanged
        ));
        assert!(matches!(
            classify_cursor_update(Some(first), Some(first)),
            CursorUpdate::Unchanged
        ));
        assert!(matches!(
            classify_cursor_update(None, Some(first)),
            CursorUpdate::Advanced(position) if position == first
        ));
        assert!(matches!(
            classify_cursor_update(Some(first), Some(second)),
            CursorUpdate::Advanced(position) if position == second
        ));
        assert!(matches!(
            classify_cursor_update(Some(second), Some(first)),
            CursorUpdate::Regressed(_)
        ));
        assert!(matches!(
            classify_cursor_update(Some(first), None),
            CursorUpdate::Regressed(_)
        ));
    }

    #[test]
    fn a_regressed_or_reset_classification_maps_to_a_non_retryable_corrupt_error() {
        let directory = TestDirectory::new("position-fixture");
        let mut log = SqliteEventLog::open(directory.path()).unwrap();
        let first = match log.append(raw(1)).unwrap() {
            AppendOutcome::Inserted(position) => position,
            other => panic!("unexpected append outcome: {other:?}"),
        };
        let second = match log.append(raw(2)).unwrap() {
            AppendOutcome::Inserted(position) => position,
            other => panic!("unexpected append outcome: {other:?}"),
        };
        assert!(first < second, "fixture assumes ascending positions");

        for update in [
            classify_cursor_update(Some(second), Some(first)),
            classify_cursor_update(Some(first), None),
        ] {
            let CursorUpdate::Regressed(message) = update else {
                panic!("expected a Regressed classification");
            };
            let error = ReadOnlyWorldError::Corrupt(message);
            assert!(!error.is_retryable());
        }
    }

    /// The live refresh loop treats exactly `Log(LogError::Io(_))` as retryable —
    /// `is_retryable`'s own doc comment cites `s2w_log::map_sqlite`, which maps
    /// `SQLITE_BUSY`/locked into this variant. Only the negative `Corrupt` case was ever
    /// asserted (see the test above); round-1 code review (both reviewers, stream2worlds#128
    /// leg E) flagged that the positive case — the one the ruling's guarantee actually rests
    /// on — had no assertion of its own.
    #[test]
    fn an_io_error_is_retryable_but_corrupt_is_not() {
        assert!(
            ReadOnlyWorldError::Log(LogError::Io("database is locked".to_owned())).is_retryable()
        );
        assert!(!ReadOnlyWorldError::Corrupt("unrelated".to_owned()).is_retryable());
    }
}
