//! The in-run trigger (s2w#197 PR 4b, decision 0025). A source that was unrouted at start and
//! had not yet filled its window is profiled once, after the bridge poll in which the log's
//! count of its events reaches the window. The rows land at once; the routes change at the next
//! start, or live once a rebuild watcher (s2w#184 2b-ii) sees the store move.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use s2w_log::{LogPosition, LogReader, SqliteEventLog};
use s2w_model::SourceId;

use super::{DiscoverConfig, Ran};
use crate::{NoteSink, Reporter};

/// A [`Reporter`] that only notes, through a [`NoteSink`]: the bridge loop's voice while the
/// pump holds `serve`'s reporter.
pub(crate) struct SinkReporter(pub(crate) NoteSink);

impl Reporter for SinkReporter {
    fn flushed(&mut self, _: u64, _: u64, _: u64, _: Option<&str>) {}
    fn duplicate(&mut self, _: u64) {}
    fn note(&mut self, message: &str) {
        (self.0)(message);
    }
    fn source_error(&mut self, message: &str, _: bool) {
        (self.0)(message);
    }
    fn wants_ticker(&self) -> bool {
        false
    }
    fn note_sink(&self) -> NoteSink {
        self.0.clone()
    }
}

/// What `serve` hands the bridge loop after the start-up producer pass.
#[derive(Debug)]
pub(crate) struct InRun {
    log_dir: PathBuf,
    cfg: DiscoverConfig,
    /// Sources still waiting for their window.
    pending: BTreeSet<SourceId>,
    /// Logged events per pending source, counted up to `scanned`.
    counts: BTreeMap<SourceId, usize>,
    /// The last log position counted; `None` before the first count.
    scanned: Option<LogPosition>,
}

/// The start-up pass's result, before `serve` knows its sources.
#[derive(Debug)]
pub(crate) struct Seed {
    pub(crate) log_dir: PathBuf,
    pub(crate) cfg: DiscoverConfig,
    /// Sources routed at start, or whose window the start-up pass already saw.
    pub(crate) settled: BTreeSet<SourceId>,
}

impl Seed {
    /// The trigger for `sources` minus the settled ones; `None` when nothing is left to wait
    /// for, so a routed demo source costs the bridge loop nothing.
    pub(crate) fn arm(self, sources: &[SourceId]) -> Option<InRun> {
        let pending: BTreeSet<SourceId> = sources
            .iter()
            .filter(|source| !self.settled.contains(*source))
            .cloned()
            .collect();
        (self.cfg.window > 0 && !pending.is_empty()).then(|| InRun {
            log_dir: self.log_dir,
            cfg: self.cfg,
            pending,
            counts: BTreeMap::new(),
            scanned: None,
        })
    }
}

impl InRun {
    /// Whether any source is still waiting. The bridge loop drops the trigger once none is.
    pub(crate) fn is_done(&self) -> bool {
        self.pending.is_empty()
    }

    /// Counts the events logged since the last call and runs the producer once for each pending
    /// source whose count reached the window. A source whose rows met a held writer lock stays
    /// pending and is tried again after the next poll. A log read error is a note and ends the
    /// trigger for this process.
    pub(crate) fn after_poll(&mut self, log: &SqliteEventLog, reporter: &mut dyn Reporter) {
        if let Err(error) = self.count(log) {
            reporter.note(&format!(
                "discover: reading the log failed: {error}; in-run discovery stops until the next start"
            ));
            self.pending.clear();
            return;
        }
        let full: Vec<SourceId> = self
            .pending
            .iter()
            .filter(|source| self.counts.get(*source).copied().unwrap_or(0) >= self.cfg.window)
            .cloned()
            .collect();
        for source in full {
            let ran: Ran = super::run_one(log, &self.log_dir, &source, &self.cfg, reporter);
            if !ran.locked.contains(&source) {
                self.pending.remove(&source);
            }
        }
    }

    fn count(&mut self, log: &SqliteEventLog) -> Result<(), s2w_log::LogError> {
        for stored in log.read_after(self.scanned)? {
            let stored = stored?;
            self.scanned = Some(stored.position);
            if self.pending.contains(&stored.event.source) {
                *self.counts.entry(stored.event.source).or_default() += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
