//! Where a `/world` body's read-guard hold goes (s2w#243, PR 1 of s2w#235): the wait for the
//! guard, building the view under it (`HeadView::new`, which runs `Graph::new` and the sorts),
//! and writing the body with the guard still held. Off unless a caller opts in with
//! [`QueryState::with_read_timings`](super::QueryState::with_read_timings); serve never does,
//! and the timings never change what is written.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// One phase of the `/world` hold across every body served: the summed and the longest time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhaseTiming {
    /// The phase's time summed over every body.
    pub total: Duration,
    /// The longest single instance of the phase.
    pub max: Duration,
}

/// What [`QueryState::read_timings`](super::QueryState::read_timings) reports: `/world`
/// bodies served (a `304` or an error builds nothing and is not counted) and the three phases
/// of each one's hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadTimingsSnapshot {
    /// `/world` bodies served (status 200).
    pub bodies: u64,
    /// From `stream_world` starting on its blocking thread to the read guard taken: queueing
    /// behind a writer. Time waiting for a blocking thread is not in it.
    pub wait: PhaseTiming,
    /// From the guard taken to the view built: offset resolution, `Graph::new` and the sorts.
    pub build: PhaseTiming,
    /// From the view built to the last chunk handed over, guard still held: serialization,
    /// which still reads each node's type and attributes out of the world, plus any time the
    /// channel was full because the client read slower than it was written (up to the stall
    /// limit).
    pub write: PhaseTiming,
}

impl ReadTimingsSnapshot {
    /// `build / (build + write)` over the summed times: the share of the hold a handoff that
    /// releases the guard after the build would keep. `None` before the first body.
    #[must_use]
    pub fn build_share(&self) -> Option<f64> {
        share(self.build.total, self.write.total)
    }

    /// The same ratio over the longest instances: the build share of the worst case.
    #[must_use]
    pub fn build_share_of_max(&self) -> Option<f64> {
        share(self.build.max, self.write.max)
    }
}

fn share(build: Duration, write: Duration) -> Option<f64> {
    let whole = build + write;
    (!whole.is_zero()).then(|| build.as_secs_f64() / whole.as_secs_f64())
}

/// One phase's counters.
#[derive(Default)]
struct Phase {
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl Phase {
    fn record(&self, elapsed: Duration) {
        let ns = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.total_ns.fetch_add(ns, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
    }

    fn get(&self) -> PhaseTiming {
        PhaseTiming {
            total: Duration::from_nanos(self.total_ns.load(Ordering::Relaxed)),
            max: Duration::from_nanos(self.max_ns.load(Ordering::Relaxed)),
        }
    }
}

/// The counters `stream_world` records into, shared by every clone of one `QueryState`.
#[derive(Default)]
pub(crate) struct ReadTimings {
    bodies: AtomicU64,
    wait: Phase,
    build: Phase,
    write: Phase,
}

impl ReadTimings {
    /// Records one body's hold.
    pub(crate) fn record(&self, wait: Duration, build: Duration, write: Duration) {
        self.bodies.fetch_add(1, Ordering::Relaxed);
        self.wait.record(wait);
        self.build.record(build);
        self.write.record(write);
    }

    /// The counters so far. Each field is read on its own, so a snapshot taken while a body is
    /// being recorded can be off by that one body; read it after the readers stop.
    pub(crate) fn snapshot(&self) -> ReadTimingsSnapshot {
        ReadTimingsSnapshot {
            bodies: self.bodies.load(Ordering::Relaxed),
            wait: self.wait.get(),
            build: self.build.get(),
            write: self.write.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_totals_maxima_and_the_build_share() {
        let timings = ReadTimings::default();
        assert_eq!(timings.snapshot().build_share(), None);
        let ms = Duration::from_millis;
        timings.record(ms(1), ms(100), ms(300));
        timings.record(ms(5), ms(300), ms(300));
        let seen = timings.snapshot();
        assert_eq!(seen.bodies, 2);
        assert_eq!(
            seen.wait,
            PhaseTiming {
                total: ms(6),
                max: ms(5)
            }
        );
        assert_eq!(
            seen.build,
            PhaseTiming {
                total: ms(400),
                max: ms(300)
            }
        );
        assert_eq!(
            seen.write,
            PhaseTiming {
                total: ms(600),
                max: ms(300)
            }
        );
        let close = |got: Option<f64>, want: f64| (got.unwrap() - want).abs() < 1e-9;
        assert!(close(seen.build_share(), 0.4));
        assert!(close(seen.build_share_of_max(), 0.5));
    }
}
