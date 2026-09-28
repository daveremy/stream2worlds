//! The `wikipedia` preset's dialect: Wikimedia EventStreams over the generic `sse` adapter.
//!
//! Wikimedia's `id:` is a JSON array of per-partition positions; it is stored verbatim as the
//! cursor and sent back as `Last-Event-ID`, so a log written before the adapter split resumes
//! unchanged (#29). `since=` asks for a start time. Canary and `examplewiki` events are dropped
//! after their cursor advances.

use std::sync::atomic::{AtomicU64, Ordering};

use s2w_model::Cursor;

use crate::sse::{SinceError, SseDialect, header_safe};

/// Consecutive real (non-canary, non-`examplewiki`) events a `--wiki` filter may match
/// nothing among before it warns on stderr (s2w#101): a typo'd wiki id filters everything
/// silently, and the busy `mediawiki.page_change` stream reaches this in well under a minute
/// so the warning is not a long wait even on a quiet connection.
const STALL_WARNING_THRESHOLD: u64 = 200;

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
///
/// `wiki` restricts ingestion to one `wiki_id` (e.g. `enwiki`) the same way the built-in
/// canary/examplewiki drop already restricts it — a client-side filter in [`accept`](
/// SseDialect::accept), applied only to events not yet stored. It does not retroactively purge
/// a log directory that already holds events from other wikis (same as the canary/examplewiki
/// filter always has: neither ever un-stores anything already written).
#[derive(Debug)]
pub(crate) struct Wikimedia {
    wiki: Option<String>,
    /// Consecutive real events seen since the last match; only tracked when `wiki` is `Some`.
    /// Interior mutability because [`SseDialect::accept`] takes `&self`. Reset to 0 on every
    /// match, so it equals [`STALL_WARNING_THRESHOLD`] exactly once per stall streak — that
    /// equality alone gates the warning below, with nothing further needed to fire it once.
    unmatched_since_last: AtomicU64,
}

impl Wikimedia {
    /// No filter: every non-canary, non-`examplewiki` wiki is accepted.
    pub(crate) fn new() -> Self {
        Self {
            wiki: None,
            unmatched_since_last: AtomicU64::new(0),
        }
    }

    /// Only `wiki`'s events are accepted (plus the same canary/examplewiki drop).
    pub(crate) fn filtered(wiki: String) -> Self {
        Self {
            wiki: Some(wiki),
            unmatched_since_last: AtomicU64::new(0),
        }
    }

    /// Tracks a real event's match outcome and warns once on stderr per stall streak once
    /// [`STALL_WARNING_THRESHOLD`] consecutive real events have matched nothing — the silent
    /// empty-world shape s2w#101 exists to catch. No-op when no filter is set.
    fn track_stall(&self, matched: bool) {
        let Some(wiki) = self.wiki.as_deref() else {
            return;
        };
        if matched {
            self.unmatched_since_last.store(0, Ordering::Relaxed);
            return;
        }
        let count = self.unmatched_since_last.fetch_add(1, Ordering::Relaxed) + 1;
        if count == STALL_WARNING_THRESHOLD {
            eprintln!(
                "s2w: wikipedia: --wiki {wiki:?} has matched none of the last {STALL_WARNING_THRESHOLD} events; check it against the stream's wiki_id values"
            );
        }
    }

    /// Real events since the last match (or start); test-only window into `track_stall`.
    #[cfg(test)]
    fn unmatched_since_last(&self) -> u64 {
        self.unmatched_since_last.load(Ordering::Relaxed)
    }
}

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
        if is_filtered(&value) {
            return Ok(false);
        }
        let matched = matches_wiki(&value, self.wiki.as_deref());
        self.track_stall(matched);
        Ok(matched)
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

/// True when no filter is set, or `value`'s `wiki_id` matches it exactly.
///
/// An event with no `wiki_id` at all is rejected once a filter is set — `wiki_id` is a
/// required field on every real page-change event (see
/// `s2w_system1::engines::wikimedia::claims`, which also treats it as required), so its
/// absence here means malformed data, never a legitimate `wiki` this filter should let through.
fn matches_wiki(value: &serde_json::Value, wiki: Option<&str>) -> bool {
    match wiki {
        None => true,
        Some(wanted) => value.get("wiki_id").and_then(serde_json::Value::as_str) == Some(wanted),
    }
}

#[cfg(test)]
mod tests {
    use s2w_model::Cursor;

    use super::{LastEventId, PositionAt, STALL_WARNING_THRESHOLD, Wikimedia};
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
        let outcome = Wikimedia::new().validate_stored(&cursor);
        assert!(outcome.is_err(), "expected a loud error, got {outcome:?}");
    }

    #[test]
    fn canary_examplewiki_and_bad_json_are_not_accepted() {
        let dialect = Wikimedia::new();
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

    /// A real (trimmed) `mediawiki.page_change` payload, matching the shape
    /// `s2w_system1::engines::wikimedia::claims` actually reads `wiki_id` from — the filter
    /// tests below check that field, not `database` (a different, legacy stream's field).
    const ENWIKI_SAMPLE: &str = r#"{"wiki_id":"enwiki","page":{"page_id":1,"page_title":"Rust_(programming_language)"},"meta":{"domain":"en.wikipedia.org"}}"#;
    const DEWIKI_SAMPLE: &str = r#"{"wiki_id":"dewiki","page":{"page_id":2,"page_title":"Rust_(Programmiersprache)"},"meta":{"domain":"de.wikipedia.org"}}"#;

    #[test]
    fn no_filter_accepts_every_real_wiki() {
        let dialect = Wikimedia::new();
        assert_eq!(dialect.accept(ENWIKI_SAMPLE), Ok(true));
        assert_eq!(dialect.accept(DEWIKI_SAMPLE), Ok(true));
    }

    #[test]
    fn wiki_filter_accepts_only_the_matching_wiki() {
        let dialect = Wikimedia::filtered("enwiki".to_owned());
        assert_eq!(dialect.accept(ENWIKI_SAMPLE), Ok(true));
        assert_eq!(dialect.accept(DEWIKI_SAMPLE), Ok(false));
    }

    #[test]
    fn wiki_filter_rejects_an_event_with_no_wiki_id() {
        // `wiki_id` is required on every real page-change event (mirrored in
        // `s2w_system1::engines::wikimedia::claims`'s own `required(.., "wiki_id")`), so an
        // event missing it is malformed, not a legitimate wiki this filter should pass through.
        let dialect = Wikimedia::filtered("enwiki".to_owned());
        assert_eq!(
            dialect.accept(r#"{"page":{"page_id":1,"page_title":"X"}}"#),
            Ok(false)
        );
    }

    #[test]
    fn no_filter_never_tracks_a_stall() {
        // Without `--wiki` there is nothing to stall against; the counter must stay inert.
        let dialect = Wikimedia::new();
        for _ in 0..STALL_WARNING_THRESHOLD {
            assert_eq!(dialect.accept(DEWIKI_SAMPLE), Ok(true));
        }
        assert_eq!(dialect.unmatched_since_last(), 0);
    }

    #[test]
    fn canary_and_examplewiki_drops_do_not_count_toward_a_stall() {
        // These are dropped for every dialect, filtered or not — they are not evidence the
        // configured `--wiki` value is wrong and must not push the stall counter.
        let dialect = Wikimedia::filtered("enwiki".to_owned());
        for filtered in [
            r#"{"meta":{"domain":"canary"}}"#,
            r#"{"wiki_id":"examplewiki"}"#,
        ] {
            assert_eq!(dialect.accept(filtered), Ok(false));
        }
        assert_eq!(dialect.unmatched_since_last(), 0);
    }

    #[test]
    fn wiki_filter_stall_count_reaches_the_threshold_exactly_once_per_streak() {
        // `unmatched_since_last` strictly increments on every unmatched event and only
        // resets on a match, so it equals `STALL_WARNING_THRESHOLD` — the warning's only
        // trigger condition — at exactly one point per streak, never again until a match
        // starts a new one.
        let dialect = Wikimedia::filtered("enwiki".to_owned());
        for i in 1..=STALL_WARNING_THRESHOLD * 2 {
            assert_eq!(dialect.accept(DEWIKI_SAMPLE), Ok(false));
            assert_eq!(dialect.unmatched_since_last(), i);
        }

        // A match resets the streak so a later, separate stall reaches the threshold again.
        assert_eq!(dialect.accept(ENWIKI_SAMPLE), Ok(true));
        assert_eq!(dialect.unmatched_since_last(), 0);
        assert_eq!(dialect.accept(DEWIKI_SAMPLE), Ok(false));
        assert_eq!(dialect.unmatched_since_last(), 1);
    }

    #[test]
    fn stored_cursors_must_be_utf8_last_event_ids() {
        let dialect = Wikimedia::new();
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
