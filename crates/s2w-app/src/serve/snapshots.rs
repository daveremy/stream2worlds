//! `serve`'s snapshot wiring (decision 0021, #33 part 1b): restore at startup, the checkpoint
//! taken after each bridge poll, the writer thread, and the final snapshot on a stop signal.
//!
//! The capture is synchronous with the poll: [`Snapshotter::after_poll`] runs right after
//! `poll_once` returns, with no `.await` in between, so the bridge's last consumed position
//! and the timeline's head describe the same moment. The expensive part (encoding, `fsync`)
//! runs on one dedicated thread; the bridge only clones the head world, once per
//! [`SnapshotConfig::every`] raw events.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Instant;

use s2w_log::{LogPosition, LogReader, VerdictStore};

use crate::bridge::EngineRegistry;
use crate::query::{QueryState, Timeline};
use crate::snapshot::{
    Expected, SNAPSHOT_FORMAT, SnapshotV1, check_fold, check_log, fold_hash, store,
};
use crate::{AppError, NoteSink, Reporter};

/// Raw log events between periodic snapshots unless `--snapshot-every` says otherwise.
/// Research 0006 §3(d): a tail of 10^6 events replays well inside the 30 s restart budget.
pub const DEFAULT_EVERY: u64 = 1_000_000;

/// The fewest raw events since the last snapshot for which a stop signal writes a final one.
/// Below this the tail replays in seconds, and a snapshot would move the base up to the head
/// and cost the restarted process its scrub history for no real saving.
pub const SHUTDOWN_MIN: u64 = 100_000;

/// How `serve` uses snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotConfig {
    /// Load the newest valid snapshot at start and write new ones. `--no-snapshot` clears it.
    pub enabled: bool,
    /// Raw log events between periodic snapshots (`--snapshot-every`); at least 1.
    pub every: u64,
    /// See [`SHUTDOWN_MIN`]. Not a flag; tests lower it.
    pub shutdown_min: u64,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            every: DEFAULT_EVERY,
            shutdown_min: SHUTDOWN_MIN,
        }
    }
}

/// With snapshots enabled: restores the newest valid one into `state` and starts the writer.
/// Returns the log position the bridge resumes after (`None`: replay from the start) and the
/// snapshotter (`None` under `--no-snapshot`).
///
/// # Errors
/// As [`restore`] and [`Snapshotter::start`].
pub(super) fn prepare<L: LogReader + ?Sized>(
    state: &QueryState,
    (log, verdicts): (&L, &dyn VerdictStore),
    log_dir: &Path,
    config: SnapshotConfig,
    reporter: &mut dyn Reporter,
) -> Result<(Option<LogPosition>, Option<Snapshotter>), AppError> {
    if !config.enabled {
        return Ok((None, None));
    }
    let feed_hash = EngineRegistry::with_defaults().feed_fingerprint();
    let resume = restore(state, log, verdicts, (log_dir, feed_hash), reporter)?;
    let snapshotter = Snapshotter::start(log_dir, config, feed_hash, reporter.note_sink())?;
    Ok((resume, Some(snapshotter)))
}

/// Loads the newest valid snapshot in `log_dir` into `state` and returns the log position the
/// bridge resumes after, or `None` for a full replay from the start. Every file that fails a
/// validity rule is reported and skipped, never deleted. Temporary files a crashed write left
/// behind are removed first.
///
/// # Errors
/// [`AppError::Log`] if the verdict store's cursor cannot be read; [`AppError::BridgeStopped`]
/// if the timeline is unavailable or the restored head disagrees with the snapshot.
pub(super) fn restore<L: LogReader + ?Sized>(
    state: &QueryState,
    log: &L,
    verdicts: &dyn VerdictStore,
    (log_dir, feed_hash): (&Path, u64),
    reporter: &mut dyn Reporter,
) -> Result<Option<LogPosition>, AppError> {
    let dir = store::dir(log_dir);
    clean_stale(&dir, reporter);
    let (_, _, hub_cap) = state.bounds().map_err(stopped)?;
    let bridge_cursor = verdicts.cursor()?;
    let expected = Expected { hub_cap, feed_hash };
    let loaded = match store::load_latest(&dir, |snapshot| {
        check_fold(snapshot, expected)?;
        let previous = snapshot
            .position
            .checked_sub(1)
            .and_then(LogPosition::from_u64);
        check_log(snapshot, log, previous, bridge_cursor)
    }) {
        Ok(loaded) => loaded,
        Err(error) => {
            reporter.note(&format!(
                "snapshots: cannot read {}: {error}; replaying the log from the start",
                dir.display()
            ));
            return Ok(None);
        }
    };
    for (path, reason) in &loaded.skipped {
        reporter.note(&format!("ignoring snapshot {}: {reason}", path.display()));
    }
    let Some((path, snapshot)) = loaded.snapshot else {
        return Ok(None);
    };
    let (offset, position) = (snapshot.offset, snapshot.position);
    let Some(resume) = LogPosition::from_u64(position) else {
        // check_log never accepts position 0; this is only reachable through a bug there.
        return Err(AppError::BridgeStopped(format!(
            "snapshot {} records log position 0",
            path.display()
        )));
    };
    state
        .replace_timeline(Timeline::from_snapshot(snapshot.world, snapshot.time))
        .map_err(stopped)?;
    let (base, head, _) = state.bounds().map_err(stopped)?;
    if base != offset || head != offset {
        return Err(AppError::BridgeStopped(format!(
            "restored timeline spans {base}..{head}, but snapshot {} is at offset {offset}",
            path.display()
        )));
    }
    reporter.note(&format!(
        "restored from snapshot {} at offset {offset} (log position {position}); replaying the tail",
        path.display()
    ));
    Ok(Some(resume))
}

/// Removes temporary files a crashed write left behind, reporting each.
fn clean_stale(dir: &Path, reporter: &mut dyn Reporter) {
    match store::clean_tmp(dir) {
        Ok(removed) => {
            for path in removed {
                reporter.note(&format!(
                    "removed a partial snapshot write: {}",
                    path.display()
                ));
            }
        }
        Err(error) => reporter.note(&format!(
            "snapshots: cannot clean {}: {error}",
            dir.display()
        )),
    }
}

fn stopped(error: impl std::fmt::Display) -> AppError {
    AppError::BridgeStopped(error.to_string())
}

/// The bridge's last consumed log position, that event's content hash, and the timeline head
/// right after the poll that consumed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Checkpoint {
    position: LogPosition,
    event_hash: i64,
    offset: u64,
}

struct Job {
    snapshot: SnapshotV1,
    /// Raw events consumed when the world was captured.
    consumed: u64,
}

/// Decides when to snapshot, captures the head world at a poll boundary, and hands it to the
/// writer thread. One per `serve` process; only it writes into the snapshot directory.
pub(super) struct Snapshotter {
    dir: PathBuf,
    config: SnapshotConfig,
    feed_hash: u64,
    notes: NoteSink,
    checkpoint: Option<Checkpoint>,
    /// Raw log events consumed by this process's bridge.
    consumed: u64,
    /// `consumed` at which the next periodic snapshot is due.
    next_attempt: u64,
    shared: Arc<Shared>,
    jobs: Option<SyncSender<Job>>,
    thread: Option<JoinHandle<()>>,
}

/// State the writer thread updates.
struct Shared {
    /// `consumed` as of the last snapshot written successfully (0: none yet).
    written_at: AtomicU64,
    /// A job is queued or being written.
    busy: AtomicBool,
}

impl Snapshotter {
    /// Starts the writer thread for `log_dir`'s snapshot directory.
    ///
    /// # Errors
    /// [`AppError::Serve`] if the thread cannot be spawned.
    pub(super) fn start(
        log_dir: &Path,
        config: SnapshotConfig,
        feed_hash: u64,
        notes: NoteSink,
    ) -> Result<Self, AppError> {
        let dir = store::dir(log_dir);
        let shared = Arc::new(Shared {
            written_at: AtomicU64::new(0),
            busy: AtomicBool::new(false),
        });
        let (jobs, queue) = sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("s2w-snapshot".to_owned())
            .spawn({
                let (dir, shared, notes) = (dir.clone(), shared.clone(), notes.clone());
                move || writer(&dir, &queue, &shared, &*notes)
            })
            .map_err(AppError::Serve)?;
        Ok(Self {
            dir,
            config,
            feed_hash,
            notes,
            checkpoint: None,
            consumed: 0,
            next_attempt: config.every.max(1),
            shared,
            jobs: Some(jobs),
            thread: Some(thread),
        })
    }

    /// Runs right after a successful `poll_once`, before anything awaits: records the
    /// checkpoint and, when one is due and the writer is idle, captures the head world and
    /// queues it. A busy writer leaves the snapshot due, so the next poll tries again.
    ///
    /// A poll that reported an error still committed a consistent prefix: `poll_once` appends
    /// only the claims of the events it judged and moves `mark` to the last of them, so the
    /// checkpoint and the head always agree.
    pub(super) fn after_poll(
        &mut self,
        mark: Option<(LogPosition, i64)>,
        consumed: u64,
        state: &QueryState,
    ) {
        self.consumed = self.consumed.saturating_add(consumed);
        if let Some((position, event_hash)) = mark
            && self.checkpoint.is_none_or(|c| c.position != position)
        {
            match state.bounds() {
                Ok((_, head, _)) => {
                    self.checkpoint = Some(Checkpoint {
                        position,
                        event_hash,
                        offset: head,
                    });
                }
                Err(error) => (self.notes)(&format!("snapshot checkpoint skipped: {error}")),
            }
        }
        if self.consumed < self.next_attempt || self.checkpoint.is_none() {
            return;
        }
        if self.shared.busy.load(Ordering::Acquire) {
            return;
        }
        // Due now; whatever happens below, the next attempt waits another `every` events.
        self.next_attempt = self.consumed.saturating_add(self.config.every.max(1));
        let snapshot = match self.capture(state) {
            Ok(snapshot) => snapshot,
            Err(reason) => {
                (self.notes)(&format!("snapshot skipped: {reason}"));
                return;
            }
        };
        let Some(jobs) = &self.jobs else { return };
        self.shared.busy.store(true, Ordering::Release);
        let job = Job {
            snapshot,
            consumed: self.consumed,
        };
        if jobs.try_send(job).is_err() {
            self.shared.busy.store(false, Ordering::Release);
            (self.notes)("snapshot skipped: the writer thread is gone");
        }
    }

    /// The head world as a snapshot at the checkpoint, refusing a head that has moved past it.
    fn capture(&self, state: &QueryState) -> Result<SnapshotV1, String> {
        let checkpoint = self.checkpoint.ok_or("nothing consumed yet")?;
        let (world, time) = state.head_capture().map_err(|e| e.to_string())?;
        if world.offset() != checkpoint.offset {
            return Err(format!(
                "the head is at offset {}, but the checkpoint is at {}",
                world.offset(),
                checkpoint.offset
            ));
        }
        let hub_cap = world.hub_in_degree_cap();
        Ok(SnapshotV1 {
            format: SNAPSHOT_FORMAT,
            fold_hash: fold_hash(hub_cap),
            feed_hash: self.feed_hash,
            hub_cap,
            offset: checkpoint.offset,
            position: checkpoint.position.as_u64(),
            position_event_hash: checkpoint.event_hash,
            // Recorded for portability, never consulted at load (decision 0021); serve does
            // not enumerate source cursors yet.
            cursors: Vec::new(),
            time,
            world,
        })
    }

    /// Runs once a stop signal arrives, before the bridge is dropped: waits for any in-flight
    /// write, then writes a final snapshot if at least [`SnapshotConfig::shutdown_min`] raw
    /// events arrived since the last one written. Blocks the (current-thread) runtime until
    /// done, so the timeline cannot move underneath it.
    pub(super) fn finish(&mut self, state: &QueryState) {
        self.stop_writer();
        let since = self
            .consumed
            .saturating_sub(self.shared.written_at.load(Ordering::Acquire));
        if since == 0 || self.checkpoint.is_none() {
            return;
        }
        if since < self.config.shutdown_min {
            (self.notes)(&format!(
                "no final snapshot: {since} events since the last one (fewer than {})",
                self.config.shutdown_min
            ));
            return;
        }
        match self.capture(state) {
            Ok(snapshot) => write_one(
                &self.dir,
                Job {
                    snapshot,
                    consumed: self.consumed,
                },
                &self.shared,
                &*self.notes,
            ),
            Err(reason) => (self.notes)(&format!("final snapshot skipped: {reason}")),
        }
    }

    fn stop_writer(&mut self) {
        drop(self.jobs.take());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            (self.notes)("the snapshot writer thread panicked");
        }
    }
}

impl Drop for Snapshotter {
    /// Lets an in-flight write finish on every exit path; the file is written atomically, so
    /// even a killed process leaves the previous snapshot intact.
    fn drop(&mut self) {
        self.stop_writer();
    }
}

fn writer(
    dir: &Path,
    queue: &Receiver<Job>,
    shared: &Shared,
    notes: &(dyn Fn(&str) + Send + Sync),
) {
    while let Ok(job) = queue.recv() {
        write_one(dir, job, shared, notes);
        shared.busy.store(false, Ordering::Release);
    }
}

fn write_one(dir: &Path, job: Job, shared: &Shared, notes: &(dyn Fn(&str) + Send + Sync)) {
    let started = Instant::now();
    let offset = job.snapshot.offset;
    match store::write(dir, &job.snapshot) {
        Ok(path) => {
            shared.written_at.fetch_max(job.consumed, Ordering::AcqRel);
            let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
            notes(&format!(
                "snapshot written at offset {offset}: {} ({bytes} bytes, {} ms)",
                path.display(),
                started.elapsed().as_millis()
            ));
            if let Err(error) = store::prune(dir, store::KEEP) {
                notes(&format!("pruning old snapshots failed: {error}"));
            }
        }
        Err(error) => notes(&format!(
            "snapshot at offset {offset} failed: {error}; the next one is due after --snapshot-every more events"
        )),
    }
}
