//! The byte-deterministic JSON envelope each Kafka record is stored as.

use rskafka::record::RecordAndOffset;

/// Encodes one record as the byte-deterministic JSON envelope stored in the log:
/// `{"key":…,"offset":…,"partition":…,"timestamp_ms":…,"topic":…,"value":…}`.
///
/// Keys are in that fixed order. `key` and `value` are the record's raw bytes: a JSON string
/// when they are valid UTF-8, `{"hex":"…"}` otherwise, and `null` when absent. The value is never
/// parsed and re-serialised, so a redelivered record encodes to identical bytes.
#[must_use]
pub(crate) fn envelope(topic: &str, partition: i32, record: &RecordAndOffset) -> String {
    let mut fields = serde_json::Map::new();
    fields.insert("key".to_owned(), bytes_value(record.record.key.as_deref()));
    fields.insert("offset".to_owned(), record.offset.into());
    fields.insert("partition".to_owned(), partition.into());
    fields.insert(
        "timestamp_ms".to_owned(),
        record.record.timestamp.timestamp_millis().into(),
    );
    fields.insert("topic".to_owned(), topic.into());
    fields.insert(
        "value".to_owned(),
        bytes_value(record.record.value.as_deref()),
    );
    serde_json::Value::Object(fields).to_string()
}

fn bytes_value(bytes: Option<&[u8]>) -> serde_json::Value {
    match bytes {
        None => serde_json::Value::Null,
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => text.into(),
            Err(_) => {
                let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                let mut object = serde_json::Map::new();
                object.insert("hex".to_owned(), hex.into());
                serde_json::Value::Object(object)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rskafka::chrono::DateTime;
    use rskafka::record::{Record, RecordAndOffset};

    use super::envelope;

    fn record(key: Option<&[u8]>, value: Option<&[u8]>, offset: i64) -> RecordAndOffset {
        RecordAndOffset {
            record: Record {
                key: key.map(<[u8]>::to_vec),
                value: value.map(<[u8]>::to_vec),
                headers: BTreeMap::new(),
                timestamp: DateTime::from_timestamp_millis(1_790_000_000_123).unwrap_or_default(),
            },
            offset,
        }
    }

    #[test]
    fn envelope_bytes_are_pinned() {
        let encoded = envelope(
            "orders",
            3,
            &record(Some(b"k1"), Some(b"{\"b\":1, \"a\":2}"), 42),
        );
        assert_eq!(
            encoded,
            r#"{"key":"k1","offset":42,"partition":3,"timestamp_ms":1790000000123,"topic":"orders","value":"{\"b\":1, \"a\":2}"}"#
        );
    }

    #[test]
    fn envelope_encodes_missing_and_binary_bytes() {
        let encoded = envelope("t", 0, &record(None, Some(&[0xff, 0x00, 0x10]), 7));
        assert_eq!(
            encoded,
            r#"{"key":null,"offset":7,"partition":0,"timestamp_ms":1790000000123,"topic":"t","value":{"hex":"ff0010"}}"#
        );
    }

    #[test]
    fn equal_values_at_different_offsets_encode_differently() {
        let first = envelope("t", 0, &record(None, Some(b"tick"), 1));
        let second = envelope("t", 0, &record(None, Some(b"tick"), 2));
        assert_ne!(first, second);
        assert_eq!(first, envelope("t", 0, &record(None, Some(b"tick"), 1)));
    }
}
