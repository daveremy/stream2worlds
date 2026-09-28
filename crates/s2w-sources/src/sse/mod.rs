//! The generic SSE adapter: `sse://`, `https://` and `http://` URIs, and the transport under
//! presets such as `wikipedia`.
//!
//! Transport failures, non-success HTTP responses and server disconnects are retried with
//! capped exponential backoff. Once a valid cursor has been seen, reconnects send it as
//! `Last-Event-ID` and stop sending `--since`. What a cursor is, how to ask for a start time
//! and which frames to keep belong to the [`SseDialect`], not to this module.

mod connect;
mod dialect;
mod envelope;
mod frame;
mod start;
#[cfg(test)]
mod tests;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::{Stream, StreamExt};

use connect::{Backoff, Connect, ConnectError, ReqwestConnect};
pub(crate) use dialect::{Opaque, SinceError, SseDialect, header_safe};
use frame::{FrameParser, RawFrame};
use start::StartPlan;

use crate::source::{CursorLookup, Ending, Source, SourceError, StartFuture, Started};

/// The `User-Agent` every SSE request sends (Wikimedia's policy asks for a descriptive one).
pub(crate) const USER_AGENT: &str =
    "stream2worlds/0.0.0 (https://github.com/daveremy/stream2worlds)";
const CHANNEL_CAPACITY: usize = 64;
const MALFORMED_ID_LIMIT: usize = 3;

type ByteStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, ConnectError>> + Send>>;
type Item = Result<RawEvent, SourceError>;

/// One SSE source: where it connects and how its frames are read.
pub(crate) struct SseConfig {
    /// The name in messages: the preset name, or `sse`.
    pub name: &'static str,
    /// The `https://` or `http://` URL to connect to.
    pub url: String,
    /// The stream-specific half.
    pub dialect: Arc<dyn SseDialect>,
    /// The log source id events are filed under.
    pub source_id: String,
    /// The `User-Agent` header.
    pub user_agent: &'static str,
}

/// An SSE source, not yet started.
pub(crate) struct SseSource {
    config: SseConfig,
}

impl SseSource {
    /// A source for `config`; nothing connects until [`Source::start`].
    #[must_use]
    pub(crate) fn new(config: SseConfig) -> Self {
        Self { config }
    }

    /// The log source id this source files events under.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn source_id(&self) -> &str {
        &self.config.source_id
    }
}

impl Source for SseSource {
    fn name(&self) -> &'static str {
        self.config.name
    }

    fn start<'a>(
        self: Box<Self>,
        since: Option<&'a str>,
        cursors: &'a dyn CursorLookup,
    ) -> StartFuture<'a> {
        Box::pin(async move {
            let SseConfig {
                name,
                url,
                dialect,
                source_id,
                user_agent,
            } = self.config;
            let parsed = reqwest::Url::parse(&url).map_err(|error| SourceError::InvalidTarget {
                name,
                uri: url.clone(),
                reason: error.to_string(),
            })?;
            let source_id = SourceId::new(source_id)?;
            let stored = cursors.cursor(&source_id)?;
            let plan = choose_start(
                name,
                dialect.as_ref(),
                &parsed,
                &source_id,
                stored.as_ref(),
                since,
            )?;
            let connector = ReqwestConnect::new(parsed, user_agent, Arc::clone(&dialect))
                .map_err(|reason| SourceError::Fatal { name, reason })?;
            let state = StreamState {
                name,
                source_id,
                dialect,
            };
            let (stream, task) = spawn(
                connector,
                state,
                plan.since,
                plan.initial_cursor,
                Backoff::production(),
            );
            drop(task);
            Ok(Started {
                stream: Box::pin(stream),
                ends: Ending::Never,
                notes: Vec::new(),
            })
        })
    }
}

/// Thin re-export so `sse::tests` (a sibling module of `start`, not a child) can call the
/// pure decision function without `start::choose` needing wider visibility than `pub(super)`.
fn choose_start(
    name: &'static str,
    dialect: &dyn SseDialect,
    url: &reqwest::Url,
    source_id: &SourceId,
    stored: Option<&Cursor>,
    since: Option<&str>,
) -> Result<StartPlan, SourceError> {
    start::choose(name, dialect, url, source_id, stored, since)
}

/// What the read loop needs to turn frames into events.
struct StreamState {
    name: &'static str,
    source_id: SourceId,
    dialect: Arc<dyn SseDialect>,
}

impl StreamState {
    fn skipped(&self, reason: String) -> SourceError {
        SourceError::Skipped {
            name: self.name,
            reason,
        }
    }
}

/// The running stream: events from the read loop's task, which is aborted on drop.
struct SseStream {
    receiver: ReceiverStream<Item>,
    task: AbortHandle,
}

impl Stream for SseStream {
    type Item = Item;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Item>> {
        Pin::new(&mut self.receiver).poll_next(context)
    }
}

impl Drop for SseStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Spawns the read loop.
///
/// # Panics
///
/// Panics if called outside a Tokio runtime — it calls [`tokio::spawn`] directly.
fn spawn<C: Connect>(
    connector: C,
    state: StreamState,
    since: Option<String>,
    initial_cursor: Option<String>,
    backoff: Backoff,
) -> (SseStream, JoinHandle<()>) {
    let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
    let task = tokio::spawn(run(
        connector,
        state,
        since,
        initial_cursor,
        sender,
        backoff,
    ));
    let stream = SseStream {
        receiver: ReceiverStream::new(receiver),
        task: task.abort_handle(),
    };
    (stream, task)
}

async fn run<C: Connect>(
    connector: C,
    stream: StreamState,
    since: Option<String>,
    initial_cursor: Option<String>,
    sender: mpsc::Sender<Item>,
    mut backoff: Backoff,
) {
    // A resumed source starts from the cursor it was handed (read back from the event log)
    // instead of `None`; from the first frame on, the last parsed cursor is authoritative.
    let mut cursor = initial_cursor;
    let mut attempt = 1_u32;
    loop {
        let requested_since = cursor.is_none().then_some(since.as_deref()).flatten();
        let requested_cursor = cursor.as_deref();
        let connection = connector.connect(requested_since, requested_cursor).await;
        let mut bytes = match connection {
            Ok(bytes) => bytes,
            Err(error) => {
                let retrying = SourceError::Retrying {
                    name: stream.name,
                    reason: format!(
                        "connect failed (attempt {attempt}): {error}; retrying in {:?}",
                        backoff.current()
                    ),
                };
                if matches!(send(&sender, Err(retrying)).await, FrameAction::Stop) {
                    return;
                }
                attempt = attempt.saturating_add(1);
                backoff.wait().await;
                continue;
            }
        };
        let mut parser = FrameParser::default();
        let mut malformed_ids = 0_usize;
        let mut force_reconnect = false;
        let mut delivered_frames = 0_usize;
        let mut read_failed = false;

        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let retrying = SourceError::Retrying {
                        name: stream.name,
                        reason: format!("connection dropped: {error}; reconnecting"),
                    };
                    if matches!(send(&sender, Err(retrying)).await, FrameAction::Stop) {
                        return;
                    }
                    read_failed = true;
                    break;
                }
            };
            for parsed in parser.push(&chunk) {
                delivered_frames = delivered_frames.saturating_add(1);
                match process_frame(
                    &stream,
                    parsed,
                    &sender,
                    &mut cursor,
                    &mut backoff,
                    &mut attempt,
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
                delivered_frames = delivered_frames.saturating_add(1);
                match process_frame(
                    &stream,
                    parsed,
                    &sender,
                    &mut cursor,
                    &mut backoff,
                    &mut attempt,
                    &mut malformed_ids,
                )
                .await
                {
                    FrameAction::Continue | FrameAction::Reconnect => {}
                    FrameAction::Stop => return,
                }
            }
        }
        if !force_reconnect && !read_failed && delivered_frames == 0 {
            let retrying = SourceError::Retrying {
                name: stream.name,
                reason: format!(
                    "connection closed before delivering a frame; reconnecting in {:?}",
                    backoff.current()
                ),
            };
            if matches!(send(&sender, Err(retrying)).await, FrameAction::Stop) {
                return;
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
    stream: &StreamState,
    parsed: Result<RawFrame, String>,
    sender: &mpsc::Sender<Item>,
    cursor: &mut Option<String>,
    backoff: &mut Backoff,
    attempt: &mut u32,
    malformed_ids: &mut usize,
) -> FrameAction {
    let frame = match parsed {
        Ok(frame) => frame,
        Err(reason) => return send(sender, Err(stream.skipped(reason))).await,
    };
    let event_cursor = match stream.dialect.cursor(frame.id.as_deref()) {
        Ok(event_cursor) => event_cursor,
        Err(reason) => {
            *malformed_ids = malformed_ids.saturating_add(1);
            let skipped = stream.skipped(format!("invalid event id: {reason}"));
            if matches!(send(sender, Err(skipped)).await, FrameAction::Stop) {
                return FrameAction::Stop;
            }
            return if *malformed_ids >= MALFORMED_ID_LIMIT {
                FrameAction::Reconnect
            } else {
                FrameAction::Continue
            };
        }
    };

    // The cursor advances before the dialect sees the payload, so a filtered or malformed
    // event is never re-requested on reconnect.
    *malformed_ids = 0;
    *cursor = Some(event_cursor.clone());
    backoff.reset();
    *attempt = 1;
    let Some(payload) = frame.data else {
        return FrameAction::Continue;
    };
    match stream.dialect.accept(&payload) {
        Ok(true) => {}
        Ok(false) => return FrameAction::Continue,
        Err(reason) => return send(sender, Err(stream.skipped(reason))).await,
    }
    let stored_payload = stream.dialect.store(&event_cursor, &payload);
    let event = Cursor::new(event_cursor.into_bytes())
        .map(|cursor| RawEvent {
            source: stream.source_id.clone(),
            cursor,
            received_at: now(),
            payload: stored_payload,
        })
        .map_err(SourceError::from);
    send(sender, event).await
}

/// Sends one item; `Stop` when the consumer has gone.
async fn send(sender: &mpsc::Sender<Item>, item: Item) -> FrameAction {
    if sender.send(item).await.is_err() {
        FrameAction::Stop
    } else {
        FrameAction::Continue
    }
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
