//! Shared parsing for source `--since` values.

use rskafka::chrono::DateTime;

/// Parses RFC 3339 or integer epoch milliseconds into epoch milliseconds.
pub(crate) fn parse_since(value: &str) -> Result<i64, String> {
    if let Ok(millis) = value.parse::<i64>() {
        return Ok(millis);
    }
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.timestamp_millis())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_since;

    #[test]
    fn accepts_rfc3339_and_epoch_millis_and_rejects_other_formats() {
        for (value, expected) in [
            ("1790000000123", Some(1_790_000_000_123)),
            ("2026-09-27T12:00:00Z", Some(1_790_510_400_000)),
            ("2026-09-27T05:00:00-07:00", Some(1_790_510_400_000)),
            ("2026-09-27", None),
            ("yesterday", None),
        ] {
            match expected {
                Some(millis) => assert_eq!(parse_since(value), Ok(millis), "{value}"),
                None => assert!(parse_since(value).is_err(), "{value} must be refused"),
            }
        }
    }
}
