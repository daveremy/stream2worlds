//! An opaque SSE dialect whose endpoint accepts a timestamp query parameter.

use s2w_model::Cursor;

use super::{Opaque, SinceError, SseDialect};

/// Generic opaque cursor handling plus a configurable start-time query parameter.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SinceQueryParam {
    /// Query key appended when the caller supplies `--since`.
    pub(crate) param: &'static str,
}

impl SseDialect for SinceQueryParam {
    fn cursor(&self, id: Option<&str>) -> Result<String, String> {
        Opaque.cursor(id)
    }

    fn validate_stored(&self, cursor: &Cursor) -> Result<String, String> {
        Opaque.validate_stored(cursor)
    }

    fn apply_since(&self, url: &mut reqwest::Url, since: &str) -> Result<(), SinceError> {
        crate::since::parse_since(since).map_err(SinceError::Invalid)?;
        url.query_pairs_mut().append_pair(self.param, since);
        Ok(())
    }

    fn accept(&self, data: &str) -> Result<bool, String> {
        Opaque.accept(data)
    }
}

#[cfg(test)]
mod tests {
    use s2w_model::Cursor;

    use super::*;

    #[test]
    fn stored_cursor_from_the_previous_dialect_resumes_verbatim() {
        let raw = r#"[{"topic":"a","partition":1,"offset":9}]"#;
        let cursor = Cursor::new(raw.as_bytes().to_vec()).expect("valid cursor bytes");
        let dialect = SinceQueryParam { param: "since" };
        assert_eq!(dialect.validate_stored(&cursor), Ok(raw.to_owned()));
    }

    #[test]
    fn since_is_validated_and_appended_under_the_configured_key() {
        let dialect = SinceQueryParam { param: "start" };
        let mut url = reqwest::Url::parse("https://example.test/events").expect("valid URL");
        dialect
            .apply_since(&mut url, "2026-09-27T00:00:00Z")
            .expect("valid timestamp");
        assert_eq!(url.query(), Some("start=2026-09-27T00%3A00%3A00Z"));
        assert!(dialect.apply_since(&mut url, "not-a-time").is_err());
    }
}
