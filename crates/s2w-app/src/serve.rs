//! One process owns ingestion, the live bridge and the loopback query API (decision 0014).

use std::cell::{Cell, RefCell};
use std::future::{Future, IntoFuture};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{StatusCode, Uri, header::HOST};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
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
use crate::discover::in_run::{InRun, Seed, SinkReporter};
use crate::discover::{self, DiscoverConfig};
use crate::query::{QueryState, router};
use crate::{AppError, Reporter, current_thread_runtime, group_commit, open_error, parse_filters};

mod rebuild;
mod snapshots;
use rebuild::{Rebuild, RouteWatcher};
use snapshots::Snapshotter;
pub use snapshots::{DEFAULT_EVERY, SHUTDOWN_MIN, SnapshotConfig};

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
    /// Snapshot restore and writing (`--snapshot-every`, `--no-snapshot`; decision 0024).
    pub snapshots: SnapshotConfig,
    /// The learned-mapping producer's window and thresholds (decision 0025). No CLI flag:
    /// production uses the default, tests shrink it.
    pub discover: DiscoverConfig,
}

/// Ingests and serves until Ctrl-C or SIGTERM, source completion or a fatal failure.
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

#[expect(
    clippy::too_many_lines,
    reason = "start-up in order; one line over since the route watcher (s2w#184); splitting it is s2w#156"
)]
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
    let manifest = WorldManifest::create_if_absent(
        &mut log,
        &args.world,
        &args.world,
        Timestamp::from_millis(now_millis()?),
        // Historical: the defaults at world creation, not the live registry (decision 0023).
        &EngineRegistry::with_defaults().names(),
        &[],
    )?;
    // Routes resolve before the source starts: a corrupt proposal store fails before it connects.
    let (registry, watcher, discover) =
        routed_registry(&log, &args.log_dir, &args.discover, reporter)?;
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
    let (resume, snapshots) = snapshots::prepare(
        &state,
        (&log, &verdicts),
        &args.log_dir,
        (args.snapshots, &registry),
        reporter,
    )?;
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
        ServeStorage {
            log,
            verdicts,
            registry,
            resume,
            snapshots,
            watch: Some((watcher, args.snapshots)),
            discover: Some(discover),
        },
        started,
        name,
        listener,
        stop_signal(),
        reporter,
    )
    .await
}

/// The start-up routes (decision 0023) with the learned-mapping producer's start pass (decision
/// 0025) run between the two resolutions, plus the proposal-store watcher for the live rebuild
/// and the seed for the in-run trigger: it waits only for sources neither routed nor windowed
/// by the start pass.
///
/// # Errors
/// As [`RouteWatcher::start`].
fn routed_registry(
    log: &SqliteEventLog,
    log_dir: &Path,
    discover: &DiscoverConfig,
    reporter: &mut dyn Reporter,
) -> Result<(EngineRegistry, RouteWatcher, Seed), AppError> {
    let mut settled = std::collections::BTreeSet::new();
    let (registry, watcher) =
        RouteWatcher::start(log_dir.to_path_buf(), reporter, |resolution, reporter| {
            let ran = discover::run(log, log_dir, resolution, discover, reporter);
            settled = ran.windowed;
            ran.resolve_again
        })?;
    settled.extend(watcher.routed().cloned());
    let seed = Seed {
        log_dir: log_dir.to_path_buf(),
        cfg: discover.clone(),
        settled,
    };
    Ok((registry, watcher, seed))
}

fn now_millis() -> Result<i64, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::Usage(e.to_string()))?;
    i64::try_from(now.as_millis()).map_err(|e| AppError::Usage(e.to_string()))
}

/// Resolves on the first Ctrl-C (SIGINT) or, on Unix, SIGTERM (what systemd sends by default).
async fn stop_signal() -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).map_err(AppError::Serve)?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.map_err(AppError::Serve),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.map_err(AppError::Serve)
    }
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
    /// Shared with [`local_bridge`], which sets it and [`Bridge::set_batch`] together.
    batch: Rc<Cell<usize>>,
}

struct ServeStorage {
    log: SqliteEventLog,
    verdicts: SqliteVerdictStore,
    /// The default routes plus one per accepted stream mapping (decision 0023).
    registry: EngineRegistry,
    /// The log position a restored snapshot covers; `None` replays from the start.
    resume: Option<LogPosition>,
    /// Absent under `--no-snapshot`.
    snapshots: Option<Snapshotter>,
    /// The proposal-store watcher that drives a live rebuild (s2w#184) and the snapshot
    /// configuration a rebuild restarts with. Absent in tests that pin a registry.
    watch: Option<(RouteWatcher, snapshots::SnapshotConfig)>,
    /// The in-run producer's start state (decision 0025); `None` turns the trigger off.
    discover: Option<Seed>,
}

impl LogReader for SharedLogReader {
    fn read_after(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        // The bridge takes exactly this many rows. Limit BEFORE collecting, not afterwards.
        let events: Vec<_> = self
            .log
            .borrow()
            .replay(from)?
            .take(self.batch.get())
            .collect();
        Ok(Box::new(events.into_iter()))
    }

    fn read_head(&self) -> Result<Option<LogPosition>, LogError> {
        self.log.borrow().head()
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

    fn head(&self) -> Result<Option<LogPosition>, LogError> {
        self.0.borrow().head()
    }
}

/// The in-run producer as the bridge loop drives it: the trigger, the shared log it counts, and
/// the note sink it reports through (the pump holds the reporter).
type LiveDiscover = (InRun, Rc<RefCell<SqliteEventLog>>, SinkReporter);

/// How long one catch-up poll may hold the runtime that also serves HTTP (s2w#331). A full
/// batch of 250 demo-stream events took ~54 ms to replay on the hub and 88-207 ms on the demo
/// box, and every request, static files included, waited behind one or more of them. Measured
/// on the hub (one core, full replay, first 30 s): mean `/main.js` latency 144 ms unbounded,
/// 29 ms at 20 ms for 14% less replay, 31 ms at 25 ms for 11% less, 21 ms at 15 ms for 19%
/// less. Replay cost grows as the budget shrinks because each poll also pays a fixed commit
/// and read cost (a 10 ms budget with a proportional cut roughly halved replay).
const POLL_BUDGET: Duration = Duration::from_millis(20);

/// The smallest batch a slow poll shrinks to.
const MIN_BATCH: usize = 8;

/// The batch after a full poll of `batch` events took `elapsed`, against `budget`. Over budget:
/// scaled toward it, but never below half, because part of a poll's cost is fixed and a
/// proportional cut undershoots. Under a quarter of it: doubled. Always within
/// [`MIN_BATCH`]..=`max`.
fn next_batch(batch: usize, max: usize, elapsed: Duration, budget: Duration) -> usize {
    let budget = budget.as_micros().max(1);
    let took = elapsed.as_micros().max(1);
    let floor = MIN_BATCH.min(max);
    let next = if took > budget {
        let scaled = u128::try_from(batch).unwrap_or(u128::MAX) * budget / took;
        usize::try_from(scaled).unwrap_or(usize::MAX).max(batch / 2)
    } else if took < budget / 4 {
        batch.saturating_mul(2)
    } else {
        batch
    };
    next.clamp(floor, max)
}

// Bridge::run requires Send and moves each poll to the blocking pool. This local driver uses
// the same poll/backoff policy, yielding even after full batches so ingestion/HTTP can run.
// Snapshot capture runs right after each poll with no await in between (decision 0024), then
// the proposal-store check that may swap in a live rebuild, also with no await (s2w#184).
// Each poll first awaits a write reservation (s2w#259); from there to the swap, no await.
async fn local_bridge(
    mut bridge: Bridge<SharedLogReader, SqliteVerdictStore>,
    config: BridgeConfig,
    ready: oneshot::Sender<()>,
    snapshots: Option<(Rc<RefCell<Snapshotter>>, QueryState)>,
    (mut rebuild, mut discover): (Option<Rebuild>, Option<LiveDiscover>),
) -> Result<(), BridgeError> {
    let mut ready = Some(ready);
    let mut delay = config.poll;
    loop {
        // The poll appends, and a rebuild swaps the timeline, on this runtime thread: wait here,
        // yielding, for any `/world` body to release its read guard, so the write below never
        // blocks the runtime that body needs to drain (s2w#259). No await until it drops.
        let reservation = bridge.state().reserve_write().await;
        let started = Instant::now();
        let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bridge.poll_once()))
            .map_err(|_| BridgeError::Task("bridge poll panicked".to_owned()))??;
        let polled = started.elapsed();
        if let Some((snapshotter, state)) = &snapshots {
            snapshotter
                .borrow_mut()
                .after_poll(bridge.mark(), report.stats.consumed, state);
        }
        // Profiling is synchronous: a source reaching its window stalls this loop once. It runs
        // before the rebuild check, so a mapping it files is picked up by the live rebuild.
        if let Some((in_run, log, reporter)) = &mut discover {
            in_run.after_poll(&log.borrow(), reporter);
            if in_run.is_done() {
                discover = None;
            }
        }
        if let Some(rebuild) = &mut rebuild {
            bridge = rebuild.after_poll(bridge, &report)?;
        }
        drop(reservation);
        if let Some(ready) = ready.take() {
            let _ignored = ready.send(());
        }
        if report.stats.consumed >= bridge.batch() as u64 && report.error.is_none() {
            // A full poll means a catch-up: size the next one to the budget, so HTTP gets a
            // turn about every POLL_BUDGET instead of every batch (s2w#331). The reader and the
            // bridge must agree on the batch, or a short read would look like the end of the log.
            // The reader copies the bridge's value, so the bridge's own clamp applies to both.
            // A small batch after catch-up is fine: a later burst fills it and it doubles back.
            // The snapshot capture after the poll is outside the budget on purpose: it runs
            // only at its own interval, not per batch.
            let batch = next_batch(bridge.batch(), config.batch.max(1), polled, POLL_BUDGET);
            bridge.set_batch(batch);
            bridge.reader().batch.set(bridge.batch());
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

#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a distinct input; a parameter struct is a follow-up refactor (s2w#156)"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one sequential pass whose steps share local state; splitting it is a follow-up refactor (s2w#156)"
)]
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
        batch: Rc::new(Cell::new(config.batch.max(1))),
    };
    let bridge = match storage.resume {
        Some(position) => Bridge::resume(
            reader,
            storage.verdicts,
            storage.registry,
            state.clone(),
            config,
            position,
        ),
        None => Bridge::new(
            reader,
            storage.verdicts,
            storage.registry,
            state.clone(),
            config,
        ),
    }
    .map_err(|error| AppError::BridgeStopped(error.to_string()))?;
    let snapshots = storage.snapshots.map(|s| Rc::new(RefCell::new(s)));
    // The final snapshot runs inside the stop branch only: after a signal, before supervise
    // drops the bridge and before the HTTP drain, never after a fatal error (decision 0024).
    let stop = {
        let (snapshots, state) = (snapshots.clone(), state.clone());
        async move {
            stop.await?;
            if let Some(snapshotter) = &snapshots {
                snapshotter.borrow_mut().finish(&state);
            }
            Ok(())
        }
    };
    let rebuild = storage.watch.map(|(watcher, snapshot_config)| {
        Rebuild::new(
            watcher,
            state.clone(),
            snapshot_config,
            snapshots.clone(),
            reporter.note_sink(),
        )
    });
    let bridge_snapshots = snapshots.map(|s| (s, state.clone()));
    // The pump holds `reporter` while the bridge runs; the trigger notes through its sink.
    let in_run = storage
        .discover
        .and_then(|seed| seed.arm(&started.sources))
        .map(|in_run| (in_run, shared.clone(), SinkReporter(reporter.note_sink())));
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
                    crate::status::Progress::named(name)
                        .with_watermarks(started.watermarks.clone()),
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
        local_bridge(
            bridge,
            config,
            ready_tx,
            bridge_snapshots,
            (rebuild, in_run),
        ),
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

#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a distinct input; a parameter struct is a follow-up refactor (s2w#156)"
)]
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
    let world_state = state.clone();
    router(state)
        .route_layer(middleware::from_fn(origin_guard))
        .fallback_service(crate::assets::asset_router())
        .layer(middleware::from_fn_with_state(world_state, world_routing))
        .layer(middleware::from_fn(host_allowlist))
}

/// `/w/{world}/` (+ no-trailing-slash, + legacy `/?world=`), stream2worlds#144. Rewrites the
/// request's URI to `/index.html` and forwards it — the same fallback asset router that
/// serves the SPA shell — rather than duplicating its serving logic (bare `/` is the home page). Applied as a
/// whole-router `.layer`, not `.route_layer`, because none of these three paths is one of
/// `router`'s own `.route`s; they only exist by intercepting requests the fallback would
/// otherwise have handled unchanged.
async fn world_routing(
    State(state): State<QueryState>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if let Some(world) = path
        .strip_prefix("/w/")
        .and_then(|rest| rest.strip_suffix('/'))
        .filter(|world| !world.is_empty() && !world.contains('/'))
    {
        // Same rule as `?world=` and the `/worlds/{world}` API routes: percent-decoded UTF-8.
        if let Err(error) = crate::query::check_world(&state, &percent_decode(world, false)) {
            return error.into_response();
        }
        *request.uri_mut() = Uri::from_static("/index.html");
        return next.run(request).await;
    }
    if let Some(world) = path
        .strip_prefix("/w/")
        .filter(|world| !world.is_empty() && !world.contains('/'))
    {
        // 307, not 301/308: preserves the GET method (irrelevant here) while never caching
        // indefinitely the way a 301/308 would — round-2 review finding.
        let mut location = format!("/w/{world}/");
        if let Some(query) = request.uri().query() {
            location.push('?');
            location.push_str(query);
        }
        return Redirect::temporary(&location).into_response();
    }
    if path == "/"
        && let Some(location) = world_query_redirect(request.uri())
    {
        // 302 (axum's `Redirect` has no public constructor for it): a legacy bookmark still
        // works, but is never cached as if the `?world=` form were canonical forever.
        return (
            StatusCode::FOUND,
            [(axum::http::header::LOCATION, location)],
        )
            .into_response();
    }
    next.run(request).await
}

/// Builds the `/w/<world>/[?<preserved params>]` target for a legacy `/?world=<x>` request, or
/// `None` when `?world=` is absent (`/` then serves the home page unchanged). The other
/// recognized query keys are forwarded as their original (already query-percent-encoded) bytes
/// — no decode/re-encode round trip, so nothing can be double-encoded.
fn world_query_redirect(uri: &Uri) -> Option<String> {
    let query = uri.query()?;
    let mut world = None;
    let mut kept = Vec::new();
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "world" => world = Some(value),
            "at" | "branch" | "lod" | "focus" | "hops" => kept.push(pair),
            _ => {}
        }
    }
    let world = percent_decode(world?, true);
    let mut location = format!("/w/{}/", percent_encode_path_segment(&world));
    if !kept.is_empty() {
        location.push('?');
        location.push_str(&kept.join("&"));
    }
    Some(location)
}

/// The one decode rule for a world name from a URL: `%XX` escapes are bytes, and the bytes are
/// read as UTF-8 (so `%C3%A9` is one `é`, not two chars). `plus_is_space` is true only for a
/// query value (`application/x-www-form-urlencoded`); in a path `+` is a literal plus. Invalid
/// or truncated escapes pass through literally and invalid UTF-8 becomes U+FFFD rather than an
/// error: this yields a redirect target or a name that then fails `check_world`, never a
/// validated input.
fn percent_decode(value: &str, plus_is_space: bool) -> String {
    let mut out = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' if plus_is_space => out.push(b' '),
            b'%' => {
                let rest = bytes.clone().take(2).collect::<Vec<_>>();
                if rest.len() == 2
                    && let Ok(hex) = std::str::from_utf8(&rest)
                    && let Ok(decoded) = u8::from_str_radix(hex, 16)
                {
                    out.push(decoded);
                    bytes.next();
                    bytes.next();
                } else {
                    out.push(b'%');
                }
            }
            other => out.push(other),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes every byte outside the URL path-segment "unreserved" set
/// (`ALPHA / DIGIT / "-" / "." / "_" / "~"`).
fn percent_encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
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
