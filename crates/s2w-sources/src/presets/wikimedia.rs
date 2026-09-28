//! The `wikipedia` preset's dialect: Wikimedia EventStreams over the generic `sse` adapter.
//!
//! Wikimedia's `id:` is a JSON array of per-partition positions; it is stored verbatim as the
//! cursor and sent back as `Last-Event-ID`, so a log written before the adapter split resumes
//! unchanged (#29). `since=` asks for a start time. Canary and `examplewiki` events are dropped
//! after their cursor advances.

use s2w_model::Cursor;

use crate::sse::{SinceError, SseDialect, header_safe};

/// The EventStreams endpoint the `wikipedia` preset reads.
pub(crate) const ENDPOINT: &str = "https://stream.wikimedia.org/v2/stream/mediawiki.page_change.v1";

/// The log source id the `wikipedia` preset files its events under.
pub(crate) const SOURCE_ID: &str = "wikipedia.page_change";

/// A Wikimedia `Last-Event-ID` that is not the documented JSON array of positions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Last-Event-ID {value:?}: {reason}")]
pub(crate) struct InvalidLastEventId {
    /// The invalid `id:` value.
    pub value: String,
    /// Why the value could not be decoded.
    pub reason: String,
}

/// One resumable Wikimedia EventStreams cursor.
///
/// The original JSON is retained byte-for-byte for the next `Last-Event-ID` request header,
/// while [`positions`](Self::positions) exposes the parsed per-partition positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LastEventId {
    raw: String,
    positions: Vec<StreamPosition>,
}

impl LastEventId {
    /// Parses the JSON array carried by an SSE `id:` field.
    ///
    /// When an entry contains both an offset and a timestamp, the offset wins as required by
    /// the Wikimedia EventStreams protocol.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidLastEventId`] when the value is not the documented
    /// non-empty array of topic, partition, and offset-or-timestamp objects.
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, InvalidLastEventId> {
        let raw = value.into();
        let decoded: serde_json::Value =
            serde_json::from_str(&raw).map_err(|error| InvalidLastEventId {
                value: raw.clone(),
                reason: error.to_string(),
            })?;
        let entries = decoded.as_array().ok_or_else(|| InvalidLastEventId {
            value: raw.clone(),
            reason: "expected a JSON array".to_owned(),
        })?;
        if entries.is_empty() {
            return Err(InvalidLastEventId {
                value: raw,
                reason: "expected at least one stream position".to_owned(),
            });
        }

        let mut positions = Vec::with_capacity(entries.len());
        for entry in entries {
            positions.push(parse_position(entry).map_err(|reason| InvalidLastEventId {
                value: raw.clone(),
                reason,
            })?);
        }
        Ok(Self { raw, positions })
    }

    /// The exact value to send in a `Last-Event-ID` request header.
    #[must_use]
    pub(crate) fn as_header_value(&self) -> &str {
        &self.raw
    }

    /// The per-topic, per-partition positions contained in this cursor.
    #[cfg(test)]
    #[must_use]
    fn positions(&self) -> &[StreamPosition] {
        &self.positions
    }
}

/// A position in one Wikimedia Kafka topic partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamPosition {
    topic: String,
    partition: i64,
    at: PositionAt,
}

#[cfg(test)]
impl StreamPosition {
    /// Whether the position resumes by offset or timestamp.
    #[must_use]
    const fn at(&self) -> PositionAt {
        self.at
    }
}

/// The value used to locate an event within one stream partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PositionAt {
    /// Resume at this Kafka offset.
    Offset(i64),
    /// Resume at this Unix timestamp in milliseconds when no offset is available.
    Timestamp(i64),
}

fn parse_position(value: &serde_json::Value) -> Result<StreamPosition, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "each stream position must be an object".to_owned())?;
    let topic = object
        .get("topic")
        .and_then(serde_json::Value::as_str)
        .filter(|topic| !topic.is_empty())
        .ok_or_else(|| "each stream position needs a non-empty string topic".to_owned())?;
    let partition = object
        .get("partition")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| "each stream position needs an integer partition".to_owned())?;
    let at = if let Some(offset) = object.get("offset").and_then(serde_json::Value::as_i64) {
        PositionAt::Offset(offset)
    } else if let Some(timestamp) = object.get("timestamp").and_then(serde_json::Value::as_i64) {
        PositionAt::Timestamp(timestamp)
    } else {
        return Err("each stream position needs an integer offset or timestamp".to_owned());
    };
    Ok(StreamPosition {
        topic: topic.to_owned(),
        partition,
        at,
    })
}

/// The Wikimedia EventStreams dialect.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Wikimedia;

impl SseDialect for Wikimedia {
    fn cursor(&self, id: Option<&str>) -> Result<String, String> {
        let value = LastEventId::parse(id.unwrap_or_default())
            .map(|cursor| cursor.as_header_value().to_owned())
            .map_err(|error| error.to_string())?;
        header_safe(&value)?;
        Ok(value)
    }

    fn validate_stored(&self, cursor: &Cursor) -> Result<String, String> {
        let text = std::str::from_utf8(cursor.as_bytes())
            .map_err(|error| format!("not valid UTF-8: {error}"))?;
        let value = LastEventId::parse(text)
            .map(|cursor| cursor.as_header_value().to_owned())
            .map_err(|error| error.to_string())?;
        header_safe(&value)?;
        Ok(value)
    }

    fn apply_since(&self, url: &mut reqwest::Url, since: &str) -> Result<(), SinceError> {
        crate::since::parse_since(since).map_err(SinceError::Invalid)?;
        url.query_pairs_mut().append_pair("since", since);
        Ok(())
    }

    fn accept(&self, data: &str) -> Result<bool, String> {
        let value: serde_json::Value = serde_json::from_str(data)
            .map_err(|error| format!("event data is not valid JSON: {error}"))?;
        Ok(!is_filtered(&value))
    }

    /// Wikimedia's `data:` already carries a stream-unique `meta.id`, so the raw JSON is
    /// stored verbatim — unlike the generic dialect, which folds the cursor in (see
    /// `SseDialect::store`'s default). Storing anything else here would both break dedupe
    /// against logs already written by the pre-envelope build and break the fold, which parses
    /// this payload as Wikimedia's own JSON shape, not the generic envelope's.
    fn store(&self, _cursor: &str, data: &str) -> Vec<u8> {
        data.as_bytes().to_vec()
    }
}

fn is_filtered(value: &serde_json::Value) -> bool {
    value
        .get("meta")
        .and_then(|meta| meta.get("domain"))
        .and_then(serde_json::Value::as_str)
        == Some("canary")
        || value.get("wiki_id").and_then(serde_json::Value::as_str) == Some("examplewiki")
        || value.get("database").and_then(serde_json::Value::as_str) == Some("examplewiki")
}

#[cfg(test)]
mod tests {
    use s2w_model::Cursor;

    use super::{LastEventId, PositionAt, Wikimedia};
    use crate::sse::SseDialect;

    #[test]
    fn cursor_prefers_offset_and_supports_timestamp() {
        let parsed = LastEventId::parse(
            r#"[{"topic":"a","partition":1,"offset":9,"timestamp":7},{"topic":"b","partition":2,"timestamp":8}]"#,
        );
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(error) => panic!("cursor should parse: {error}"),
        };
        assert_eq!(parsed.positions()[0].at(), PositionAt::Offset(9));
        assert_eq!(parsed.positions()[1].at(), PositionAt::Timestamp(8));
    }

    #[test]
    fn a_stored_cursor_with_embedded_whitespace_is_a_loud_error_not_an_infinite_retry() {
        // Valid JSON allows a literal newline as whitespace between tokens; that byte survives
        // verbatim into `raw` (the exact `Last-Event-ID` value sent on reconnect) but is
        // refused as an HTTP header value. Accepting it here would only surface as a connect
        // failure the read loop treats as transient and retries forever.
        let cursor = Cursor::new(b"[{\"topic\":\"a\",\n\"partition\":1,\"offset\":9}]".to_vec())
            .expect("valid cursor bytes");
        let outcome = Wikimedia.validate_stored(&cursor);
        assert!(outcome.is_err(), "expected a loud error, got {outcome:?}");
    }

    #[test]
    fn canary_examplewiki_and_bad_json_are_not_accepted() {
        let dialect = Wikimedia;
        assert_eq!(
            dialect.accept(r#"{"database":"enwiki","meta":{"domain":"en.wikipedia.org"}}"#),
            Ok(true)
        );
        for filtered in [
            r#"{"meta":{"domain":"canary"}}"#,
            r#"{"wiki_id":"examplewiki"}"#,
            r#"{"database":"examplewiki"}"#,
        ] {
            assert_eq!(dialect.accept(filtered), Ok(false), "{filtered}");
        }
        assert!(dialect.accept("not json").is_err());
    }

    #[test]
    fn stored_cursors_must_be_utf8_last_event_ids() {
        let dialect = Wikimedia;
        let valid = r#"[{"topic":"t","partition":0,"offset":1}]"#;
        let decode = |bytes: &[u8]| {
            Cursor::new(bytes.to_vec())
                .map_err(|error| error.to_string())
                .and_then(|cursor| dialect.validate_stored(&cursor))
        };
        assert_eq!(decode(valid.as_bytes()), Ok(valid.to_owned()));
        assert!(decode(&[0xff, 0xfe]).is_err());
        assert!(decode(b"not a last-event-id").is_err());
    }
}
