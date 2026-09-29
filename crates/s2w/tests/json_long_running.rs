//! CLI smoke tests for prefix-only JSON mode on the long-running commands.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn looks_like_one_json_object(line: &str) -> bool {
    let text = line.trim_end();
    if !text.starts_with('{') || !text.ends_with('}') || text.contains(['\n', '\r']) {
        return false;
    }
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else if byte == b'"' {
            quoted = true;
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            let Some(next) = depth.checked_sub(1) else {
                return false;
            };
            depth = next;
        }
    }
    !quoted && !escaped && depth == 0
}

#[test]
fn json_prefixed_mcp_completes_a_clean_json_rpc_handshake() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_s2w"))
        .args(["--json", "mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("s2w --json mcp should spawn");
    let mut stdin = child.stdin.take().expect("child stdin");
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"smoke","version":"0"}}}}}}"#
    )
    .expect("write initialize request");
    stdin.flush().expect("flush initialize request");

    let stdout = child.stdout.take().expect("child stdout");
    let (response_tx, response_rx) = mpsc::channel();
    let (remainder_tx, remainder_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut response = String::new();
        let first = reader.read_line(&mut response);
        let _ignored = response_tx.send((first, response));
        let mut remainder = Vec::new();
        let rest = reader.read_to_end(&mut remainder);
        let _ignored = remainder_tx.send((rest, remainder));
    });

    let (read, response) = response_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("MCP initialize response before timeout");
    assert!(read.expect("read initialize response") > 0);
    assert!(looks_like_one_json_object(&response), "{response:?}");
    assert!(response.contains(r#""jsonrpc":"2.0""#), "{response}");
    assert!(response.contains(r#""id":1"#), "{response}");
    assert!(response.contains(r#""result""#), "{response}");

    child.kill().expect("stop MCP server after handshake");
    let status = child.wait().expect("wait for MCP server");
    assert!(
        !status.success(),
        "the smoke test deliberately kills the server"
    );
    let (read, remainder) = remainder_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("stdout closes after kill");
    read.expect("read remaining stdout");
    assert!(
        remainder.is_empty(),
        "unexpected extra stdout: {remainder:?}"
    );
    reader.join().expect("stdout reader thread");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("child stderr")
        .read_to_string(&mut stderr)
        .expect("read child stderr");
    assert!(
        !stderr.contains("--json is unavailable"),
        "old refusal must be gone: {stderr}"
    );
}

#[test]
fn json_prefixed_mcp_renders_usage_errors_without_touching_stdout() {
    let output = Command::new(env!("CARGO_BIN_EXE_s2w"))
        .args(["--json", "mcp", "unexpected"])
        .output()
        .expect("s2w --json mcp usage error");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("JSON stderr is UTF-8");
    assert!(looks_like_one_json_object(&stderr), "{stderr:?}");
    assert!(
        stderr.contains(
            r#""error": "unexpected argument 'unexpected': expected --log-dir, --world or --allow-decisions""#
        )
    );
}

// `mcp --log-dir` pointing at a directory that does not exist is a fatal startup error, so
// under `--json` it renders the `{"error": ..., "fatal": true}` shape (#110/#125), not the
// plain `{"error": ...}` object a usage/parse failure gets. Both formats must name the missing
// path in plain text, never a raw SQLite `CANTOPEN` code (#115).
#[test]
fn mcp_with_a_missing_log_dir_fatally_names_the_path_in_both_formats() {
    let missing = std::env::temp_dir().join(format!(
        "s2w-mcp-absent-{}-definitely-not-created",
        std::process::id()
    ));
    let human = Command::new(env!("CARGO_BIN_EXE_s2w"))
        .args(["mcp", "--log-dir"])
        .arg(&missing)
        .output()
        .expect("s2w mcp --log-dir <missing> should run");
    assert_eq!(human.status.code(), Some(1));
    assert!(human.stdout.is_empty(), "stdout is JSON-RPC-only");
    let stderr = String::from_utf8(human.stderr).expect("human stderr is UTF-8");
    assert!(
        stderr.contains(&missing.display().to_string()),
        "must name the missing path: {stderr}"
    );
    assert!(!stderr.contains("CANTOPEN"), "{stderr}");

    let json = Command::new(env!("CARGO_BIN_EXE_s2w"))
        .args(["--json", "mcp", "--log-dir"])
        .arg(&missing)
        .output()
        .expect("s2w --json mcp --log-dir <missing> should run");
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stdout.is_empty(), "stdout is JSON-RPC-only");
    let stderr = String::from_utf8(json.stderr).expect("JSON stderr is UTF-8");
    assert!(looks_like_one_json_object(&stderr), "{stderr:?}");
    assert!(stderr.contains(r#""fatal": true"#), "{stderr}");
    assert!(
        stderr.contains(&missing.display().to_string()),
        "must name the missing path: {stderr}"
    );
    assert!(!stderr.contains("CANTOPEN"), "{stderr}");
}

// `early_bridge_exit_is_fatal_and_signals_http_shutdown` in
// `crates/s2w-app/src/serve/tests.rs` covers the "a genuine fatal error is rendered exactly
// once" requirement directly at the `supervise` level. This CLI-level test instead exercises
// the one post-listener failure that's reachable from stdin alone: an invalid-UTF-8 line,
// which `serve`'s pump treats as a *non-fatal* skip (matching `watch`'s existing behavior,
// s2w#79) — the source_error is reported, ingestion then ends normally at stdin EOF, and the
// process exits 0. This is deliberately NOT the double-print scenario; it proves JSON
// rendering end-to-end for the listener note + a non-fatal source error + the clean-shutdown
// note, all through the real binary.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
)]
fn stdin_serve_json_reports_a_nonfatal_source_error_then_shuts_down_cleanly() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let log_dir = std::env::temp_dir().join(format!(
        "s2w-serve-json-nonfatal-{}-{nanos}",
        std::process::id()
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_s2w"))
        .args(["--json", "serve", "-", "--port", "0", "--log-dir"])
        .arg(&log_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("s2w --json serve should spawn");
    let mut stdin = child.stdin.take().expect("child stdin");
    stdin
        .write_all(b"\xff\n")
        .expect("write invalid UTF-8 after startup");
    drop(stdin);
    let output = child
        .wait_with_output()
        .expect("serve exits after stdin closes");
    let _ignored = std::fs::remove_dir_all(&log_dir);
    let stderr = String::from_utf8(output.stderr).expect("JSON stderr is UTF-8");
    if stderr.contains("binding 127.0.0.1:0: Operation not permitted") {
        eprintln!("skipping TCP integration: sandbox denies child loopback sockets: {stderr}");
        return;
    }

    assert_eq!(
        output.status.code(),
        Some(0),
        "an invalid-UTF-8 line is a non-fatal skip, not a process failure: {}",
        output.status
    );
    assert!(output.stdout.is_empty(), "serve JSON notes stay on stderr");
    let lines: Vec<_> = stderr.lines().collect();
    assert!(
        lines
            .iter()
            .any(|line| line.contains(r#""note": "serving on http://127.0.0.1:"#)),
        "listener must be bound before the injected line: {stderr}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains(r#""fatal": false"#) && line.contains("not valid UTF-8")),
        "the invalid line must render as a non-fatal source error: {stderr}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains(r#""note": "ingestion stopped; shutting down HTTP""#)),
        "stdin EOF ends the pump normally: {stderr}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("shutting down HTTP after a fatal error")),
        "a non-fatal skip must not be reported as a fatal error: {stderr}"
    );
    for line in &lines {
        assert!(looks_like_one_json_object(line), "{line:?}");
    }
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
)]
fn serve_stops_cleanly_on_sigterm_after_writing_a_snapshot() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let log_dir =
        std::env::temp_dir().join(format!("s2w-serve-sigterm-{}-{nanos}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_s2w"))
        .args([
            "serve",
            "-",
            "--port",
            "0",
            "--snapshot-every",
            "1",
            "--log-dir",
        ])
        .arg(&log_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("s2w serve should spawn");
    let (lines_tx, lines_rx) = mpsc::channel();
    let stderr = child.stderr.take().expect("child stderr");
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    // Keep stdin open: end of input would stop serve on its own, not through the signal.
    let mut stdin = child.stdin.take().expect("child stdin");
    stdin
        .write_all(b"{\"EntityObserved\":{\"key\":\"a\",\"entity_type\":\"thing\",\"attrs\":{}}}\n")
        .expect("write one event");
    stdin.flush().expect("flush stdin");
    let mut seen = Vec::new();
    let written = loop {
        match lines_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(line) => {
                let done = line.contains("snapshot written at offset");
                seen.push(line);
                if done {
                    break true;
                }
            }
            Err(_) => break false,
        }
    };
    let joined = seen.join("\n");
    if joined.contains("binding 127.0.0.1:0: Operation not permitted") {
        let _ignored = child.kill();
        let _ignored = child.wait();
        let _ignored = std::fs::remove_dir_all(&log_dir);
        eprintln!("skipping TCP integration: sandbox denies child loopback sockets: {joined}");
        return;
    }
    assert!(written, "a periodic snapshot after one event: {joined}");
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill -TERM");
    assert!(status.success());
    let exit = child.wait().expect("serve exits after SIGTERM");
    drop(stdin);
    drop(lines_rx);
    let _ignored = reader.join();
    let files = std::fs::read_dir(log_dir.join("snapshots"))
        .map(|dir| {
            dir.filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().ends_with(".s2w"))
                .count()
        })
        .unwrap_or(0);
    let _ignored = std::fs::remove_dir_all(&log_dir);
    assert_eq!(exit.code(), Some(0), "SIGTERM is a clean stop: {exit}");
    assert_eq!(files, 1, "the snapshot survives the stop");
}
