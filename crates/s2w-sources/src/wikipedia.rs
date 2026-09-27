//! Wikipedia EventStreams ingestion with protocol-aware reconnects.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use s2w_model::Timestamp;
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::{Stream, StreamExt};

use crate::SourceEvent;

const ENDPOINT: &str = "https://stream.wikimedia.org/v2/stream/mediawiki.page_change.v1";
const USER_AGENT: &str = "stream2worlds/0.0.0 (https://github.com/daveremy/stream2worlds)";
const CHANNEL_CAPACITY: usize = 64;
const MALFORMED_ID_LIMIT: usize = 3;

type ByteStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, ConnectError>> + Send>>;

/// One resumable Wikimedia EventStreams cursor.
///
/// The original JSON is retained byte-for-byte for the next `Last-Event-ID` request header,
/// while [`positions`](Self::positions) exposes the parsed per-partition positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastEventId {
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
    /// Returns [`WikipediaSourceError::InvalidLastEventId`] when the value is not the documented
    /// non-empty array of topic, partition, and offset-or-timestamp objects.
    pub fn parse(value: impl Into<String>) -> Result<Self, WikipediaSourceError> {
        let raw = value.into();
        let decoded: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
            WikipediaSourceError::InvalidLastEventId {
                value: raw.clone(),
                reason: error.to_string(),
            }
        })?;
        let entries =
            decoded
                .as_array()
                .ok_or_else(|| WikipediaSourceError::InvalidLastEventId {
                    value: raw.clone(),
                    reason: "expected a JSON array".to_owned(),
                })?;
        if entries.is_empty() {
            return Err(WikipediaSourceError::InvalidLastEventId {
                value: raw,
                reason: "expected at least one stream position".to_owned(),
            });
        }

        let mut positions = Vec::with_capacity(entries.len());
        for entry in entries {
            positions.push(parse_position(entry).map_err(|reason| {
                WikipediaSourceError::InvalidLastEventId {
                    value: raw.clone(),
                    reason,
                }
            })?);
        }
        Ok(Self { raw, positions })
    }

    /// The exact value to send in a `Last-Event-ID` request header.
    #[must_use]
    pub fn as_header_value(&self) -> &str {
        &self.raw
    }

    /// The per-topic, per-partition positions contained in this cursor.
    #[must_use]
    pub fn positions(&self) -> &[StreamPosition] {
        &self.positions
    }
}

/// A position in one Wikimedia Kafka topic partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPosition {
    topic: String,
    partition: i64,
    at: PositionAt,
}

impl StreamPosition {
    /// The Kafka topic represented by this position.
    #[must_use]
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// The Kafka partition represented by this position.
    #[must_use]
    pub const fn partition(&self) -> i64 {
        self.partition
    }

    /// Whether the position resumes by offset or timestamp.
    #[must_use]
    pub const fn at(&self) -> PositionAt {
        self.at
    }
}

/// The value used to locate an event within one stream partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionAt {
    /// Resume at this Kafka offset.
    Offset(i64),
    /// Resume at this Unix timestamp in milliseconds when no offset is available.
    Timestamp(i64),
}

/// A malformed event surfaced by the Wikipedia source.
#[derive(Debug, thiserror::Error)]
pub enum WikipediaSourceError {
    /// A complete SSE frame carried an invalid Wikimedia cursor.
    #[error("invalid Last-Event-ID {value:?}: {reason}")]
    InvalidLastEventId {
        /// The invalid `id:` value.
        value: String,
        /// Why the value could not be decoded.
        reason: String,
    },
    /// An event's `data:` field was not valid JSON.
    #[error("Wikipedia event data is not valid JSON: {reason}")]
    InvalidEventData {
        /// The JSON decoder's diagnostic.
        reason: String,
    },
    /// An SSE line was not valid UTF-8.
    #[error("Wikipedia SSE line is not valid UTF-8: {reason}")]
    InvalidUtf8 {
        /// The UTF-8 decoder's diagnostic.
        reason: String,
    },
    /// The production HTTP client could not be initialized.
    #[error("could not initialize the Wikipedia HTTP client: {reason}")]
    Initialization {
        /// The HTTP client's diagnostic.
        reason: String,
    },
}

/// A stream of Wikipedia `mediawiki.page_change.v1` events.
///
/// Transport failures, non-success HTTP responses, and normal 15-minute server disconnects are
/// retried with capped exponential backoff. Once a valid cursor has been observed, reconnects
/// send it as `Last-Event-ID` and stop sending the original `since` query parameter.
pub struct WikipediaSource {
    receiver: ReceiverStream<Result<SourceEvent, WikipediaSourceError>>,
    task: AbortHandle,
}

impl WikipediaSource {
    /// Starts the production Wikimedia EventStreams source.
    ///
    /// `since` is passed through to Wikimedia unchanged on every connection attempt until the
    /// first complete frame with a valid cursor arrives.
    ///
    /// # Errors
    ///
    /// Returns [`WikipediaSourceError::Initialization`] if reqwest cannot construct its client.
    ///
    /// # Panics
    ///
    /// Panics (via [`tokio::spawn`], through [`Self::spawn`]) if called outside a Tokio runtime.
    pub fn new(since: Option<String>) -> Result<Self, WikipediaSourceError> {
        let connector = ReqwestConnect::new()?;
        let (source, task) = Self::spawn(connector, since, None, Backoff::production());
        drop(task);
        Ok(source)
    }

    /// Resumes the production source from an already-known cursor, such as one read back from
    /// the event log.
    ///
    /// The cursor is sent as `Last-Event-ID` on the very first connection; `since` is never
    /// sent, because the cursor already locates the resume point.
    ///
    /// # Errors
    ///
    /// Returns [`WikipediaSourceError::Initialization`] if reqwest cannot construct its client.
    ///
    /// # Panics
    ///
    /// Panics (via [`tokio::spawn`], through [`Self::spawn`]) if called outside a Tokio runtime.
    pub fn resume(cursor: LastEventId) -> Result<Self, WikipediaSourceError> {
        let connector = ReqwestConnect::new()?;
        let (source, task) = Self::spawn(connector, None, Some(cursor), Backoff::production());
        drop(task);
        Ok(source)
    }

    /// # Panics
    ///
    /// Panics if called outside a Tokio runtime — it calls [`tokio::spawn`] directly.
    fn spawn<C: Connect>(
        connector: C,
        since: Option<String>,
        initial_cursor: Option<LastEventId>,
        backoff: Backoff,
    ) -> (Self, JoinHandle<()>) {
        let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
        let task = tokio::spawn(run(connector, since, initial_cursor, sender, backoff));
        let source = Self {
            receiver: ReceiverStream::new(receiver),
            task: task.abort_handle(),
        };
        (source, task)
    }
}

impl Stream for WikipediaSource {
    type Item = Result<SourceEvent, WikipediaSourceError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.receiver).poll_next(context)
    }
}

impl Drop for WikipediaSource {
    fn drop(&mut self) {
        self.task.abort();
    }
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

trait Connect: Send + Sync + 'static {
    fn connect<'a>(
        &'a self,
        since: Option<&'a str>,
        last_event_id: Option<&'a str>,
    ) -> impl Future<Output = Result<ByteStream, ConnectError>> + Send + 'a;
}

#[derive(Debug, thiserror::Error)]
enum ConnectError {
    #[error("HTTP transport failed: {0}")]
    Transport(String),
    #[error("HTTP response had status {0}")]
    HttpStatus(u16),
    #[error("HTTP request could not be built: {0}")]
    Request(String),
}

struct ReqwestConnect {
    client: reqwest::Client,
}

impl ReqwestConnect {
    fn new() -> Result<Self, WikipediaSourceError> {
        let client = reqwest::Client::builder().build().map_err(|error| {
            WikipediaSourceError::Initialization {
                reason: error.to_string(),
            }
        })?;
        Ok(Self { client })
    }

    fn build_request(
        &self,
        since: Option<&str>,
        last_event_id: Option<&str>,
    ) -> Result<reqwest::Request, ConnectError> {
        let mut url = reqwest::Url::parse(ENDPOINT)
            .map_err(|error| ConnectError::Request(error.to_string()))?;
        if let Some(value) = since {
            url.query_pairs_mut().append_pair("since", value);
        }
        let mut request = self.client.get(url).header("User-Agent", USER_AGENT);
        if let Some(value) = last_event_id {
            request = request.header("Last-Event-ID", value);
        }
        request
            .build()
            .map_err(|error| ConnectError::Request(error.to_string()))
    }
}

impl Connect for ReqwestConnect {
    async fn connect<'a>(
        &'a self,
        since: Option<&'a str>,
        last_event_id: Option<&'a str>,
    ) -> Result<ByteStream, ConnectError> {
        let request = self.build_request(since, last_event_id)?;
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|error| ConnectError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ConnectError::HttpStatus(response.status().as_u16()));
        }
        let stream = response.bytes_stream().map(|chunk| {
            chunk
                .map(|bytes| bytes.to_vec())
                .map_err(|error| ConnectError::Transport(error.to_string()))
        });
        Ok(Box::pin(stream) as ByteStream)
    }
}

#[derive(Clone, Copy)]
struct Backoff {
    initial: Duration,
    maximum: Duration,
    current: Duration,
}

impl Backoff {
    const fn production() -> Self {
        Self {
            initial: Duration::from_millis(250),
            maximum: Duration::from_secs(30),
            current: Duration::from_millis(250),
        }
    }

    #[cfg(test)]
    const fn fixed(duration: Duration) -> Self {
        Self {
            initial: duration,
            maximum: duration,
            current: duration,
        }
    }

    async fn wait(&mut self) {
        tokio::time::sleep(self.current).await;
        self.current = self.current.saturating_mul(2).min(self.maximum);
    }

    fn reset(&mut self) {
        self.current = self.initial;
    }
}

async fn run<C: Connect>(
    connector: C,
    since: Option<String>,
    initial_cursor: Option<LastEventId>,
    sender: mpsc::Sender<Result<SourceEvent, WikipediaSourceError>>,
    mut backoff: Backoff,
) {
    // A resumed source starts from the cursor it was handed (read back from the event log)
    // instead of `None`; from the first frame on, the last parsed cursor is authoritative.
    let mut cursor = initial_cursor;
    loop {
        let requested_since = cursor.is_none().then_some(since.as_deref()).flatten();
        let requested_cursor = cursor.as_ref().map(LastEventId::as_header_value);
        let connection = connector.connect(requested_since, requested_cursor).await;
        let mut bytes = match connection {
            Ok(bytes) => bytes,
            Err(_) => {
                backoff.wait().await;
                continue;
            }
        };
        let mut parser = FrameParser::default();
        let mut malformed_ids = 0_usize;
        let mut force_reconnect = false;

        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => break,
            };
            for parsed in parser.push(&chunk) {
                match process_frame(
                    parsed,
                    &sender,
                    &mut cursor,
                    &mut backoff,
                    &mut malformed_ids,
                )
                .await
                {
                    FrameAction::Continue => {}
                    FrameAction::Reconnect => {
                        force_reconnect = true;
                        break;
                    }
                    FrameAction::Stop => return,
                }
            }
            if force_reconnect {
                break;
            }
        }
        if !force_reconnect {
            for parsed in parser.finish() {
                match process_frame(
                    parsed,
                    &sender,
                    &mut cursor,
                    &mut backoff,
                    &mut malformed_ids,
                )
                .await
                {
                    FrameAction::Continue | FrameAction::Reconnect => {}
                    FrameAction::Stop => return,
                }
            }
        }
        backoff.wait().await;
    }
}

enum FrameAction {
    Continue,
    Reconnect,
    Stop,
}

async fn process_frame(
    parsed: Result<RawFrame, WikipediaSourceError>,
    sender: &mpsc::Sender<Result<SourceEvent, WikipediaSourceError>>,
    cursor: &mut Option<LastEventId>,
    backoff: &mut Backoff,
    malformed_ids: &mut usize,
) -> FrameAction {
    let frame = match parsed {
        Ok(frame) => frame,
        Err(error) => {
            return if sender.send(Err(error)).await.is_err() {
                FrameAction::Stop
            } else {
                FrameAction::Continue
            };
        }
    };
    let event_cursor = match LastEventId::parse(frame.id.unwrap_or_default()) {
        Ok(event_cursor) => event_cursor,
        Err(error) => {
            *malformed_ids = malformed_ids.saturating_add(1);
            if sender.send(Err(error)).await.is_err() {
                return FrameAction::Stop;
            }
            return if *malformed_ids >= MALFORMED_ID_LIMIT {
                FrameAction::Reconnect
            } else {
                FrameAction::Continue
            };
        }
    };

    *malformed_ids = 0;
    *cursor = Some(event_cursor.clone());
    backoff.reset();
    let Some(payload) = frame.data else {
        return FrameAction::Continue;
    };
    let decoded: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(decoded) => decoded,
        Err(error) => {
            let error = WikipediaSourceError::InvalidEventData {
                reason: error.to_string(),
            };
            return if sender.send(Err(error)).await.is_err() {
                FrameAction::Stop
            } else {
                FrameAction::Continue
            };
        }
    };
    if is_filtered(&decoded) {
        return FrameAction::Continue;
    }
    let event = SourceEvent {
        payload,
        cursor: event_cursor,
        received_at: now(),
    };
    if sender.send(Ok(event)).await.is_err() {
        FrameAction::Stop
    } else {
        FrameAction::Continue
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

fn now() -> Timestamp {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let millis = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
            Timestamp::from_millis(millis)
        }
        Err(error) => {
            let millis = i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX);
            Timestamp::from_millis(millis.saturating_neg())
        }
    }
}

#[derive(Default)]
struct FrameParser {
    buffered: Vec<u8>,
    frame: FrameBuilder,
}

impl FrameParser {
    fn push(&mut self, chunk: &[u8]) -> Vec<Result<RawFrame, WikipediaSourceError>> {
        self.buffered.extend_from_slice(chunk);
        let mut consumed = 0;
        let mut output = Vec::new();
        while let Some((line_end, delimiter_len)) = complete_line(&self.buffered, consumed) {
            let line = self.buffered[consumed..line_end].to_vec();
            consumed = line_end + delimiter_len;
            match String::from_utf8(line) {
                Ok(line) => {
                    if line.is_empty() {
                        if let Some(frame) = self.frame.take() {
                            output.push(Ok(frame));
                        }
                    } else {
                        self.frame.push_line(&line);
                    }
                }
                Err(error) => {
                    self.frame = FrameBuilder::default();
                    output.push(Err(WikipediaSourceError::InvalidUtf8 {
                        reason: error.to_string(),
                    }));
                }
            }
        }
        if consumed > 0 {
            self.buffered.drain(..consumed);
        }
        output
    }

    fn finish(&mut self) -> Vec<Result<RawFrame, WikipediaSourceError>> {
        if self.buffered.last() == Some(&b'\r') {
            self.push(b"\n")
        } else {
            Vec::new()
        }
    }
}

fn complete_line(buffer: &[u8], start: usize) -> Option<(usize, usize)> {
    for index in start..buffer.len() {
        match buffer[index] {
            b'\n' => return Some((index, 1)),
            b'\r' if index + 1 == buffer.len() => return None,
            b'\r' if buffer[index + 1] == b'\n' => return Some((index, 2)),
            b'\r' => return Some((index, 1)),
            _ => {}
        }
    }
    None
}

#[derive(Default)]
struct FrameBuilder {
    id: Option<String>,
    data: Vec<String>,
}

impl FrameBuilder {
    fn push_line(&mut self, line: &str) {
        if line.starts_with(':') {
            return;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "id" if !value.contains('\0') => self.id = Some(value.to_owned()),
            "data" => self.data.push(value.to_owned()),
            _ => {}
        }
    }

    fn take(&mut self) -> Option<RawFrame> {
        if self.id.is_none() && self.data.is_empty() {
            return None;
        }
        let id = self.id.take();
        let data = if self.data.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.data).join("\n"))
        };
        Some(RawFrame { id, data })
    }
}

struct RawFrame {
    id: Option<String>,
    data: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future;
    use std::sync::{Arc, Mutex};

    use super::*;

    const FIRST_ID: &str =
        r#"[{"topic":"eqiad.mediawiki.page_change.v1","partition":0,"offset":1}]"#;
    const SECOND_ID: &str =
        r#"[{"topic":"eqiad.mediawiki.page_change.v1","partition":0,"offset":2}]"#;
    const NORMAL_DATA: &str = r#"{"database":"enwiki","meta":{"domain":"en.wikipedia.org"}}"#;

    macro_rules! async_test {
        ($name:ident, $body:block) => {
            #[test]
            fn $name() {
                run_test(async $body);
            }
        };
    }

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

    async_test!(
        recorded_fixture_is_delivered_once_across_cursor_checked_reconnect,
        {
            let bytes = include_bytes!("../testdata/wikipedia-page-change.raw.sse");
            let expected = fixture_frames(bytes);
            assert!(
                expected.len() > 1_000,
                "the real capture should be substantial"
            );
            let split_after = 23;
            let boundary = nth_frame_boundary(bytes, split_after);
            let first_cursor = expected[split_after - 1].0.clone();
            let connector = FakeConnect::new(vec![
                Action::stream(None, None, vec![bytes[..boundary].to_vec()]),
                Action::stream(None, Some(first_cursor), vec![bytes[boundary..].to_vec()]),
                Action::pending_connect(None, expected.last().map(|frame| frame.0.clone())),
            ]);
            let observer = connector.clone();
            let (mut source, _task) = WikipediaSource::spawn(
                connector,
                None,
                None,
                Backoff::fixed(Duration::from_millis(0)),
            );

            let mut actual = Vec::with_capacity(expected.len());
            while actual.len() < expected.len() {
                let item = tokio::time::timeout(Duration::from_secs(10), source.next()).await;
                match item {
                    Ok(Some(Ok(event))) => {
                        actual.push((event.cursor.as_header_value().to_owned(), event.payload))
                    }
                    Ok(Some(Err(error))) => panic!("real fixture produced an error: {error}"),
                    Ok(None) => panic!("source ended before the fixture was consumed"),
                    Err(_) => panic!("timed out consuming the real fixture"),
                }
            }
            assert_eq!(actual, expected);
            observer.assert_no_mismatches().await;
        }
    );

    async_test!(canary_and_examplewiki_frames_are_each_filtered, {
        let synthetic = include_str!("../testdata/wikipedia-malformed.synthetic.sse");
        for marker in [r#""domain":"canary""#, r#""wiki_id":"examplewiki""#] {
            let filtered = frame_containing(synthetic, marker);
            let filtered_id = match filtered.lines().find_map(|line| line.strip_prefix("id: ")) {
                Some(id) => id.to_owned(),
                None => panic!("filtered fixture frame should have an id"),
            };
            let connector = FakeConnect::new(vec![
                Action::stream(None, None, vec![filtered.as_bytes().to_vec()]),
                Action::stream(
                    None,
                    Some(filtered_id),
                    vec![frame(FIRST_ID, NORMAL_DATA).into_bytes()],
                ),
                Action::pending_connect(None, Some(FIRST_ID.to_owned())),
            ]);
            let observer = connector.clone();
            let (mut source, _task) = WikipediaSource::spawn(
                connector,
                None,
                None,
                Backoff::fixed(Duration::from_millis(0)),
            );
            let event = next_ok(&mut source).await;
            assert_eq!(event.payload, NORMAL_DATA);
            assert_eq!(event.cursor.as_header_value(), FIRST_ID);
            observer.assert_no_mismatches().await;
        }
    });

    async_test!(since_survives_failures_until_a_cursor_exists, {
        let since = "2026-09-27T12:00:00Z";
        let connector = FakeConnect::new(vec![
            Action::error(Some(since), None, FakeError::Status(503)),
            Action::stream(Some(since), None, Vec::new()),
            Action::stream(
                Some(since),
                None,
                vec![frame(FIRST_ID, NORMAL_DATA).into_bytes()],
            ),
            Action::pending_connect(None, Some(FIRST_ID.to_owned())),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = WikipediaSource::spawn(
            connector,
            Some(since.to_owned()),
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );
        assert_eq!(next_ok(&mut source).await.payload, NORMAL_DATA);
        observer.assert_no_mismatches().await;
    });

    async_test!(resume_cursor_is_sent_first_and_since_is_never_sent, {
        let resume = match LastEventId::parse(SECOND_ID) {
            Ok(resume) => resume,
            Err(error) => panic!("resume cursor should parse: {error}"),
        };
        let connector = FakeConnect::new(vec![
            Action::stream(
                None,
                Some(SECOND_ID.to_owned()),
                vec![frame(FIRST_ID, NORMAL_DATA).into_bytes()],
            ),
            Action::pending_connect(None, Some(FIRST_ID.to_owned())),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = WikipediaSource::spawn(
            connector,
            None,
            Some(resume),
            Backoff::fixed(Duration::from_millis(0)),
        );
        assert_eq!(next_ok(&mut source).await.payload, NORMAL_DATA);
        observer.assert_no_mismatches().await;
    });

    async_test!(partial_frame_is_discarded_and_does_not_advance_cursor, {
        let synthetic = include_str!("../testdata/wikipedia-malformed.synthetic.sse");
        let partial = synthetic
            .rsplit_once("\n\n")
            .map_or(synthetic, |(_, tail)| tail);
        let first = format!("{}{partial}", frame(FIRST_ID, NORMAL_DATA)).into_bytes();
        let second = frame(SECOND_ID, NORMAL_DATA).into_bytes();
        let connector = FakeConnect::new(vec![
            Action::stream(None, None, vec![first]),
            Action::stream(None, Some(FIRST_ID.to_owned()), vec![second]),
            Action::pending_connect(None, Some(SECOND_ID.to_owned())),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = WikipediaSource::spawn(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );
        let first = next_ok(&mut source).await;
        let second = next_ok(&mut source).await;
        assert_eq!(first.cursor.as_header_value(), FIRST_ID);
        assert_eq!(second.cursor.as_header_value(), SECOND_ID);
        observer.assert_no_mismatches().await;
    });

    async_test!(
        malformed_ids_surface_errors_then_force_a_fresh_connection,
        {
            let poison = frame_containing(
                include_str!("../testdata/wikipedia-malformed.synthetic.sse"),
                "id: not-json",
            );
            let bytes = format!(
                "{}{poison}{poison}{poison}{poison}",
                frame(FIRST_ID, NORMAL_DATA)
            )
            .into_bytes();
            let connector = FakeConnect::new(vec![
                Action::stream(None, None, vec![bytes]),
                Action::pending_connect(None, Some(FIRST_ID.to_owned())),
            ]);
            let observer = connector.clone();
            let (mut source, _task) = WikipediaSource::spawn(
                connector,
                None,
                None,
                Backoff::fixed(Duration::from_millis(0)),
            );
            assert_eq!(
                next_ok(&mut source).await.cursor.as_header_value(),
                FIRST_ID
            );
            for _ in 0..MALFORMED_ID_LIMIT {
                let item = source.next().await;
                assert!(matches!(
                    item,
                    Some(Err(WikipediaSourceError::InvalidLastEventId { .. }))
                ));
            }
            observer.assert_no_mismatches().await;
        }
    );

    async_test!(crlf_split_between_chunks_is_one_line_ending, {
        let complete = frame(FIRST_ID, NORMAL_DATA).replace('\n', "\r\n");
        let split = match complete.find("\r\n") {
            Some(index) => index + 1,
            None => panic!("test frame should contain CRLF"),
        };
        let connector = FakeConnect::new(vec![
            Action::stream(
                None,
                None,
                vec![
                    complete.as_bytes()[..split].to_vec(),
                    complete.as_bytes()[split..].to_vec(),
                ],
            ),
            Action::pending_connect(None, Some(FIRST_ID.to_owned())),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = WikipediaSource::spawn(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );
        assert_eq!(next_ok(&mut source).await.payload, NORMAL_DATA);
        observer.assert_no_mismatches().await;
    });

    async_test!(bare_cr_at_disconnect_dispatches_a_complete_frame, {
        let complete = frame(FIRST_ID, NORMAL_DATA).replace('\n', "\r");
        let connector = FakeConnect::new(vec![
            Action::stream(None, None, vec![complete.into_bytes()]),
            Action::pending_connect(None, Some(FIRST_ID.to_owned())),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = WikipediaSource::spawn(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );
        assert_eq!(next_ok(&mut source).await.payload, NORMAL_DATA);
        observer.assert_no_mismatches().await;
    });

    async_test!(status_and_transport_errors_both_retry, {
        let connector = FakeConnect::new(vec![
            Action::error(Some("123"), None, FakeError::Status(429)),
            Action::error(Some("123"), None, FakeError::Transport),
            Action::stream(
                Some("123"),
                None,
                vec![frame(FIRST_ID, NORMAL_DATA).into_bytes()],
            ),
            Action::pending_connect(None, Some(FIRST_ID.to_owned())),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = WikipediaSource::spawn(
            connector,
            Some("123".to_owned()),
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );
        assert_eq!(next_ok(&mut source).await.payload, NORMAL_DATA);
        observer.assert_no_mismatches().await;
    });

    #[test]
    fn reqwest_request_has_exact_url_and_resume_header() {
        let connector = match ReqwestConnect::new() {
            Ok(connector) => connector,
            Err(error) => panic!("client should initialize: {error}"),
        };
        let with_since = match connector.build_request(Some("2026-09-27T12:00:00Z"), None) {
            Ok(request) => request,
            Err(error) => panic!("request should build: {error}"),
        };
        assert_eq!(
            with_since.url().as_str(),
            "https://stream.wikimedia.org/v2/stream/mediawiki.page_change.v1?since=2026-09-27T12%3A00%3A00Z"
        );
        assert!(with_since.headers().get("Last-Event-ID").is_none());
        assert_eq!(
            with_since
                .headers()
                .get("User-Agent")
                .and_then(|value| value.to_str().ok()),
            Some(USER_AGENT)
        );

        let with_cursor = match connector.build_request(None, Some(FIRST_ID)) {
            Ok(request) => request,
            Err(error) => panic!("request should build: {error}"),
        };
        assert_eq!(with_cursor.url().as_str(), ENDPOINT);
        assert_eq!(
            with_cursor
                .headers()
                .get("Last-Event-ID")
                .and_then(|value| value.to_str().ok()),
            Some(FIRST_ID)
        );
    }

    async_test!(dropping_source_aborts_idle_read, {
        let connector = FakeConnect::new(vec![Action::idle_stream(None, None)]);
        let (source, task) = WikipediaSource::spawn(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_secs(60)),
        );
        tokio::task::yield_now().await;
        drop(source);
        assert_task_aborted(task).await;
    });

    async_test!(dropping_source_aborts_backoff_sleep, {
        let connector = FakeConnect::new(vec![Action::error(None, None, FakeError::Transport)]);
        let (source, task) = WikipediaSource::spawn(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_secs(60)),
        );
        tokio::task::yield_now().await;
        drop(source);
        assert_task_aborted(task).await;
    });

    fn run_test(test: impl Future<Output = ()>) {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => panic!("test runtime should build: {error}"),
        };
        runtime.block_on(test);
    }

    async fn assert_task_aborted(task: JoinHandle<()>) {
        match tokio::time::timeout(Duration::from_secs(1), task).await {
            Ok(Err(error)) => assert!(error.is_cancelled(), "task should be cancelled: {error}"),
            Ok(Ok(())) => panic!("producer stopped before drop could abort it"),
            Err(_) => panic!("producer did not stop promptly after source drop"),
        }
    }

    async fn next_ok(source: &mut WikipediaSource) -> SourceEvent {
        match tokio::time::timeout(Duration::from_secs(2), source.next()).await {
            Ok(Some(Ok(event))) => event,
            Ok(Some(Err(error))) => panic!("expected event, got error: {error}"),
            Ok(None) => panic!("source ended unexpectedly"),
            Err(_) => panic!("timed out waiting for source event"),
        }
    }

    fn frame(id: &str, data: &str) -> String {
        format!("event: message\nid: {id}\ndata: {data}\n\n")
    }

    fn fixture_frames(bytes: &[u8]) -> Vec<(String, String)> {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => panic!("recorded fixture should be UTF-8: {error}"),
        };
        text.split("\n\n")
            .filter_map(|block| {
                let id = block.lines().find_map(|line| line.strip_prefix("id: "))?;
                let data = block.lines().find_map(|line| line.strip_prefix("data: "))?;
                Some((id.to_owned(), data.to_owned()))
            })
            .collect()
    }

    fn nth_frame_boundary(bytes: &[u8], frames: usize) -> usize {
        let mut boundaries = 0;
        for index in 0..bytes.len().saturating_sub(1) {
            if bytes[index] == b'\n' && bytes[index + 1] == b'\n' {
                let block = &bytes[..index];
                let starts_frame = block
                    .rsplit(|byte| *byte == b'\n')
                    .any(|line| line.starts_with(b"id: "));
                if starts_frame {
                    boundaries += 1;
                    if boundaries == frames {
                        return index + 2;
                    }
                }
            }
        }
        panic!("recorded fixture did not contain {frames} complete frames")
    }

    fn frame_containing<'a>(fixture: &'a str, marker: &str) -> &'a str {
        match fixture
            .split_inclusive("\n\n")
            .find(|frame| frame.contains(marker))
        {
            Some(frame) => frame,
            None => panic!("synthetic fixture should contain {marker}"),
        }
    }

    type IdleSenders = Arc<Mutex<Vec<mpsc::Sender<Result<Vec<u8>, ConnectError>>>>>;

    #[derive(Clone)]
    struct FakeConnect {
        actions: Arc<Mutex<VecDeque<Action>>>,
        idle_senders: IdleSenders,
        // A background task calls `connect()`, so a mismatch discovered there
        // can't fail the test by panicking in place (lifeos#844 review, opus
        // BLOCK #2): the spawned task's JoinHandle is discarded in every test
        // (`let (mut source, _task) = ...`), so a panic inside it never
        // surfaces. Recording mismatches here and asserting on them from the
        // test's own thread (`assert_no_mismatches`) makes a wrong
        // since/last_event_id actually fail the test.
        mismatches: Arc<Mutex<Vec<String>>>,
    }

    impl FakeConnect {
        fn new(actions: Vec<Action>) -> Self {
            Self {
                actions: Arc::new(Mutex::new(actions.into())),
                idle_senders: Arc::new(Mutex::new(Vec::new())),
                mismatches: Arc::new(Mutex::new(Vec::new())),
            }
        }

        async fn wait_until_all_actions_started(&self) {
            let wait = async {
                loop {
                    let is_empty = match self.actions.lock() {
                        Ok(actions) => actions.is_empty(),
                        Err(error) => panic!("fake action lock poisoned: {error}"),
                    };
                    if is_empty {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            };
            assert!(
                tokio::time::timeout(Duration::from_secs(1), wait)
                    .await
                    .is_ok(),
                "source did not start the expected reconnect"
            );
        }

        fn record_mismatch(&self, message: String) {
            match self.mismatches.lock() {
                Ok(mut mismatches) => mismatches.push(message),
                Err(error) => panic!("fake mismatch lock poisoned: {error}"),
            }
        }

        /// Waits until every configured [`Action`] has been dequeued by a
        /// `connect()` call, then asserts none of them saw an unexpected
        /// since/last_event_id. Folding the wait in here is deliberate
        /// (opus review round 2, s2w#6): a test that only awaited its own
        /// events and then checked mismatches immediately would race the
        /// background task's LAST reconnect — that reconnect's `connect()`
        /// call, and the mismatch check it can produce, might not have
        /// happened yet. Every FakeConnect-based test's final action is a
        /// `pending_connect`/`idle_stream`-style action that the source only
        /// reaches after consuming the test's own expected events, so
        /// waiting for the queue to empty is what actually proves the final
        /// reconnect's parameters were checked, not just the earlier ones.
        async fn assert_no_mismatches(&self) {
            self.wait_until_all_actions_started().await;
            let mismatches = match self.mismatches.lock() {
                Ok(mismatches) => mismatches.clone(),
                Err(error) => panic!("fake mismatch lock poisoned: {error}"),
            };
            assert!(
                mismatches.is_empty(),
                "FakeConnect::connect() saw unexpected since/last_event_id: {mismatches:?}"
            );
        }
    }

    impl Connect for FakeConnect {
        async fn connect<'a>(
            &'a self,
            since: Option<&'a str>,
            last_event_id: Option<&'a str>,
        ) -> Result<ByteStream, ConnectError> {
            let action = match self.actions.lock() {
                Ok(mut actions) => actions.pop_front(),
                Err(error) => panic!("fake action lock poisoned: {error}"),
            };
            let Some(action) = action else {
                return future::pending().await;
            };
            if since != action.since.as_deref() {
                self.record_mismatch(format!(
                    "since: expected {:?}, got {:?}",
                    action.since, since
                ));
            }
            if last_event_id != action.last_event_id.as_deref() {
                self.record_mismatch(format!(
                    "last_event_id: expected {:?}, got {:?}",
                    action.last_event_id, last_event_id
                ));
            }
            match action.outcome {
                Outcome::Stream(chunks) => {
                    Ok(Box::pin(tokio_stream::iter(chunks.into_iter().map(Ok))) as ByteStream)
                }
                Outcome::Error(FakeError::Status(status)) => Err(ConnectError::HttpStatus(status)),
                Outcome::Error(FakeError::Transport) => Err(ConnectError::Transport(
                    "synthetic transport failure".to_owned(),
                )),
                Outcome::PendingConnect => future::pending().await,
                Outcome::IdleStream => {
                    let (sender, receiver) = mpsc::channel(1);
                    match self.idle_senders.lock() {
                        Ok(mut senders) => senders.push(sender),
                        Err(error) => panic!("fake idle sender lock poisoned: {error}"),
                    }
                    Ok(Box::pin(ReceiverStream::new(receiver)) as ByteStream)
                }
            }
        }
    }

    struct Action {
        since: Option<String>,
        last_event_id: Option<String>,
        outcome: Outcome,
    }

    impl Action {
        fn stream(
            since: Option<&str>,
            last_event_id: Option<String>,
            chunks: Vec<Vec<u8>>,
        ) -> Self {
            Self {
                since: since.map(str::to_owned),
                last_event_id,
                outcome: Outcome::Stream(chunks),
            }
        }

        fn error(since: Option<&str>, last_event_id: Option<String>, error: FakeError) -> Self {
            Self {
                since: since.map(str::to_owned),
                last_event_id,
                outcome: Outcome::Error(error),
            }
        }

        fn pending_connect(since: Option<&str>, last_event_id: Option<String>) -> Self {
            Self {
                since: since.map(str::to_owned),
                last_event_id,
                outcome: Outcome::PendingConnect,
            }
        }

        fn idle_stream(since: Option<&str>, last_event_id: Option<String>) -> Self {
            Self {
                since: since.map(str::to_owned),
                last_event_id,
                outcome: Outcome::IdleStream,
            }
        }
    }

    enum Outcome {
        Stream(Vec<Vec<u8>>),
        Error(FakeError),
        PendingConnect,
        IdleStream,
    }

    enum FakeError {
        Status(u16),
        Transport,
    }
}
