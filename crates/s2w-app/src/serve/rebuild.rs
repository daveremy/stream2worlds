//! Live rebuild on a mapping change (s2w#184, decision 0023 "Rebuild").
//!
//! After every bridge poll, [`Rebuild::after_poll`] reads the proposal store's watermark. When
//! it moved, the routes are resolved again; when the resolved registry's feed fingerprint
//! differs from the serving one, the world is rebuilt in-process by the start-up sequence:
//! retire the snapshot writer, install an empty timeline under the new epoch (the old world is
//! dropped first, so peak memory is one world, never two), `snapshots::prepare` (restore the
//! new fingerprint's newest valid snapshot, or none), then [`Bridge::restart`] from position 0
//! or from the snapshot. The whole swap runs synchronously between two polls on the
//! current-thread runtime, so no request observes a half-swapped state.
//!
//! The change test is fingerprint inequality, the same value the epoch and snapshot rule 4
//! use, so the three never disagree about whether what feeds the world changed.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use s2w_log::{LogReader, VerdictStore};
use s2w_model::SourceId;

use super::snapshots::{self, SnapshotConfig, Snapshotter};
use crate::bridge::{Bridge, BridgeError, EngineRegistry, PollReport};
use crate::query::{Epoch, QueryState, Rebuilding, Timeline};
use crate::routes::{self, Resolution, Watermark};
use crate::{AppError, NoteSink, Reporter};

/// Watches the proposal store for a change in what feeds the world.
pub(super) struct RouteWatcher {
    log_dir: PathBuf,
    seen: Watermark,
    feed: u64,
    resolution: Resolution,
    /// The last read failure noted, so a failing store is reported once, not every poll.
    failing: Option<String>,
}

/// A resolved change of routes: the registry to rebuild under.
pub(super) struct Change {
    registry: EngineRegistry,
    resolution: Resolution,
}

impl RouteWatcher {
    /// Resolves `log_dir`'s routes at start-up, reporting each route line, and returns the
    /// registry to serve plus a watcher that starts from what was just read. The watermark is
    /// read before the rows, so a proposal written in between is seen on the first poll.
    /// `learn` runs once on the first resolution; when it returns `true` (it filed and accepted
    /// a mapping, decision 0025) the routes are read again before anything is reported.
    ///
    /// # Errors
    /// As [`routes::watermark`], [`routes::load`] and [`routes::registry`]: a store that cannot
    /// be read at start-up stops `serve` before the source connects.
    pub(super) fn start(
        log_dir: PathBuf,
        reporter: &mut dyn Reporter,
        learn: impl FnOnce(&Resolution, &mut dyn Reporter) -> bool,
    ) -> Result<(EngineRegistry, Self), AppError> {
        let mut seen = routes::watermark(&log_dir)?;
        let mut resolution = routes::load(&log_dir)?;
        // `learn` may file and accept a mapping (decision 0025): resolve again, watermark first.
        if learn(&resolution, reporter) {
            seen = routes::watermark(&log_dir)?;
            resolution = routes::load(&log_dir)?;
        }
        for line in routes::report_lines(&resolution) {
            reporter.note(&line);
        }
        let registry = routes::registry(&resolution)?;
        let watcher = Self {
            log_dir,
            seen,
            feed: registry.feed_fingerprint(),
            resolution,
            failing: None,
        };
        Ok((registry, watcher))
    }

    /// Checks the store once: `None` unless the routes now resolve to another feed fingerprint.
    fn check(&mut self, notes: &NoteSink) -> Option<Change> {
        let dir = self.log_dir.clone();
        self.check_with(|| routes::watermark(&dir), || routes::load(&dir), notes)
    }

    /// [`Self::check`] with the two reads injected, so a test can write between them.
    /// Watermark first, then rows: see [`routes::watermark`].
    fn check_with(
        &mut self,
        watermark: impl FnOnce() -> Result<Watermark, AppError>,
        rows: impl FnOnce() -> Result<Resolution, AppError>,
        notes: &NoteSink,
    ) -> Option<Change> {
        let read = watermark().and_then(|mark| {
            if mark == self.seen {
                Ok(None)
            } else {
                rows().map(|resolution| Some((mark, resolution)))
            }
        });
        let (mark, resolution) = match read {
            Ok(changed) => {
                if self.failing.take().is_some() {
                    notes("routes: the proposal store is readable again");
                }
                changed?
            }
            Err(error) => {
                let error = error.to_string();
                if self.failing.as_ref() != Some(&error) {
                    notes(&format!(
                        "routes: cannot read the proposal store: {error}; keeping the current routes"
                    ));
                    self.failing = Some(error);
                }
                return None;
            }
        };
        self.seen = mark;
        let (proposals, decisions) = mark.unwrap_or_default();
        notes(&format!(
            "routes: proposal store changed (proposals <= {}, decisions <= {}); resolving",
            proposals.unwrap_or(0),
            decisions.unwrap_or(0)
        ));
        let registry = match routes::registry(&resolution) {
            Ok(registry) => registry,
            Err(error) => {
                // The watermark has advanced, so this deterministic failure is noted once.
                notes(&format!(
                    "rebuild refused: {error}; serving the previous routes"
                ));
                return None;
            }
        };
        if registry.feed_fingerprint() == self.feed {
            notes("routes: unchanged after proposal store change; no rebuild");
            self.resolution = resolution;
            return None;
        }
        for line in routes::report_lines(&resolution) {
            notes(&line);
        }
        Some(Change {
            registry,
            resolution,
        })
    }
}

/// A [`Reporter`] whose notes go to a [`NoteSink`]: the bridge loop has no reporter of its own
/// (the pump holds it), and `snapshots::prepare` reports through one.
struct Notes(NoteSink);

impl Reporter for Notes {
    fn flushed(&mut self, _: u64, _: u64, _: u64, _: Option<&str>) {}
    fn duplicate(&mut self, _: u64) {}
    fn note(&mut self, message: &str) {
        (self.0)(message);
    }
    fn source_error(&mut self, message: &str, _: bool) {
        (self.0)(message);
    }
    fn note_sink(&self) -> NoteSink {
        self.0.clone()
    }
}

/// A rebuild whose backfill has not caught up yet.
struct InFlight {
    started: Instant,
    identities: Vec<String>,
    /// The routes of the last world whose backfill completed, and the log position it was
    /// served to: a superseding change is compared with these, not with the unfinished one.
    from: (Resolution, u64),
    /// The backfill replays the whole log (no snapshot restored), so its counts cover it all.
    whole_log: bool,
}

/// How often the proposal store is checked at most: each check opens it read-only.
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// The live-rebuild driver `local_bridge` runs after each poll.
pub(super) struct Rebuild {
    watcher: RouteWatcher,
    state: QueryState,
    config: SnapshotConfig,
    snapshotter: Option<std::rc::Rc<std::cell::RefCell<Snapshotter>>>,
    notes: NoteSink,
    in_flight: Option<InFlight>,
    checked: Option<Instant>,
}

impl Rebuild {
    pub(super) const fn new(
        watcher: RouteWatcher,
        state: QueryState,
        config: SnapshotConfig,
        snapshotter: Option<std::rc::Rc<std::cell::RefCell<Snapshotter>>>,
        notes: NoteSink,
    ) -> Self {
        Self {
            watcher,
            state,
            config,
            snapshotter,
            notes,
            in_flight: None,
            checked: None,
        }
    }

    /// Runs right after a successful poll: reports a completed backfill, then checks the
    /// proposal store and swaps in a rebuild when the feed fingerprint changed.
    ///
    /// # Errors
    /// A rebuild that cannot install its timeline, restore, start the snapshot writer or
    /// restart the bridge is fatal, like the same failure at start-up.
    pub(super) fn after_poll<R: LogReader, V: VerdictStore>(
        &mut self,
        bridge: Bridge<R, V>,
        report: &PollReport,
    ) -> Result<Bridge<R, V>, BridgeError> {
        if report.stats.consumed == 0 && report.error.is_none() {
            self.complete(&bridge.stats());
        }
        if self.checked.is_some_and(|at| at.elapsed() < CHECK_EVERY) {
            return Ok(bridge);
        }
        self.checked = Some(Instant::now());
        let Some(change) = self.watcher.check(&self.notes) else {
            return Ok(bridge);
        };
        self.swap(bridge, change)
    }

    fn complete(&mut self, stats: &crate::bridge::BridgeStats) {
        let Some(done) = self.in_flight.take() else {
            return;
        };
        (self.notes)(&format!(
            "rebuild complete: {} events in {:.1} s (replayed {}, evaluated {}, claims {}, abstained {:?})",
            stats.consumed,
            done.started.elapsed().as_secs_f64(),
            stats.replayed,
            stats.evaluated,
            stats.proposed_claims,
            stats.abstained
        ));
        // Only a whole-log backfill's counts say anything about the mapping over the log.
        if done.whole_log && stats.proposed_claims == 0 {
            for identity in &done.identities {
                (self.notes)(&format!(
                    "mapping {identity} produced no claims over {} events (abstained: {:?})",
                    stats.consumed, stats.abstained
                ));
            }
        }
        self.state.publish_rebuilding(BTreeMap::new());
    }

    fn swap<R: LogReader, V: VerdictStore>(
        &mut self,
        bridge: Bridge<R, V>,
        change: Change,
    ) -> Result<Bridge<R, V>, BridgeError> {
        let position = bridge.mark().map_or(0, |(position, _)| position.as_u64());
        let from = match self.in_flight.take() {
            Some(superseded) => {
                (self.notes)(&format!("rebuild: superseded at position {position}"));
                superseded.from
            }
            None => (self.watcher.resolution.clone(), position),
        };
        let (old, new) = (self.watcher.feed, change.registry.feed_fingerprint());
        let (_, head, hub_cap) = self.state.bounds()?;
        (self.notes)(&format!(
            "rebuild: feed {old:016x} -> {new:016x}; dropping the world at offset {head}"
        ));
        if let Some(snapshotter) = &self.snapshotter {
            snapshotter.borrow_mut().retire();
        }
        // The old world goes first: peak memory is one world, never two.
        self.state
            .replace_timeline(Timeline::new(hub_cap).with_epoch(Epoch(new)))?;
        let (resume, snapshotter) = snapshots::prepare(
            &self.state,
            (bridge.reader(), bridge.verdicts() as &dyn VerdictStore),
            &self.watcher.log_dir,
            (self.config, &change.registry),
            &mut Notes(self.notes.clone()),
        )
        .map_err(|error| BridgeError::Task(format!("rebuild failed: {error}")))?;
        if let (Some(cell), Some(snapshotter)) = (&self.snapshotter, snapshotter) {
            *cell.borrow_mut() = snapshotter;
        }
        match resume {
            Some(after) => (self.notes)(&format!(
                "rebuild: resuming after log position {}",
                after.as_u64()
            )),
            None => (self.notes)("rebuild: replaying the log from the start"),
        }
        let rebuilding = rebuilding(&from.0, &change.resolution, from.1);
        let identities = rebuilding.values().map(|r| r.identity.clone()).collect();
        self.state.publish_rebuilding(rebuilding);
        self.in_flight = Some(InFlight {
            started: Instant::now(),
            identities,
            from,
            whole_log: resume.is_none(),
        });
        self.watcher.feed = new;
        self.watcher.resolution = change.resolution;
        bridge.restart(change.registry, resume)
    }
}

/// The sources whose effective mapping changed to a new one: each is rebuilding under it.
/// A source whose mapping was revoked is not listed; it is unrouted again.
fn rebuilding(
    old: &Resolution,
    new: &Resolution,
    since_position: u64,
) -> BTreeMap<SourceId, Rebuilding> {
    new.routes
        .iter()
        .filter(|(source, resolved)| old.routes.get(*source) != Some(*resolved))
        .map(|(source, resolved)| {
            (
                source.clone(),
                Rebuilding {
                    identity: resolved.identity.clone(),
                    since_position,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;
