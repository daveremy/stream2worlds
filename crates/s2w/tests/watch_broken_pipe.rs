//! Integration test for s2w#105: `s2w watch - --json | head -n1` (or any consumer that exits
//! before the stream ends) must exit cleanly, not panic. `crates/s2w/src/reporter.rs`'s
//! `JsonReporter::write_line` treats a `BrokenPipe` write error as a clean stop; a unit test of
//! `is_broken_pipe` alone would still pass if `flushed` reverted to `println!` (codex-review
//! round 1 finding on this issue) — this test spawns the real binary and closes its stdout
//! mid-stream to prove the whole path.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// More than `s2w_app::group_commit::MAX_BATCH` (100, private to that crate) worth of lines, so
/// a second flush is attempted after the first is read and the pipe is broken.
const TOTAL_LINES: usize = 150;
const FIRST_BATCH: usize = 100;

#[test]
fn broken_stdout_pipe_exits_cleanly_instead_of_panicking() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let log_dir = std::env::temp_dir().join(format!(
        "s2w-watch-broken-pipe-{}-{nanos}",
        std::process::id()
    ));

    let mut child = Command::new(env!("CARGO_BIN_EXE_s2w"))
        .arg("watch")
        .arg("-")
        .arg("--json")
        .arg("--log-dir")
        .arg(&log_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("s2w watch should spawn");

    let mut child_stdin = child.stdin.take().expect("child stdin pipe available");
    let child_stdout = child.stdout.take().expect("child stdout pipe available");

    // The first MAX_BATCH lines fill the buffer and force an immediate flush — one progress
    // line on stdout — before we've read anything.
    for index in 0..FIRST_BATCH {
        writeln!(child_stdin, "{{\"n\":{index}}}").expect("writing to child stdin");
    }
    child_stdin.flush().expect("flushing child stdin");

    {
        let mut lines = BufReader::new(child_stdout).lines();
        let first = lines
            .next()
            .expect("child should print one progress line before exiting")
            .expect("reading the first progress line");
        assert!(
            first.contains("\"appended\": 100"),
            "expected the first flush to report 100 appended events: {first}"
        );
        // `lines` (and its `BufReader`) is dropped at the end of this block, closing our read
        // end of the stdout pipe. The child's next flush attempt now hits `BrokenPipe`.
    }

    // More lines than one MAX_BATCH again, so a second flush is attempted against the
    // now-closed pipe. Before the s2w#105 fix, `JsonReporter`'s `println!` would panic here
    // (exit 101, "failed printing to stdout") instead of exiting 0.
    for index in FIRST_BATCH..TOTAL_LINES {
        writeln!(child_stdin, "{{\"n\":{index}}}").expect("writing to child stdin");
    }
    // Closing stdin ends the stdin source (end-of-stream also flushes, as a backup trigger for
    // the second flush if MAX_DELAY hasn't already fired it).
    drop(child_stdin);

    let status = child.wait().expect("waiting on the child process");
    let _ = std::fs::remove_dir_all(&log_dir);

    assert!(
        status.success(),
        "s2w watch --json should exit 0 on a broken stdout pipe, got {status:?}"
    );
}
