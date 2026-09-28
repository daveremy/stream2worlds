//! Runtime wiring: composes sources, the log, the core and the engines, and serves the read-only MCP server and the local web view.

pub mod bridge;
mod group_commit;
pub mod mcp;
pub mod query;
pub mod serve;

/// `s2w`'s `--json` reporter lives on the CLI side (it needs `output.rs`'s rendering seam,
/// which this crate cannot depend on — see `crates/s2w/AGENTS.md`'s dependency direction);
/// this is the trait it implements, plus the default human one `watch`/`run_watch` need a
/// caller to supply explicitly (s2w#79).
pub use group_commit::{HumanReporter, Reporter};
/// The hub in-degree cap a served timeline starts under, re-exported so the CLI can build the
/// MCP server's empty world without depending on the core itself.
pub use s2w_core::DEFAULT_HUB_IN_DEGREE_CAP;

use std::path::{Path, PathBuf};

use s2w_log::{EventLog, LogError, SqliteEventLog};
use s2w_model::{Cursor, SourceId};
use s2w_sources::registry::resolve;
use s2w_sources::source::{CursorLookup, Ending, SourceError};

/// Failures from wiring and running a watch command.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The HTTP listener or server failed.
    #[error("HTTP server: {0}")]
    Serve(#[source] std::io::Error),
    /// The bridge could not start or exited before shutdown.
    #[error("fatal bridge exit: {0}")]
    BridgeStopped(String),
    /// The event log could not be opened, read or appended to.
    #[error("event log: {0}")]
    Log(#[from] s2w_log::LogError),
    /// A source could not start, or stopped.
    #[error("{0}")]
    Source(#[from] SourceError),
    /// A model value was rejected.
    #[error("model: {0}")]
    Model(#[from] s2w_model::ModelError),
    /// The async runtime could not be built.
    #[error("could not build the async runtime: {0}")]
    Runtime(#[source] std::io::Error),
    /// The MCP server could not serve: its initialize handshake failed (rmcp's
    /// `ServerInitializeError`) or its serving task failed (`tokio`'s `JoinError`). Boxed:
    /// rmcp's error is large and would bloat every `Result<_, AppError>` in the crate.
    #[error("mcp server: {0}")]
    Mcp(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// A live source's stream ended. Wikipedia and Kafka keep their streams open (Wikipedia
    /// drops it only to reconnect it), so an ended stream means something is wrong, not that
    /// the work is done.
    #[error("the {0} source stream ended unexpectedly; it should keep running until stopped")]
    StreamEnded(&'static str),
    /// The command cannot run as given, and says what to try instead. The binary maps this to
    /// its usage exit code.
    #[error("{0}")]
    Usage(String),
}

/// Arguments for `s2w watch <source-uri>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchArgs {
    /// The source URI: a preset name, `-`, or `<scheme>://…` (see `s2w_sources::registry`).
    pub uri: String,
    /// Where a source with no stored cursor starts, in the source's own form.
    pub since: Option<String>,
    /// The directory holding (or creating) the SQLite event log.
    pub log_dir: PathBuf,
    /// `--json` (s2w#79): NDJSON progress on stdout, and `{"note":…}` / `{"error":…,
    /// "fatal":bool}` objects on stderr, instead of the human status lines.
    pub json: bool,
}

/// Runs `s2w watch <source-uri>` until the source ends or the process is stopped.
///
/// # Errors
///
/// [`AppError::Usage`] for an unknown URI, or `--since` the source cannot honour (a stored
/// cursor wins over it); otherwise the first failure that should stop the watch. A skipped
/// frame or line is reported on stderr and does not stop it.
///
/// `report` receives every progress, duplicate and note event; the caller picks the
/// reporter (s2w#79) — the CLI's `--json` one lives in `crates/s2w` since it needs
/// `output.rs`'s rendering seam, which this crate cannot depend on.
pub fn watch(args: WatchArgs, report: &mut dyn Reporter) -> Result<(), AppError> {
    current_thread_runtime()?.block_on(run_watch(args, report))
}

/// [`watch`] on the caller's runtime (current-thread: the stream is not `Send`).
///
/// # Errors
///
/// As [`watch`].
pub async fn run_watch(args: WatchArgs, report: &mut dyn Reporter) -> Result<(), AppError> {
    let source = resolve(&args.uri, None).map_err(|error| AppError::Usage(error.to_string()))?;
    let mut log = SqliteEventLog::open(&args.log_dir)
        .map_err(|error| open_error(error, &args.log_dir, "event log"))?;
    let name = source.name();
    let started = source
        .start(args.since.as_deref(), &LogCursors(&log))
        .await
        .map_err(|error| {
            if error.is_usage() {
                if matches!(&error, SourceError::SinceWithStoredCursor { .. }) {
                    AppError::Usage(format!("--log-dir {}: {error}", args.log_dir.display()))
                } else {
                    AppError::Usage(error.to_string())
                }
            } else {
                AppError::Source(error)
            }
        })?;
    for note in &started.notes {
        report.note(&format!("{name}: {note}"));
    }
    group_commit::pump_events(&mut log, started.stream, name, report).await?;
    match started.ends {
        Ending::AtEndOfInput => Ok(()),
        Ending::Never => Err(AppError::StreamEnded(name)),
    }
}

/// Maps a held writer lock to a usage error naming which lock blocked (`watch` and `serve`
/// share this so a second invocation of either against the same `--log-dir` exits 2, not 1).
pub(crate) fn open_error(error: LogError, directory: &Path, store: &str) -> AppError {
    match error {
        LogError::Locked => AppError::Usage(format!(
            "the {store} at {} is already open by another process (run `s2w watch`/`s2w serve` only once per --log-dir)",
            directory.display()
        )),
        other => AppError::Log(other),
    }
}

/// The log's stored cursors, as the read-only view sources consult before starting.
struct LogCursors<'a, L>(&'a L);

impl<L: EventLog> CursorLookup for LogCursors<'_, L> {
    fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, SourceError> {
        self.0
            .cursor(source)
            .map_err(|error| SourceError::Lookup(error.to_string()))
    }
}

/// A current-thread Tokio runtime with the I/O and timer drivers: enough for one source.
fn current_thread_runtime() -> Result<tokio::runtime::Runtime, AppError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(AppError::Runtime)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use s2w_log::{EventLog, SqliteEventLog};
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
    use s2w_sources::registry::resolve;
    use s2w_sources::source::SourceError;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_stream::StreamExt;

    use super::{AppError, HumanReporter, LogCursors, WatchArgs, watch};

    #[test]
    fn watch_lock_conflict_maps_to_usage_and_releases() {
        let directory = TestDirectory::new("watch-lock");
        let log = SqliteEventLog::open(directory.path()).expect("first event log opens");
        let error = watch(
            WatchArgs {
                uri: "-".to_owned(),
                since: None,
                log_dir: directory.path().to_path_buf(),
                json: false,
            },
            &mut HumanReporter,
        )
        .expect_err("a second open against the same --log-dir must fail");
        assert!(
            matches!(&error, AppError::Usage(message)
                if message.contains("the event log at") && message.contains("only once per --log-dir")),
            "unexpected error: {error:?}"
        );
        drop(log);
        SqliteEventLog::open(directory.path()).expect("event lock released after watch's error");
    }

    const WIKIPEDIA_SOURCE: &str = "wikipedia.page_change";

    /// Runs `test` on a current-thread runtime whose clock is paused (auto-advancing when idle)
    /// when `paused` is set. `#[tokio::test]` expands to an `allow(clippy::expect_used)` the
    /// workspace forbids.
    pub(crate) fn run<F: std::future::Future>(paused: bool, test: F) -> F::Output {
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(paused)
            .build()
        {
            Ok(runtime) => runtime.block_on(test),
            Err(error) => panic!("building the test runtime should succeed: {error}"),
        }
    }

    /// A per-test scratch directory under the system temp dir, removed on drop.
    pub(crate) struct TestDirectory(PathBuf);

    impl TestDirectory {
        pub(crate) fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            Self(
                std::env::temp_dir().join(format!("s2w-app-{name}-{}-{nanos}", std::process::id())),
            )
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ignored = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Stores one event under `source` whose cursor bytes are `cursor`.
    fn seed(directory: &Path, source: &str, cursor: &[u8]) {
        let appended = SourceId::new(source)
            .map_err(|error| error.to_string())
            .and_then(|source| {
                let cursor = Cursor::new(cursor.to_vec()).map_err(|error| error.to_string())?;
                let mut log = SqliteEventLog::open(directory).map_err(|error| error.to_string())?;
                log.append(RawEvent {
                    source,
                    cursor,
                    received_at: Timestamp::from_millis(1),
                    payload: b"{}".to_vec(),
                })
                .map_err(|error| error.to_string())
            });
        if let Err(error) = appended {
            panic!("seeding the log should succeed: {error}");
        }
    }

    fn fnv1a64_hex(bytes: &[u8]) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{hash:016x}")
    }

    // Each case below fails before any network connection is attempted, so these run offline.

    #[test]
    fn since_with_a_stored_cursor_is_a_usage_error() {
        let directory = TestDirectory::new("since-and-cursor");
        seed(
            directory.path(),
            WIKIPEDIA_SOURCE,
            br#"[{"topic":"eqiad.mediawiki.page_change.v1","partition":0,"timestamp":1}]"#,
        );
        let outcome = watch(
            WatchArgs {
                uri: "wikipedia".to_owned(),
                since: Some("2026-09-27T00:00:00Z".to_owned()),
                log_dir: directory.path().to_path_buf(),
                json: false,
            },
            &mut HumanReporter,
        );
        match outcome {
            Err(AppError::Usage(message)) => {
                assert!(
                    message.contains("drop --since"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("--since with a stored cursor must be a usage error, got {other:?}"),
        }
    }

    #[test]
    fn a_stored_cursor_that_is_not_utf8_is_a_loud_error() {
        let directory = TestDirectory::new("cursor-not-utf8");
        seed(directory.path(), WIKIPEDIA_SOURCE, &[0xff, 0xfe]);
        let outcome = watch(
            WatchArgs {
                uri: "wikipedia".to_owned(),
                since: None,
                log_dir: directory.path().to_path_buf(),
                json: false,
            },
            &mut HumanReporter,
        );
        assert!(
            matches!(
                outcome,
                Err(AppError::Source(SourceError::StoredCursor { .. }))
            ),
            "a non-UTF-8 cursor must never fall back to a fresh start, got {outcome:?}"
        );
    }

    #[test]
    fn a_stored_cursor_that_does_not_parse_is_a_loud_error() {
        let directory = TestDirectory::new("cursor-unparseable");
        seed(directory.path(), WIKIPEDIA_SOURCE, b"not a last-event-id");
        let outcome = watch(
            WatchArgs {
                uri: "wikipedia".to_owned(),
                since: None,
                log_dir: directory.path().to_path_buf(),
                json: false,
            },
            &mut HumanReporter,
        );
        assert!(
            matches!(
                outcome,
                Err(AppError::Source(SourceError::StoredCursor { .. }))
            ),
            "an unparseable cursor must never fall back to a fresh start, got {outcome:?}"
        );
    }

    #[test]
    fn kafka_since_with_a_stored_partition_cursor_is_a_usage_error() {
        // Mirrors s2w_sources::kafka's private `cluster_id`: a length-prefixed hash of the
        // sorted broker list, so the seeded source id matches what watch() will look up.
        let broker = "127.0.0.1:1";
        let cluster = fnv1a64_hex(format!("{}:{broker}", broker.len()).as_bytes());
        let directory = TestDirectory::new("kafka-since-and-cursor");
        seed(
            directory.path(),
            &format!("kafka.{cluster}.orders.p0"),
            b"41",
        );
        let outcome = watch(
            WatchArgs {
                uri: "kafka://127.0.0.1:1/orders".to_owned(),
                since: Some("2026-09-27T00:00:00Z".to_owned()),
                log_dir: directory.path().to_path_buf(),
                json: false,
            },
            &mut HumanReporter,
        );
        assert!(
            matches!(&outcome, Err(AppError::Usage(message)) if message.contains("drop --since")),
            "got {outcome:?}"
        );
    }

    #[test]
    fn an_unknown_uri_is_a_usage_error() {
        let directory = TestDirectory::new("unknown-uri");
        let outcome = watch(
            WatchArgs {
                uri: "kafka".to_owned(),
                since: None,
                log_dir: directory.path().to_path_buf(),
                json: false,
            },
            &mut HumanReporter,
        );
        assert!(
            matches!(&outcome, Err(AppError::Usage(message)) if message.contains("kafka://")),
            "got {outcome:?}"
        );
    }

    #[test]
    fn invalid_wikipedia_since_is_a_usage_error_without_a_log_dir_prefix() {
        let directory = TestDirectory::new("wikipedia-invalid-since");
        let outcome = watch(
            WatchArgs {
                uri: "wikipedia".to_owned(),
                since: Some("2026-09-27".to_owned()),
                log_dir: directory.path().to_path_buf(),
                json: false,
            },
            &mut HumanReporter,
        );
        assert!(
            matches!(&outcome, Err(AppError::Usage(message))
                if message.contains("invalid --since") && !message.contains("--log-dir")),
            "got {outcome:?}"
        );
    }

    #[test]
    fn invalid_kafka_since_is_a_usage_error_without_a_log_dir_prefix() {
        let directory = TestDirectory::new("kafka-invalid-since");
        let outcome = watch(
            WatchArgs {
                uri: "kafka://127.0.0.1:1/orders".to_owned(),
                since: Some("yesterday".to_owned()),
                log_dir: directory.path().to_path_buf(),
                json: false,
            },
            &mut HumanReporter,
        );
        assert!(
            matches!(&outcome, Err(AppError::Usage(message))
                if message.contains("invalid --since") && !message.contains("--log-dir")),
            "got {outcome:?}"
        );
    }

    #[test]
    fn stored_sse_cursor_is_sent_as_last_event_id_and_the_event_is_logged() {
        run(false, async {
            let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
                Ok(listener) => listener,
                // The managed implementation sandbox forbids all socket syscalls, including
                // loopback. Normal CI and developer machines run the assertion below.
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
                Err(error) => panic!("loopback listener should bind: {error}"),
            };
            let address = listener
                .local_addr()
                .expect("listener should have an address");
            let uri = format!("http://{address}/events");
            // Mirrors the registry's private generic-SSE identity for this fixed loopback URL.
            let source_id = format!(
                "sse.127.0.0.1_{}.events.{}",
                address.port(),
                fnv1a64_hex(uri.as_bytes())
            );
            let directory = TestDirectory::new("sse-positive-resume");
            seed(directory.path(), &source_id, b"resume-41");

            let (header_sender, header_receiver) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.expect("request should connect");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = socket
                        .read(&mut buffer)
                        .await
                        .expect("request should be readable");
                    assert!(read > 0, "request ended before its headers");
                    request.extend_from_slice(&buffer[..read]);
                }
                let request = String::from_utf8_lossy(&request);
                let last_event_id = request.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("last-event-id")
                        .then(|| value.trim().to_owned())
                });
                let _ignored = header_sender.send(last_event_id);
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\nid: next-42\ndata: {\"ok\":true}\n\n",
                    )
                    .await
                    .expect("response should be writable");
                std::future::pending::<()>().await;
            });

            let mut log = SqliteEventLog::open(directory.path()).expect("log should reopen");
            let source = resolve(&uri, None).expect("loopback URL should resolve");
            let started = source
                .start(None, &LogCursors(&log))
                .await
                .expect("source should start from its stored cursor");
            let header = tokio::time::timeout(Duration::from_secs(2), header_receiver)
                .await
                .expect("request should arrive")
                .expect("server should report the header");
            assert_eq!(header.as_deref(), Some("resume-41"));

            let mut stream = started.stream;
            let event = tokio::time::timeout(Duration::from_secs(2), stream.next())
                .await
                .expect("event should arrive")
                .expect("source should stay open")
                .expect("frame should be accepted");
            log.append(event).expect("event should append");

            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            loop {
                let appeared = log
                    .replay(None)
                    .expect("log should replay")
                    .filter_map(Result::ok)
                    .any(|stored| stored.event.cursor.as_bytes() == b"next-42");
                if appeared {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the loopback event did not appear in the SQLite log"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }

            server.abort();
            let _ignored = server.await;
        });
    }
}
