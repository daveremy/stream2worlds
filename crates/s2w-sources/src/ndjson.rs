//! Newline-delimited JSON from any async reader; `s2w watch -` feeds it stdin.
//!
//! Each non-blank line must be one JSON value. The line's bytes are the event payload, unchanged,
//! so the log's content-hash dedupe collapses identical lines and re-piping a file is idempotent.
//! The cursor is the physical line number (blank lines count). Stdin cannot seek, so the cursor
//! records provenance and is never a resume point.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use s2w_model::Timestamp;
use tokio::io::AsyncBufRead;
use tokio_stream::Stream;
use tokio_stream::wrappers::LinesStream;

/// One NDJSON line, ready for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NdjsonEvent {
    /// The 1-based physical line number.
    pub line: u64,
    /// The line's text, without its line ending.
    pub payload: String,
    /// The wall-clock time at which the line was read.
    pub received_at: Timestamp,
}

/// Failures from the NDJSON source.
#[derive(Debug, thiserror::Error)]
pub(crate) enum NdjsonSourceError {
    /// A line is not valid JSON. It is reported and skipped; reading continues.
    #[error("line {line} is not valid JSON and was skipped: {reason}")]
    InvalidJson {
        /// The 1-based physical line number.
        line: u64,
        /// The parser's complaint.
        reason: String,
    },
    /// A line is not valid UTF-8. It is reported and skipped; reading continues.
    #[error("line {line} is not valid UTF-8 and was skipped")]
    InvalidUtf8 {
        /// The 1-based physical line number.
        line: u64,
    },
    /// Reading failed. The source stops.
    #[error("reading line {line} failed: {message}")]
    Io {
        /// The 1-based line number that could not be read.
        line: u64,
        /// The reader's error.
        message: String,
    },
}

impl NdjsonSourceError {
    /// Whether the source has stopped because of this error.
    #[must_use]
    pub(crate) fn is_fatal(&self) -> bool {
        matches!(self, Self::Io { .. })
    }
}

/// A stream of NDJSON events that ends at end of input.
#[derive(Debug)]
pub(crate) struct NdjsonSource<R> {
    lines: LinesStream<R>,
    line: u64,
    stopped: bool,
}

impl<R: AsyncBufRead> NdjsonSource<R> {
    /// Reads NDJSON from `reader`.
    pub(crate) fn new(reader: R) -> Self {
        use tokio::io::AsyncBufReadExt;
        Self {
            lines: LinesStream::new(reader.lines()),
            line: 0,
            stopped: false,
        }
    }
}

impl<R: AsyncBufRead + Unpin> Stream for NdjsonSource<R> {
    type Item = Result<NdjsonEvent, NdjsonSourceError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if self.stopped {
                return Poll::Ready(None);
            }
            let next = match Pin::new(&mut self.lines).poll_next(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(next) => next,
            };
            self.line = self.line.saturating_add(1);
            let line = self.line;
            let text = match next {
                None => return Poll::Ready(None),
                Some(Ok(text)) => text,
                Some(Err(error)) if error.kind() == std::io::ErrorKind::InvalidData => {
                    return Poll::Ready(Some(Err(NdjsonSourceError::InvalidUtf8 { line })));
                }
                Some(Err(error)) => {
                    self.stopped = true;
                    return Poll::Ready(Some(Err(NdjsonSourceError::Io {
                        line,
                        message: error.to_string(),
                    })));
                }
            };
            if text.trim().is_empty() {
                continue;
            }
            if let Err(error) = serde_json::from_str::<serde_json::Value>(&text) {
                return Poll::Ready(Some(Err(NdjsonSourceError::InvalidJson {
                    line,
                    reason: error.to_string(),
                })));
            }
            return Poll::Ready(Some(Ok(NdjsonEvent {
                line,
                payload: text,
                received_at: now(),
            })));
        }
    }
}

fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        });
    Timestamp::from_millis(millis)
}

#[cfg(test)]
mod tests {
    use tokio_stream::StreamExt;

    use super::{NdjsonSource, NdjsonSourceError};

    fn collect(input: &'static [u8]) -> Vec<Result<(u64, String), String>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap_or_else(|error| panic!("runtime: {error}"));
        runtime.block_on(async {
            NdjsonSource::new(input)
                .map(|item| {
                    item.map(|event| (event.line, event.payload))
                        .map_err(|error: NdjsonSourceError| error.to_string())
                })
                .collect()
                .await
        })
    }

    #[test]
    fn lines_carry_physical_line_numbers_and_raw_bytes() {
        let items = collect(b"{\"b\":1, \"a\":2}\n\n  \n[1,2]\r\n\"x\"");
        assert_eq!(
            items,
            vec![
                Ok((1, "{\"b\":1, \"a\":2}".to_owned())),
                Ok((4, "[1,2]".to_owned())),
                Ok((5, "\"x\"".to_owned())),
            ]
        );
    }

    #[test]
    fn invalid_lines_are_reported_and_reading_continues() {
        let items = collect(b"{\"ok\":1}\nnot json\n\xff\xfe\n{\"ok\":2}\n");
        assert_eq!(items.len(), 4);
        assert_eq!(items[0], Ok((1, "{\"ok\":1}".to_owned())));
        assert!(
            matches!(&items[1], Err(message) if message.starts_with("line 2 is not valid JSON"))
        );
        assert!(
            matches!(&items[2], Err(message) if message.starts_with("line 3 is not valid UTF-8"))
        );
        assert_eq!(items[3], Ok((4, "{\"ok\":2}".to_owned())));
    }

    #[test]
    fn empty_input_ends_immediately() {
        assert!(collect(b"").is_empty());
    }
}
