//! The byte-deterministic JSON envelope each SSE event is stored as.

/// Encodes one event's cursor (its dialect-derived `id:`) and `data:` text as the
/// byte-deterministic JSON envelope stored in the log: `{"data":…,"id":…}`.
///
/// The cursor is included so that two distinct events whose `data:` happens to be identical
/// (arbitrary SSE streams need not carry identity in the payload the way Wikimedia's do) never
/// collapse into one another under the log's content-hash dedupe — only a genuine redelivery
/// (same id, same data) does. `data` is never re-parsed, so a redelivered event encodes to
/// identical bytes.
#[must_use]
pub(crate) fn envelope(cursor: &str, data: &str) -> String {
    let mut fields = serde_json::Map::new();
    fields.insert("data".to_owned(), data.into());
    fields.insert("id".to_owned(), cursor.into());
    serde_json::Value::Object(fields).to_string()
}

#[cfg(test)]
mod tests {
    use super::envelope;

    #[test]
    fn distinct_ids_with_identical_data_encode_differently() {
        let first = envelope("1", "tick");
        let second = envelope("2", "tick");
        assert_ne!(first, second);
    }

    #[test]
    fn a_redelivery_encodes_identically() {
        let first = envelope("1", "tick");
        let redelivered = envelope("1", "tick");
        assert_eq!(first, redelivered);
    }

    #[test]
    fn bytes_are_pinned() {
        assert_eq!(envelope("42", "hello"), r#"{"data":"hello","id":"42"}"#);
    }
}
