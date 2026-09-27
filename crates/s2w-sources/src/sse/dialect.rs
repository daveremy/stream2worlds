//! What the SSE transport cannot know about a particular stream.

use s2w_model::Cursor;

/// Whether `value` can be sent verbatim as an HTTP header value (what every cursor here
/// eventually becomes, as `Last-Event-ID`). A cursor that fails this must never be accepted as
/// valid: `connect.rs::build_request` would fail to build the request, and the read loop
/// treats every connect failure as transient and retries forever (decision 0008) — the one
/// permanent failure that loop cannot distinguish from a network blip. Catching it here, at
/// cursor-acceptance time, turns it into the loud, immediate error `SourceError::StoredCursor`
/// or a malformed-id reconnect instead of a silent infinite retry.
pub(crate) fn header_safe(value: &str) -> Result<(), String> {
    reqwest::header::HeaderValue::from_str(value)
        .map(|_| ())
        .map_err(|error| format!("not usable as an HTTP header value: {error}"))
}

/// The stream-specific half of an SSE source: how an `id:` becomes a cursor, how to ask for a
/// start time, and which frames to keep. The transport carries no knowledge of any one stream.
///
/// Cursors are text: SSE's `Last-Event-ID` is a string, so the cursor stored in the log and
/// the header sent on reconnect are the same bytes.
pub(crate) trait SseDialect: Send + Sync + 'static {
    /// The cursor for a frame's `id:` field (`None` when the frame has none). `Err` is a
    /// malformed id: reported, and counted toward the forced-reconnect limit.
    ///
    /// # Errors
    ///
    /// Why the id cannot be a cursor.
    fn cursor(&self, id: Option<&str>) -> Result<String, String>;

    /// Decodes a cursor read back from the log into the `Last-Event-ID` to resume with.
    ///
    /// # Errors
    ///
    /// Why the stored bytes are not a cursor this dialect writes.
    fn validate_stored(&self, cursor: &Cursor) -> Result<String, String>;

    /// Asks the stream to start at `since`, by editing the request URL.
    ///
    /// # Errors
    ///
    /// Why `--since` does not apply to this stream.
    fn apply_since(&self, url: &mut reqwest::Url, since: &str) -> Result<(), String>;

    /// Called with a frame's `data:` AFTER its cursor has advanced. `Ok(true)` keeps the
    /// event, `Ok(false)` drops it silently, `Err` is a malformed payload that is reported and
    /// skipped. The payload is not assumed to be JSON.
    ///
    /// # Errors
    ///
    /// Why the payload is malformed.
    fn accept(&self, data: &str) -> Result<bool, String>;

    /// The bytes stored in the log for one accepted frame, given its cursor and `data:` text.
    ///
    /// Default: the byte-deterministic `{"data":…,"id":…}` envelope
    /// (`super::envelope::envelope`) — see its doc for why a generic stream needs the cursor
    /// folded into the stored bytes. Override to store `data` verbatim when the payload
    /// already carries stream-unique identity on its own (Wikimedia's `meta.id`) — changing
    /// those stored bytes would break dedupe against logs already written under the old,
    /// unenveloped format, and the fold that parses the payload as JSON.
    fn store(&self, cursor: &str, data: &str) -> Vec<u8> {
        super::envelope::envelope(cursor, data).into_bytes()
    }
}

/// The default dialect for a bare `sse://`, `https://` or `http://` URI: the `id:` verbatim as
/// the cursor, no start-time parameter, every payload kept. A frame without an `id:` has no
/// cursor and is skipped; three in a row force a reconnect (decision 0008).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Opaque;

impl SseDialect for Opaque {
    fn cursor(&self, id: Option<&str>) -> Result<String, String> {
        match id {
            Some(id) if !id.is_empty() => {
                header_safe(id)?;
                Ok(id.to_owned())
            }
            _ => Err("frame has no id, so it has no cursor to resume from".to_owned()),
        }
    }

    fn validate_stored(&self, cursor: &Cursor) -> Result<String, String> {
        let value = std::str::from_utf8(cursor.as_bytes())
            .map(str::to_owned)
            .map_err(|error| format!("not valid UTF-8: {error}"))?;
        header_safe(&value)?;
        Ok(value)
    }

    fn apply_since(&self, _url: &mut reqwest::Url, _since: &str) -> Result<(), String> {
        Err("a generic SSE stream has no start-time parameter; restarts resume from the stored Last-Event-ID".to_owned())
    }

    fn accept(&self, _data: &str) -> Result<bool, String> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use s2w_model::Cursor;

    use super::{Opaque, SseDialect};

    #[test]
    fn an_id_with_a_control_character_is_a_malformed_cursor() {
        // A frame id containing e.g. a CR/LF cannot be sent back as `Last-Event-ID` — accepting
        // it as a cursor would only surface later, as a connect failure the read loop retries
        // forever (it cannot tell "permanently broken" from "network blip").
        assert!(Opaque.cursor(Some("line1\r\nline2")).is_err());
    }

    #[test]
    fn a_stored_cursor_with_a_control_character_is_a_loud_error() {
        let cursor = Cursor::new(b"line1\r\nline2".to_vec()).expect("valid cursor bytes");
        assert!(Opaque.validate_stored(&cursor).is_err());
    }
}
