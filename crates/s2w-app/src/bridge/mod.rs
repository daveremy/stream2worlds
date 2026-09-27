//! The live bridge: log → System 1 engines → [`QueryState`].
//!
//! [`Bridge`] reads stored events it has not consumed yet, runs every engine the
//! [`EngineRegistry`] routes for the event's source, and appends each proposed claim to the
//! served timeline at the event's receipt time. Decision 0011 records the design.
//!
//! The bridge resumes from the last [`LogPosition`] it consumed, never from a fold offset: one
//! raw event yields zero or more claims. That position is in memory only, so a new bridge
//! replays the whole log into an empty timeline. Verdicts are not persisted yet, which is safe
//! only because every registered engine is a pure function of its payload (see
//! `crates/s2w-system1/AGENTS.md`).

mod registry;

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use s2w_log::{LogError, LogPosition, LogReader, StoredEvent};
use s2w_model::SourceId;
use s2w_system1::{AbstainReason, Engine, Verdict};
use tokio::sync::watch;

use crate::query::{QueryError, QueryState};

pub use registry::{EngineRegistry, Route};

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
    /// Engine calls that panicked; the bridge recorded them as `Abstain(Panicked)`.
    /// (Backwards receipt times are clamped and counted by the timeline, `TimeRange::clamped`.)
    pub engine_panics: u64,
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
    }
}

/// One [`Bridge::poll_once`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PollReport {
    /// What this poll did.
    pub stats: BridgeStats,
    /// A log error that ended this poll early. Everything before it was consumed; the next
    /// poll resumes after the last consumed event.
    pub error: Option<LogError>,
}

/// One engine's verdict on one stored event: the unit a future verdict log persists.
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
}

/// Runs `engines` on `stored`, in order. The one place verdicts are produced, so persisting
/// them later is one write inserted here.
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
    /// The blocking task running a poll failed.
    #[error("bridge poll task: {0}")]
    Task(String),
}

/// Reads the log, runs System 1 and appends claims to the served timeline.
///
/// Generic over [`LogReader`], so it runs against [`s2w_log::InMemoryEventLog`] in tests and
/// [`s2w_log::SqliteEventLog`] on disk.
pub struct Bridge<R: LogReader> {
    reader: R,
    registry: EngineRegistry,
    state: QueryState,
    config: BridgeConfig,
    last: Option<LogPosition>,
    warned_unrouted: BTreeSet<SourceId>,
    stats: BridgeStats,
}

impl<R: LogReader> Bridge<R> {
    /// A bridge that will replay `reader` from its first event into `state`.
    ///
    /// # Errors
    /// [`BridgeError::TimelineNotEmpty`] if `state` already has events;
    /// [`BridgeError::Query`] if it is unavailable.
    pub fn new(
        reader: R,
        registry: EngineRegistry,
        state: QueryState,
        config: BridgeConfig,
    ) -> Result<Self, BridgeError> {
        let head = state.branches()?.first().map_or(0, |branch| branch.head);
        if head != 0 {
            return Err(BridgeError::TimelineNotEmpty { head });
        }
        // A zero batch would never advance, and a zero delay would spin.
        let poll = config.poll.max(Duration::from_millis(1));
        let config = BridgeConfig {
            poll,
            max_backoff: config.max_backoff.max(poll),
            batch: config.batch.max(1),
        };
        Ok(Self {
            reader,
            registry,
            state,
            config,
            last: None,
            warned_unrouted: BTreeSet::new(),
            stats: BridgeStats::default(),
        })
    }

    /// Everything this bridge has done so far.
    #[must_use]
    pub const fn stats(&self) -> BridgeStats {
        self.stats
    }

    /// Consumes at most `batch` new events. Synchronous, so a test can drive it without a
    /// runtime.
    ///
    /// A log error ends the poll early and is returned in [`PollReport::error`], never as `Err`:
    /// the bridge keeps its position and the next poll retries.
    ///
    /// # Errors
    /// [`BridgeError::Query`] if the timeline is unavailable. That is fatal: the lock is
    /// poisoned for good, and the event in progress may be partly appended.
    pub fn poll_once(&mut self) -> Result<PollReport, BridgeError> {
        let mut report = PollReport::default();
        let events = match self.reader.read_after(self.last) {
            Ok(events) => events,
            Err(error) => {
                report.error = Some(error);
                return Ok(report);
            }
        };
        for item in events.take(self.config.batch) {
            let stored = match item {
                Ok(stored) => stored,
                Err(error) => {
                    report.error = Some(error);
                    break;
                }
            };
            report.stats.consumed += 1;
            let engines = self.registry.engines_for(&stored.event.source);
            if engines.is_empty() {
                report.stats.unrouted += 1;
                if self.warned_unrouted.insert(stored.event.source.clone()) {
                    eprintln!(
                        "s2w: bridge: no System 1 engine is routed for source '{}'; its events are skipped",
                        stored.event.source.as_str()
                    );
                }
            }
            let at = stored.event.received_at;
            for record in evaluate_stored(&stored, &engines) {
                match record.verdict {
                    Verdict::Propose { claims, .. } => {
                        if claims.is_empty() {
                            report.stats.proposed_empty += 1;
                        }
                        for claim in claims {
                            self.state.append(at, claim)?;
                            report.stats.proposed_claims += 1;
                        }
                    }
                    Verdict::Abstain { reason } => match reason {
                        AbstainReason::NotMine => report.stats.abstained.not_mine += 1,
                        AbstainReason::Unparseable(_) => report.stats.abstained.unparseable += 1,
                        AbstainReason::Insufficient(_) => {
                            report.stats.abstained.insufficient += 1;
                        }
                        AbstainReason::Panicked(message) => {
                            report.stats.engine_panics += 1;
                            eprintln!(
                                "s2w: bridge: engine '{}' panicked at log position {}: {message}",
                                record.engine,
                                record.position.as_u64()
                            );
                        }
                    },
                }
            }
            self.last = Some(stored.position);
        }
        if let Some(error) = &report.error {
            eprintln!("s2w: bridge: reading the log failed, will retry: {error}");
        }
        self.stats.add(&report.stats);
        Ok(report)
    }
}

impl<R: LogReader + Send + 'static> Bridge<R> {
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
