//! The seam every adapter implements: [`Source`] starts a stream of [`RawEvent`]s after
//! resolving any stored cursor against `--since`.
//!
//! Adapters (`kafka`, `sse`, `stdin`) are transports; presets (`wikipedia`) are data over an
//! adapter. The app composes a source with the log without knowing which one it has.

use std::future::Future;
use std::pin::Pin;

use s2w_model::{Cursor, ModelError, RawEvent, SourceId};
use tokio_stream::Stream;

/// The running stream a started source yields. Current-thread runtime, one consumer: no `Send`.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<RawEvent, SourceError>>>>;

/// The future [`Source::start`] returns.
pub type StartFuture<'a> = Pin<Box<dyn Future<Output = Result<Started, SourceError>> + 'a>>;

/// Read-only view of the log's stored cursors. `s2w-app` implements it for its event log, so
/// adapters never depend on `s2w-log`.
pub trait CursorLookup {
    /// The stored cursor for `source`, if the log holds one.
    ///
    /// # Errors
    ///
    /// [`SourceError::Lookup`] when the log cannot be read.
    fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, SourceError>;
}

/// A started source.
pub struct Started {
    /// The events, each already carrying its source id and cursor.
    pub stream: EventStream,
    /// Whether the stream is expected to end on its own.
    pub ends: Ending,
    /// Lines for stderr about how the source started (fresh partitions, resume points).
    pub notes: Vec<String>,
}

impl std::fmt::Debug for Started {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Started")
            .field("ends", &self.ends)
            .field("notes", &self.notes)
            .finish_non_exhaustive()
    }
}

/// Whether a source's stream is expected to end on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The source runs until stopped; an ended stream is a bug.
    Never,
    /// The source reads a finite input; its stream ending is success (stdin at EOF).
    AtEndOfInput,
}

/// A stream source, resolved from a URI by [`crate::registry::resolve`].
pub trait Source {
    /// Human name for messages: "wikipedia", "kafka", "stdin", "sse".
    fn name(&self) -> &'static str;

    /// Resolves stored cursors against `since`, connects, and returns the running stream.
    /// A stored cursor plus `since` is [`SourceError::SinceWithStoredCursor`], never a silent
    /// ignore; a stored cursor that cannot be decoded is an error, never a fresh start.
    fn start<'a>(
        self: Box<Self>,
        since: Option<&'a str>,
        cursors: &'a dyn CursorLookup,
    ) -> StartFuture<'a>;
}

/// Failures from starting or running a source. Only [`SourceError::Skipped`] and
/// [`SourceError::Retrying`] let the stream continue.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// `--since` was given for a source that already has a stored cursor.
    #[error(
        "a stored cursor already exists for {source_id}; drop --since to resume from it, or use a fresh --log-dir to replay from {since:?}"
    )]
    SinceWithStoredCursor {
        /// The source id holding the cursor.
        source_id: String,
        /// The `--since` value given.
        since: String,
    },
    /// The source has no notion of a start time.
    #[error("--since does not apply to {name}: {reason}")]
    SinceUnsupported {
        /// The source name.
        name: &'static str,
        /// Why, and what to do instead.
        reason: String,
    },
    /// A stored cursor is not in the form this source writes.
    #[error("stored cursor {cursor:?} for {source_id} cannot be decoded: {reason}")]
    StoredCursor {
        /// The source id holding the cursor.
        source_id: String,
        /// The stored bytes, lossily decoded for the message.
        cursor: String,
        /// Why it cannot be decoded.
        reason: String,
    },
    /// A stored cursor decodes but the stream can no longer resume from it.
    #[error(
        "stored cursor for {source_id} cannot resume this stream: {reason}; use a fresh --log-dir to start over"
    )]
    CursorUnresumable {
        /// The source id holding the cursor.
        source_id: String,
        /// Why it cannot resume.
        reason: String,
    },
    /// The `--since` value is not in a form this source accepts.
    #[error("{name}: invalid --since {value:?}: {reason}")]
    InvalidSince {
        /// The source name.
        name: &'static str,
        /// The value given.
        value: String,
        /// What the source accepts.
        reason: String,
    },
    /// The URI names this source but is malformed.
    #[error("{name}: invalid target {uri:?}: {reason}")]
    InvalidTarget {
        /// The source name.
        name: &'static str,
        /// The URI given.
        uri: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The source cannot start, or stopped: unreachable broker, missing topic, stdin I/O.
    #[error("{name}: {reason}")]
    Fatal {
        /// The source name.
        name: &'static str,
        /// What happened.
        reason: String,
    },
    /// One bad frame or line, reported by the pump; the stream continues.
    #[error("{name}: {reason}")]
    Skipped {
        /// The source name.
        name: &'static str,
        /// What was skipped and why.
        reason: String,
    },
    /// A transient failure (a fetch, a dropped connection); the source retries from the same
    /// position, so nothing is skipped. Reported by the pump; the stream continues.
    #[error("{name}: {reason}")]
    Retrying {
        /// The source name.
        name: &'static str,
        /// What failed and is being retried.
        reason: String,
    },
    /// The log's stored cursors could not be read.
    #[error("cursor lookup: {0}")]
    Lookup(String),
    /// A model value was rejected.
    #[error(transparent)]
    Model(#[from] ModelError),
}

impl SourceError {
    /// Whether this error stops the stream. [`SourceError::Skipped`] and
    /// [`SourceError::Retrying`] do not.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        !matches!(self, Self::Skipped { .. } | Self::Retrying { .. })
    }

    /// Whether this error means the command was given wrongly (the CLI's usage exit code).
    #[must_use]
    pub fn is_usage(&self) -> bool {
        matches!(
            self,
            Self::SinceWithStoredCursor { .. } | Self::SinceUnsupported { .. }
        )
    }
}

/// Checks `ids` for a stored cursor when `since` is given: any stored cursor plus `since` is
/// [`SourceError::SinceWithStoredCursor`].
///
/// # Errors
///
/// That error, or a failed lookup.
pub fn refuse_since_with_stored(
    since: Option<&str>,
    cursors: &dyn CursorLookup,
    ids: &[SourceId],
) -> Result<(), SourceError> {
    let Some(since) = since else {
        return Ok(());
    };
    for id in ids {
        if cursors.cursor(id)?.is_some() {
            return Err(SourceError::SinceWithStoredCursor {
                source_id: id.as_str().to_owned(),
                since: since.to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SourceError;

    #[test]
    fn only_skipped_and_retrying_are_non_fatal() {
        let fatal = [
            SourceError::SinceWithStoredCursor {
                source_id: "s".into(),
                since: "s".into(),
            },
            SourceError::SinceUnsupported {
                name: "n",
                reason: "r".into(),
            },
            SourceError::StoredCursor {
                source_id: "s".into(),
                cursor: "c".into(),
                reason: "r".into(),
            },
            SourceError::CursorUnresumable {
                source_id: "s".into(),
                reason: "r".into(),
            },
            SourceError::InvalidSince {
                name: "n",
                value: "v".into(),
                reason: "r".into(),
            },
            SourceError::InvalidTarget {
                name: "n",
                uri: "u".into(),
                reason: "r".into(),
            },
            SourceError::Fatal {
                name: "n",
                reason: "r".into(),
            },
            SourceError::Lookup("l".into()),
            SourceError::Model(s2w_model::ModelError::EmptyCursor),
        ];
        for variant in fatal {
            assert!(variant.is_fatal(), "{variant:?} must be fatal");
        }
        for variant in [
            SourceError::Skipped {
                name: "n",
                reason: "r".into(),
            },
            SourceError::Retrying {
                name: "n",
                reason: "r".into(),
            },
        ] {
            assert!(!variant.is_fatal(), "{variant:?} must not be fatal");
        }
    }
}
