//! Runtime wiring: composes sources, the log, the core and the engines, and serves the read-only MCP server and the local web view.

mod group_commit;
pub mod mcp;
pub mod query;

/// The hub in-degree cap a served timeline starts under, re-exported so the CLI can build the
/// MCP server's empty world without depending on the core itself.
pub use s2w_core::DEFAULT_HUB_IN_DEGREE_CAP;

use std::path::PathBuf;

use s2w_log::{AppendOutcome, EventLog, SqliteEventLog};
use s2w_model::{Cursor, RawEvent, SourceId};
use s2w_sources::registry::resolve;
use s2w_sources::source::{CursorLookup, Ending, SourceError};
use s2w_sources::wikipedia::{LastEventId, WikipediaSource, WikipediaSourceError};
use tokio_stream::StreamExt;

/// The source id under which `s2w watch wikipedia` files its events.
const WIKIPEDIA_SOURCE: &str = "wikipedia.page_change";

/// Arguments for `s2w watch wikipedia`.
#[derive(Debug, Clone)]
pub struct WatchWikipediaArgs {
    /// An ISO-8601 timestamp passed to Wikimedia as `since=`, used only when the log holds no
    /// stored cursor for the source yet.
    pub since: Option<String>,
    /// The directory holding (or creating) the SQLite event log.
    pub log_dir: PathBuf,
}

/// Failures from wiring and running a watch command.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The event log could not be opened, read or appended to.
    #[error("event log: {0}")]
    Log(#[from] s2w_log::LogError),
    /// The Wikipedia source could not be started, or its cursor could not be decoded.
    #[error("wikipedia source: {0}")]
    Wikipedia(#[from] WikipediaSourceError),
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
    /// A stored cursor could not be decoded as the UTF-8 `Last-Event-ID` the source stores.
    #[error("stored wikipedia cursor is not valid UTF-8: {0}")]
    StoredCursorUtf8(#[from] std::str::Utf8Error),
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
#[derive(Debug, Clone)]
pub struct WatchArgs {
    /// The source URI: a preset name, `-`, or `<scheme>://…` (see `s2w_sources::registry`).
    pub uri: String,
    /// Where a source with no stored cursor starts, in the source's own form.
    pub since: Option<String>,
    /// The directory holding (or creating) the SQLite event log.
    pub log_dir: PathBuf,
}

/// Runs `s2w watch <source-uri>` until the source ends or the process is stopped.
///
/// # Errors
///
/// [`AppError::Usage`] for an unknown URI, or `--since` the source cannot honour (a stored
/// cursor wins over it); otherwise the first failure that should stop the watch. A skipped
/// frame or line is reported on stderr and does not stop it.
pub fn watch(args: WatchArgs) -> Result<(), AppError> {
    current_thread_runtime()?.block_on(run_watch(args))
}

/// [`watch`] on the caller's runtime (current-thread: the stream is not `Send`).
///
/// # Errors
///
/// As [`watch`].
pub async fn run_watch(args: WatchArgs) -> Result<(), AppError> {
    let source = resolve(&args.uri).map_err(|error| AppError::Usage(error.to_string()))?;
    let mut log = SqliteEventLog::open(&args.log_dir)?;
    let name = source.name();
    let started = source
        .start(args.since.as_deref(), &LogCursors(&log))
        .await
        .map_err(|error| {
            if error.is_usage() {
                AppError::Usage(format!("--log-dir {}: {error}", args.log_dir.display()))
            } else {
                AppError::Source(error)
            }
        })?;
    for note in &started.notes {
        eprintln!("s2w: {name}: {note}");
    }
    group_commit::pump_events(&mut log, started.stream).await?;
    match started.ends {
        Ending::AtEndOfInput => Ok(()),
        Ending::Never => Err(AppError::StreamEnded(name)),
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

/// Runs `s2w watch wikipedia` until the process is stopped.
///
/// Builds a current-thread Tokio runtime (enough for one source) and blocks on
/// [`run_wikipedia`]. Every event is appended to the durable log in its own transaction;
/// redeliveries are collapsed by the log and reported on stderr.
///
/// # Errors
///
/// Returns the first failure that should stop the watch: a log that cannot be opened or
/// appended to, a cursor that cannot be decoded, or a source that cannot start. A single
/// malformed frame from the stream does not stop it.
pub fn watch_wikipedia(args: WatchWikipediaArgs) -> Result<(), AppError> {
    current_thread_runtime()?.block_on(run_wikipedia(args))
}

/// A current-thread Tokio runtime with the I/O and timer drivers: enough for one source.
fn current_thread_runtime() -> Result<tokio::runtime::Runtime, AppError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(AppError::Runtime)
}

/// Composes the Wikipedia source with the durable log and consumes the stream.
///
/// A stored cursor wins over `--since`: passing both is refused rather than silently ignored.
async fn run_wikipedia(args: WatchWikipediaArgs) -> Result<(), AppError> {
    let mut log = SqliteEventLog::open(&args.log_dir)?;
    let source_id = SourceId::new(WIKIPEDIA_SOURCE)?;
    let mut source = match log.cursor(&source_id)? {
        Some(stored) => {
            if let Some(since) = &args.since {
                return Err(AppError::Usage(format!(
                    "a stored cursor already exists for --log-dir {}; drop --since to resume from it, or use a fresh --log-dir to replay history from {since:?}",
                    args.log_dir.display()
                )));
            }
            let raw = std::str::from_utf8(stored.as_bytes())?;
            let last_event_id = LastEventId::parse(raw)?;
            WikipediaSource::resume(last_event_id)?
        }
        None => WikipediaSource::new(args.since.clone())?,
    };

    // Appends run on the runtime's only thread, so each synchronous=FULL commit pauses reads
    // from the stream. Nothing is lost: the source's bounded channel and TCP backpressure hold
    // the stream. Moving writes off this thread belongs with group commit (#7).
    while let Some(item) = source.next().await {
        let event = match item {
            Ok(event) => event,
            // One malformed frame is logged and skipped; the stream itself keeps going.
            Err(error) => {
                eprintln!("s2w: wikipedia source error: {error}");
                continue;
            }
        };
        let raw = RawEvent {
            source: source_id.clone(),
            cursor: Cursor::new(event.cursor.as_header_value().as_bytes().to_vec())?,
            received_at: event.received_at,
            payload: event.payload.into_bytes(),
        };
        match log.append(raw)? {
            AppendOutcome::Inserted(_) => {}
            AppendOutcome::Duplicate(position) => {
                eprintln!(
                    "s2w: duplicate wikipedia event collapsed at log position {}",
                    position.as_u64()
                );
            }
        }
    }
    Err(AppError::StreamEnded("wikipedia"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use s2w_log::{EventLog, SqliteEventLog};
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};

    use super::{AppError, WIKIPEDIA_SOURCE, WatchWikipediaArgs, watch_wikipedia};

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

    /// Stores one Wikipedia event whose cursor bytes are `cursor`, so the log holds a cursor.
    fn seed_cursor(directory: &Path, cursor: &[u8]) {
        let appended = SourceId::new(WIKIPEDIA_SOURCE)
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

    // Each case below fails before any network connection is attempted, so these run offline.

    #[test]
    fn since_with_a_stored_cursor_is_a_usage_error() {
        let directory = TestDirectory::new("since-and-cursor");
        seed_cursor(
            directory.path(),
            br#"[{"topic":"eqiad.mediawiki.page_change.v1","partition":0,"timestamp":1}]"#,
        );
        let outcome = watch_wikipedia(WatchWikipediaArgs {
            since: Some("2026-09-27T00:00:00Z".to_owned()),
            log_dir: directory.path().to_path_buf(),
        });
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
        seed_cursor(directory.path(), &[0xff, 0xfe]);
        let outcome = watch_wikipedia(WatchWikipediaArgs {
            since: None,
            log_dir: directory.path().to_path_buf(),
        });
        assert!(
            matches!(outcome, Err(AppError::StoredCursorUtf8(_))),
            "a non-UTF-8 cursor must never fall back to a fresh start, got {outcome:?}"
        );
    }

    #[test]
    fn a_stored_cursor_that_does_not_parse_is_a_loud_error() {
        let directory = TestDirectory::new("cursor-unparseable");
        seed_cursor(directory.path(), b"not a last-event-id");
        let outcome = watch_wikipedia(WatchWikipediaArgs {
            since: None,
            log_dir: directory.path().to_path_buf(),
        });
        assert!(
            matches!(outcome, Err(AppError::Wikipedia(_))),
            "an unparseable cursor must never fall back to a fresh start, got {outcome:?}"
        );
    }
}
