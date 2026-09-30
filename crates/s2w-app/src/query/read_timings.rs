//! Where a `/world` generation's time goes (s2w#243, PR 1 of s2w#235): the wait for the read
//! guard, the capture under it (`Projection::capture`: `Graph::new` and the copies), and, after
//! the guard is released (s2w#272), the sort and the write. Since s2w#270 one generation can
//! serve several bodies, so the phases are per generation and `bodies` counts what they served.
//! Off unless a caller opts in with
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
    /// (`lod=entity` or full `lod=type`) however many subscribers it served (s2w#270, s2w#297).
    /// The phases below are per hold.
    pub builds: u64,
    /// `/world` bodies served (status 200). `bodies / builds` is the sharing.
    pub bodies: u64,
    /// From the hold starting on its blocking thread to the read guard taken: queueing behind
    /// a writer. Time waiting for a blocking thread, or queued for a generation, is not in it.
    pub wait: PhaseTiming,
    /// From the guard taken to the view captured: offset resolution, `Graph::new` and the
    /// copies. The whole time the guard is held.
    pub build: PhaseTiming,
    /// The sorts, after the guard is released (s2w#272).
    pub prepare: PhaseTiming,
    /// From the view sorted to the last chunk handed over, no guard held: serialization, plus
    /// any time the channel was full because the client read slower than it was written (up to
    /// the stall limit).
    pub write: PhaseTiming,
    /// Entity states the fold replaced while a view still held them, summed over generations
    /// (decision 0028's divergence), counted after each write. Meaningful for views of the head:
    /// a view of an `at` below it counts every state, since its world is dropped at capture.
    pub diverged: u64,
    /// The most in one generation.
    pub diverged_max: u64,
}

impl ReadTimingsSnapshot {
    /// `build / (build + prepare + write)` over the summed times: the share of a generation
    /// spent under the guard (before s2w#272 the whole of it). `None` before the first body.
    #[must_use]
    pub fn build_share(&self) -> Option<f64> {
        share(self.build.total, self.prepare.total + self.write.total)
    }

    /// The same ratio over the longest instances: the build share of the worst case.
    #[must_use]
    pub fn build_share_of_max(&self) -> Option<f64> {
        share(self.build.max, self.prepare.max + self.write.max)
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
    prepare: Phase,
    write: Phase,
    diverged: AtomicU64,
    diverged_max: AtomicU64,
}

/// One generation's phases, as [`ReadTimingsSnapshot`] names them.
pub(crate) struct Phases {
    pub(crate) wait: Duration,
    pub(crate) build: Duration,
    pub(crate) prepare: Duration,
    pub(crate) write: Duration,
}

impl ReadTimings {
    /// Records one generation that built a view and served `bodies` bodies from it.
    pub(crate) fn record(&self, bodies: u64, phases: Phases, diverged: u64) {
        self.builds.fetch_add(1, Ordering::Relaxed);
        self.bodies.fetch_add(bodies, Ordering::Relaxed);
        self.wait.record(phases.wait);
        self.build.record(phases.build);
        self.prepare.record(phases.prepare);
        self.write.record(phases.write);
        self.diverged.fetch_add(diverged, Ordering::Relaxed);
        self.diverged_max.fetch_max(diverged, Ordering::Relaxed);
    }

    /// The counters so far. Each field is read on its own, so a snapshot taken while a body is
    /// being recorded can be off by that one body; read it after the readers stop.
    pub(crate) fn snapshot(&self) -> ReadTimingsSnapshot {
        ReadTimingsSnapshot {
            builds: self.builds.load(Ordering::Relaxed),
            bodies: self.bodies.load(Ordering::Relaxed),
            wait: self.wait.get(),
            build: self.build.get(),
            prepare: self.prepare.get(),
            write: self.write.get(),
            diverged: self.diverged.load(Ordering::Relaxed),
            diverged_max: self.diverged_max.load(Ordering::Relaxed),
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
        let phases = |wait, build, write| Phases {
            wait: ms(wait),
            build: ms(build),
            prepare: ms(build / 10),
            write: ms(write),
        };
        timings.record(1, phases(1, 100, 300), 7);
        timings.record(3, phases(5, 300, 300), 2);
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
            seen.prepare,
            PhaseTiming {
                total: ms(40),
                max: ms(30)
            }
        );
        assert_eq!((seen.diverged, seen.diverged_max), (9, 7));
        assert_eq!(
            seen.write,
            PhaseTiming {
                total: ms(600),
                max: ms(300)
            }
        );
        let close = |got: Option<f64>, want: f64| (got.unwrap() - want).abs() < 1e-9;
        // build / (build + prepare + write).
        assert!(close(seen.build_share(), 400.0 / 1040.0));
        assert!(close(seen.build_share_of_max(), 300.0 / 630.0));
    }
}
