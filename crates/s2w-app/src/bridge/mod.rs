//! The live bridge: log → System 1 engines → [`QueryState`].
//!
//! [`Bridge`] reads stored events it has not consumed yet, runs every engine the
//! [`EngineRegistry`] routes for the event's source, and appends each proposed claim to the
//! served timeline at the event's receipt time. Decision 0011 records the design.
//!
//! The bridge resumes from the last [`LogPosition`] it consumed, never from a fold offset: one
//! raw event yields zero or more claims. That position is in memory only, so a new bridge
//! replays the whole log into an empty timeline — but through the [`VerdictStore`]: a stored
//! verdict for an event and engine name is served as stored, whatever its version, and the
//! engine is not called; only an event an engine has no stored verdict for is evaluated
//! (decision 0012). Each poll batch commits its new verdicts in one transaction before any
//! of its claims is served, so no claim is ever served whose verdict is not durable.

mod registry;

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use s2w_log::{LogError, LogPosition, LogReader, StoredEvent, StoredVerdict, VerdictStore};
use s2w_model::{SourceId, Timestamp, WorldEvent};
use s2w_system1::{AbstainReason, Engine, Verdict};
use tokio::sync::watch;

use crate::query::{QueryError, QueryState};

pub use registry::{EngineRegistry, RegistryError, Route};

/// How the bridge polls. SQLite has no cross-process notification, so discovery is a poll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeConfig {
    /// The delay after a poll that found events, and the first delay after one that did not.
    pub poll: Duration,
    /// The ceiling the empty-poll delay doubles up to.
    pub max_backoff: Duration,
    /// The most events one [`Bridge::poll_once`] consumes.
    pub batch: usize,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(250),
            max_backoff: Duration::from_secs(2),
            batch: 1000,
        }
    }
}

/// Abstentions by reason. An engine panic is counted in [`BridgeStats::engine_panics`] instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbstainCounts {
    /// [`AbstainReason::NotMine`].
    pub not_mine: u64,
    /// [`AbstainReason::Unparseable`].
    pub unparseable: u64,
    /// [`AbstainReason::Insufficient`].
    pub insufficient: u64,
}

/// What the bridge did. Each consumed event and each verdict increments exactly one counter
/// of its kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BridgeStats {
    /// Stored events read from the log.
    pub consumed: u64,
    /// Claims appended to the timeline.
    pub proposed_claims: u64,
    /// `Propose` verdicts with no claims.
    pub proposed_empty: u64,
    /// `Abstain` verdicts, by reason.
    pub abstained: AbstainCounts,
    /// Consumed events no engine is routed for.
    pub unrouted: u64,
    /// `Abstain(Panicked)` verdicts: an engine call panicked, now or when the verdict was
    /// stored. (Backwards receipt times are clamped and counted by the timeline,
    /// `TimeRange::clamped`.)
    pub engine_panics: u64,
    /// Verdicts served from the verdict store; the engine was not called.
    pub replayed: u64,
    /// Replayed verdicts whose stored version differs from the registered engine's version,
    /// so a version bump is visible.
    pub replayed_stale_version: u64,
    /// Engine calls: verdicts evaluated now and stored.
    pub evaluated: u64,
}

impl BridgeStats {
    fn add(&mut self, other: &Self) {
        self.consumed += other.consumed;
        self.proposed_claims += other.proposed_claims;
        self.proposed_empty += other.proposed_empty;
        self.abstained.not_mine += other.abstained.not_mine;
        self.abstained.unparseable += other.abstained.unparseable;
        self.abstained.insufficient += other.abstained.insufficient;
        self.unrouted += other.unrouted;
        self.engine_panics += other.engine_panics;
        self.replayed += other.replayed;
        self.replayed_stale_version += other.replayed_stale_version;
        self.evaluated += other.evaluated;
    }
}

/// One [`Bridge::poll_once`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PollReport {
    /// What this poll did.
    pub stats: BridgeStats,
    /// A log or verdict-store error that ended this poll early. Everything before it was
    /// consumed; the next poll resumes after the last consumed event. A failed verdict commit
    /// consumes nothing from the batch.
    pub error: Option<LogError>,
}

/// One engine's verdict on one stored event: the unit the verdict store persists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictRecord {
    /// The evaluated event's log position.
    pub position: LogPosition,
    /// [`Engine::name`].
    pub engine: &'static str,
    /// [`Engine::version`].
    pub version: u32,
    /// The verdict, with a caught panic as `Abstain(Panicked)`.
    pub verdict: Verdict,
    /// Encoded provenance (decision 0012's reserved keys). `None` from every shipped engine.
    pub provenance: Option<Vec<u8>>,
}

impl VerdictRecord {
    /// The stored form, bound to the judged event's content hash.
    fn to_stored(&self, event_hash: i64) -> Result<StoredVerdict, LogError> {
        let verdict = serde_json::to_vec(&self.verdict)
            .map_err(|error| LogError::Io(format!("encoding a verdict: {error}")))?;
        Ok(StoredVerdict {
            position: self.position,
            event_hash,
            engine: self.engine.to_owned(),
            version: self.version,
            verdict,
            provenance: self.provenance.clone(),
        })
    }
}

/// Runs `engines` on `stored`, in order. The one place verdicts are produced; the bridge
/// persists every one in its [`VerdictStore`] before serving it.
///
/// Engines are infallible by signature; a panic anyway is caught and recorded as
/// [`AbstainReason::Panicked`], so one bad payload cannot stop the bridge. This relies on the
/// workspace's default `panic = "unwind"`. `AssertUnwindSafe` is sound because engines are pure
/// functions of the payload (the `s2w-system1` invariant): a panic leaves no torn state behind.
#[must_use]
pub fn evaluate_stored(stored: &StoredEvent, engines: &[&dyn Engine]) -> Vec<VerdictRecord> {
    engines
        .iter()
        .map(|engine| {
            let verdict = catch_unwind(AssertUnwindSafe(|| engine.evaluate(&stored.event)))
                .unwrap_or_else(|panic| Verdict::Abstain {
                    reason: AbstainReason::Panicked(panic_message(panic.as_ref())),
                });
            VerdictRecord {
                position: stored.position,
                engine: engine.name(),
                version: engine.version(),
                verdict,
                provenance: None,
            }
        })
        .collect()
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}

/// Why the bridge could not start or had to stop.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// The timeline already holds events; a bridge replaying from the start would fold them
    /// twice.
    #[error("the bridge needs an empty timeline, but its head is {head}")]
    TimelineNotEmpty {
        /// The timeline's head offset.
        head: u64,
    },
    /// The served timeline is unavailable (its lock was poisoned).
    #[error("query state: {0}")]
    Query(#[from] QueryError),
    /// The verdict store could not be read at start.
    #[error("verdict store: {0}")]
    Store(LogError),
    /// The blocking task running a poll failed.
    #[error("bridge poll task: {0}")]
    Task(String),
}

/// Reads the log, runs System 1 (or replays its stored verdicts) and appends claims to the
/// served timeline.
///
/// Generic over [`LogReader`] and [`VerdictStore`], so it runs against the in-memory log and
/// store in tests and the SQLite ones on disk.
pub struct Bridge<R: LogReader, V: VerdictStore> {
    reader: R,
    verdicts: V,
    registry: EngineRegistry,
    state: QueryState,
    config: BridgeConfig,
    last: Option<LogPosition>,
    /// The verdict store's cursor at start: the log must reach it (store ahead of log is
    /// corruption).
    store_cursor: Option<LogPosition>,
    warned_unrouted: BTreeSet<SourceId>,
    stats: BridgeStats,
}

/// One poll batch, judged but not yet committed or served.
#[derive(Default)]
struct Judged {
    stats: BridgeStats,
    new_rows: Vec<StoredVerdict>,
    claims: Vec<(Timestamp, WorldEvent)>,
    through: Option<LogPosition>,
}

impl<R: LogReader, V: VerdictStore> Bridge<R, V> {
    /// A bridge that will replay `reader` from its first event into `state`, serving the
    /// verdicts `verdicts` already holds and storing the ones it evaluates.
    ///
    /// # Errors
    /// [`BridgeError::TimelineNotEmpty`] if `state` already has events;
    /// [`BridgeError::Query`] if it is unavailable; [`BridgeError::Store`] if the verdict
    /// store's cursor cannot be read.
    pub fn new(
        reader: R,
        verdicts: V,
        registry: EngineRegistry,
        state: QueryState,
        config: BridgeConfig,
    ) -> Result<Self, BridgeError> {
        let head = state.branches()?.first().map_or(0, |branch| branch.head);
        if head != 0 {
            return Err(BridgeError::TimelineNotEmpty { head });
        }
        let store_cursor = verdicts.cursor().map_err(BridgeError::Store)?;
        // A zero batch would never advance, and a zero delay would spin.
        let poll = config.poll.max(Duration::from_millis(1));
        let config = BridgeConfig {
            poll,
            max_backoff: config.max_backoff.max(poll),
            batch: config.batch.max(1),
        };
        Ok(Self {
            reader,
            verdicts,
            registry,
            state,
            config,
            last: None,
            store_cursor,
            warned_unrouted: BTreeSet::new(),
            stats: BridgeStats::default(),
        })
    }

    /// Everything this bridge has done so far.
    #[must_use]
    pub const fn stats(&self) -> BridgeStats {
        self.stats
    }

    /// The verdict store, e.g. to inspect what a poll stored.
    #[must_use]
    pub const fn verdicts(&self) -> &V {
        &self.verdicts
    }

    /// Consumes at most `batch` new events. Synchronous, so a test can drive it without a
    /// runtime.
    ///
    /// Per batch: judge every event (a stored verdict is served as stored; an engine with none
    /// is evaluated), commit the new verdicts and the cursor in one transaction, and only then
    /// append the batch's claims and advance. A log or store error ends the poll early and is
    /// returned in [`PollReport::error`], never as `Err`: the bridge keeps its position and the
    /// next poll retries. Corruption (a verdict whose event hash does not match the log, bytes
    /// that do not decode, a store ahead of the log) is reported the same way and never falls
    /// back to re-evaluation.
    ///
    /// # Errors
    /// [`BridgeError::Query`] if the timeline is unavailable. That is fatal: the lock is
    /// poisoned for good, and the batch in progress may be partly appended.
    pub fn poll_once(&mut self) -> Result<PollReport, BridgeError> {
        let mut report = PollReport::default();
        let (events, read_error) = self.read_batch();
        let exhausted = read_error.is_none() && events.len() < self.config.batch;
        report.error = read_error;

        let consumed_through = events.last().map(|e| e.position).max(self.last);
        if exhausted && consumed_through < self.store_cursor {
            report.error = Some(LogError::Corrupt(format!(
                "the verdict store's cursor is {}, but the log ends at {}",
                self.store_cursor.map_or(0, LogPosition::as_u64),
                consumed_through.map_or(0, LogPosition::as_u64),
            )));
            return Ok(self.finish(report));
        }

        let (judged, corrupt) = self.judge(&events);
        if corrupt.is_some() {
            report.error = corrupt;
        }
        let Some(through) = judged.through else {
            return Ok(self.finish(report));
        };
        if let Err(error) = self.verdicts.commit_batch(&judged.new_rows, through) {
            // Nothing is durable, so nothing is served: the next poll retries the batch.
            report.error = Some(error);
            return Ok(self.finish(report));
        }
        for (at, claim) in judged.claims {
            self.state.append(at, claim)?;
        }
        report.stats = judged.stats;
        self.last = Some(through);
        self.stats.add(&report.stats);
        Ok(self.finish(report))
    }

    fn finish(&self, report: PollReport) -> PollReport {
        if let Some(error) = &report.error {
            eprintln!("s2w: bridge: poll ended early, will retry: {error}");
        }
        report
    }

    /// At most `batch` events after `last`, and the log error that cut the read short.
    fn read_batch(&self) -> (Vec<StoredEvent>, Option<LogError>) {
        let mut events = Vec::new();
        let items = match self.reader.read_after(self.last) {
            Ok(items) => items,
            Err(error) => return (events, Some(error)),
        };
        for item in items.take(self.config.batch) {
            match item {
                Ok(stored) => events.push(stored),
                Err(error) => return (events, Some(error)),
            }
        }
        (events, None)
    }

    /// Judges `events` in order. On an error, returns what was judged before the bad event
    /// together with the error, so the good prefix is still committed and served.
    fn judge(&mut self, events: &[StoredEvent]) -> (Judged, Option<LogError>) {
        let mut judged = Judged::default();
        let Some(batch_end) = events.last().map(|e| e.position) else {
            return (judged, None);
        };
        let stored = match self.verdicts.read_range(self.last, batch_end) {
            Ok(stored) => stored,
            Err(error) => return (judged, Some(error)),
        };
        let mut stored = stored.as_slice();
        for event in events {
            let here = stored.partition_point(|row| row.position <= event.position);
            let (at_event, rest) = stored.split_at(here);
            if let Err(error) = self.judge_event(event, at_event, &mut judged) {
                return (judged, Some(error));
            }
            stored = rest;
            judged.through = Some(event.position);
        }
        (judged, None)
    }

    /// Judges one event into `judged`, which it leaves untouched on error. `rows` are the
    /// stored verdicts at positions after the previous event through this one.
    fn judge_event(
        &mut self,
        event: &StoredEvent,
        rows: &[StoredVerdict],
        judged: &mut Judged,
    ) -> Result<(), LogError> {
        let at = event.position.as_u64();
        if let Some(row) = rows.iter().find(|row| row.position != event.position) {
            return Err(LogError::Corrupt(format!(
                "a stored verdict names log position {}, which holds no event",
                row.position.as_u64()
            )));
        }
        if rows.iter().any(|row| row.event_hash != event.content_hash) {
            return Err(LogError::Corrupt(format!(
                "stored verdicts at log position {at} judged a different event than the log holds"
            )));
        }

        let engines = self.registry.engines_for(&event.event.source);
        let mut stats = BridgeStats {
            consumed: 1,
            ..BridgeStats::default()
        };
        if engines.is_empty() {
            stats.unrouted += 1;
            if !self.warned_unrouted.contains(&event.event.source) {
                self.warned_unrouted.insert(event.event.source.clone());
                eprintln!(
                    "s2w: bridge: no System 1 engine is routed for source '{}'; its events are skipped",
                    event.event.source.as_str()
                );
            }
        }
        let mut verdicts = Vec::with_capacity(engines.len());
        let mut new_rows = Vec::new();
        for engine in engines {
            // Rows at one position are in write order, so the first match is the one first
            // served (the lowest seq), whatever its version.
            if let Some(row) = rows.iter().find(|row| row.engine == engine.name()) {
                let verdict: Verdict = serde_json::from_slice(&row.verdict).map_err(|error| {
                    LogError::Corrupt(format!(
                        "stored verdict of engine '{}' at log position {at} does not decode: {error}",
                        row.engine
                    ))
                })?;
                stats.replayed += 1;
                if row.version != engine.version() {
                    stats.replayed_stale_version += 1;
                }
                verdicts.push(verdict);
            } else {
                for record in evaluate_stored(event, &[engine]) {
                    stats.evaluated += 1;
                    if let Verdict::Abstain {
                        reason: AbstainReason::Panicked(message),
                    } = &record.verdict
                    {
                        eprintln!(
                            "s2w: bridge: engine '{}' panicked at log position {at}: {message}",
                            record.engine
                        );
                    }
                    new_rows.push(record.to_stored(event.content_hash)?);
                    verdicts.push(record.verdict);
                }
            }
        }

        let received = event.event.received_at;
        let mut claims = Vec::new();
        for verdict in verdicts {
            match verdict {
                Verdict::Propose {
                    claims: proposed, ..
                } => {
                    if proposed.is_empty() {
                        stats.proposed_empty += 1;
                    }
                    stats.proposed_claims += proposed.len() as u64;
                    claims.extend(proposed.into_iter().map(|claim| (received, claim)));
                }
                Verdict::Abstain { reason } => match reason {
                    AbstainReason::NotMine => stats.abstained.not_mine += 1,
                    AbstainReason::Unparseable(_) => stats.abstained.unparseable += 1,
                    AbstainReason::Insufficient(_) => stats.abstained.insufficient += 1,
                    AbstainReason::Panicked(_) => stats.engine_panics += 1,
                },
            }
        }
        judged.stats.add(&stats);
        judged.new_rows.extend(new_rows);
        judged.claims.extend(claims);
        Ok(())
    }
}

impl<R: LogReader + Send + 'static, V: VerdictStore + Send + 'static> Bridge<R, V> {
    /// Polls until `shutdown` turns true, then returns the totals.
    ///
    /// An empty poll backs off from `poll`, doubling up to `max_backoff`; a poll that found
    /// events resets the delay, and a full batch polls again at once.
    ///
    /// Each poll runs on the blocking pool so SQLite I/O never stalls the async runtime. The
    /// bridge moves into the task and back out by value (`move || { b.poll_once(); b }`), which
    /// is why a reader holding a non-`Sync` SQLite connection needs no `Arc<Mutex<_>>`.
    ///
    /// # Errors
    /// [`BridgeError::Query`] when the timeline becomes unavailable, or
    /// [`BridgeError::Task`] if a poll task could not complete.
    pub async fn run(
        mut self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<BridgeStats, BridgeError> {
        let mut delay = self.config.poll;
        while !*shutdown.borrow() {
            let (bridge, report) = tokio::task::spawn_blocking(move || {
                let report = self.poll_once();
                (self, report)
            })
            .await
            .map_err(|error| BridgeError::Task(error.to_string()))?;
            self = bridge;
            let report = report?;
            let full = report.stats.consumed >= self.config.batch as u64;
            let sleep_for = if report.stats.consumed == 0 {
                let current = delay;
                delay = delay.saturating_mul(2).min(self.config.max_backoff);
                current
            } else {
                delay = self.config.poll;
                if full && report.error.is_none() {
                    continue;
                }
                delay
            };
            tokio::select! {
                () = tokio::time::sleep(sleep_for) => {}
                changed = shutdown.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
        Ok(self.stats)
    }
}
