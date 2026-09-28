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
/// returning. The bound is one in-flight `refresh()` *batch*, not one `REFRESH_INTERVAL` tick —
/// [`replay::LiveReadOnlyWorld::refresh_checking_stop`] checks the flag between batches inside a
/// large catch-up, not only once per poll cycle, so a big backlog can't stall shutdown for its
/// whole duration.
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
        .expect("spawning the s2w-mcp-refresh thread");
    (stop, handle)
}

/// Polls `live` every [`REFRESH_INTERVAL`] (backing off on repeated `Io` failures, capped at
/// [`MAX_REFRESH_BACKOFF`]) and folds new verdicts into `state`, until `stop` is set or a
/// non-`Io` error freezes the loop. See [`run_mcp_live`] for the full shutdown/crash/error
/// contract this implements.
fn refresh_loop(state: QueryState, mut live: LiveReadOnlyWorld, stop: &AtomicBool) {
    let mut interval = REFRESH_INTERVAL;
    let mut consecutive_io_failures: u32 = 0;
    loop {
        std::thread::sleep(interval);
        if stop.load(Ordering::Relaxed) {
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
                if consecutive_io_failures > 0 {
                    eprintln!(
                        "s2w mcp: refresh recovered after {consecutive_io_failures} failed poll(s)"
                    );
                }
                consecutive_io_failures = 0;
                interval = REFRESH_INTERVAL;
            }
            Err(error) if error.is_retryable() => {
                consecutive_io_failures += 1;
                if consecutive_io_failures == 1
                    || consecutive_io_failures % WARN_EVERY_N_FAILURES == 0
                {
                    eprintln!(
                        "s2w mcp: refresh poll failed ({consecutive_io_failures} consecutive): \
                         {error}"
                    );
                }
                interval = (interval * 2).min(MAX_REFRESH_BACKOFF);
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
