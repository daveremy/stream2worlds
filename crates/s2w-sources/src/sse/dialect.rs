//! What the SSE transport cannot know about a particular stream.

use s2w_model::Cursor;

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
}

/// The default dialect for a bare `sse://`, `https://` or `http://` URI: the `id:` verbatim as
/// the cursor, no start-time parameter, every payload kept. A frame without an `id:` has no
/// cursor and is skipped; three in a row force a reconnect (decision 0008).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Opaque;

impl SseDialect for Opaque {
    fn cursor(&self, id: Option<&str>) -> Result<String, String> {
        match id {
            Some(id) if !id.is_empty() => Ok(id.to_owned()),
            _ => Err("frame has no id, so it has no cursor to resume from".to_owned()),
        }
    }

    fn validate_stored(&self, cursor: &Cursor) -> Result<String, String> {
        std::str::from_utf8(cursor.as_bytes())
            .map(str::to_owned)
            .map_err(|error| format!("not valid UTF-8: {error}"))
    }

    fn apply_since(&self, _url: &mut reqwest::Url, _since: &str) -> Result<(), String> {
        Err("a generic SSE stream has no start-time parameter; restarts resume from the stored Last-Event-ID".to_owned())
    }

    fn accept(&self, _data: &str) -> Result<bool, String> {
        Ok(true)
    }
}
