//! One process owns ingestion, the live bridge and the loopback query API (decision 0014).

use std::cell::RefCell;
use std::future::{Future, IntoFuture};
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use axum::extract::Request;
use axum::http::{StatusCode, header::HOST};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use s2w_log::{
    AppendOutcome, EventLog, LogError, LogPosition, LogReader, SqliteEventLog, SqliteVerdictStore,
    StoredEvent, WorldManifest,
};
use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
use s2w_sources::registry::resolve;
use s2w_sources::source::{CursorLookup, Ending, SourceError, Started};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};

use crate::bridge::{Bridge, BridgeConfig, BridgeError, EngineRegistry};
use crate::query::{QueryState, router};
use crate::{AppError, Reporter, current_thread_runtime, group_commit, open_error, parse_filters};

const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Arguments for `s2w serve <source>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeArgs {
    /// A source URI accepted by `watch`, including `-` for stdin.
    pub uri: String,
    /// The string identifier of the world served by this process.
    pub world: String,
    /// Directory containing the event log and verdict store; created if absent.
    pub log_dir: PathBuf,
    /// Loopback HTTP port; zero asks the OS for an available port.
    pub port: u16,
    /// Raw `--filter <path>[!]=<value>` specs; see [`crate::WatchArgs::filters`].
    pub filters: Vec<String>,
}

/// Ingests and serves until Ctrl-C, source completion or a fatal failure.
///
/// # Errors
/// Unknown sources and held writer locks are [`AppError::Usage`]; other failures are fatal.
pub fn run_serve(
    state: QueryState,
    args: ServeArgs,
    reporter: &mut dyn Reporter,
) -> Result<(), AppError> {
    let runtime = current_thread_runtime()?;
    let result = runtime.block_on(run_serve_async(state, args, reporter));
    // Tokio stdin uses an uncancellable blocking read. Do not let runtime drop wait for
    // another input byte after Ctrl-C; async HTTP tasks are still shut down immediately.
    runtime.shutdown_background();
    result
}

async fn run_serve_async(
    state: QueryState,
    args: ServeArgs,
    reporter: &mut dyn Reporter,
) -> Result<(), AppError> {
    let filters = parse_filters(&args.filters)?;
    let source =
        resolve(&args.uri, &filters).map_err(|error| AppError::Usage(error.to_string()))?;
    let mut log = SqliteEventLog::open(&args.log_dir)
        .map_err(|error| open_error(error, &args.log_dir, "event log"))?;
    let verdicts = SqliteVerdictStore::open(&args.log_dir)
        .map_err(|error| open_error(error, &args.log_dir, "verdict store"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::Usage(e.to_string()))?;
    let now = i64::try_from(now.as_millis()).map_err(|e| AppError::Usage(e.to_string()))?;
    let engines = EngineRegistry::with_defaults().names();
    let manifest = WorldManifest::create_if_absent(
        &mut log,
        &args.world,
        &args.world,
        Timestamp::from_millis(now),
        &engines,
        &[],
    )?;
    let name = source.name();
    // Start before sharing: no RefCell borrow survives an await, even during cursor lookup.
    let started = source
        .start(None, &ServingCursors(RefCell::new(&mut log)))
        .await?;
    let state = state
        .with_world(args.world.clone())
        .with_metadata(Some(manifest), log.membership_history()?)
        .with_log_dir(args.log_dir.clone());
    report_source_start(reporter, name, &started.notes);
    let listener = TcpListener::bind(("127.0.0.1", args.port))
        .await
        .map_err(|error| {
            AppError::Serve(std::io::Error::new(
                error.kind(),
                format!("binding 127.0.0.1:{}: {error}", args.port),
            ))
        })?;
    report_listener(reporter, &listener)?;
    serve_live(
        state,
        ServeStorage { log, verdicts },
        started,
        name,
        listener,
        async { tokio::signal::ctrl_c().await.map_err(AppError::Serve) },
        reporter,
    )
    .await
}

fn report_source_start(reporter: &mut dyn Reporter, name: &str, notes: &[String]) {
    for note in notes {
        reporter.note(&format!("{name}: {note}"));
    }
}

fn report_listener(reporter: &mut dyn Reporter, listener: &TcpListener) -> Result<(), AppError> {
    let addr = listener.local_addr().map_err(AppError::Serve)?;
    reporter.note(&format!("serving on http://{addr}"));
    Ok(())
}

// Adapters call the gate after discovering real source IDs but before spawning producers.
struct ServingCursors<'a>(RefCell<&'a mut SqliteEventLog>);
impl CursorLookup for ServingCursors<'_> {
    fn is_member(&self, source: &SourceId) -> Result<bool, SourceError> {
        let mut log = self.0.borrow_mut();
        log.bootstrap_source(source)
            .and_then(|()| log.is_source_member(source))
            .map_err(|e| SourceError::Lookup(e.to_string()))
    }
    fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, SourceError> {
        self.0
            .borrow()
            .cursor(source)
            .map_err(|e| SourceError::Lookup(e.to_string()))
    }
}

struct SharedLogReader {
    log: Rc<RefCell<SqliteEventLog>>,
    batch: usize,
}

struct ServeStorage {
    log: SqliteEventLog,
    verdicts: SqliteVerdictStore,
}

impl LogReader for SharedLogReader {
    fn read_after(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        // The bridge takes exactly this many rows. Limit BEFORE collecting, not afterwards.
        let events: Vec<_> = self.log.borrow().replay(from)?.take(self.batch).collect();
        Ok(Box::new(events.into_iter()))
    }
}

struct SharedLogWriter(Rc<RefCell<SqliteEventLog>>);

impl EventLog for SharedLogWriter {
    fn append(&mut self, event: RawEvent) -> Result<AppendOutcome, LogError> {
        self.0.borrow_mut().append(event)
    }

    fn append_batch(&mut self, events: Vec<RawEvent>) -> Result<Vec<AppendOutcome>, LogError> {
        self.0.borrow_mut().append_batch(events)
    }

    fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, LogError> {
        self.0.borrow().cursor(source)
    }

    fn replay(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        // Required by EventLog, but the ingestion pump only calls append_batch.
        let events: Vec<_> = self.0.borrow().replay(from)?.collect();
        Ok(Box::new(events.into_iter()))
    }
}

// Bridge::run requires Send and moves each poll to the blocking pool. This local driver uses
// the same poll/backoff policy, yielding even after full batches so ingestion/HTTP can run.
async fn local_bridge(
    mut bridge: Bridge<SharedLogReader, SqliteVerdictStore>,
    config: BridgeConfig,
    ready: oneshot::Sender<()>,
) -> Result<(), BridgeError> {
    let mut ready = Some(ready);
    let mut delay = config.poll;
    loop {
        let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bridge.poll_once()))
            .map_err(|_| BridgeError::Task("bridge poll panicked".to_owned()))??;
        if let Some(ready) = ready.take() {
            let _ignored = ready.send(());
        }
        if report.stats.consumed >= config.batch as u64 && report.error.is_none() {
            delay = config.poll;
            tokio::task::yield_now().await;
            continue;
        }
        let sleep = if report.stats.consumed == 0 {
            let current = delay;
            delay = delay.saturating_mul(2).min(config.max_backoff);
            current
        } else {
            delay = config.poll;
            delay
        };
        tokio::time::sleep(sleep).await;
    }
}

async fn serve_live(
    state: QueryState,
    storage: ServeStorage,
    started: Started,
    name: &'static str,
    listener: TcpListener,
    stop: impl Future<Output = Result<(), AppError>>,
    reporter: &mut dyn Reporter,
) -> Result<(), AppError> {
    let config = BridgeConfig::default();
    let shared = Rc::new(RefCell::new(storage.log));
    let reader = SharedLogReader {
        log: shared.clone(),
        batch: config.batch,
    };
    let bridge = Bridge::new(
        reader,
        storage.verdicts,
        EngineRegistry::with_defaults(),
        state.clone(),
        config,
    )
    .map_err(|error| AppError::BridgeStopped(error.to_string()))?;
    let writer = SharedLogWriter(shared.clone());
    let (ready_tx, ready_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let app = app(state);
    let server = async {
        // First replay batch gets a head start, but a large history cannot postpone HTTP
        // indefinitely. A synchronous SQLite poll itself cannot be preempted by this timer.
        let _ready = tokio::time::timeout(Duration::from_millis(500), ready_rx).await;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(shutdown_rx))
            .into_future()
            .await
            .map_err(AppError::Serve)
    };
    supervise(
        move |reporter| {
            Box::pin(async move {
                let stopped_early = group_commit::pump_events_gated(
                    |events, generations| {
                        writer
                            .0
                            .borrow_mut()
                            .append_batch_with_generations(events, generations)
                    },
                    started.stream,
                    name,
                    reporter,
                    &started.sources,
                    |source| Ok(shared.borrow().source_membership(source)?),
                )
                .await?;
                if stopped_early {
                    report_membership_changed(reporter);
                    return std::future::pending::<Result<(), AppError>>().await;
                }
                match started.ends {
                    Ending::AtEndOfInput => Ok(()),
                    Ending::Never => Err(AppError::StreamEnded(name)),
                }
            })
        },
        local_bridge(bridge, config, ready_tx),
        server,
        stop,
        shutdown_tx,
        reporter,
    )
    .await
}

fn report_membership_changed(reporter: &mut dyn Reporter) {
    reporter.note(
        "source membership changed; HTTP remains available; re-add with a cursor and restart to resume",
    );
}

type PumpFuture<'a> = Pin<Box<dyn Future<Output = Result<(), AppError>> + 'a>>;

async fn supervise(
    pump: impl for<'a> FnOnce(&'a mut dyn Reporter) -> PumpFuture<'a>,
    bridge: impl Future<Output = Result<(), BridgeError>>,
    server: impl Future<Output = Result<(), AppError>>,
    stop: impl Future<Output = Result<(), AppError>>,
    shutdown: watch::Sender<bool>,
    reporter: &mut dyn Reporter,
) -> Result<(), AppError> {
    let mut server = std::pin::pin!(server);
    let (result, ingestion_stopped) = {
        let mut pump = pump(reporter);
        tokio::select! {
            result = &mut pump => (result, true),
            result = bridge => (Err(AppError::BridgeStopped(match result {
                Ok(()) => "exited before shutdown".to_owned(),
                Err(error) => error.to_string(),
            })), false),
            result = &mut server => return result,
            result = stop => (result, false),
        }
    };
    if ingestion_stopped {
        reporter.note("ingestion stopped; shutting down HTTP");
    }
    if result.is_err() {
        reporter.note("shutting down HTTP after a fatal error");
    }
    let _ignored = shutdown.send(true);
    match tokio::time::timeout(DRAIN_TIMEOUT, &mut server).await {
        Ok(drained) => result.and(drained),
        Err(_) => {
            reporter.note("HTTP drain timed out after 5 seconds; closing remaining connections");
            result
        }
    }
}

async fn shutdown_signal(mut shutdown: watch::Receiver<bool>) {
    while !*shutdown.borrow_and_update() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

// Assemble every route and fallback before applying the Host boundary.
fn app(state: QueryState) -> axum::Router {
    router(state)
        .route_layer(middleware::from_fn(origin_guard))
        .fallback_service(crate::assets::asset_router())
        .layer(middleware::from_fn(host_allowlist))
}

// An `Origin` is optional (top-level navigation omits it), but when present it must be exactly
// `http://<loopback>[:<port>]` with the Host header's port: scheme, host and port all compared.
async fn origin_guard(request: Request, next: Next) -> Response {
    let mut origins = request.headers().get_all(axum::http::header::ORIGIN).iter();
    if let Some(origin) = origins.next() {
        let host_port = request
            .headers()
            .get(HOST)
            .and_then(|h| h.to_str().ok())
            .and_then(loopback_port);
        let allowed = origin
            .to_str()
            .ok()
            .and_then(|origin| origin.strip_prefix("http://"))
            .and_then(loopback_port)
            .is_some_and(|port| host_port == Some(port));
        if !allowed || origins.next().is_some() {
            return (
                StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({
                    "error": "origin_rejected",
                    "message": "Origin must be http://localhost, 127.0.0.1 or [::1] on this port",
                })),
            )
                .into_response();
        }
    }
    next.run(request).await
}

pub(crate) fn sse_cap_guard(
    slots: std::sync::Arc<tokio::sync::Semaphore>,
) -> Result<tokio::sync::OwnedSemaphorePermit, crate::query::QueryError> {
    slots
        .try_acquire_owned()
        .map_err(|_| crate::query::QueryError::StreamLimit)
}

// A literal loopback authority's port (`None` when absent), or `None` for anything else.
fn loopback_port(authority: &str) -> Option<Option<&str>> {
    ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .find_map(|allowed| authority.strip_prefix(allowed))
        .and_then(|suffix| match suffix.strip_prefix(':') {
            None if suffix.is_empty() => Some(None),
            Some(port)
                if !port.is_empty()
                    && port.bytes().all(|b| b.is_ascii_digit())
                    && port.parse::<u16>().is_ok() =>
            {
                Some(Some(port))
            }
            _ => None,
        })
}

async fn host_allowlist(request: Request, next: Next) -> Response {
    let mut hosts = request.headers().get_all(HOST).iter();
    let allowed = hosts
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(loopback_port)
        .is_some();
    if !allowed || hosts.next().is_some() {
        return (
            StatusCode::BAD_REQUEST,
            "Host must be localhost, 127.0.0.1 or [::1]",
        )
            .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests;
