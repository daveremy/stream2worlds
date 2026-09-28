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
/// stderr.
#[derive(Default)]
pub(crate) struct JsonReporter {
    /// Running total of `SourceError::Retrying` reports, surfaced in every `flushed` line.
    reconnects: u64,
}

impl Reporter for JsonReporter {
    fn flushed(&mut self, appended: u64, duplicates: u64, cursor: Option<&str>) {
        println!(
            "{}",
            output::render_progress_line(
                appended,
                duplicates,
                self.reconnects,
                cursor,
                at_millis()
            )
        );
    }

    fn duplicate(&mut self, _position: u64) {
        // Folded into the next `flushed` call's `duplicates` total instead of its own line —
        // the log position isn't part of the sketched shape and would need its own field.
    }

    fn note(&mut self, message: &str) {
        eprintln!("{}", output::render_source_note(message));
    }

    fn source_error(&mut self, message: &str, retry: bool) {
        if retry {
            self.reconnects += 1;
        }
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
    fn flushed_counts_reconnects_from_retried_source_errors() {
        let mut reporter = JsonReporter::default();
        reporter.source_error("kafka: reset", true);
        reporter.source_error("kafka: benign skip", false);
        assert_eq!(reporter.reconnects, 1);
    }

    #[test]
    fn wants_ticker_is_false() {
        assert!(!JsonReporter::default().wants_ticker());
    }
}
