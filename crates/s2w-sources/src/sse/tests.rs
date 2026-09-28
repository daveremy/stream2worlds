use std::collections::VecDeque;
use std::future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::presets::wikimedia::{ENDPOINT, LastEventId, SOURCE_ID, Wikimedia};
use crate::sse::envelope;

/// One delivered event as `(cursor, payload)` text.
#[derive(Debug)]
struct TestEvent {
    cursor: String,
    payload: String,
}

fn text(event: RawEvent) -> (String, String) {
    (
        String::from_utf8_lossy(event.cursor.as_bytes()).into_owned(),
        String::from_utf8_lossy(&event.payload).into_owned(),
    )
}

/// The read loop with the `wikipedia` preset's dialect and source id.
fn wikipedia<C: Connect>(
    connector: C,
    since: Option<String>,
    initial_cursor: Option<LastEventId>,
    backoff: Backoff,
) -> (SseStream, JoinHandle<()>) {
    let source_id = match SourceId::new(SOURCE_ID) {
        Ok(source_id) => source_id,
        Err(error) => panic!("the preset source id is valid: {error}"),
    };
    let state = StreamState {
        name: "wikipedia",
        source_id,
        dialect: Arc::new(Wikimedia),
    };
    spawn(
        connector,
        state,
        since,
        initial_cursor.map(|cursor| cursor.as_header_value().to_owned()),
        backoff,
    )
}

/// The read loop with the generic `Opaque` dialect (no preset), for a bare `sse://`/`https://`
/// target that carries no `id:` field.
fn opaque<C: Connect>(connector: C, backoff: Backoff) -> (SseStream, JoinHandle<()>) {
    let source_id = match SourceId::new("opaque-test") {
        Ok(source_id) => source_id,
        Err(error) => panic!("the test source id is valid: {error}"),
    };
    let state = StreamState {
        name: "sse",
        source_id,
        dialect: Arc::new(Opaque),
    };
    spawn(connector, state, None, None, backoff)
}

const FIRST_ID: &str = r#"[{"topic":"eqiad.mediawiki.page_change.v1","partition":0,"offset":1}]"#;
const SECOND_ID: &str = r#"[{"topic":"eqiad.mediawiki.page_change.v1","partition":0,"offset":2}]"#;
const NORMAL_DATA: &str = r#"{"database":"enwiki","meta":{"domain":"en.wikipedia.org"}}"#;

macro_rules! async_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() {
            run_test(async $body);
        }
    };
}

async_test!(
    recorded_fixture_is_delivered_once_across_cursor_checked_reconnect,
    {
        let bytes = include_bytes!("../../testdata/wikipedia-page-change.raw.sse");
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
        let (mut source, _task) = wikipedia(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );

        let mut actual = Vec::with_capacity(expected.len());
        while actual.len() < expected.len() {
            let item = tokio::time::timeout(Duration::from_secs(10), source.next()).await;
            match item {
                Ok(Some(Ok(event))) => actual.push(text(event)),
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
    let synthetic = include_str!("../../testdata/wikipedia-malformed.synthetic.sse");
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
        let (mut source, _task) = wikipedia(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );
        let event = next_ok(&mut source).await;
        assert_eq!(event.payload, NORMAL_DATA);
        assert_eq!(event.cursor, FIRST_ID);
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
    let (mut source, _task) = wikipedia(
        connector,
        Some(since.to_owned()),
        None,
        Backoff::fixed(Duration::from_millis(0)),
    );
    assert!(next_retrying(&mut source).await.contains("attempt 1"));
    assert!(
        next_retrying(&mut source)
            .await
            .contains("before delivering a frame")
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
    let (mut source, _task) = wikipedia(
        connector,
        None,
        Some(resume),
        Backoff::fixed(Duration::from_millis(0)),
    );
    assert_eq!(next_ok(&mut source).await.payload, NORMAL_DATA);
    observer.assert_no_mismatches().await;
});

async_test!(partial_frame_is_discarded_and_does_not_advance_cursor, {
    let synthetic = include_str!("../../testdata/wikipedia-malformed.synthetic.sse");
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
    let (mut source, _task) = wikipedia(
        connector,
        None,
        None,
        Backoff::fixed(Duration::from_millis(0)),
    );
    let first = next_ok(&mut source).await;
    let second = next_ok(&mut source).await;
    assert_eq!(first.cursor, FIRST_ID);
    assert_eq!(second.cursor, SECOND_ID);
    observer.assert_no_mismatches().await;
});

async_test!(
    malformed_ids_surface_errors_then_force_a_fresh_connection,
    {
        let poison = frame_containing(
            include_str!("../../testdata/wikipedia-malformed.synthetic.sse"),
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
        let (mut source, _task) = wikipedia(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );
        assert_eq!(next_ok(&mut source).await.cursor, FIRST_ID);
        for _ in 0..MALFORMED_ID_LIMIT {
            let item = source.next().await;
            assert!(matches!(item, Some(Err(SourceError::Skipped { .. }))));
        }
        observer.assert_no_mismatches().await;
    }
);

async_test!(
    opaque_dialect_with_no_ids_reconnects_forever_without_crashing,
    {
        // Decision 0008: a generic `sse://`/`https://` target with no `id:` field on any frame
        // has no cursor to advance, so every frame is `Skipped` and three in a row force a
        // reconnect (`MALFORMED_ID_LIMIT`) — forever, not a crash or a hang, because the
        // transport cannot know whether this stream was ever expected to carry ids.
        let no_id_frames = "event: message\ndata: no id here\n\n".repeat(MALFORMED_ID_LIMIT);
        let connector = FakeConnect::new(vec![
            Action::stream(None, None, vec![no_id_frames.clone().into_bytes()]),
            Action::stream(None, None, vec![no_id_frames.into_bytes()]),
            Action::pending_connect(None, None),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = opaque(connector, Backoff::fixed(Duration::from_millis(0)));
        for _ in 0..(MALFORMED_ID_LIMIT * 2) {
            let item = tokio::time::timeout(Duration::from_secs(2), source.next()).await;
            match item {
                Ok(Some(Err(SourceError::Skipped { .. }))) => {}
                other => panic!("expected a Skipped error for a frame with no id, got {other:?}"),
            }
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
    let (mut source, _task) = wikipedia(
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
    let (mut source, _task) = wikipedia(
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
    let (mut source, _task) = wikipedia(
        connector,
        Some("123".to_owned()),
        None,
        Backoff::fixed(Duration::from_millis(0)),
    );
    assert!(next_retrying(&mut source).await.contains("attempt 1"));
    assert!(next_retrying(&mut source).await.contains("attempt 2"));
    assert_eq!(next_ok(&mut source).await.payload, NORMAL_DATA);
    observer.assert_no_mismatches().await;
});

async_test!(dropped_byte_stream_reports_then_reconnects, {
    let connector = FakeConnect::new(vec![
        Action::dropped(None, None, Vec::new()),
        Action::stream(None, None, vec![frame(FIRST_ID, NORMAL_DATA).into_bytes()]),
        Action::pending_connect(None, Some(FIRST_ID.to_owned())),
    ]);
    let observer = connector.clone();
    let (mut source, _task) = wikipedia(
        connector,
        None,
        None,
        Backoff::fixed(Duration::from_millis(0)),
    );
    let retry = next_retrying(&mut source).await;
    assert!(retry.contains("connection dropped"), "got {retry:?}");
    assert_eq!(next_ok(&mut source).await.cursor, FIRST_ID);
    observer.assert_no_mismatches().await;
});

async_test!(connect_attempts_reset_only_after_an_accepted_event, {
    let connector = FakeConnect::new(vec![
        Action::error(None, None, FakeError::Transport),
        Action::stream(None, None, Vec::new()),
        Action::error(None, None, FakeError::Transport),
        Action::stream(None, None, vec![frame(FIRST_ID, NORMAL_DATA).into_bytes()]),
        Action::error(None, Some(FIRST_ID.to_owned()), FakeError::Transport),
        Action::stream(
            None,
            Some(FIRST_ID.to_owned()),
            vec![frame(SECOND_ID, NORMAL_DATA).into_bytes()],
        ),
        Action::pending_connect(None, Some(SECOND_ID.to_owned())),
    ]);
    let observer = connector.clone();
    let (mut source, _task) = wikipedia(
        connector,
        None,
        None,
        Backoff::fixed(Duration::from_millis(0)),
    );
    assert!(next_retrying(&mut source).await.contains("attempt 1"));
    assert!(
        next_retrying(&mut source)
            .await
            .contains("before delivering a frame")
    );
    assert!(next_retrying(&mut source).await.contains("attempt 2"));
    assert_eq!(next_ok(&mut source).await.cursor, FIRST_ID);
    assert!(next_retrying(&mut source).await.contains("attempt 1"));
    assert_eq!(next_ok(&mut source).await.cursor, SECOND_ID);
    observer.assert_no_mismatches().await;
});

async_test!(
    opaque_dialect_envelopes_distinct_ids_with_identical_data_differently,
    {
        // codex astra round-3 BLOCK: an arbitrary/opaque SSE stream carries no guarantee that
        // `data:` alone is unique per event (unlike Wikimedia's `meta.id`), so the generic
        // dialect must fold the cursor into the stored bytes — otherwise the log's
        // content-hash dedupe collapses two distinct events into one.
        let connector = FakeConnect::new(vec![
            Action::stream(
                None,
                None,
                vec![
                    format!(
                        "{}{}",
                        frame(FIRST_ID, NORMAL_DATA),
                        frame(SECOND_ID, NORMAL_DATA)
                    )
                    .into_bytes(),
                ],
            ),
            Action::pending_connect(None, Some(SECOND_ID.to_owned())),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = opaque(connector, Backoff::fixed(Duration::from_millis(0)));
        let first = next_ok(&mut source).await;
        let second = next_ok(&mut source).await;
        assert_ne!(
            first.payload, second.payload,
            "distinct ids with identical data must not collapse under the log's dedupe"
        );
        assert_eq!(first.payload, envelope::envelope(FIRST_ID, NORMAL_DATA));
        assert_eq!(second.payload, envelope::envelope(SECOND_ID, NORMAL_DATA));
        observer.assert_no_mismatches().await;
    }
);

async_test!(
    wikipedia_dialect_stores_the_raw_data_field_byte_identical,
    {
        // Wikimedia's payload already carries a stream-unique `meta.id`, so it is stored verbatim
        // (never enveloped): the stored bytes must match dedupe against logs written by the
        // pre-envelope build, and the fold parses this payload as Wikimedia's own JSON shape.
        let connector = FakeConnect::new(vec![
            Action::stream(None, None, vec![frame(FIRST_ID, NORMAL_DATA).into_bytes()]),
            Action::pending_connect(None, Some(FIRST_ID.to_owned())),
        ]);
        let observer = connector.clone();
        let (mut source, _task) = wikipedia(
            connector,
            None,
            None,
            Backoff::fixed(Duration::from_millis(0)),
        );
        assert_eq!(next_ok(&mut source).await.payload, NORMAL_DATA);
        observer.assert_no_mismatches().await;
    }
);

#[test]
fn reqwest_request_has_exact_url_and_resume_header() {
    let url = match reqwest::Url::parse(ENDPOINT) {
        Ok(url) => url,
        Err(error) => panic!("endpoint should parse: {error}"),
    };
    let connector = match ReqwestConnect::new(url, USER_AGENT, Arc::new(Wikimedia)) {
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
    let (source, task) = wikipedia(
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
    let (source, task) = wikipedia(
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

async fn next_ok(source: &mut SseStream) -> TestEvent {
    match tokio::time::timeout(Duration::from_secs(2), source.next()).await {
        Ok(Some(Ok(event))) => {
            let (cursor, payload) = text(event);
            TestEvent { cursor, payload }
        }
        Ok(Some(Err(error))) => panic!("expected event, got error: {error}"),
        Ok(None) => panic!("source ended unexpectedly"),
        Err(_) => panic!("timed out waiting for source event"),
    }
}

async fn next_retrying(source: &mut SseStream) -> String {
    match tokio::time::timeout(Duration::from_secs(2), source.next()).await {
        Ok(Some(Err(SourceError::Retrying { reason, .. }))) => reason,
        Ok(Some(other)) => panic!("expected retry report, got {other:?}"),
        Ok(None) => panic!("source ended unexpectedly"),
        Err(_) => panic!("timed out waiting for retry report"),
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
            Outcome::Dropped(chunks) => {
                let mut items: Vec<Result<Vec<u8>, ConnectError>> =
                    chunks.into_iter().map(Ok).collect();
                items.push(Err(ConnectError::Transport(
                    "synthetic dropped byte stream".to_owned(),
                )));
                Ok(Box::pin(tokio_stream::iter(items)) as ByteStream)
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
    fn stream(since: Option<&str>, last_event_id: Option<String>, chunks: Vec<Vec<u8>>) -> Self {
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

    fn dropped(since: Option<&str>, last_event_id: Option<String>, chunks: Vec<Vec<u8>>) -> Self {
        Self {
            since: since.map(str::to_owned),
            last_event_id,
            outcome: Outcome::Dropped(chunks),
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
    Dropped(Vec<Vec<u8>>),
    Error(FakeError),
    PendingConnect,
    IdleStream,
}

enum FakeError {
    Status(u16),
    Transport,
}
