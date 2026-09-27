//! The served world log: timestamped [`WorldEvent`]s the query API folds on demand.

use s2w_core::{World, WorldEvent};
use s2w_model::Timestamp;
use serde::Serialize;

use super::QueryError;
use super::delta::{Delta, fold_with_delta};

/// One world event and the time it was received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedEvent {
    /// When the event was received. Never earlier than the previous event's (see
    /// [`Timeline::append`]).
    pub at: Timestamp,
    /// The event.
    pub event: WorldEvent,
}

/// One entry of an entity's history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryEntry {
    /// The world offset after the event.
    pub offset: u64,
    /// What the event did.
    pub delta: Delta,
}

/// Summary of the time index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TimeRange {
    /// The latest offset.
    pub head: u64,
    /// The first event's timestamp in milliseconds, if any.
    pub first_ts: Option<i64>,
    /// The last event's timestamp in milliseconds, if any.
    pub last_ts: Option<i64>,
    /// How many appends arrived with a timestamp earlier than the previous event's and were
    /// clamped to it.
    pub clamped: u64,
}

/// The world log the query API serves, folded from an empty world with a fixed hub cap.
///
/// Offsets are fold offsets ([`World::offset`]): offset `n` is the world after the first `n`
/// events. This is a stand-in for the event log until snapshots map fold offsets to log
/// positions (#33, decision 0006).
#[derive(Clone, Debug)]
pub struct Timeline {
    hub_cap: u64,
    events: Vec<TimedEvent>,
    clamped: u64,
}

impl Timeline {
    /// An empty timeline whose worlds use `hub_cap` as the in-degree cap.
    #[must_use]
    pub const fn new(hub_cap: u64) -> Self {
        Self {
            hub_cap,
            events: Vec::new(),
            clamped: 0,
        }
    }

    /// The in-degree cap every world on this timeline is folded under.
    #[must_use]
    pub const fn hub_cap(&self) -> u64 {
        self.hub_cap
    }

    /// The latest offset: the number of events.
    #[must_use]
    pub fn head(&self) -> u64 {
        u64::try_from(self.events.len()).unwrap_or(u64::MAX)
    }

    /// The events, oldest first.
    #[must_use]
    pub fn events(&self) -> &[TimedEvent] {
        &self.events
    }

    /// Appends an event and returns the new head. Never refuses an event: a timestamp earlier
    /// than the previous event's is clamped to it (and counted), so the time index stays
    /// sorted without dropping a world event.
    pub fn append(&mut self, at: Timestamp, event: WorldEvent) -> u64 {
        let at = match self.events.last() {
            Some(last) if at < last.at => {
                self.clamped = self.clamped.saturating_add(1);
                last.at
            }
            _ => at,
        };
        self.events.push(TimedEvent { at, event });
        self.head()
    }

    /// The events before `offset`, or an error if `offset` is past the head.
    fn prefix(&self, offset: u64) -> Result<&[TimedEvent], QueryError> {
        let head = self.head();
        usize::try_from(offset)
            .ok()
            .and_then(|n| self.events.get(..n))
            .ok_or(QueryError::OffsetBeyondHead { at: offset, head })
    }

    /// The world at `offset`.
    ///
    /// # Errors
    /// [`QueryError::OffsetBeyondHead`] if `offset` is past the head.
    pub fn world_at(&self, offset: u64) -> Result<World, QueryError> {
        let prefix = self.prefix(offset)?;
        Ok(s2w_core::fold(
            World::with_hub_cap(self.hub_cap),
            prefix.iter().map(|e| &e.event),
        ))
    }

    /// The largest offset whose events were all received at or before `ts`; 0 if none was.
    #[must_use]
    pub fn offset_at(&self, ts: Timestamp) -> u64 {
        let n = self.events.partition_point(|e| e.at <= ts);
        u64::try_from(n).unwrap_or(u64::MAX)
    }

    /// The time index's range.
    #[must_use]
    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            head: self.head(),
            first_ts: self.events.first().map(|e| e.at.as_millis()),
            last_ts: self.events.last().map(|e| e.at.as_millis()),
            clamped: self.clamped,
        }
    }

    /// Every delta up to `to` that names entity `id`, or names an id that resolved to it at
    /// that moment (so events on a merged-away alias appear on the survivor while merged).
    ///
    /// # Errors
    /// [`QueryError::OffsetBeyondHead`] if `to` is past the head.
    pub fn history(&self, id: u64, to: u64) -> Result<Vec<HistoryEntry>, QueryError> {
        let prefix = self.prefix(to)?;
        let mut world = World::with_hub_cap(self.hub_cap);
        let mut out = Vec::new();
        for timed in prefix {
            let (next, delta) = fold_with_delta(world, &timed.event);
            let touches = delta
                .entities()
                .into_iter()
                .any(|e| e.get() == id || next.resolve(e).get() == id);
            if touches {
                out.push(HistoryEntry {
                    offset: next.offset(),
                    delta,
                });
            }
            world = next;
        }
        if world.entities().keys().any(|e| e.get() == id) {
            Ok(out)
        } else {
            Err(QueryError::UnknownEntity { id })
        }
    }
}
