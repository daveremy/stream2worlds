//! The read-only MCP server (decision 0009): `s2w mcp` serves the query API's five read tools
//! over stdio, one per HTTP route, each returning the route's exact JSON bytes as its text.
//!
//! Stdout is the JSON-RPC channel, so nothing on this path writes to it; the only failure that
//! stops the server is the transport itself ending.
//!
//! With `--log-dir`, [`run_mcp_live`] additionally polls for verdicts a concurrently-running
//! `s2w serve`/`s2w watch` process commits, on a plain `std::thread` outside the tokio runtime
//! that serves stdio (stream2worlds#128) — see that function's doc comment for the shutdown and
//! crash contract.

pub mod replay;
mod tools;

pub use tools::{BranchesArgs, EntityHistoryArgs, TimeArgs, WorldDiffArgs, WorldViewArgs};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool_handler};

use crate::AppError;
use crate::mcp::replay::LiveReadOnlyWorld;
use crate::query::QueryState;

/// The fixed poll interval for the live refresh loop: a read-only SQLite cursor read is cheap
/// (a single-row query on an already-open WAL connection), and nothing in stream2worlds#128
/// asked for configurability, so this is a `const`, not a CLI flag.
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// The refresh loop's poll interval never backs off past this, however many consecutive `Io`
/// failures it sees in a row.
const MAX_REFRESH_BACKOFF: Duration = Duration::from_secs(30);

/// After the first consecutive `Io` failure, the refresh loop logs a warning again only every
/// this-many failures — sustained lock contention logs periodically, not once per 500ms tick.
const WARN_EVERY_N_FAILURES: u32 = 10;

/// The MCP server over a [`QueryState`]: the five tools of `tools.rs`, each a read-only mirror
/// of one query API route.
#[derive(Clone)]
pub struct WorldMcp {
    /// The timeline every tool reads; cheap to clone (an `Arc` inside), so a later live
    /// instance can be shared with the HTTP server unchanged.
    state: QueryState,
    /// The macro-generated router over the `#[tool]` methods.
    tool_router: ToolRouter<Self>,
}

impl WorldMcp {
    /// Serves queries against `state`.
    #[must_use]
    pub fn new(state: QueryState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for WorldMcp {
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // rmcp 3.4.1's ToolRouter::call converts deserialization errors into an `is_error` tool
        // result (`into_tool_argument_error`); we want them as a protocol-level INVALID_PARAMS
        // error instead, so this bypasses that one conversion step. `has_route` reproduces
        // `call`'s own "unknown or disabled" check first, so a future disabled tool (nothing
        // calls `ToolRouter::disable_route` today) is rejected exactly as `call` would reject it.
        if !self.tool_router.has_route(request.name.as_ref()) {
            return Err(ErrorData::invalid_params("tool not found", None));
        }
        let Some(route) = self.tool_router.map.get(request.name.as_ref()) else {
            // `has_route` just confirmed this name is present and enabled.
            return Err(ErrorData::invalid_params("tool not found", None));
        };
        (route.call)(ToolCallContext::new(self, request, context)).await
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("s2w", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Read-only view of the s2w world model. Every tool returns the same JSON as the \
                 matching s2w query API route and requires the tool call's world parameter to \
                 match the server's configured world id; errors come back as {\"error\", \
                 \"message\"} objects with stable codes. An on-disk replay serves the first \
                 stored verdict per engine, including historic verdicts from retired engines. \
                 When started against a directory an s2w serve/s2w watch process is actively \
                 writing, the served world refreshes periodically (about every 500ms) as new \
                 verdicts are committed, so results reflect live activity rather than a frozen \
                 snapshot — unless a structural error freezes the refresh loop, in which case \
                 the last good snapshot keeps being served (see stderr for the reason).",
            )
    }
}

/// Runs `s2w mcp` over stdio until the client disconnects, serving a fixed, never-refreshed
/// snapshot (no `--log-dir`, or `--log-dir` on a directory nothing is actively writing).
///
/// Builds a current-thread Tokio runtime (the same shape as [`crate::watch`]) from a
/// plain sync entry point, so the CLI never nests runtimes. The serving future returns when the
/// client closes the pipe; stderr stays free for diagnostics, stdout does not.
///
/// # Errors
///
/// [`AppError::Mcp`] if the initialize handshake or the serving task fails,
/// [`AppError::Runtime`] if the runtime cannot be built.
pub fn run_mcp(state: QueryState) -> Result<(), AppError> {
    run(state, None)
}

/// Runs `s2w mcp --log-dir <dir>` over stdio, refreshing `state` from `live` on a plain
/// `std::thread` outside the tokio runtime until the client disconnects.
///
/// **Why a plain thread, not `tokio::spawn`:** the runtime below is current-thread, the same
/// one serving stdio JSON-RPC. Spawning the refresh loop onto it would put synchronous SQLite
/// I/O on that single thread: a large catch-up would block JSON-RPC I/O until it finished
/// (starvation). `LiveReadOnlyWorld`'s two connections wrap `rusqlite::Connection`, which is
/// `Send`, so a plain OS thread owns them directly — no `spawn_blocking`, no second runtime.
///
/// **Shutdown contract:** after `service.waiting()` returns (the transport ending is still the
/// only thing that stops the server), the stop flag is set and the thread is joined before
/// returning. The bound is one in-flight `refresh()` *batch* plus at most one [`STOP_CHECK_SLICE`]
/// — [`replay::LiveReadOnlyWorld::refresh_checking_stop`] checks the flag between batches inside
/// a large catch-up, not only once per poll cycle, so a big backlog can't stall shutdown for its
/// whole duration, and [`sleep_checking_stop`] checks the flag every `STOP_CHECK_SLICE` rather
/// than sleeping the full (possibly backed-off-to-`MAX_REFRESH_BACKOFF`) poll interval in one
/// uninterruptible sleep.
///
/// **Crash contract:** if the refresh thread panics (e.g. inside `QueryState`'s write lock),
/// the panic is caught at the thread's top level, logged to stderr, and the thread exits —
/// `state`'s `std::sync::RwLock` is never poisoned this way, and the server keeps serving its
/// last good snapshot rather than every later tool call panicking on a poisoned lock.
///
/// **Error contract:** only a `LogError::Io`-wrapped error is retried (with backoff, capped at
/// [`MAX_REFRESH_BACKOFF`], reset on the next success); every other error stops the refresh
/// thread and logs that the served snapshot is now frozen, naming the error. `QueryState` keeps
/// whatever it last held — never cleared.
///
/// # Errors
///
/// [`AppError::Mcp`] if the initialize handshake or the serving task fails,
/// [`AppError::Runtime`] if the runtime cannot be built.
pub fn run_mcp_live(state: QueryState, live: LiveReadOnlyWorld) -> Result<(), AppError> {
    run(state, Some(live))
}

fn run(state: QueryState, live: Option<LiveReadOnlyWorld>) -> Result<(), AppError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(AppError::Runtime)?;
    let refresh = live.map(|live| spawn_refresh(state.clone(), live));
    let result = runtime.block_on(async move {
        let service = WorldMcp::new(state)
            .serve(stdio())
            .await
            .map_err(mcp_error)?;
        service.waiting().await.map(|_| ()).map_err(mcp_error)?;
        Ok(())
    });
    if let Some((stop, handle)) = refresh {
        stop.store(true, Ordering::Relaxed);
        // The thread only reads `state`/`live` and never panics past `catch_unwind` inside
        // `refresh_loop`, so a join failure here would mean the thread itself was killed
        // externally — nothing this process can recover from; log and move on rather than
        // letting a `.unwrap()` mask the *real* result computed above.
        if handle.join().is_err() {
            eprintln!(
                "s2w mcp: refresh thread did not exit cleanly (already logged if it panicked)"
            );
        }
    }
    result
}

/// Spawns the refresh loop on a plain OS thread and returns the stop flag plus its handle.
fn spawn_refresh(
    state: QueryState,
    live: LiveReadOnlyWorld,
) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let stop = Arc::new(AtomicBool::new(false));
    let handle = std::thread::Builder::new()
        .name("s2w-mcp-refresh".to_owned())
        .spawn({
            let stop = Arc::clone(&stop);
            move || refresh_loop(state, live, &stop)
        })
        .unwrap_or_else(|error| panic!("spawning the s2w-mcp-refresh thread: {error}"));
    (stop, handle)
}

/// The pure retry/backoff/warn-cadence decision logic [`refresh_loop`] runs on each poll
/// outcome, pulled out so it is directly unit-testable without a real SQLite lock
/// (stream2worlds#128 leg D) — mirroring how [`replay::classify_cursor_update`] was pulled out
/// of [`replay::LiveReadOnlyWorld::refresh_checking_stop`] for the same reason. A real
/// `SQLITE_BUSY`/locked repro under WAL mode proved too unreliable to construct cheaply for an
/// integration test; this exercises the same state machine `refresh_loop` drives.
struct RefreshBackoff {
    interval: Duration,
    consecutive_failures: u32,
}

impl RefreshBackoff {
    fn new() -> Self {
        Self {
            interval: REFRESH_INTERVAL,
            consecutive_failures: 0,
        }
    }

    /// Records a retryable failure. Returns the interval to sleep before the next poll, and
    /// whether this failure should be logged (the first one, then every
    /// [`WARN_EVERY_N_FAILURES`]th).
    fn on_retryable_failure(&mut self) -> (Duration, bool) {
        self.consecutive_failures += 1;
        let should_warn = self.consecutive_failures == 1
            || self
                .consecutive_failures
                .is_multiple_of(WARN_EVERY_N_FAILURES);
        self.interval = (self.interval * 2).min(MAX_REFRESH_BACKOFF);
        (self.interval, should_warn)
    }

    /// Records a success. Returns the number of consecutive failures just recovered from (0 if
    /// the previous poll already succeeded, so the caller can skip logging a "recovered" note).
    fn on_success(&mut self) -> u32 {
        let recovered_from = self.consecutive_failures;
        self.consecutive_failures = 0;
        self.interval = REFRESH_INTERVAL;
        recovered_from
    }
}

/// The slice `sleep_checking_stop` sleeps between `stop` checks — bounds shutdown latency to
/// this much even while backed off to [`MAX_REFRESH_BACKOFF`] (round-D refine pass,
/// stream2worlds#128): one long uninterruptible sleep would let `run`'s post-`stop`-store
/// `handle.join()` block for up to the full backoff interval.
const STOP_CHECK_SLICE: Duration = Duration::from_millis(100);

/// Sleeps up to `interval`, checking `stop` every [`STOP_CHECK_SLICE`] so a shutdown request
/// is noticed within one slice rather than waiting out the whole interval. Returns `true` if
/// `stop` was observed set (caller should return immediately, without polling `live` again).
fn sleep_checking_stop(interval: Duration, stop: &AtomicBool) -> bool {
    let mut remaining = interval;
    loop {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        if remaining.is_zero() {
            return false;
        }
        let slice = remaining.min(STOP_CHECK_SLICE);
        std::thread::sleep(slice);
        remaining -= slice;
    }
}

/// Polls `live` every [`REFRESH_INTERVAL`] (backing off on repeated `Io` failures, capped at
/// [`MAX_REFRESH_BACKOFF`]) and folds new verdicts into `state`, until `stop` is set or a
/// non-`Io` error freezes the loop. See [`run_mcp_live`] for the full shutdown/crash/error
/// contract this implements. The sleep between polls itself checks `stop` every
/// [`STOP_CHECK_SLICE`] ([`sleep_checking_stop`]), so shutdown latency is bounded even while
/// backed off to [`MAX_REFRESH_BACKOFF`].
fn refresh_loop(state: QueryState, mut live: LiveReadOnlyWorld, stop: &AtomicBool) {
    let mut backoff = RefreshBackoff::new();
    loop {
        if sleep_checking_stop(backoff.interval, stop) {
            return;
        }

        // Catch a panic (e.g. one raised while `state`'s write lock is held inside `append`)
        // at this thread's top level so it can never poison `state`'s `std::sync::RwLock` for
        // every later tool call on the main thread — see `run_mcp_live`'s crash contract.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            live.refresh_checking_stop(&state, Some(stop))
        }));

        let result = match outcome {
            Ok(result) => result,
            Err(panic) => {
                let message = panic_message(&panic);
                eprintln!(
                    "s2w mcp: refresh thread panicked ({message}); the served snapshot is now \
                     frozen at its last good state"
                );
                return;
            }
        };

        match result {
            Ok(_) => {
                let recovered_from = backoff.on_success();
                if recovered_from > 0 {
                    eprintln!("s2w mcp: refresh recovered after {recovered_from} failed poll(s)");
                }
            }
            Err(error) if error.is_retryable() => {
                let (_interval, should_warn) = backoff.on_retryable_failure();
                if should_warn {
                    eprintln!(
                        "s2w mcp: refresh poll failed ({} consecutive): {error}",
                        backoff.consecutive_failures
                    );
                }
            }
            Err(error) => {
                eprintln!(
                    "s2w mcp: refresh loop stopped ({error}); the served snapshot is now frozen \
                     at its last good state"
                );
                return;
            }
        }
    }
}

/// Renders a caught panic payload as a string, mirroring the standard panic hook's own
/// `&str`/`String` handling (a panic payload is `Box<dyn Any + Send>`, rarely anything else).
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

/// Wraps an rmcp or tokio serving failure for [`AppError::Mcp`].
fn mcp_error(error: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Mcp(Box::new(error))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;

    use s2w_log::{
        AppendOutcome, EventLog, SqliteEventLog, SqliteVerdictStore, StoredVerdict, VerdictStore,
    };
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
    use s2w_system1::{Confidence, Verdict};

    use super::{
        AtomicBool, Duration, MAX_REFRESH_BACKOFF, REFRESH_INTERVAL, RefreshBackoff,
        WARN_EVERY_N_FAILURES, refresh_loop,
    };
    use crate::mcp::replay::LiveReadOnlyWorld;
    use crate::query::QueryState;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            loop {
                let sequence = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path = env::temp_dir().join(format!(
                    "s2w-app-mcp-mod-{label}-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("creating test directory: {error}"),
                }
            }
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn raw(index: u8) -> RawEvent {
        RawEvent {
            source: SourceId::new("test.mcp-mod").unwrap(),
            cursor: Cursor::new(vec![index]).unwrap(),
            received_at: Timestamp::from_millis(1_000 + i64::from(index)),
            payload: vec![index],
        }
    }

    /// Commits one event plus a matching verdict for it — same shape as `replay::tests::commit`,
    /// duplicated locally rather than shared across a `#[cfg(test)]` boundary between modules.
    fn commit(log: &mut SqliteEventLog, verdicts: &mut SqliteVerdictStore, index: u8, key: &str) {
        let position = match log.append(raw(index)).unwrap() {
            AppendOutcome::Inserted(position) => position,
            other => panic!("unexpected append outcome: {other:?}"),
        };
        let stored = log
            .replay(None)
            .unwrap()
            .filter_map(Result::ok)
            .find(|event| event.position == position)
            .expect("the just-inserted event replays back");
        let encoded = serde_json::to_vec(&Verdict::Propose {
            claims: vec![s2w_core::WorldEvent::EntityObserved {
                key: s2w_core::NaturalKey::new(key),
                entity_type: "fixture".to_owned(),
                attrs: std::collections::BTreeMap::new(),
            }],
            confidence: Confidence::CERTAIN,
        })
        .unwrap();
        verdicts
            .commit_batch(
                &[StoredVerdict {
                    position,
                    event_hash: stored.content_hash,
                    engine: "fixture".to_owned(),
                    version: 1,
                    verdict: encoded,
                    provenance: None,
                }],
                position,
            )
            .unwrap();
    }

    fn world_json(state: &QueryState) -> String {
        serde_json::to_string(&state.world_at(None).unwrap()).unwrap()
    }

    #[test]
    fn sleep_checking_stop_returns_promptly_once_stop_is_set_even_mid_long_interval() {
        let stop = Arc::new(AtomicBool::new(false));
        let flipper = std::thread::spawn({
            let stop = Arc::clone(&stop);
            move || {
                std::thread::sleep(Duration::from_millis(50));
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        let started = std::time::Instant::now();
        let stopped = super::sleep_checking_stop(MAX_REFRESH_BACKOFF, &stop);
        let elapsed = started.elapsed();
        flipper.join().unwrap();

        assert!(stopped, "sleep_checking_stop must report stop was observed");
        assert!(
            elapsed < Duration::from_secs(1),
            "must return within about one STOP_CHECK_SLICE of stop being set, not wait out the \
             full {MAX_REFRESH_BACKOFF:?} interval: took {elapsed:?}"
        );
    }

    #[test]
    fn sleep_checking_stop_sleeps_out_the_full_interval_when_stop_never_fires() {
        let stop = AtomicBool::new(false);
        let started = std::time::Instant::now();
        let stopped = super::sleep_checking_stop(Duration::from_millis(150), &stop);
        let elapsed = started.elapsed();

        assert!(!stopped, "no stop was ever set");
        assert!(elapsed >= Duration::from_millis(150), "took {elapsed:?}");
    }

    #[test]
    fn refresh_backoff_doubles_on_failure_caps_at_max_warns_on_first_and_every_nth_and_resets_on_success()
     {
        let mut backoff = RefreshBackoff::new();

        let (interval, should_warn) = backoff.on_retryable_failure();
        assert_eq!(interval, REFRESH_INTERVAL * 2);
        assert!(should_warn, "the first failure always warns");

        for expected_warn_at in 2..=WARN_EVERY_N_FAILURES {
            let (_interval, should_warn) = backoff.on_retryable_failure();
            assert_eq!(
                should_warn,
                expected_warn_at == WARN_EVERY_N_FAILURES,
                "failure #{expected_warn_at}"
            );
        }
        assert_eq!(backoff.consecutive_failures, WARN_EVERY_N_FAILURES);

        // Enough further failures to have doubled well past the cap.
        for _ in 0..20 {
            let (interval, _should_warn) = backoff.on_retryable_failure();
            assert!(interval <= MAX_REFRESH_BACKOFF);
        }
        assert_eq!(backoff.interval, MAX_REFRESH_BACKOFF);

        let recovered_from = backoff.on_success();
        assert_eq!(recovered_from, WARN_EVERY_N_FAILURES + 20);
        assert_eq!(backoff.consecutive_failures, 0);
        assert_eq!(backoff.interval, REFRESH_INTERVAL);

        // A success with no prior failure reports nothing recovered.
        assert_eq!(backoff.on_success(), 0);
    }

    /// A non-`Io` (structural) error must stop `refresh_loop` outright rather than retrying —
    /// karpathy ruling, stream2worlds#128. Forces the real `catch_up` "cursor advanced past what
    /// the event log holds" corruption check by committing a verdict batch that names a log
    /// position minted from an unrelated, throwaway event log (so it is a real `LogPosition`
    /// value, but this test's own event log has no event there) — no private-field poking, only
    /// the same public store APIs a real writer would use.
    ///
    /// Verifying the loop's `eprintln!` text would need capturing this test binary's real
    /// stderr (no stable in-process API for that); this test instead asserts the functional
    /// contract the log line documents: the thread actually returns (never spins forever) and
    /// `state` is left exactly as it was before the corrupt poll, never partially updated.
    #[test]
    fn refresh_loop_stops_and_freezes_the_snapshot_on_a_non_retryable_error() {
        let mint = TestDirectory::new("mint-position");
        let mut mint_log = SqliteEventLog::open(mint.path()).unwrap();
        mint_log.append(raw(1)).unwrap();
        let bogus_position = match mint_log.append(raw(2)).unwrap() {
            AppendOutcome::Inserted(position) => position,
            other => panic!("unexpected append outcome: {other:?}"),
        };

        let directory = TestDirectory::new("refresh-loop-stop");
        let mut log = SqliteEventLog::open(directory.path()).unwrap();
        let mut verdicts = SqliteVerdictStore::open(directory.path()).unwrap();
        commit(&mut log, &mut verdicts, 1, "seen-at-open");

        let (state, live) = LiveReadOnlyWorld::open(directory.path(), "default", 10_000).unwrap();
        assert!(world_json(&state).contains("seen-at-open"));
        let before = world_json(&state);

        // This directory's event log only has position 1; `bogus_position` (minted from `mint`,
        // above) does not exist here, so the refresh loop's catch-up will find the cursor
        // advanced with nothing to replay — the real "cursor moved past the event log" defect.
        verdicts
            .commit_batch(
                &[StoredVerdict {
                    position: bogus_position,
                    event_hash: 0,
                    engine: "fixture".to_owned(),
                    version: 1,
                    verdict: serde_json::to_vec(&Verdict::Propose {
                        claims: vec![],
                        confidence: Confidence::CERTAIN,
                    })
                    .unwrap(),
                    provenance: None,
                }],
                bogus_position,
            )
            .unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = mpsc::channel();
        let handle = std::thread::spawn({
            let stop = Arc::clone(&stop);
            move || {
                refresh_loop(state.clone(), live, &stop);
                let _ignored = done_tx.send(world_json(&state));
            }
        });

        let after = done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("refresh_loop must return on a non-retryable error, not spin forever");
        handle.join().expect("refresh thread must not panic");
        assert_eq!(
            after, before,
            "state must be frozen, never partially updated"
        );
    }
}
