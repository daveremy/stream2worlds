//! Per-source bridge telemetry the sources view reads (s2w#240: it lives in `query`, the
//! bottom of the crate, so `query::http` does not depend on `bridge`). The bridge writes it
//! through `QueryState::publish_source_stats`.

use std::collections::VecDeque;

use s2w_log::StoredEvent;

/// How many of a source's most recent unrouted events are kept for the sources view, so a
/// viewer can see the stream is alive however long it runs.
pub const RECENT_UNROUTED_CAP: usize = 20;

/// What the bridge did with one source's events: [`crate::bridge::BridgeStats`]'s aggregates,
/// broken out per source so the query API can name an unrouted stream instead of saying nothing
/// arrived.
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
    /// [`crate::bridge::Bridge::absorb_source_stats`] uses it to fold a committed batch into the
    /// bridge's running totals.
    pub(crate) fn add(&mut self, other: &Self) {
        self.consumed += other.consumed;
        self.unrouted += other.unrouted;
        for event in other.recent_unrouted.iter().cloned() {
            self.push_recent_unrouted(event);
        }
    }

    /// Pushes one more unrouted event, evicting the oldest until the ring is back at
    /// [`RECENT_UNROUTED_CAP`]. The one place the cap invariant lives: [`Self::add`] and the
    /// bridge's per-event judging are its callers.
    pub(crate) fn push_recent_unrouted(&mut self, event: StoredEvent) {
        self.recent_unrouted.push_back(event);
        while self.recent_unrouted.len() > RECENT_UNROUTED_CAP {
            self.recent_unrouted.pop_front();
        }
    }
}
