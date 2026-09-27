//! Test support: fixtures, golden replays and stream builders. Used only as a dev-dependency.

/// Constructs an empty in-memory event log for tests in other crates.
#[must_use]
pub fn in_memory_log() -> s2w_log::InMemoryEventLog {
    s2w_log::InMemoryEventLog::new()
}
