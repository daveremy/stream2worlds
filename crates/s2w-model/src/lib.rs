//! Shared vocabulary for every `s2w` crate.
//!
//! This crate depends on `serde`, `serde_json` (a mapping's canonical bytes, decision 0023) and
//! `thiserror` only, does no I/O and never reads a clock.
//! Types arrive here when a second crate needs them, not before.
#![deny(clippy::print_stdout, clippy::print_stderr)]

mod dashboard;
mod event;
mod hash;
mod mapping;
mod natural_key;
pub use dashboard::{
    AcceptedMapping, AttrLabel, BuiltOn, DASHBOARD_FORMAT, DashboardError, DashboardManifest,
    DeltaField, Domain, EventSentence, KeyLabel, Kind, Label, MAX_BUILT_ON, MAX_EVENTS,
    MAX_QUESTIONS, MAX_ROLES, MAX_SENTENCE_FIELDS, MAX_SLOT_ITEMS, MAX_STRING_CHARS, MAX_TYPES,
    ManifestContext, ManifestInput, ManifestOutcome, ManifestProposer, PathStats, Projection,
    ProposerId, ProposerTrace, QuintessentialProjection, Role, Sentence, SentenceField, Slots,
    SourceInput, TRUNCATE_CHARS, Template, TruncateField, TypeRow, When, fits_text,
    render_sentence, sentence_for,
};
pub use event::{AttrValue, WorldEvent};
pub use hash::{Fnv64, fnv1a64, fnv1a64_hex, is_hex16};
pub use mapping::{
    AttrRule, EntityRule, FieldPath, LinkRule, MAPPING_VERSION, MAPPING_VERSION_LINKS,
    MappingError, RelationshipRule, Segment, StreamMapping,
};
pub use natural_key::{KEY_FORMAT, KEY_SEPARATOR, KeyError, KeyPart, NaturalKey};

use serde::{Deserialize, Serialize};

/// A point in time, in milliseconds since the Unix epoch.
///
/// Always passed in from the imperative shell; nothing in the model or the core reads a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// A timestamp from milliseconds since the Unix epoch.
    #[must_use]
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// Milliseconds since the Unix epoch.
    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0
    }
}

/// The name of one configured source, such as `wikipedia.recentchange` or `kafka.orders`. <!-- vocabulary: allow -->
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SourceId(String);

impl SourceId {
    /// A source id: non-empty, at most 128 bytes, ASCII letters, digits, `.`, `-` and `_` only.
    ///
    /// # Errors
    /// Returns [`ModelError::InvalidSourceId`] when the name breaks those rules.
    pub fn new(name: impl Into<String>) -> Result<Self, ModelError> {
        let name = name.into();
        let valid = !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
        if valid {
            Ok(Self(name))
        } else {
            Err(ModelError::InvalidSourceId(name))
        }
    }

    /// The source id as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SourceId {
    type Error = ModelError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::new(name)
    }
}

impl From<SourceId> for String {
    fn from(id: SourceId) -> Self {
        id.0
    }
}

/// An opaque, source-defined resume position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub struct Cursor(Vec<u8>);

impl Cursor {
    /// A non-empty source cursor of at most 4096 bytes.
    ///
    /// # Errors
    /// Returns [`ModelError::EmptyCursor`] for an empty cursor and
    /// [`ModelError::CursorTooLarge`] when `bytes` exceeds 4096 bytes.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, ModelError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            Err(ModelError::EmptyCursor)
        } else if bytes.len() > 4096 {
            Err(ModelError::CursorTooLarge(bytes.len()))
        } else {
            Ok(Self(bytes))
        }
    }

    /// The source cursor as opaque bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for Cursor {
    type Error = ModelError;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::new(bytes)
    }
}

impl From<Cursor> for Vec<u8> {
    fn from(cursor: Cursor) -> Self {
        cursor.0
    }
}

/// An uninterpreted event received from a configured source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawEvent {
    /// The configured source that produced the event.
    pub source: SourceId,
    /// The source-defined position to resume after this event.
    pub cursor: Cursor,
    /// When the imperative shell received the event.
    pub received_at: Timestamp,
    /// Opaque event bytes; the log never interprets them.
    pub payload: Vec<u8>,
}

/// Errors from constructing model values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    /// A source id broke the naming rules.
    #[error(
        "invalid source id {0:?}: use 1-128 ASCII letters, digits, '.', '-' or '_', for example 'kafka.orders'"
    )]
    InvalidSourceId(String),
    /// A source cursor was empty.
    #[error("a source cursor must not be empty")]
    EmptyCursor,
    /// A source cursor exceeded the defensive size cap.
    #[error("source cursor is {0} bytes; the maximum is 4096")]
    CursorTooLarge(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_id_accepts_the_documented_shape() {
        assert_eq!(
            SourceId::new("kafka.orders-v2_eu").map(|s| s.as_str().to_owned()),
            Ok("kafka.orders-v2_eu".to_owned())
        );
    }

    #[test]
    fn source_id_rejects_empty_spaces_and_overlong_names() {
        for bad in ["", "has space", "slash/name", &"x".repeat(129)] {
            assert!(SourceId::new(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn source_id_validates_on_deserialize() {
        assert!(serde_json::from_str::<SourceId>("\"no spaces allowed\"").is_err());
        assert!(serde_json::from_str::<SourceId>("\"wikipedia.recentchange\"").is_ok());
    }

    #[test]
    fn timestamp_round_trips_millis() {
        assert_eq!(
            Timestamp::from_millis(1_790_519_122_000).as_millis(),
            1_790_519_122_000
        );
    }

    #[test]
    fn cursor_accepts_non_empty_bytes_at_the_limit() {
        let bytes = vec![7; 4096];
        assert_eq!(
            Cursor::new(bytes.clone()).map(|cursor| cursor.as_bytes().to_vec()),
            Ok(bytes)
        );
    }

    #[test]
    fn cursor_rejects_empty_and_overlong_values() {
        assert_eq!(Cursor::new(Vec::new()), Err(ModelError::EmptyCursor));
        assert_eq!(
            Cursor::new(vec![0; 4097]),
            Err(ModelError::CursorTooLarge(4097))
        );
    }

    #[test]
    fn cursor_validates_on_deserialize() {
        assert!(serde_json::from_str::<Cursor>("[]").is_err());
        assert!(serde_json::from_str::<Cursor>("[1,2,3]").is_ok());
    }
}
