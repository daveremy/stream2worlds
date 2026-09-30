//! The lag signal a started source carries: how far the reader is behind the head of the
//! stream, per source id (s2w#168).
//!
//! An adapter whose protocol exposes the head position (Kafka's high watermark) tracks one
//! [`Watermark`] per source and hands the app [`Watermarks::tracked`]. An adapter whose protocol
//! has no such position (SSE, stdin) says so with [`Watermarks::not_reported`], so the app shows
//! the lag as absent, never as zero.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use s2w_model::SourceId;

/// Marks a position that is not known yet.
const UNKNOWN: i64 = -1;

/// One source's position against the head of its stream. The adapter writes it from its fetch
/// tasks and its stream; the app reads it for display. `Send + Sync`.
///
/// Both positions use the protocol's own numbering (Kafka offsets). The two atomics are read
/// independently, so a reading taken while both move can be off by the records in flight for
/// one tick; the status line tolerates that.
#[derive(Debug)]
pub struct Watermark {
    /// The position the next record written to the source will take (Kafka's high watermark).
    high: AtomicI64,
    /// The position of the next record the stream will hand the app.
    next: AtomicI64,
}

impl Watermark {
    /// A watermark with neither position known yet.
    pub(crate) const fn unknown() -> Self {
        Self {
            high: AtomicI64::new(UNKNOWN),
            next: AtomicI64::new(UNKNOWN),
        }
    }

    /// Records the head position a fetch reply reported.
    pub(crate) fn observe_high(&self, high: i64) {
        self.high.store(high, Ordering::Relaxed);
    }

    /// Records the resolved start position, before any record is delivered.
    pub(crate) fn resume_at(&self, next: i64) {
        self.next.store(next, Ordering::Relaxed);
    }

    /// Records that the record at `position` left the stream for the app.
    pub(crate) fn delivered(&self, position: i64) {
        self.next
            .store(position.saturating_add(1), Ordering::Relaxed);
    }

    /// The head position, and how many records the reader is behind it: `high - next`,
    /// clamped at zero (a head read before the latest delivery). Each is `None` until known.
    fn reading(&self) -> (Option<i64>, Option<u64>) {
        let high = known(self.high.load(Ordering::Relaxed));
        let next = known(self.next.load(Ordering::Relaxed));
        let behind = high
            .zip(next)
            .map(|(high, next)| u64::try_from(high.saturating_sub(next)).unwrap_or(0));
        (high, behind)
    }
}

/// `None` for the unknown marker (any negative position).
fn known(position: i64) -> Option<i64> {
    (position >= 0).then_some(position)
}

/// The lag signal of one started source: either "this source reports no watermark", or one
/// tracked [`Watermark`] per source id. Cheap to clone; clones share the same watermarks.
#[derive(Debug, Clone, Default)]
pub struct Watermarks(Option<Arc<Vec<Tracked>>>);

/// One tracked source: its id, the short label the status line shows, and its watermark.
#[derive(Debug)]
struct Tracked {
    source: SourceId,
    label: String,
    mark: Arc<Watermark>,
}

impl Watermarks {
    /// The source's protocol carries no head position, so it reports no lag.
    pub const fn not_reported() -> Self {
        Self(None)
    }

    /// One watermark per source, each with the label the status line shows ("p0"). Readings
    /// come back ordered by source id.
    pub(crate) fn tracked(entries: Vec<(SourceId, String, Arc<Watermark>)>) -> Self {
        let mut tracked: Vec<Tracked> = entries
            .into_iter()
            .map(|(source, label, mark)| Tracked {
                source,
                label,
                mark,
            })
            .collect();
        tracked.sort_by(|left, right| left.source.cmp(&right.source));
        Self(Some(Arc::new(tracked)))
    }

    /// `None` when the source reports no watermark; otherwise one reading per tracked source,
    /// ordered by source id.
    #[must_use]
    pub fn read(&self) -> Option<Vec<LagReading>> {
        self.0.as_ref().map(|tracked| {
            tracked
                .iter()
                .map(|entry| {
                    let (high_watermark, behind) = entry.mark.reading();
                    LagReading {
                        source: entry.source.clone(),
                        label: entry.label.clone(),
                        high_watermark,
                        behind,
                    }
                })
                .collect()
        })
    }
}

/// One source's lag at the moment it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LagReading {
    /// The source this reading is for.
    pub source: SourceId,
    /// The short label the adapter chose for display ("p0" for Kafka partition 0).
    pub label: String,
    /// The head position the source last reported, if any reply has arrived yet.
    pub high_watermark: Option<i64>,
    /// Records written to the source that the app has not yet pulled off the stream, including
    /// any buffered in the process. `None` until both positions are known. On a Kafka topic
    /// with gaps (compaction, transaction markers) this is an upper bound, not an exact count.
    pub behind: Option<u64>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use s2w_model::SourceId;

    use super::{LagReading, Watermark, Watermarks};

    fn source(name: &str) -> SourceId {
        match SourceId::new(name) {
            Ok(source) => source,
            Err(error) => panic!("{name:?} should be a valid source id: {error}"),
        }
    }

    fn reading(watermarks: &Watermarks) -> Vec<LagReading> {
        watermarks
            .read()
            .unwrap_or_else(|| panic!("a tracked source should report readings"))
    }

    const fn assert_send_sync<T: Send + Sync>() {}
    const _: () = assert_send_sync::<Watermarks>();

    #[test]
    fn not_reported_reads_as_none_never_as_zero() {
        assert_eq!(Watermarks::not_reported().read(), None);
        assert_eq!(Watermarks::default().read(), None);
    }

    #[test]
    fn behind_is_unknown_until_both_positions_are_known() {
        let mark = Arc::new(Watermark::unknown());
        let watermarks = Watermarks::tracked(vec![(source("k.p0"), "p0".into(), mark.clone())]);
        let only = |watermarks: &Watermarks| reading(watermarks).remove(0);
        assert_eq!(only(&watermarks).high_watermark, None);
        assert_eq!(only(&watermarks).behind, None);

        mark.observe_high(10);
        assert_eq!(only(&watermarks).high_watermark, Some(10));
        assert_eq!(only(&watermarks).behind, None);

        mark.resume_at(7);
        assert_eq!(only(&watermarks).behind, Some(3));

        mark.delivered(9);
        assert_eq!(only(&watermarks).behind, Some(0));
    }

    #[test]
    fn a_stale_head_clamps_to_zero() {
        let mark = Arc::new(Watermark::unknown());
        let watermarks = Watermarks::tracked(vec![(source("k.p0"), "p0".into(), mark.clone())]);
        mark.observe_high(5);
        mark.delivered(8);
        assert_eq!(reading(&watermarks).remove(0).behind, Some(0));
    }

    #[test]
    fn readings_come_back_in_source_order_and_clones_share_state() {
        let first = Arc::new(Watermark::unknown());
        let second = Arc::new(Watermark::unknown());
        let watermarks = Watermarks::tracked(vec![
            (source("k.p1"), "p1".into(), second.clone()),
            (source("k.p0"), "p0".into(), first.clone()),
        ]);
        let clone = watermarks.clone();
        second.observe_high(4);
        second.resume_at(1);
        let labels: Vec<_> = reading(&clone)
            .into_iter()
            .map(|reading| (reading.label, reading.behind))
            .collect();
        assert_eq!(labels, vec![("p0".into(), None), ("p1".into(), Some(3))]);
    }
}
