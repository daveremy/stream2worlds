//! `s2w watch -`: newline-delimited JSON from stdin until end of input.

use std::path::PathBuf;

use s2w_log::{EventLog, SqliteEventLog};
use s2w_model::{Cursor, RawEvent, SourceId};
use s2w_sources::ndjson::{NdjsonEvent, NdjsonSource, NdjsonSourceError};
use tokio::io::AsyncBufRead;

use crate::{AppError, group_commit};

/// The source id under which `s2w watch -` files its events.
const STDIN_SOURCE: &str = "stdin";

/// Arguments for `s2w watch -`.
#[derive(Debug, Clone)]
pub struct WatchStdinArgs {
    /// The directory holding (or creating) the SQLite event log.
    pub log_dir: PathBuf,
}

/// Runs `s2w watch -` until stdin ends.
///
/// Stdin cannot seek, so there is no resume: the cursor is the line number, kept as
/// provenance. Piping the same input again stores nothing new, because the log collapses
/// byte-identical payloads from one source.
///
/// # Errors
///
/// Returns a log that cannot be opened or written, or a read failure. A line that is not JSON
/// is reported and skipped.
pub fn watch_stdin(args: WatchStdinArgs) -> Result<(), AppError> {
    crate::current_thread_runtime()?.block_on(async {
        let mut log = SqliteEventLog::open(&args.log_dir)?;
        run_ndjson(&mut log, tokio::io::BufReader::new(tokio::io::stdin())).await
    })
}

/// Consumes NDJSON from `reader` into `log` with group commit; `Ok` at end of input.
async fn run_ndjson<L, R>(log: &mut L, reader: R) -> Result<(), AppError>
where
    L: EventLog,
    R: AsyncBufRead + Unpin,
{
    let source_id = SourceId::new(STDIN_SOURCE)?;
    group_commit::pump(
        log,
        NdjsonSource::new(reader),
        |event: NdjsonEvent| {
            Ok(RawEvent {
                source: source_id.clone(),
                cursor: Cursor::new(event.line.to_string().into_bytes())?,
                received_at: event.received_at,
                payload: event.payload.into_bytes(),
            })
        },
        |error: NdjsonSourceError| {
            if error.is_fatal() {
                Err(error.into())
            } else {
                eprintln!("s2w: stdin: {error}");
                Ok(())
            }
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use s2w_log::{EventLog, InMemoryEventLog};
    use s2w_model::SourceId;

    use super::{STDIN_SOURCE, run_ndjson};

    #[test]
    fn stores_every_json_line_skips_the_rest_and_ends_ok() {
        crate::tests::run(false, async {
            let mut log = InMemoryEventLog::default();
            let input: &[u8] = b"{\"a\":1}\nnot json\n\n{\"a\":2}\n{\"a\":1}\n";
            let outcome = run_ndjson(&mut log, input).await;
            assert!(outcome.is_ok(), "got {outcome:?}");

            let stored: Vec<(Vec<u8>, Vec<u8>)> = log
                .replay(None)
                .into_iter()
                .flatten()
                .flatten()
                .map(|stored| {
                    (
                        stored.event.payload,
                        stored.event.cursor.as_bytes().to_vec(),
                    )
                })
                .collect();
            assert_eq!(
                stored,
                vec![
                    (b"{\"a\":1}".to_vec(), b"1".to_vec()),
                    (b"{\"a\":2}".to_vec(), b"4".to_vec()),
                ],
                "the repeated line collapses; cursors are physical line numbers"
            );
            let cursor = SourceId::new(STDIN_SOURCE)
                .ok()
                .and_then(|source| log.cursor(&source).ok().flatten());
            assert_eq!(cursor.map(|c| c.as_bytes().to_vec()), Some(b"4".to_vec()));
        });
    }
}
