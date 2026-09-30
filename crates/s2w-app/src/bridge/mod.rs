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

mod judge;
mod registry;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use s2w_log::{LogError, LogPosition, LogReader, StoredEvent, StoredVerdict, VerdictStore};
use s2w_model::SourceId;
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
            // s2w#220: 250 peaked 58 MiB lower than 1000 on the recorded backfill, at the
            // same wall time; smaller batches saved little more and cost time.
            batch: 250,
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
    /// [`AbstainReason::BelowThreshold`].
    pub below_threshold: u64,
    /// [`AbstainReason::Ambiguous`].
    pub ambiguous: u64,
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
        self.abstained.below_threshold += other.abstained.below_threshold;
        self.abstained.ambiguous += other.abstained.ambiguous;
        self.unrouted += other.unrouted;
        self.engine_panics += other.engine_panics;
        self.replayed += other.replayed;
        self.replayed_stale_version += other.replayed_stale_version;
        self.evaluated += other.evaluated;
    }
}

/// How many of a source's most recent unrouted events are kept for the sources view, so a
/// viewer can see the stream is alive however long it runs.
pub const RECENT_UNROUTED_CAP: usize = 20;

/// What the bridge did with one source's events: [`BridgeStats`]'s aggregates, broken out per
/// source so the query API can name an unrouted stream instead of saying nothing arrived.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceStats {
    /// Stored events of this source read from the log.
    pub consumed: u64,
    /// Consumed events of this source no engine is routed for.
    pub unrouted: u64,
    /// The most recent unrouted events, in log order (oldest first), capped at
    /// [`RECENT_UNROUTED_CAP`].
    pub recent_unrouted: VecDeque<StoredEvent>,
}

impl SourceStats {
    /// Folds `other`'s counters into `self`, keeping the newest [`RECENT_UNROUTED_CAP`] unrouted
    /// events across both. `judge_event` uses this to fold one judged event's local delta into a
    /// batch's per-source stats only after every fallible step of that event has succeeded, so a
    /// mid-event error leaves the batch's counters untouched;
    /// [`Bridge::absorb_source_stats`] uses it to fold a committed batch into the bridge's
    /// running totals.
    fn add(&mut self, other: &Self) {
        self.consumed += other.consumed;
        self.unrouted += other.unrouted;
        for event in other.recent_unrouted.iter().cloned() {
            self.push_recent_unrouted(event);
        }
    }

    /// Pushes one more unrouted event, evicting the oldest until the ring is back at
    /// [`RECENT_UNROUTED_CAP`]. The one place the cap invariant lives — [`Self::add`] is the
    /// only caller.
    fn push_recent_unrouted(&mut self, event: StoredEvent) {
        self.recent_unrouted.push_back(event);
        while self.recent_unrouted.len() > RECENT_UNROUTED_CAP {
            self.recent_unrouted.pop_front();
        }
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
    pub engine: String,
    /// [`Engine::version`].
    pub version: u32,
    /// The verdict, with a caught panic as `Abstain(Panicked)`.
    pub verdict: Verdict,
    /// Encoded provenance (decision 0012's reserved keys). `None` when an engine doesn't
    /// implement it.
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
            engine: self.engine.clone(),
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
        .map(|engine| evaluate_one(stored, *engine))
        .collect()
}

/// [`evaluate_stored`] for one engine.
fn evaluate_one(stored: &StoredEvent, engine: &dyn Engine) -> VerdictRecord {
    let verdict =
        catch_unwind(AssertUnwindSafe(|| engine.evaluate(&stored.event))).unwrap_or_else(|panic| {
            Verdict::Abstain {
                reason: AbstainReason::Panicked(panic_message(panic.as_ref())),
            }
        });
    VerdictRecord {
        position: stored.position,
        engine: engine.name().to_owned(),
        version: engine.version(),
        verdict,
        provenance: engine.provenance(),
    }
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
    /// The timeline already holds events (or, for [`Bridge::resume`], events after its restored
    /// base); a bridge replaying them would fold them twice.
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
    /// `registry.names()`, computed once: the only engines whose stored rows replay reads.
    engine_names: Vec<String>,
    state: QueryState,
    config: BridgeConfig,
    last: Option<LogPosition>,
    /// The content hash of the event at `last`, for the checkpoint a snapshot records.
    last_hash: Option<i64>,
    /// The verdict store's cursor at start. The log must reach it: a poll that reaches the end
    /// of the log below it reports `Corrupt` (the store is ahead of the log). Read once, on
    /// purpose: every stored row sits at or below the cursor, so a truncated log whose prefix
    /// still matches serves correctly up to its end, and replaced events fail the hash check.
    store_cursor: Option<LogPosition>,
    warned_unrouted: BTreeSet<SourceId>,
    stats: BridgeStats,
    per_source: BTreeMap<SourceId, SourceStats>,
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
        let (base, head, _) = state.bounds()?;
        if base != 0 || head != 0 {
            return Err(BridgeError::TimelineNotEmpty { head });
        }
        Self::build(reader, verdicts, registry, state, config)
    }

    /// A bridge that continues after `position` into `state`, which holds a restored snapshot
    /// whose world was folded from every event up to and including `position` (decision 0024).
    /// The caller has validated the snapshot against the log. A verdict store ahead of
    /// `position` is the normal case: the bridge serves those stored verdicts (decision 0012).
    ///
    /// # Errors
    /// [`BridgeError::TimelineNotEmpty`] if `state` has events after its base;
    /// [`BridgeError::Query`] if it is unavailable; [`BridgeError::Store`] if the verdict
    /// store's cursor cannot be read.
    #[expect(
        clippy::too_many_arguments,
        reason = "new's inputs plus the resume position; a parameter struct is a follow-up refactor (s2w#156)"
    )]
    pub fn resume(
        reader: R,
        verdicts: V,
        registry: EngineRegistry,
        state: QueryState,
        config: BridgeConfig,
        position: LogPosition,
    ) -> Result<Self, BridgeError> {
        let (base, head, _) = state.bounds()?;
        if head != base {
            return Err(BridgeError::TimelineNotEmpty { head });
        }
        let mut bridge = Self::build(reader, verdicts, registry, state, config)?;
        bridge.last = Some(position);
        Ok(bridge)
    }

    /// The same bridge under a new `registry`, for a live rebuild (s2w#184): keeps the log
    /// reader and the verdict store (and with it the store's writer lock), re-reads the store's
    /// cursor, and starts over from the log's first event, or after `resume` when the caller
    /// restored a snapshot folded under `registry` into `state`. Every counter and the unrouted
    /// warnings reset, and the empty per-source counters are published at once, so `/sources`
    /// never shows the old routing's counts under the new one. The caller has already replaced
    /// the timeline; like [`Self::new`] and [`Self::resume`] this refuses one holding events
    /// past its base.
    ///
    /// # Errors
    /// As [`Self::new`] (without `resume`) or [`Self::resume`] (with it).
    pub fn restart(
        self,
        registry: EngineRegistry,
        resume: Option<LogPosition>,
    ) -> Result<Self, BridgeError> {
        let Self {
            reader,
            verdicts,
            state,
            config,
            ..
        } = self;
        let bridge = match resume {
            Some(position) => Self::resume(reader, verdicts, registry, state, config, position)?,
            None => Self::new(reader, verdicts, registry, state, config)?,
        };
        bridge.state.publish_source_stats(BTreeMap::new());
        Ok(bridge)
    }

    /// The log reader, e.g. for a rebuild's snapshot restore to validate against.
    #[must_use]
    pub const fn reader(&self) -> &R {
        &self.reader
    }

    fn build(
        reader: R,
        verdicts: V,
        registry: EngineRegistry,
        state: QueryState,
        config: BridgeConfig,
    ) -> Result<Self, BridgeError> {
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
            engine_names: registry.names(),
            registry,
            state,
            config,
            last: None,
            last_hash: None,
            store_cursor,
            warned_unrouted: BTreeSet::new(),
            stats: BridgeStats::default(),
            per_source: BTreeMap::new(),
        })
    }

    /// The most events one [`Self::poll_once`] consumes.
    #[must_use]
    pub(crate) const fn batch(&self) -> usize {
        self.config.batch
    }

    /// Sets the most events the next [`Self::poll_once`] consumes (at least 1). The reader
    /// must return at most as many, since a shorter read means the log is exhausted. `serve`'s
    /// local driver shrinks both during a catch-up so one poll never holds its runtime long
    /// (s2w#331).
    pub(crate) fn set_batch(&mut self, batch: usize) {
        self.config.batch = batch.max(1);
    }

    /// The state this bridge appends to.
    pub(crate) fn state(&self) -> &QueryState {
        &self.state
    }

    /// The last log position this bridge consumed and that event's content hash: the checkpoint
    /// a snapshot records (decision 0024). `None` until this bridge has consumed an event, even
    /// after [`Self::resume`], which knows the position but not its hash.
    #[must_use]
    pub fn mark(&self) -> Option<(LogPosition, i64)> {
        self.last.zip(self.last_hash)
    }

    /// Everything this bridge has done so far.
    #[must_use]
    pub const fn stats(&self) -> BridgeStats {
        self.stats
    }

    /// Everything this bridge has done so far, per source: the same counters as
    /// [`Self::stats`] plus each unrouted source's most recent unrouted events.
    #[must_use]
    pub fn source_stats(&self) -> BTreeMap<SourceId, SourceStats> {
        self.per_source.clone()
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
    /// poisoned for good. The batch's claims are appended under one lock, so that
    /// error leaves none of them appended.
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
        let through_hash = events
            .iter()
            .find(|event| event.position == through)
            .map(|event| event.content_hash);
        // The batch's events and verdict rows are durable now: free them before the fold grows
        // the world. At batch 250 this measured no saving (s2w#220); it keeps the order right if
        // the batch grows again.
        drop(events);
        drop(judged.new_rows);
        // One write lock per batch, not per claim (s2w#216): a reader holding the lock (a
        // `/world` projection) then delays the fold once per batch.
        self.state.append_batch(judged.claims)?;
        report.stats = judged.stats;
        self.last = Some(through);
        self.last_hash = through_hash;
        self.stats.add(&report.stats);
        self.absorb_source_stats(&judged.per_source);
        Ok(self.finish(report))
    }

    /// Folds a committed batch's per-source counters into the bridge's totals and republishes
    /// them to the query API. Called on the same path [`Self::stats`] advances, so a batch
    /// whose commit failed counts nowhere.
    fn absorb_source_stats(&mut self, batch: &BTreeMap<SourceId, SourceStats>) {
        if batch.is_empty() {
            return;
        }
        for (source, stats) in batch {
            self.per_source
                .entry(source.clone())
                .or_default()
                .add(stats);
        }
        // Telemetry, not claims: the one write to QueryState besides `append` (see AGENTS.md).
        self.state.publish_source_stats(self.per_source.clone());
    }

    fn finish(&self, report: PollReport) -> PollReport {
        match &report.error {
            Some(error @ LogError::Corrupt(_)) => eprintln!(
                "s2w: bridge: the verdict store and the log disagree; the bridge cannot advance \
                 past this point until that is repaired: {error}"
            ),
            Some(error) => eprintln!("s2w: bridge: poll ended early, will retry: {error}"),
            None => {}
        }
        report
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
