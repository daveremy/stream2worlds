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
