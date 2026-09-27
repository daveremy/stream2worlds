//! Stream sources. Each yields raw events with a source cursor; none joins a consumer group or commits offsets.

pub mod kafka;
pub mod wikipedia;

use s2w_model::Timestamp;
use wikipedia::LastEventId;

/// A raw source event paired with the cursor needed to resume after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEvent {
    /// The raw JSON text from the SSE `data:` field.
    pub payload: String,
    /// The structured Wikimedia `Last-Event-ID` for this event.
    pub cursor: LastEventId,
    /// The wall-clock time at which the source finished decoding the event.
    pub received_at: Timestamp,
}
