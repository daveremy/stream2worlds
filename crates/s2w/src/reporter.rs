//! The CLI's `s2w watch --json` reporter (s2w#79, round 2).
//!
//! `s2w_app::group_commit`'s `Reporter` trait can't render JSON itself — `crates/s2w-app`
//! cannot depend on `crates/s2w` (see `crates/s2w-app/AGENTS.md`'s dependency direction), so
//! its only JSON-producing consumer has to live on this side and reuse `output.rs`'s existing
//! rendering seam rather than a second JSON path.

use std::io::{self, ErrorKind, Write};
use std::sync::Arc;

use s2w_app::{NoteSink, Reporter};

use crate::output;

/// Prints NDJSON: one progress object per flush on stdout, one `{"note": "…"}` per benign
/// startup note and one `{"error": "…", "fatal": false}` per non-fatal source error, both on
/// stderr. Stateless: `flushed`'s `reconnects` total is counted by `s2w-app`'s pump and only
/// rendered here — `crates/s2w/AGENTS.md` holds "no logic here beyond argument parsing and
/// output formatting" (round-2 review finding: a reporter-side counter violated that).
pub(crate) struct JsonReporter;

impl Reporter for JsonReporter {
    fn flushed(&mut self, appended: u64, duplicates: u64, reconnects: u64, cursor: Option<&str>) {
        let line =
            output::render_progress_line(appended, duplicates, reconnects, cursor, at_millis());
        write_line(&line);
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

    fn note_sink(&self) -> NoteSink {
        Arc::new(|message: &str| eprintln!("{}", output::render_source_note(message)))
    }
}

/// Writes one NDJSON line to stdout. Rust ignores `SIGPIPE` by default, so a `println!` here
/// panics (exit 101, "failed printing to stdout") the moment a piped consumer closes stdout
/// early (`s2w watch wikipedia --json | head -n 5`, s2w#105) — a `writeln!` on a lock lets us <!-- vocabulary: allow -->
/// see the write's `Result` and treat `BrokenPipe` as the reader simply going away rather than
/// an unexpected failure. `wants_ticker` returning `false` for this reporter means this is the
/// only stdout write `--json` mode makes, so there's nothing left to flush once the pipe is gone.
fn write_line(line: &str) {
    if let Err(error) = writeln!(io::stdout().lock(), "{line}") {
        if is_broken_pipe(&error) {
            std::process::exit(0);
        }
        panic!("failed printing to stdout: {error}");
    }
}

fn is_broken_pipe(error: &io::Error) -> bool {
    error.kind() == ErrorKind::BrokenPipe
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

    #[test]
    fn broken_pipe_is_recognized() {
        assert!(is_broken_pipe(&io::Error::from(ErrorKind::BrokenPipe)));
    }

    #[test]
    fn other_write_errors_are_not_treated_as_broken_pipe() {
        // s2w#105: only BrokenPipe (a reader going away) is a clean stop; anything else
        // (e.g. a genuinely full disk backing a redirected stdout) still panics loudly.
        assert!(!is_broken_pipe(&io::Error::from(ErrorKind::WriteZero)));
        assert!(!is_broken_pipe(&io::Error::from(
            ErrorKind::PermissionDenied
        )));
    }
}
