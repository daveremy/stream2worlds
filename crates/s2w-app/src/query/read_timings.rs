//! Where a `/world` body's read-guard hold goes (s2w#243, PR 1 of s2w#235): the wait for the
//! guard, building the view under it (`HeadView::new`, which runs `Graph::new` and the sorts),
//! and writing the body with the guard still held. Since s2w#270 one hold can serve several
//! bodies (a single-flight generation), so the phases are per hold and `bodies` counts what
//! the holds served. Off unless a caller opts in with
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
/// holds that built a view, the bodies they served (a `304` or an error builds nothing and is
/// not counted), and the three phases of each hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadTimingsSnapshot {
    /// Holds that built a view: one per type-summary body, one per single-flight generation
    /// (`lod=entity` or full `lod=type`) however many subscribers it served (s2w#270, s2w#297). The phases below are per hold.
    pub builds: u64,
    /// `/world` bodies served (status 200). `bodies / builds` is the sharing.
    pub bodies: u64,
    /// From the hold starting on its blocking thread to the read guard taken: queueing behind
    /// a writer. Time waiting for a blocking thread, or queued for a generation, is not in it.
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

/// The counters every `/world` hold records into, shared by every clone of one `QueryState`.
#[derive(Default)]
pub(crate) struct ReadTimings {
    builds: AtomicU64,
    bodies: AtomicU64,
    wait: Phase,
    build: Phase,
    write: Phase,
}

impl ReadTimings {
    /// Records one hold that built a view and served `bodies` bodies from it.
    pub(crate) fn record(&self, bodies: u64, wait: Duration, build: Duration, write: Duration) {
        self.builds.fetch_add(1, Ordering::Relaxed);
        self.bodies.fetch_add(bodies, Ordering::Relaxed);
        self.wait.record(wait);
        self.build.record(build);
        self.write.record(write);
    }

    /// The counters so far. Each field is read on its own, so a snapshot taken while a body is
    /// being recorded can be off by that one body; read it after the readers stop.
    pub(crate) fn snapshot(&self) -> ReadTimingsSnapshot {
        ReadTimingsSnapshot {
            builds: self.builds.load(Ordering::Relaxed),
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
        timings.record(1, ms(1), ms(100), ms(300));
        timings.record(3, ms(5), ms(300), ms(300));
        let seen = timings.snapshot();
        assert_eq!(seen.builds, 2);
        assert_eq!(seen.bodies, 4);
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
