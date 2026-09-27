//! Runtime wiring: composes sources, the log, the core and the engines, and serves the read-only MCP server and the local web view.

use std::path::PathBuf;

use s2w_log::{AppendOutcome, EventLog, SqliteEventLog};
use s2w_model::{Cursor, RawEvent, SourceId};
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
    Source(#[from] WikipediaSourceError),
    /// A model value was rejected.
    #[error("model: {0}")]
    Model(#[from] s2w_model::ModelError),
    /// The async runtime could not be built.
    #[error("could not build the async runtime: {0}")]
    Runtime(#[from] std::io::Error),
    /// A stored cursor could not be decoded as the UTF-8 `Last-Event-ID` the source stores.
    #[error("stored wikipedia cursor is not valid UTF-8: {0}")]
    StoredCursorUtf8(#[from] std::str::Utf8Error),
    /// The source's stream ended. Wikimedia keeps this stream open and drops it only to
    /// reconnect it, so an ended stream means something is wrong, not that the work is done.
    #[error("the wikipedia source stream ended unexpectedly; it should reconnect until stopped")]
    StreamEnded,
    /// The command cannot run as given, and says what to try instead. The binary maps this to
    /// its usage exit code.
    #[error("{0}")]
    Usage(String),
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_wikipedia(args))
}

/// Composes the Wikipedia source with the durable log and consumes the stream.
///
/// A stored cursor wins over `--since`: passing both is refused rather than silently ignored.
async fn run_wikipedia(args: WatchWikipediaArgs) -> Result<(), AppError> {
    let mut log = SqliteEventLog::open(&args.log_dir)?;
    let source_id = SourceId::new(WIKIPEDIA_SOURCE)?;
    let source = match log.cursor(&source_id)? {
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

    let mut source = source;
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
    Err(AppError::StreamEnded)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use s2w_log::{EventLog, SqliteEventLog};
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};

    use super::{AppError, WIKIPEDIA_SOURCE, WatchWikipediaArgs, watch_wikipedia};

    /// A per-test scratch directory under the system temp dir, removed on drop.
    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            Self(
                std::env::temp_dir().join(format!("s2w-app-{name}-{}-{nanos}", std::process::id())),
            )
        }

        fn path(&self) -> &Path {
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
            matches!(outcome, Err(AppError::Source(_))),
            "an unparseable cursor must never fall back to a fresh start, got {outcome:?}"
        );
    }
}
