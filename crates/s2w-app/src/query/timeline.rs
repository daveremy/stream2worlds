//! The served world log: the head world and a bounded window of recent timestamped
//! [`WorldEvent`]s, each with the [`Delta`] it made (decisions 0024 and 0026).

use std::borrow::Cow;

use s2w_core::{World, WorldEvent};
use s2w_model::Timestamp;
use serde::{Deserialize, Serialize};

use super::QueryError;
use super::delta::{Delta, fold_with_delta};
use super::epoch::Epoch;

/// How many recent world events a [`Timeline`] keeps by default (decision 0026): about 12 MiB
/// on the recorded fixture, and about 270 raw events there at ~75 world events per raw event.
/// Set by the measured 600 MiB serve budget (`tests/backfill_memory.rs`): 100,000 held
/// 631 MiB at the peak, 20,000 holds 588 MiB.
pub const DEFAULT_HISTORY_CAP: usize = 20_000;

/// One world event, the time it was received, and what it did to the world. Held in memory
/// only: never serialized or persisted (snapshots store the world and its [`BaseTime`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedEvent {
    /// When the event was received. Never earlier than the previous event's (see
    /// [`Timeline::append`]).
    pub at: Timestamp,
    /// The event.
    pub event: WorldEvent,
    /// What the event did, computed once when it was folded into the head, so a follower
    /// replays deltas without a world of its own.
    pub delta: Delta,
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
    /// The earliest offset world queries (`/world?at`, `/diff`, `/entity/{id}/history`,
    /// `/time?ts`) can serve: 0 while the timeline holds every event since offset 0, else the
    /// head (decision 0026). Offsets below it answer `offset_before_base`.
    pub base: u64,
    /// The earliest offset `/events` can replay from: the offset before the oldest retained
    /// event (decision 0026). Never above `base`.
    pub replay_base: u64,
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

/// The world log the query API serves: the live head world and at most `history_cap` recent
/// events before it, each with its delta (decision 0026, amending 0024).
///
/// Offsets are fold offsets ([`World::offset`]): offset `n` is the world after the first `n`
/// events. The timeline holds exactly one world, the head, folded once per
/// [`Timeline::append`]. It never holds a base world: while every event since offset 0 is
/// retained (`full_history`), an older world is refolded from the empty world on demand; once
/// the oldest events are dropped, or after a restore from a snapshot above offset 0, world
/// queries are served at the head only and anything older is
/// [`QueryError::OffsetBeforeBase`]. `/events` replays the retained window from its stored
/// deltas ([`Timeline::replay_base`]).
///
/// When more than `history_cap` events are retained, the oldest are dropped down to
/// `history_cap / 2`, so the drop is amortized O(1) per append and the window a follower can
/// resume from is at least `history_cap / 2` events.
#[derive(Debug)]
pub struct Timeline {
    head: World,
    /// Every event since offset 0 is retained, so the empty world plus `events` is any world.
    full_history: bool,
    /// The offset before `events[0]`.
    replay_base: u64,
    /// The time index of the events before `replay_base`: dropped ones, or a snapshot's.
    base_time: BaseTime,
    events: Vec<TimedEvent>,
    history_cap: usize,
    clamped: u64,
    epoch: Epoch,
}

impl Timeline {
    /// An empty timeline whose worlds use `hub_cap` as the in-degree cap.
    #[must_use]
    pub fn new(hub_cap: u64) -> Self {
        Self::from_snapshot(World::with_hub_cap(hub_cap), BaseTime::default())
    }

    /// A timeline whose head is `world`, restored from a snapshot, with the snapshot's time
    /// index `time`, keeping [`DEFAULT_HISTORY_CAP`] events. Appends continue from
    /// `world.offset()`. A world above offset 0 starts without full history: world queries are
    /// served at the head only (decision 0026).
    #[must_use]
    pub fn from_snapshot(world: World, time: BaseTime) -> Self {
        let offset = world.offset();
        Self {
            head: world,
            full_history: offset == 0,
            replay_base: offset,
            base_time: time,
            events: Vec::new(),
            history_cap: DEFAULT_HISTORY_CAP,
            clamped: time.clamped,
            epoch: Epoch::default(),
        }
    }

    /// This timeline, keeping at most `cap` recent events (at least 2). Applies from the next
    /// [`Timeline::append`].
    #[must_use]
    pub fn with_history_cap(mut self, cap: usize) -> Self {
        self.history_cap = cap.max(2);
        self
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
        self.head.hub_in_degree_cap()
    }

    /// The earliest offset world queries serve: 0 while every event since offset 0 is
    /// retained, else the head (decision 0026).
    #[must_use]
    pub fn base(&self) -> u64 {
        if self.full_history { 0 } else { self.head() }
    }

    /// The earliest offset [`Timeline::events_after`] serves: the offset before the oldest
    /// retained event.
    #[must_use]
    pub fn replay_base(&self) -> u64 {
        self.replay_base
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
    /// [`QueryError::OffsetBeforeBase`] below [`Timeline::replay_base`];
    /// [`QueryError::OffsetBeyondHead`] past the head.
    pub fn events_after(&self, offset: u64) -> Result<&[TimedEvent], QueryError> {
        let start = self.index(offset, self.replay_base)?;
        Ok(self.events.get(start..).unwrap_or_default())
    }

    /// Appends an event, folds it into the head world, and returns the new head. Never refuses
    /// an event: a timestamp earlier than the previous event's (or than the base's last, for
    /// the first event after a snapshot) is clamped to it and counted, so the time index stays
    /// sorted without dropping a world event. Past the history cap, drops the oldest events
    /// down to half the cap.
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
        let (next, delta) = fold_with_delta(std::mem::take(&mut self.head), &event);
        self.head = next;
        self.events.push(TimedEvent { at, event, delta });
        if self.events.len() > self.history_cap {
            self.drop_oldest(self.events.len() - self.history_cap / 2);
        }
        self.head()
    }

    /// Drops the oldest `n` retained events, folding their time range into `base_time`.
    fn drop_oldest(&mut self, n: usize) {
        let (Some(first), Some(last)) = (
            self.events.first(),
            n.checked_sub(1).and_then(|i| self.events.get(i)),
        ) else {
            return;
        };
        self.base_time.first_ts = self.base_time.first_ts.or(Some(first.at.as_millis()));
        self.base_time.last_ts = Some(last.at.as_millis());
        self.events.drain(..n);
        self.replay_base = self
            .replay_base
            .saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        self.full_history = false;
    }

    /// The index into `events` for `offset`, checked against `base` and the head.
    fn index(&self, offset: u64, base: u64) -> Result<usize, QueryError> {
        // Callers pass a floor at or above the window's start, so the subtraction cannot wrap.
        debug_assert!(base >= self.replay_base, "index floor below replay_base");
        let head = self.head();
        if offset < base {
            return Err(QueryError::OffsetBeforeBase { at: offset, base });
        }
        if offset > head {
            return Err(QueryError::OffsetBeyondHead { at: offset, head });
        }
        usize::try_from(offset - self.replay_base)
            .map_err(|_| QueryError::OffsetBeyondHead { at: offset, head })
    }

    /// The retained events up to `offset`, checked against the world base.
    fn prefix(&self, offset: u64) -> Result<&[TimedEvent], QueryError> {
        let end = self.index(offset, self.base())?;
        Ok(self.events.get(..end).unwrap_or_default())
    }

    /// The world every retained event folds from. Only called with full history, where it is
    /// the empty world.
    fn empty_world(&self) -> World {
        World::with_hub_cap(self.hub_cap())
    }

    /// The world at `offset`: the head itself, borrowed, at the head (never a copy of it,
    /// #216), or a fresh fold of the retained events below it.
    ///
    /// # Errors
    /// [`QueryError::OffsetBeforeBase`] below the base; [`QueryError::OffsetBeyondHead`] past
    /// the head.
    pub fn world_at(&self, offset: u64) -> Result<Cow<'_, World>, QueryError> {
        let prefix = self.prefix(offset)?;
        if offset == self.head() {
            return Ok(Cow::Borrowed(self.head_world()));
        }
        // Below the head, `prefix` succeeded only with full history (the base is the head
        // otherwise), so the retained events fold from the empty world.
        Ok(Cow::Owned(s2w_core::fold(
            self.empty_world(),
            prefix.iter().map(|e| &e.event),
        )))
    }

    /// Checks that `offset` is one [`Timeline::world_at`] serves, without building its world.
    ///
    /// # Errors
    /// Exactly the errors [`Timeline::world_at`] returns for `offset`.
    pub fn check_offset(&self, offset: u64) -> Result<(), QueryError> {
        self.prefix(offset).map(|_| ())
    }

    /// The largest offset whose events were all received at or before `ts`; 0 if none was.
    ///
    /// # Errors
    /// [`QueryError::TimeBeforeBase`] when the answer is an offset world queries cannot serve:
    /// `ts` is before the last dropped or snapshotted event (whose per-event times are not
    /// kept) or, without full history, before the newest event.
    pub fn offset_at(&self, ts: Timestamp) -> Result<u64, QueryError> {
        let before = || QueryError::TimeBeforeBase {
            ts: ts.as_millis(),
            base: self.base(),
        };
        if !self.full_history {
            let last = self.events.last().map(|e| e.at.as_millis());
            return match last.or(self.base_time.last_ts) {
                Some(last) if ts.as_millis() < last => Err(before()),
                _ => Ok(self.head()),
            };
        }
        if let Some(last) = self.base_time.last_ts
            && ts.as_millis() < last
        {
            return Err(before());
        }
        let n = self.events.partition_point(|e| e.at <= ts);
        Ok(self
            .replay_base
            .saturating_add(u64::try_from(n).unwrap_or(u64::MAX)))
    }

    /// The time index's range.
    #[must_use]
    pub fn time_range(&self) -> TimeRange {
        TimeRange {
            head: self.head(),
            base: self.base(),
            replay_base: self.replay_base,
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

    /// Every delta up to `to` that names entity `id`, or names an id that resolved to it at
    /// that moment (so events on a merged-away alias appear on the survivor while merged).
    ///
    /// # Errors
    /// [`QueryError::OffsetBeforeBase`] without full history (an entity's history needs every
    /// event since offset 0, decision 0026) or below the base;
    /// [`QueryError::OffsetBeyondHead`] past the head.
    pub fn history(&self, id: u64, to: u64) -> Result<Vec<HistoryEntry>, QueryError> {
        let prefix = self.prefix(to)?;
        if !self.full_history {
            return Err(QueryError::OffsetBeforeBase {
                at: to,
                base: self.head(),
            });
        }
        let mut world = self.empty_world();
        let mut out = Vec::new();
        for timed in prefix {
            // The stored delta is the one this fold would compute; reuse it.
            let next = s2w_core::fold_one(world, &timed.event);
            let touches = timed
                .delta
                .entities()
                .into_iter()
                .any(|e| e.get() == id || next.resolve(e).get() == id);
            if touches {
                out.push(HistoryEntry {
                    offset: next.offset(),
                    delta: timed.delta.clone(),
                });
            }
            world = next;
        }
        if world.minted_id(id).is_some() {
            Ok(out)
        } else {
            Err(QueryError::UnknownEntity { id })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use s2w_core::{NaturalKey, World, WorldEvent};
    use s2w_model::Timestamp;

    use super::{BaseTime, QueryError, Timeline, fold_with_delta};

    fn observed(key: &str) -> WorldEvent {
        WorldEvent::EntityObserved {
            key: NaturalKey::new(key.to_owned()),
            entity_type: "thing".to_owned(),
            attrs: BTreeMap::new(),
        }
    }

    /// A log mixing new entities, repeats, relationships and merges.
    fn log(n: usize) -> Vec<WorldEvent> {
        (0..n)
            .map(|i| match i % 4 {
                0 | 1 => observed(&format!("k{}", i % 7)),
                2 => WorldEvent::RelationshipObserved {
                    from: NaturalKey::new(format!("k{}", i % 7)),
                    to: NaturalKey::new(format!("k{}", (i + 3) % 7)),
                    kind: "on".to_owned(),
                },
                _ => WorldEvent::EntitiesMerged {
                    survivor: NaturalKey::new(format!("k{}", i % 5)),
                    absorbed: NaturalKey::new(format!("k{}", (i + 1) % 5)),
                },
            })
            .collect()
    }

    /// Decision 0026: a restored timeline holds one world, the head, and serves world queries
    /// at the head only; `/events` replays what arrived after the restore.
    #[test]
    fn a_restored_timeline_holds_only_the_head_world() -> Result<(), QueryError> {
        let restored = s2w_core::fold(World::with_hub_cap(8), &[observed("a")]);
        let mut timeline = Timeline::from_snapshot(restored.clone(), BaseTime::default());
        assert_eq!(
            (timeline.base(), timeline.replay_base(), timeline.head()),
            (1, 1, 1)
        );
        assert_eq!(*timeline.world_at(1)?, restored);

        timeline.append(Timestamp::from_millis(1), observed("b"));
        timeline.append(Timestamp::from_millis(2), observed("c"));
        assert_eq!(
            (timeline.base(), timeline.replay_base(), timeline.head()),
            (3, 1, 3)
        );
        assert_eq!(
            timeline.world_at(2),
            Err(QueryError::OffsetBeforeBase { at: 2, base: 3 })
        );
        assert_eq!(timeline.events_after(1)?.len(), 2);
        assert_eq!(
            timeline.history(0, 3),
            Err(QueryError::OffsetBeforeBase { at: 3, base: 3 })
        );
        Ok(())
    }

    /// The stored deltas are the ones a follower folding from the empty world computes, and
    /// folding with a delta yields the same world as `fold_one`.
    #[test]
    fn stored_deltas_match_a_reference_fold() {
        let events = log(40);
        let mut timeline = Timeline::new(3);
        let mut world = World::with_hub_cap(3);
        for (i, event) in events.iter().enumerate() {
            timeline.append(
                Timestamp::from_millis(i64::try_from(i).unwrap_or(0)),
                event.clone(),
            );
            let plain = s2w_core::fold_one(world.clone(), event);
            let (next, delta) = fold_with_delta(world, event);
            assert_eq!(next, plain);
            assert_eq!(timeline.events.last().map(|e| &e.delta), Some(&delta));
            world = next;
        }
        assert_eq!(timeline.head, world);
    }

    /// Every offset `capped` still retains replays exactly what `full` replays; before the
    /// first drop, every world query matches too.
    fn assert_window_matches(capped: &Timeline, full: &Timeline) -> Result<(), QueryError> {
        assert_eq!(capped.head_world(), full.head_world());
        for o in capped.replay_base()..=capped.head() {
            assert_eq!(capped.events_after(o)?, full.events_after(o)?);
        }
        if capped.full_history {
            assert_eq!(capped.base(), 0);
            for o in 0..=capped.head() {
                assert_eq!(capped.world_at(o)?, full.world_at(o)?);
            }
            assert_eq!(
                capped.history(0, capped.head()),
                full.history(0, full.head())
            );
        }
        Ok(())
    }

    /// Past the cap the window drops to half the cap, and every offset still retained replays
    /// exactly what an uncapped timeline replays; world queries then serve the head only.
    #[test]
    fn a_capped_timeline_keeps_a_window_equal_to_the_uncapped_one() -> Result<(), QueryError> {
        const CAP: usize = 6;
        let mut full = Timeline::new(3).with_history_cap(usize::MAX);
        let mut capped = Timeline::new(3).with_history_cap(CAP);
        for (i, event) in log(50).into_iter().enumerate() {
            let at = Timestamp::from_millis(i64::try_from(i).unwrap_or(0));
            full.append(at, event.clone());
            capped.append(at, event);
            assert!(capped.events.len() <= CAP);
            assert_eq!(capped.full_history, i < CAP);
            assert_window_matches(&capped, &full)?;
        }
        let head = capped.head();
        assert_eq!(capped.base(), head);
        assert_eq!(capped.time_range().replay_base, capped.replay_base());
        assert!(capped.replay_base() >= head - u64::try_from(CAP).unwrap_or(0));
        assert_eq!(
            capped.world_at(head - 1),
            Err(QueryError::OffsetBeforeBase {
                at: head - 1,
                base: head
            })
        );
        assert_eq!(capped.world_at(head)?, full.world_at(head)?);
        assert!(matches!(
            capped.events_after(capped.replay_base() - 1),
            Err(QueryError::OffsetBeforeBase { .. })
        ));
        // `/time?ts` never names an offset the world queries would refuse.
        assert_eq!(capped.offset_at(Timestamp::from_millis(49))?, head);
        assert_eq!(
            capped.offset_at(Timestamp::from_millis(48)),
            Err(QueryError::TimeBeforeBase { ts: 48, base: head })
        );
        // The time index still spans every event, dropped or not.
        assert_eq!(capped.head_time(), full.head_time());
        Ok(())
    }

    /// The minimum cap is 2, which still keeps one event to replay.
    #[test]
    fn the_smallest_cap_keeps_a_window() -> Result<(), QueryError> {
        let mut timeline = Timeline::new(3).with_history_cap(0);
        for (i, event) in log(9).into_iter().enumerate() {
            timeline.append(Timestamp::from_millis(i64::try_from(i).unwrap_or(0)), event);
            assert!((1..=2).contains(&timeline.events.len()));
        }
        assert_eq!(
            timeline.events_after(timeline.replay_base())?.len(),
            timeline.events.len()
        );
        Ok(())
    }

    /// The vector does not keep a peak allocation: its capacity stays within twice the cap.
    #[test]
    fn the_window_allocation_stays_bounded() {
        const CAP: usize = 1_000;
        let mut timeline = Timeline::new(3).with_history_cap(CAP);
        for i in 0..10_000_i64 {
            timeline.append(Timestamp::from_millis(i), observed("a"));
        }
        assert_eq!(timeline.head(), 10_000);
        assert!(timeline.events.capacity() <= 2 * CAP);
    }
}
