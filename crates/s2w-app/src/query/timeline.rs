//! The served world log: timestamped [`WorldEvent`]s the query API folds on demand, on top of a
//! base world (empty, or restored from a snapshot, decision 0024).

use std::sync::Arc;

use s2w_core::{World, WorldEvent};
use s2w_model::Timestamp;
use serde::{Deserialize, Serialize};

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
    /// The earliest offset this process can serve: 0, or the offset of the snapshot it was
    /// restored from (decision 0024). Offsets below it answer `offset_before_base`.
    pub base: u64,
    /// The first event's timestamp in milliseconds, if any.
    pub first_ts: Option<i64>,
    /// The last event's timestamp in milliseconds, if any.
    pub last_ts: Option<i64>,
    /// How many appends arrived with a timestamp earlier than the previous event's and were
    /// clamped to it.
    pub clamped: u64,
    /// The history this range belongs to ([`Epoch`]).
    pub epoch: Epoch,
}

/// Which history an offset belongs to: the serving registry's feed fingerprint (decision 0023,
/// amended by PR 2b-i of s2w#184). Two timelines with the same epoch are the same deterministic
/// fold of the same log, so `(epoch, offset)` names one world; the same offset under a different
/// epoch may name a different world and answers `stale_epoch` (410).
///
/// Serialized as 16 lowercase hex digits (a string: 64-bit values do not survive JS numbers).
/// `0` is reserved for "no serving registry" (a fresh [`Timeline::new`], the standalone
/// `s2w mcp` replay).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Epoch(pub u64);

impl std::fmt::Display for Epoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl std::str::FromStr for Epoch {
    type Err = String;

    /// Exactly 16 hex digits, either case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("expected 16 hex digits".to_owned());
        }
        u64::from_str_radix(s, 16)
            .map(Self)
            .map_err(|e| e.to_string())
    }
}

impl Serialize for Epoch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// The time index of the events folded into a base world, carried by a snapshot so a restored
/// timeline answers `/time` exactly as the full history would.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseTime {
    /// The first event's timestamp in milliseconds, if any.
    pub first_ts: Option<i64>,
    /// The last event's timestamp in milliseconds, if any.
    pub last_ts: Option<i64>,
    /// How many appends were clamped.
    pub clamped: u64,
}

/// The world log the query API serves: a base world, the live head world, and the events
/// between them.
///
/// Offsets are fold offsets ([`World::offset`]): offset `n` is the world after the first `n`
/// events. The base is the empty world unless the timeline was restored from a snapshot
/// ([`Timeline::from_snapshot`]); offsets below it are gone ([`QueryError::OffsetBeforeBase`]).
/// The head world is folded once per [`Timeline::append`], so the world at the head is a clone,
/// never a refold.
///
/// Base and head share one world until the first append (#179): a restored timeline holds one
/// copy of the snapshot's world, and the first [`Timeline::append`] copies it into a separate
/// head (`Arc::make_mut`). From then on the timeline holds two worlds, base and head.
#[derive(Debug)]
pub struct Timeline {
    base: Arc<World>,
    base_time: BaseTime,
    head: Arc<World>,
    events: Vec<TimedEvent>,
    clamped: u64,
    epoch: Epoch,
}

impl Timeline {
    /// An empty timeline whose worlds use `hub_cap` as the in-degree cap.
    #[must_use]
    pub fn new(hub_cap: u64) -> Self {
        Self::from_snapshot(World::with_hub_cap(hub_cap), BaseTime::default())
    }

    /// A timeline whose base is `world`, restored from a snapshot, with the base's time index
    /// `time`. Appends continue from `world.offset()`. The head shares the base's world until
    /// the first append, so restoring holds one copy of it, not two.
    #[must_use]
    pub fn from_snapshot(world: World, time: BaseTime) -> Self {
        let world = Arc::new(world);
        Self {
            head: Arc::clone(&world),
            base: world,
            base_time: time,
            events: Vec::new(),
            clamped: time.clamped,
            epoch: Epoch::default(),
        }
    }

    /// This timeline, serving history `epoch`. [`Timeline::new`] and [`Timeline::from_snapshot`]
    /// start at epoch 0; `serve` sets the registry's feed fingerprint before anything is served.
    #[must_use]
    pub fn with_epoch(mut self, epoch: Epoch) -> Self {
        self.epoch = epoch;
        self
    }

    /// The history this timeline serves.
    #[must_use]
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// `Ok` when `supplied` is absent (the caller opted out) or equals this timeline's epoch;
    /// otherwise [`QueryError::StaleEpoch`]. Every read that resolves a client offset calls this
    /// first, before any bounds check, so a stale client gets 410 rather than a bounds error
    /// about another history's offsets.
    ///
    /// # Errors
    /// [`QueryError::StaleEpoch`] when the epochs differ.
    pub fn check_epoch(&self, supplied: Option<Epoch>) -> Result<(), QueryError> {
        match supplied {
            Some(supplied) if supplied != self.epoch => Err(QueryError::StaleEpoch {
                supplied,
                current: self.epoch,
            }),
            _ => Ok(()),
        }
    }

    /// The in-degree cap every world on this timeline is folded under.
    #[must_use]
    pub fn hub_cap(&self) -> u64 {
        self.base.hub_in_degree_cap()
    }

    /// The earliest servable offset: 0, or the snapshot's offset.
    #[must_use]
    pub fn base(&self) -> u64 {
        self.base.offset()
    }

    /// The latest offset.
    #[must_use]
    pub fn head(&self) -> u64 {
        self.head.offset()
    }

    /// The world at the head, without folding.
    #[must_use]
    pub fn head_world(&self) -> &World {
        &self.head
    }

    /// The events strictly after `offset`, oldest first.
    ///
    /// # Errors
    /// [`QueryError::OffsetBeforeBase`] below the base; [`QueryError::OffsetBeyondHead`] past
    /// the head.
    pub fn events_after(&self, offset: u64) -> Result<&[TimedEvent], QueryError> {
        let start = self.index(offset)?;
        Ok(self.events.get(start..).unwrap_or_default())
    }

    /// Appends an event, folds it into the head world, and returns the new head. Never refuses
    /// an event: a timestamp earlier than the previous event's (or than the base's last, for
    /// the first event after a snapshot) is clamped to it and counted, so the time index stays
    /// sorted without dropping a world event.
    pub fn append(&mut self, at: Timestamp, event: WorldEvent) -> u64 {
        let previous = self
            .events
            .last()
            .map(|last| last.at)
            .or_else(|| self.base_time.last_ts.map(Timestamp::from_millis));
        let at = match previous {
            Some(previous) if at < previous => {
                self.clamped = self.clamped.saturating_add(1);
                previous
            }
            _ => at,
        };
        // Copies the world only while the head still shares it with the base: the first append
        // after a restore (or on a new timeline, whose world is empty).
        let head = Arc::make_mut(&mut self.head);
        *head = s2w_core::fold_one(std::mem::take(head), &event);
        self.events.push(TimedEvent { at, event });
        self.head()
    }

    /// The index into `events` for `offset`: `offset - base`, checked against both ends.
    fn index(&self, offset: u64) -> Result<usize, QueryError> {
        let (base, head) = (self.base(), self.head());
        if offset < base {
            return Err(QueryError::OffsetBeforeBase { at: offset, base });
        }
        if offset > head {
            return Err(QueryError::OffsetBeyondHead { at: offset, head });
        }
        usize::try_from(offset - base)
            .map_err(|_| QueryError::OffsetBeyondHead { at: offset, head })
    }

    /// The events from the base up to `offset`.
    fn prefix(&self, offset: u64) -> Result<&[TimedEvent], QueryError> {
        let end = self.index(offset)?;
        Ok(self.events.get(..end).unwrap_or_default())
    }

    /// The world at `offset`.
    ///
    /// # Errors
    /// [`QueryError::OffsetBeforeBase`] below the base; [`QueryError::OffsetBeyondHead`] past
    /// the head.
    pub fn world_at(&self, offset: u64) -> Result<World, QueryError> {
        let prefix = self.prefix(offset)?;
        if offset == self.head() {
            return Ok(World::clone(&self.head));
        }
        Ok(s2w_core::fold(
            World::clone(&self.base),
            prefix.iter().map(|e| &e.event),
        ))
    }

    /// The largest offset whose events were all received at or before `ts`; 0 if none was.
    ///
    /// # Errors
    /// [`QueryError::TimeBeforeBase`] when `ts` is before the base's last event: the answer
    /// lies inside the snapshot, whose per-event times are not kept.
    pub fn offset_at(&self, ts: Timestamp) -> Result<u64, QueryError> {
        if let Some(last) = self.base_time.last_ts
            && ts.as_millis() < last
        {
            return Err(QueryError::TimeBeforeBase {
                ts: ts.as_millis(),
                base: self.base(),
            });
        }
        let n = self.events.partition_point(|e| e.at <= ts);
        Ok(self
            .base()
            .saturating_add(u64::try_from(n).unwrap_or(u64::MAX)))
    }

    /// The time index's range.
    #[must_use]
    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            head: self.head(),
            base: self.base(),
            first_ts: self
                .base_time
                .first_ts
                .or_else(|| self.events.first().map(|e| e.at.as_millis())),
            last_ts: self
                .events
                .last()
                .map(|e| e.at.as_millis())
                .or(self.base_time.last_ts),
            clamped: self.clamped,
            epoch: self.epoch,
        }
    }

    /// The time index of everything folded so far: what a snapshot of the head records.
    #[must_use]
    pub fn head_time(&self) -> BaseTime {
        let range = self.time_range();
        BaseTime {
            first_ts: range.first_ts,
            last_ts: range.last_ts,
            clamped: range.clamped,
        }
    }

    /// Every delta after the base and up to `to` that names entity `id`, or names an id that
    /// resolved to it at that moment (so events on a merged-away alias appear on the survivor
    /// while merged).
    ///
    /// # Errors
    /// [`QueryError::OffsetBeforeBase`] below the base; [`QueryError::OffsetBeyondHead`] past
    /// the head.
    pub fn history(&self, id: u64, to: u64) -> Result<Vec<HistoryEntry>, QueryError> {
        let prefix = self.prefix(to)?;
        let mut world = World::clone(&self.base);
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
        if world.entity_id(id).is_some() {
            Ok(out)
        } else {
            Err(QueryError::UnknownEntity { id })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use s2w_core::{NaturalKey, World, WorldEvent};
    use s2w_model::Timestamp;

    use super::{BaseTime, Timeline};

    fn observed(key: &str) -> WorldEvent {
        WorldEvent::EntityObserved {
            key: NaturalKey::new(key.to_owned()),
            entity_type: "thing".to_owned(),
            attrs: BTreeMap::new(),
        }
    }

    /// #179: a restored timeline holds one world until its first append, which copies it into
    /// a separate head and leaves the base as restored.
    #[test]
    fn base_and_head_share_one_world_until_the_first_append() -> Result<(), super::QueryError> {
        let restored = s2w_core::fold(World::with_hub_cap(8), &[observed("a")]);
        let mut timeline = Timeline::from_snapshot(restored.clone(), BaseTime::default());
        assert!(Arc::ptr_eq(&timeline.base, &timeline.head));

        timeline.append(Timestamp::from_millis(1), observed("b"));
        assert!(!Arc::ptr_eq(&timeline.base, &timeline.head));
        assert_eq!(*timeline.base, restored);
        assert_eq!(timeline.world_at(timeline.base())?, restored);
        assert_eq!((timeline.base(), timeline.head()), (1, 2));

        // Later appends fold the head in place: nothing else holds it.
        timeline.append(Timestamp::from_millis(2), observed("c"));
        assert_eq!(Arc::strong_count(&timeline.head), 1);
        assert_eq!(timeline.head(), 3);
        Ok(())
    }
}
