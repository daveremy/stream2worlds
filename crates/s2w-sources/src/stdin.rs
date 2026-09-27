//! The `-` adapter: newline-delimited JSON from stdin until end of input.
//!
//! Stdin cannot seek, so there is no resume and no `--since`: the cursor is the physical line
//! number, kept as provenance. Piping the same input again stores nothing new, because the log
//! collapses byte-identical payloads from one source.

use s2w_model::{Cursor, RawEvent, SourceId};
use tokio::io::AsyncBufRead;
use tokio_stream::StreamExt;

use crate::ndjson::{NdjsonEvent, NdjsonSource, NdjsonSourceError};
use crate::source::{CursorLookup, Ending, Source, SourceError, StartFuture, Started};

/// The source id under which `s2w watch -` files its events.
const STDIN_SOURCE: &str = "stdin";

/// The adapter's name in messages.
const NAME: &str = "stdin";

/// NDJSON read from a byte stream, stdin in production.
pub struct StdinSource {
    reader: Box<dyn AsyncBufRead + Unpin>,
}

impl StdinSource {
    /// Reads the process's stdin.
    #[must_use]
    pub fn from_stdin() -> Self {
        Self::from_reader(tokio::io::BufReader::new(tokio::io::stdin()))
    }

    /// Reads `reader`; tests feed it bytes.
    #[must_use]
    pub fn from_reader(reader: impl AsyncBufRead + Unpin + 'static) -> Self {
        Self {
            reader: Box::new(reader),
        }
    }
}

impl Source for StdinSource {
    fn name(&self) -> &'static str {
        NAME
    }

    fn start<'a>(
        self: Box<Self>,
        since: Option<&'a str>,
        _cursors: &'a dyn CursorLookup,
    ) -> StartFuture<'a> {
        Box::pin(async move {
            if since.is_some() {
                return Err(SourceError::SinceUnsupported {
                    name: NAME,
                    reason: "stdin cannot seek; pipe the part you want instead".to_owned(),
                });
            }
            let source_id = SourceId::new(STDIN_SOURCE)?;
            let stream = NdjsonSource::new(self.reader).map(move |item| raw(&source_id, item));
            Ok(Started {
                stream: Box::pin(stream),
                ends: Ending::AtEndOfInput,
                notes: Vec::new(),
            })
        })
    }
}

/// One NDJSON item as a log event, or the error the pump reports or stops on.
fn raw(
    source: &SourceId,
    item: Result<NdjsonEvent, NdjsonSourceError>,
) -> Result<RawEvent, SourceError> {
    match item {
        Ok(event) => Ok(RawEvent {
            source: source.clone(),
            cursor: Cursor::new(event.line.to_string().into_bytes())?,
            received_at: event.received_at,
            payload: event.payload.into_bytes(),
        }),
        Err(error) if error.is_fatal() => Err(SourceError::Fatal {
            name: NAME,
            reason: error.to_string(),
        }),
        Err(error) => Err(SourceError::Skipped {
            name: NAME,
            reason: error.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use s2w_model::{Cursor, SourceId};
    use tokio_stream::StreamExt;

    use super::StdinSource;
    use crate::source::{CursorLookup, Ending, Source, SourceError};

    /// A log with no stored cursors.
    struct NoCursors;

    impl CursorLookup for NoCursors {
        fn cursor(&self, _source: &SourceId) -> Result<Option<Cursor>, SourceError> {
            Ok(None)
        }
    }

    fn run<F: std::future::Future>(test: F) -> F::Output {
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(test),
            Err(error) => panic!("building the test runtime should succeed: {error}"),
        }
    }

    #[test]
    fn yields_every_json_line_skips_the_rest_and_ends() {
        run(async {
            let input: &[u8] = b"{\"a\":1}\nnot json\n\n{\"a\":2}\n";
            let source = Box::new(StdinSource::from_reader(input));
            let started = match source.start(None, &NoCursors).await {
                Ok(started) => started,
                Err(error) => panic!("stdin should start: {error}"),
            };
            assert_eq!(started.ends, Ending::AtEndOfInput);
            let items: Vec<_> = started.stream.collect().await;
            assert_eq!(items.len(), 3, "two events and one skipped line: {items:?}");
            let mut events = Vec::new();
            for item in items {
                match item {
                    Ok(event) => {
                        assert_eq!(event.source.as_str(), "stdin");
                        events.push((event.payload, event.cursor.as_bytes().to_vec()));
                    }
                    Err(error) => assert!(!error.is_fatal(), "only skips expected: {error}"),
                }
            }
            assert_eq!(
                events,
                vec![
                    (b"{\"a\":1}".to_vec(), b"1".to_vec()),
                    (b"{\"a\":2}".to_vec(), b"4".to_vec()),
                ],
                "cursors are physical line numbers"
            );
        });
    }

    #[test]
    fn since_is_unsupported() {
        run(async {
            let input: &[u8] = b"";
            let source = Box::new(StdinSource::from_reader(input));
            let outcome = source.start(Some("1"), &NoCursors).await;
            assert!(
                matches!(
                    outcome,
                    Err(SourceError::SinceUnsupported { name: "stdin", .. })
                ),
                "got {outcome:?}"
            );
        });
    }
}
