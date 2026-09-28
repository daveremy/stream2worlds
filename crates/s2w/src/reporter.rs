//! The CLI's `s2w watch --json` reporter (s2w#79, round 2).
//!
//! `s2w_app::group_commit`'s `Reporter` trait can't render JSON itself — `crates/s2w-app`
//! cannot depend on `crates/s2w` (see `crates/s2w-app/AGENTS.md`'s dependency direction), so
//! its only JSON-producing consumer has to live on this side and reuse `output.rs`'s existing
//! rendering seam rather than a second JSON path.

use s2w_app::Reporter;

use crate::output;

/// Prints NDJSON: one progress object per flush on stdout, one `{"note": "…"}` per benign
/// startup note and one `{"error": "…", "fatal": false}` per non-fatal source error, both on
/// stderr. Stateless: `flushed`'s `reconnects` total is counted by `s2w-app`'s pump and only
/// rendered here — `crates/s2w/AGENTS.md` holds "no logic here beyond argument parsing and
/// output formatting" (round-2 review finding: a reporter-side counter violated that).
pub(crate) struct JsonReporter;

impl Reporter for JsonReporter {
    fn flushed(&mut self, appended: u64, duplicates: u64, reconnects: u64, cursor: Option<&str>) {
        println!(
            "{}",
            output::render_progress_line(appended, duplicates, reconnects, cursor, at_millis())
        );
    }

    fn duplicate(&mut self, _position: u64) {
        // Folded into the next `flushed` call's `duplicates` total instead of its own line —
        // the log position isn't part of the sketched shape and would need its own field.
    }

    fn note(&mut self, message: &str) {
        eprintln!("{}", output::render_source_note(message));
    }

    fn source_error(&mut self, message: &str, _retry: bool) {
        eprintln!("{}", output::render_source_error(message));
    }

    fn wants_ticker(&self) -> bool {
        false
    }
}

/// Milliseconds since the Unix epoch, for a progress line's `at` field. Its own copy rather
/// than sharing `s2w_app`'s near-identical helpers (`s2w-sources`' ndjson/kafka/sse clocks) —
/// a real `s2w_model::Timestamp::now()` would need to cross a pure-core boundary that may not
/// want a wall clock; left as a documented nit (s2w#79 round-1 checkpoint, finding 4).
fn at_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wants_ticker_is_false() {
        assert!(!JsonReporter.wants_ticker());
    }

    #[test]
    fn source_error_never_mutates_state() {
        // Stateless (round 2): this must compile and run with an immutable-looking call
        // pattern repeated any number of times without any counter drifting internally —
        // the reconnect total lives in s2w-app's pump, not here.
        let mut reporter = JsonReporter;
        reporter.source_error("kafka: reset", true);
        reporter.source_error("kafka: benign skip", false);
    }
}
