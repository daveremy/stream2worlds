//! One process owns ingestion, the live bridge and the loopback query API (decision 0014).

use std::cell::RefCell;
use std::future::{Future, IntoFuture};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use axum::extract::Request;
use axum::http::{StatusCode, header::HOST};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use s2w_log::{
    AppendOutcome, EventLog, LogError, LogPosition, LogReader, SqliteEventLog, SqliteVerdictStore,
    StoredEvent,
};
use s2w_model::{Cursor, RawEvent, SourceId};
use s2w_sources::registry::resolve;
use s2w_sources::source::{Ending, Started};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};

use crate::bridge::{Bridge, BridgeConfig, BridgeError, EngineRegistry};
use crate::query::{QueryState, router};
use crate::{AppError, LogCursors, current_thread_runtime, group_commit};

const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Arguments for `s2w serve <source>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeArgs {
    /// A source URI accepted by `watch`, including `-` for stdin.
    pub uri: String,
    /// Directory containing the event log and verdict store; created if absent.
    pub log_dir: PathBuf,
    /// Loopback HTTP port; zero asks the OS for an available port.
    pub port: u16,
}

/// Ingests and serves until Ctrl-C, source completion or a fatal failure.
///
/// # Errors
/// Unknown sources and held writer locks are [`AppError::Usage`]; other failures are fatal.
pub fn run_serve(state: QueryState, args: ServeArgs) -> Result<(), AppError> {
    let runtime = current_thread_runtime()?;
    let result = runtime.block_on(run_serve_async(state, args));
    // Tokio stdin uses an uncancellable blocking read. Do not let runtime drop wait for
    // another input byte after Ctrl-C; async HTTP tasks are still shut down immediately.
    runtime.shutdown_background();
    result
}

async fn run_serve_async(state: QueryState, args: ServeArgs) -> Result<(), AppError> {
    let source = resolve(&args.uri).map_err(|error| AppError::Usage(error.to_string()))?;
    let log = SqliteEventLog::open(&args.log_dir)
        .map_err(|error| open_error(error, &args.log_dir, "event log"))?;
    let verdicts = SqliteVerdictStore::open(&args.log_dir)
        .map_err(|error| open_error(error, &args.log_dir, "verdict store"))?;
    let name = source.name();
    // Start before sharing: no RefCell borrow survives an await, even during cursor lookup.
    let started = source.start(None, &LogCursors(&log)).await?;
    for note in &started.notes {
        eprintln!("s2w: {name}: {note}");
    }
    let listener = TcpListener::bind(("127.0.0.1", args.port))
        .await
        .map_err(|error| {
            AppError::Serve(std::io::Error::new(
                error.kind(),
                format!("binding 127.0.0.1:{}: {error}", args.port),
            ))
        })?;
    eprintln!(
        "s2w: serving on http://{}",
        listener.local_addr().map_err(AppError::Serve)?
    );
    serve_live(state, log, verdicts, started, name, listener, async {
        tokio::signal::ctrl_c().await.map_err(AppError::Serve)
    })
    .await
}

fn open_error(error: LogError, directory: &Path, store: &str) -> AppError {
    match error {
        LogError::Locked => AppError::Usage(format!(
            "the {store} at {} is already open by another process (run `s2w watch`/`s2w serve` only once per --log-dir)",
            directory.display()
        )),
        other => AppError::Log(other),
    }
}

struct SharedLogReader {
    log: Rc<RefCell<SqliteEventLog>>,
    batch: usize,
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
    log: SqliteEventLog,
    verdicts: SqliteVerdictStore,
    started: Started,
    name: &'static str,
    listener: TcpListener,
    stop: impl Future<Output = Result<(), AppError>>,
) -> Result<(), AppError> {
    let config = BridgeConfig::default();
    let shared = Rc::new(RefCell::new(log));
    let reader = SharedLogReader {
        log: shared.clone(),
        batch: config.batch,
    };
    let bridge = Bridge::new(
        reader,
        verdicts,
        EngineRegistry::with_defaults(),
        state.clone(),
        config,
    )
    .map_err(|error| AppError::BridgeStopped(error.to_string()))?;
    let mut writer = SharedLogWriter(shared);
    let (ready_tx, ready_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let app = router(state).layer(middleware::from_fn(host_allowlist));
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
        async {
            group_commit::pump_events(&mut writer, started.stream).await?;
            match started.ends {
                Ending::AtEndOfInput => Ok(()),
                Ending::Never => Err(AppError::StreamEnded(name)),
            }
        },
        local_bridge(bridge, config, ready_tx),
        server,
        stop,
        shutdown_tx,
    )
    .await
}

async fn supervise(
    pump: impl Future<Output = Result<(), AppError>>,
    bridge: impl Future<Output = Result<(), BridgeError>>,
    server: impl Future<Output = Result<(), AppError>>,
    stop: impl Future<Output = Result<(), AppError>>,
    shutdown: watch::Sender<bool>,
) -> Result<(), AppError> {
    let mut server = std::pin::pin!(server);
    let result = tokio::select! {
        result = pump => { eprintln!("s2w: ingestion stopped; shutting down HTTP"); result }
        result = bridge => Err(AppError::BridgeStopped(match result {
            Ok(()) => "exited before shutdown".to_owned(),
            Err(error) => error.to_string(),
        })),
        result = &mut server => return result,
        result = stop => result,
    };
    if let Err(error) = &result {
        eprintln!("s2w: {error}; shutting down HTTP");
    }
    let _ignored = shutdown.send(true);
    match tokio::time::timeout(DRAIN_TIMEOUT, &mut server).await {
        Ok(drained) => result.and(drained),
        Err(_) => {
            eprintln!("s2w: HTTP drain timed out after 5 seconds; closing remaining connections");
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

async fn host_allowlist(request: Request, next: Next) -> Response {
    let mut hosts = request.headers().get_all(HOST).iter();
    let allowed = hosts
        .next()
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| {
            ["localhost", "127.0.0.1", "[::1]"].iter().any(|allowed| {
                host.strip_prefix(allowed).is_some_and(|suffix| {
                    suffix.is_empty()
                        || suffix.strip_prefix(':').is_some_and(|port| {
                            !port.is_empty()
                                && port.bytes().all(|b| b.is_ascii_digit())
                                && port.parse::<u16>().is_ok()
                        })
                })
            })
        });
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
