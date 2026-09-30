//! The query API over HTTP (axum), with SSE deltas. Handlers parse parameters and call the
//! pure functions; every error is `{ "error": <code>, "message": <text> }`.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, TryLockError};
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use s2w_core::{FOLD_VERSION, World, WorldEvent};
use s2w_log::{
    MembershipRow, ReadOnlySqliteEventLog, WorldManifest, WorldPresentation, members_at,
};
use s2w_model::{SourceId, Timestamp};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::wrappers::ReceiverStream;

use super::QueryError;
use super::dashboard::DashboardView;
use super::delta::Delta;
use super::diff::{WorldDiff, diff};
use super::epoch::Epoch;
use super::generation::{self, Generations};
use super::projection::HeadView;
use super::proposals::ProposalsView;
use super::read_timings::{ReadTimings, ReadTimingsSnapshot};
use super::sentences::SentencesView;
use super::sources::SourceStats;
use super::stream;
use super::summary_memo::{SummaryKey, SummaryMemo};
use super::timeline::{BaseTime, HistoryEntry, TimeRange, Timeline};
use super::view::{
    ACTUAL_BRANCH, LinkDetail, Lod, ViewParams, WorldView, check_links, type_summary, world_view,
};

/// How long a `/world` body waits for reserved writes before answering 503 (s2w#259). A
/// reserved section is one bridge poll; the viewer retries a 503.
const BODY_YIELD_LIMIT: Duration = Duration::from_secs(30);

/// The longest a reserved writer, or a `/world` body yielding to one, sleeps between checks.
const YIELD_MAX_BACKOFF: Duration = Duration::from_millis(5);

/// A reserved timeline write ([`QueryState::reserve_write`]); `/world` bodies wait while it lives.
pub(crate) struct WriteReservation {
    _count: Counted,
}

/// One count in a counter, released on drop.
struct Counted(Arc<AtomicUsize>);

impl Counted {
    fn enter(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(counter))
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Shared server state: the timeline and a head-offset signal that wakes SSE subscribers.
#[derive(Clone)]
pub struct QueryState {
    timeline: Arc<RwLock<Timeline>>,
    /// Live [`WriteReservation`]s: a `/world` body takes no read guard while one exists.
    write_reservations: Arc<AtomicUsize>,
    /// `/world` bodies waiting out a reservation: the next reservation lets them in first, so
    /// back-to-back polls (a backfill) cannot keep every body out.
    bodies_waiting: Arc<AtomicUsize>,
    head: Arc<watch::Sender<u64>>,
    world: Arc<str>,
    sse_slots: Arc<tokio::sync::Semaphore>,
    manifest: Option<Arc<WorldManifest>>,
    membership: Arc<Vec<MembershipRow>>,
    source_stats: Arc<watch::Sender<BTreeMap<SourceId, SourceStats>>>,
    /// Sources whose world a live rebuild is refolding (s2w#184), published by `serve`.
    rebuilding: Arc<watch::Sender<BTreeMap<SourceId, Rebuilding>>>,
    /// The event-log directory, so presentation can be read fresh per request rather than
    /// cached at startup — a live `s2w presentation set` is visible without a restart.
    log_dir: Option<Arc<PathBuf>>,
    /// Where each `/world` generation's time went (s2w#243); `None` unless a measurement
    /// opts in with [`Self::with_read_timings`].
    read_timings: Option<Arc<ReadTimings>>,
    /// The single-flight gate for full `/world` bodies (s2w#270, s2w#297).
    generations: Arc<Generations>,
    /// The last type summary built (s2w#296).
    summaries: Arc<SummaryMemo>,
    /// How long a `/world` request waits for its answer, and a body for writers, before a 503:
    /// [`BODY_YIELD_LIMIT`], shorter in tests.
    body_wait: Duration,
}

impl QueryState {
    /// Serves `timeline`.
    #[must_use]
    pub fn new(timeline: Timeline) -> Self {
        let (head, _) = watch::channel(timeline.head());
        let (source_stats, _) = watch::channel(BTreeMap::new());
        let (rebuilding, _) = watch::channel(BTreeMap::new());
        Self {
            timeline: Arc::new(RwLock::new(timeline)),
            write_reservations: Arc::new(AtomicUsize::new(0)),
            bodies_waiting: Arc::new(AtomicUsize::new(0)),
            head: Arc::new(head),
            world: Arc::from("default"),
            sse_slots: Arc::new(tokio::sync::Semaphore::new(32)),
            manifest: None,
            membership: Arc::new(Vec::new()),
            source_stats: Arc::new(source_stats),
            rebuilding: Arc::new(rebuilding),
            log_dir: None,
            read_timings: None,
            generations: Arc::default(),
            summaries: Arc::default(),
            body_wait: BODY_YIELD_LIMIT,
        }
    }

    /// Configures the string identifier this process serves.
    #[must_use]
    pub fn with_world(mut self, world: impl Into<Arc<str>>) -> Self {
        self.world = world.into();
        self
    }

    /// Attaches immutable directory metadata loaded before the serving pump starts.
    #[must_use]
    pub fn with_metadata(
        mut self,
        manifest: Option<WorldManifest>,
        membership: Vec<MembershipRow>,
    ) -> Self {
        self.manifest = manifest.map(Arc::new);
        self.membership = Arc::new(membership);
        self
    }

    /// Configures the event-log directory presentation reads are served fresh from. Absent in
    /// tests that never write presentation: [`Self::presentation`] then always reports the
    /// default (empty) record.
    #[must_use]
    pub fn with_log_dir(mut self, log_dir: impl Into<PathBuf>) -> Self {
        self.log_dir = Some(Arc::new(log_dir.into()));
        self
    }

    /// Records where each `/world` generation's time goes: the wait for the guard, the capture
    /// under it, then the sort and the write after it is released (s2w#243, s2w#272). A
    /// measurement hook: serve
    /// never calls it, and it changes no response byte.
    #[must_use]
    pub fn with_read_timings(mut self) -> Self {
        self.read_timings = Some(Arc::default());
        self
    }

    /// The `/world` hold timings so far, or `None` unless [`Self::with_read_timings`] was set.
    #[must_use]
    pub fn read_timings(&self) -> Option<ReadTimingsSnapshot> {
        self.read_timings.as_deref().map(ReadTimings::snapshot)
    }

    /// The hold timings to record into, when a measurement opted in.
    pub(super) fn timings(&self) -> Option<&ReadTimings> {
        self.read_timings.as_deref()
    }

    /// The single-flight gate for full `/world` bodies.
    pub(super) fn generations(&self) -> &Generations {
        &self.generations
    }

    /// The type summary of `t` at `offset`, labelled with the served epoch: the memo's copy
    /// when it holds that (epoch, hub cap, offset), else a fresh build that replaces it.
    ///
    /// # Errors
    /// As [`Timeline::check_offset`], checked before the memo so an offset that fell below the
    /// base is refused even when its summary is memoised.
    pub(super) fn summary_at(&self, t: &Timeline, offset: u64) -> Result<WorldView, QueryError> {
        t.check_offset(offset)?;
        let key = SummaryKey {
            epoch: t.epoch(),
            hub_cap: t.hub_cap(),
            offset,
        };
        self.summaries.get_or_build(key, || {
            let world = t.world_at(offset)?;
            Ok(WorldView {
                epoch: t.epoch(),
                ..type_summary(&world)
            })
        })
    }

    /// Shortens how long a `/world` request waits before a 503.
    #[cfg(test)]
    pub(super) fn with_body_wait(mut self, wait: Duration) -> Self {
        self.body_wait = wait;
        self
    }

    /// The string identifier of the world this process serves.
    #[must_use]
    pub fn world(&self) -> &str {
        &self.world
    }

    /// The world's presentation, read fresh from the log directory on every call (never
    /// cached): a live `s2w presentation set` must be visible on the next request, and
    /// `/worlds` and `/worlds/{world}/presentation` must never disagree because one of them
    /// serves a startup-time snapshot. Returns the default (empty) record when no log
    /// directory is configured or no presentation has ever been set.
    ///
    /// # Errors
    /// [`QueryError::Storage`] if the log directory exists but cannot be opened or read.
    pub fn presentation(&self) -> Result<WorldPresentation, QueryError> {
        let Some(log_dir) = &self.log_dir else {
            return Ok(WorldPresentation::default());
        };
        let reader = ReadOnlySqliteEventLog::open(log_dir.as_path())
            .map_err(|error| QueryError::Storage(error.to_string()))?;
        Ok(reader
            .world_presentation(&self.world)
            .map_err(|error| QueryError::Storage(error.to_string()))?
            .unwrap_or_default())
    }

    /// The configured log directory, if any.
    pub(crate) fn log_dir(&self) -> Option<&std::path::Path> {
        self.log_dir.as_deref().map(PathBuf::as_path)
    }

    /// The proposals view, read fresh from the log directory's proposal store on every call.
    /// Empty when no log directory is configured or the proposal store file does not exist;
    /// never creates it.
    ///
    /// # Errors
    /// [`QueryError::Storage`] if the store exists but cannot be opened or read — never an
    /// empty view.
    pub fn proposals(&self) -> Result<ProposalsView, QueryError> {
        let Some(log_dir) = self.log_dir() else {
            return Ok(ProposalsView::default());
        };
        super::proposal_store::read_view(log_dir)
    }

    /// The served world's dashboard (decision 0029), read fresh from the log directory's
    /// proposal store on every call. Empty when no log directory is configured or the store
    /// file does not exist; never creates it.
    ///
    /// # Errors
    /// [`QueryError::Storage`] if the store exists but cannot be opened or read.
    pub fn dashboard(&self) -> Result<DashboardView, QueryError> {
        let Some(log_dir) = self.log_dir() else {
            return Ok(DashboardView::default());
        };
        super::dashboard::read_dashboard(log_dir, self.world())
    }

    /// The last `last` events of the world's member sources as sentences (s2w#302), read
    /// fresh from the log directory; each entity the head world holds carries its id. Empty
    /// when no log directory is configured.
    ///
    /// # Errors
    /// [`QueryError::BadParameter`] when `last` is outside `1..=MAX_SENTENCES`;
    /// [`QueryError::Storage`] if the log or the proposal store cannot be read.
    pub fn sentences(&self, last: u64) -> Result<SentencesView, QueryError> {
        let last = super::sentences::check_last(last)?;
        let Some(log_dir) = self.log_dir() else {
            return Ok(SentencesView::default());
        };
        let mut view = super::sentences::read_sentences(log_dir, self.world(), last)?;
        self.read(|t| {
            let world = t.head_world();
            for entity in view.rows.iter_mut().flat_map(|row| &mut row.entities) {
                entity.entity = world
                    .keys()
                    .get(&s2w_model::NaturalKey::new(entity.key.as_str()))
                    .map(|id| world.resolve(*id).get());
            }
            Ok(())
        })?;
        Ok(view)
    }

    /// Appends an event (see [`Timeline::append`]) and wakes live subscribers.
    ///
    /// # Errors
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn append(&self, at: Timestamp, event: WorldEvent) -> Result<u64, QueryError> {
        let head = self
            .timeline
            .write()
            .map_err(|_| QueryError::Unavailable)?
            .append(at, event);
        self.head.send_replace(head);
        Ok(head)
    }

    /// Appends a batch of events under ONE write lock (see [`Timeline::append`], called once
    /// per event, so every event keeps its own offset and delta) and wakes live subscribers
    /// once with the final head (s2w#216). The bridge calls this once per poll batch rather
    /// than [`Self::append`] once per claim, so a long read (a `/world` projection) delays a
    /// batch at most once instead of once per claim. An empty batch takes no write lock and
    /// wakes nobody; it returns the current head (under a read lock).
    ///
    /// # Errors
    /// [`QueryError::Unavailable`] if the lock was poisoned; nothing in the batch is appended.
    pub fn append_batch(
        &self,
        events: impl IntoIterator<Item = (Timestamp, WorldEvent)>,
    ) -> Result<u64, QueryError> {
        let mut events = events.into_iter().peekable();
        if events.peek().is_none() {
            return self.read(|t| Ok(t.head()));
        }
        // The guard drops at the end of this block, before subscribers are woken.
        let head = {
            let mut timeline = self.timeline.write().map_err(|_| QueryError::Unavailable)?;
            let mut head = timeline.head();
            for (at, event) in events {
                head = timeline.append(at, event);
            }
            head
        };
        self.head.send_replace(head);
        Ok(head)
    }

    /// Installs a timeline (decision 0024) and wakes subscribers with its head. World and
    /// [`Epoch`] are one value, so they are swapped under one write lock and every read sees a
    /// consistent pair; an SSE follower notices the new epoch on its next read and ends with
    /// `stale_epoch`. `serve` calls this at start-up (`snapshots::prepare`: the empty timeline
    /// under the registry's epoch, then a restored one), before the bridge starts, and again on
    /// a live rebuild (s2w#184) between two bridge polls.
    ///
    /// # Errors
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn replace_timeline(&self, timeline: Timeline) -> Result<(), QueryError> {
        let head = timeline.head();
        *self.timeline.write().map_err(|_| QueryError::Unavailable)? = timeline;
        self.head.send_replace(head);
        Ok(())
    }

    /// The timeline's replay base (the offset before its oldest retained event, so
    /// `base == head` means it holds no events), head offset and hub cap, read under one lock.
    ///
    /// # Errors
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn bounds(&self) -> Result<(u64, u64, u64), QueryError> {
        self.read(|t| Ok((t.replay_base(), t.head(), t.hub_cap())))
    }

    /// Runs `f` on the head world and its time bounds under one read lock: what a snapshot
    /// records (decision 0024). `serve` encodes the snapshot inside `f`, so the head is never
    /// cloned (#179). `f` runs on the caller's thread with the read lock held: other readers
    /// proceed, appends wait. Only the bridge appends, and it calls this between polls (under
    /// its `reserve_write`, s2w#259).
    ///
    /// # Errors
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn with_head<T>(&self, f: impl FnOnce(&World, BaseTime) -> T) -> Result<T, QueryError> {
        self.read(|t| Ok(f(t.head_world(), t.head_time())))
    }

    /// Replaces the per-source bridge statistics `/worlds/{world}/sources` serves. The live
    /// bridge publishes after each poll; a process with no bridge (the read-only MCP replay
    /// path) never does, so every source there reports zeros.
    pub(crate) fn publish_source_stats(&self, stats: BTreeMap<SourceId, SourceStats>) {
        self.source_stats.send_replace(stats);
    }

    /// Replaces the sources `/worlds/{world}/sources` reports as rebuilding: set by `serve`
    /// when a mapping change starts a live rebuild, cleared when its backfill completes.
    pub(crate) fn publish_rebuilding(&self, rebuilding: BTreeMap<SourceId, Rebuilding>) {
        self.rebuilding.send_replace(rebuilding);
    }

    /// Events of `source` the bridge has consumed so far (zero before its first batch).
    #[cfg(test)]
    pub(crate) fn consumed(&self, source: &SourceId) -> u64 {
        self.source_stats
            .borrow()
            .get(source)
            .map_or(0, |stats| stats.consumed)
    }

    /// The served epoch and the sources reported as rebuilding, read with no await between
    /// them, so a test on the current-thread runtime sees both from one side of a rebuild swap.
    #[cfg(test)]
    pub(crate) fn epoch_and_rebuilding(&self) -> (Epoch, usize) {
        let epoch = self.timeline.read().expect("timeline lock").epoch();
        (epoch, self.rebuilding.borrow().len())
    }

    /// The retained events, in order.
    #[cfg(test)]
    pub(crate) fn timeline_events(&self) -> Vec<super::TimedEvent> {
        self.read(|t| Ok(t.events_after(t.replay_base())?.to_vec()))
            .unwrap_or_default()
    }

    fn read<T>(&self, f: impl FnOnce(&Timeline) -> Result<T, QueryError>) -> Result<T, QueryError> {
        f(&*self.timeline.read().map_err(|_| QueryError::Unavailable)?)
    }

    /// Waits, without blocking the runtime, until the timeline can be written, and keeps
    /// `/world` bodies from taking a read guard until the reservation drops (s2w#259).
    ///
    /// `serve`'s bridge appends on the current-thread runtime, while a `/world` generation holds
    /// a read guard on a blocking thread for its capture. Before s2w#272 the guard was held
    /// through the write, which waits for that same runtime to drain the body's channel: a
    /// plain `write` there blocked the runtime, nothing drained the body, and after
    /// [`stream::STALL`] the body ended cut short at the channel's capacity (~4 MiB), the demo
    /// viewer's "Failed to fetch". The capture no longer waits on the runtime, but a plain
    /// `write` would still block the runtime for a whole capture. Hold the reservation across
    /// the synchronous section that writes, and take the write lock there with no await in
    /// between.
    ///
    /// A body already waiting goes first, for at most [`stream::STALL`]: it wakes within
    /// [`YIELD_MAX_BACKOFF`], so this wait is short unless a body is stuck. A capture already
    /// under way is waited out: the guard is released once the view is copied out.
    pub(crate) async fn reserve_write(&self) -> WriteReservation {
        let until = Instant::now() + stream::STALL;
        let mut backoff = Duration::from_millis(1);
        while self.bodies_waiting.load(Ordering::SeqCst) > 0 && Instant::now() < until {
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(YIELD_MAX_BACKOFF);
        }
        let reservation = WriteReservation {
            _count: Counted::enter(&self.write_reservations),
        };
        let mut backoff = Duration::from_millis(1);
        // A poisoned lock stops the wait; the write itself reports it.
        while matches!(self.timeline.try_write(), Err(TryLockError::WouldBlock)) {
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(YIELD_MAX_BACKOFF);
        }
        reservation
    }

    /// The read guard a `/world` generation holds while it captures its view (s2w#272: released
    /// before the sort and the write). Taken only while no write is
    /// reserved (checked after acquiring, so a reservation made meanwhile is seen): a body never
    /// holds the guard a reserved writer is about to take on the runtime it waits for. Gives up
    /// with [`QueryError::Unavailable`] (503; the viewer retries) after the body wait limit
    /// ([`BODY_YIELD_LIMIT`]), or as soon as `wanted` says no client is waiting any more.
    pub(super) fn read_for_body(
        &self,
        wanted: impl Fn() -> bool,
    ) -> Result<RwLockReadGuard<'_, Timeline>, QueryError> {
        let until = Instant::now() + self.body_wait;
        let mut backoff = Duration::from_millis(1);
        let mut waiting = None;
        loop {
            let guard = self.timeline.read().map_err(|_| QueryError::Unavailable)?;
            if self.write_reservations.load(Ordering::SeqCst) == 0 {
                return Ok(guard);
            }
            drop(guard);
            if waiting.is_none() {
                waiting = Some(Counted::enter(&self.bodies_waiting));
            }
            if Instant::now() >= until || !wanted() {
                return Err(QueryError::Unavailable);
            }
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(YIELD_MAX_BACKOFF);
        }
    }

    /// A copy of the world at `at`, or at the head when `at` is absent. At the head this
    /// clones the whole world: for tests and replay checks, never a request path (#216).
    ///
    /// # Errors
    /// Whatever [`Timeline::world_at`] returns, or [`QueryError::Unavailable`] if the lock was
    /// poisoned.
    pub fn world_at(&self, at: Option<u64>) -> Result<World, QueryError> {
        self.read(|t| Ok(t.world_at(at.unwrap_or_else(|| t.head()))?.into_owned()))
    }

    /// The view at `at` (or the head), labelled with the epoch it was read under. World,
    /// epoch and projection all come from one read: the head is projected where it lies,
    /// never copied (#216), so appends wait for the projection (`serve`'s bridge without
    /// blocking the runtime: `reserve_write`, s2w#259).
    ///
    /// # Errors
    /// [`QueryError::StaleEpoch`] when `epoch` is given and is not the served one (checked
    /// first); whatever [`Timeline::world_at`] and [`world_view`] return; or
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn view_at(
        &self,
        at: Option<u64>,
        epoch: Option<Epoch>,
        params: &ViewParams,
    ) -> Result<WorldView, QueryError> {
        self.read(|t| {
            t.check_epoch(epoch)?;
            let at = at.unwrap_or_else(|| t.head());
            if params.links == LinkDetail::None {
                check_links(params)?;
                return self.summary_at(t, at);
            }
            let world = t.world_at(at)?;
            Ok(WorldView {
                epoch: t.epoch(),
                ..world_view(&world, params)?
            })
        })
    }

    /// The one branch served, with its head and fold version.
    ///
    /// # Errors
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn branches(&self) -> Result<Vec<Branch>, QueryError> {
        self.read(|t| {
            Ok(vec![Branch {
                name: ACTUAL_BRANCH,
                world_id: 0,
                head: t.head(),
                fold_version: FOLD_VERSION,
                hub_in_degree_cap: t.hub_cap(),
            }])
        })
    }

    /// The entity's history up to `to`, or up to the head when `to` is absent.
    ///
    /// # Errors
    /// [`QueryError::StaleEpoch`] (checked first), whatever [`Timeline::history`] returns, or
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn history(
        &self,
        id: u64,
        to: Option<u64>,
        epoch: Option<Epoch>,
    ) -> Result<Vec<HistoryEntry>, QueryError> {
        self.read(|t| {
            t.check_epoch(epoch)?;
            t.history(id, to.unwrap_or_else(|| t.head()))
        })
    }

    /// What changed between `from` and `to`, or the head when `to` is absent.
    ///
    /// # Errors
    /// [`QueryError::StaleEpoch`] (checked first), [`QueryError::OffsetBeforeBase`] below the
    /// base, [`QueryError::OffsetBeyondHead`] past the head, or [`QueryError::Unavailable`] if
    /// the lock was poisoned. Both worlds come from one read, so a diff never spans two
    /// histories, and the diff is computed under it: the head is borrowed, never copied
    /// (#216). Appends wait for both projections, and for the fold of `from` below the head
    /// (full history only). `from == to` is empty without projecting anything; once the
    /// history window has dropped, head..head is the only diff there is.
    pub fn diff(
        &self,
        from: u64,
        to: Option<u64>,
        epoch: Option<Epoch>,
    ) -> Result<WorldDiff, QueryError> {
        self.read(|t| {
            t.check_epoch(epoch)?;
            let to = to.unwrap_or_else(|| t.head());
            if from == to {
                t.check_offset(from)?;
                return Ok(WorldDiff::unchanged(from));
            }
            diff(&*t.world_at(from)?, &*t.world_at(to)?)
        })
    }

    /// The time index at `ts`, or its whole range when `ts` is absent.
    ///
    /// # Errors
    /// [`QueryError::StaleEpoch`] (checked first); [`QueryError::TimeBeforeBase`] for a `ts`
    /// inside a restored snapshot; [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn time(&self, ts: Option<i64>, epoch: Option<Epoch>) -> Result<TimeResult, QueryError> {
        self.read(|t| {
            t.check_epoch(epoch)?;
            Ok(match ts {
                Some(ts) => TimeResult::At(TimeAt {
                    ts,
                    offset: t.offset_at(Timestamp::from_millis(ts))?,
                    epoch: t.epoch(),
                }),
                None => TimeResult::Range(t.time_range()),
            })
        })
    }
}

impl IntoResponse for QueryError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::OffsetBeyondHead { .. }
            | Self::UnknownEntity { .. }
            | Self::UnknownWorld { .. }
            | Self::UnknownProposal { .. } => StatusCode::NOT_FOUND,
            Self::OffsetBeforeBase { .. }
            | Self::TimeBeforeBase { .. }
            | Self::StaleEpoch { .. } => StatusCode::GONE,
            Self::BranchNotYet { .. } | Self::LodNotYet { .. } => StatusCode::NOT_IMPLEMENTED,
            Self::BadParameter { .. } | Self::HopsTooLarge { .. } => StatusCode::BAD_REQUEST,
            Self::Unavailable | Self::StreamLimit | Self::WorldQueueFull => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::StoreLocked => StatusCode::CONFLICT,
        };
        (status, Json(self.json_body())).into_response()
    }
}

/// The query API's routes over `state`.
pub fn router(state: QueryState) -> Router {
    Router::new()
        .route("/worlds", get(list_worlds))
        .route("/worlds/{world}/world", get(world))
        .route("/worlds/{world}/events", get(events))
        .route("/worlds/{world}/branches", get(branches))
        .route("/worlds/{world}/diff", get(world_diff))
        .route("/worlds/{world}/entity/{id}/history", get(history))
        .route("/worlds/{world}/time", get(time))
        .route("/worlds/{world}/sources", get(sources))
        .route("/worlds/{world}/presentation", get(world_presentation))
        .route("/worlds/{world}/proposals", get(world_proposals))
        .route("/worlds/{world}/dashboard", get(world_dashboard))
        .route("/worlds/{world}/sentences", get(world_sentences))
        .with_state(state)
}

/// Raw query parameters: parsed by hand so every failure is a typed JSON error.
#[derive(Debug, Default, Deserialize)]
struct Params {
    at: Option<String>,
    branch: Option<String>,
    lod: Option<String>,
    focus: Option<String>,
    hops: Option<String>,
    from: Option<String>,
    to: Option<String>,
    ts: Option<String>,
    epoch: Option<String>,
    last: Option<String>,
    links: Option<String>,
}

pub(crate) fn parse<T: std::str::FromStr>(
    name: &'static str,
    raw: Option<&str>,
) -> Result<Option<T>, QueryError>
where
    T::Err: std::fmt::Display,
{
    raw.map(|s| {
        s.parse().map_err(|e: T::Err| QueryError::BadParameter {
            name,
            reason: format!("'{s}': {e}"),
        })
    })
    .transpose()
}

pub(crate) fn check_branch(branch: Option<&str>) -> Result<(), QueryError> {
    match branch {
        None => Ok(()),
        Some(b) if b == ACTUAL_BRANCH => Ok(()),
        Some(b) => Err(QueryError::BranchNotYet {
            branch: b.to_owned(),
        }),
    }
}

pub(crate) fn check_world(state: &QueryState, world: &str) -> Result<(), QueryError> {
    if world == state.world() {
        Ok(())
    } else {
        Err(QueryError::UnknownWorld {
            world: world.to_owned(),
        })
    }
}

pub(crate) fn parse_lod(raw: Option<&str>) -> Result<Lod, QueryError> {
    match raw {
        None | Some("entity") => Ok(Lod::Entity),
        Some("type") => Ok(Lod::Type),
        Some("cluster") => Err(QueryError::LodNotYet {
            lod: "cluster".to_owned(),
        }),
        Some(other) => Err(QueryError::BadParameter {
            name: "lod",
            reason: format!("'{other}' is not one of type, cluster, entity"),
        }),
    }
}

pub(crate) fn parse_links(raw: Option<&str>) -> Result<LinkDetail, QueryError> {
    match raw {
        None | Some("all") => Ok(LinkDetail::All),
        Some("none") => Ok(LinkDetail::None),
        Some(other) => Err(QueryError::BadParameter {
            name: "links",
            reason: format!("'{other}' is not one of all, none"),
        }),
    }
}

/// `/world`: the view, streamed (#216). The projection runs on a blocking thread: captured
/// under the read guard, then sorted and serialized after it is released (s2w#272). The JSON
/// reaches the client in bounded chunks, so the whole body is never resident. Answers `304` to a matching
/// `If-None-Match` before projecting anything. Every full view (`lod=entity`, and `lod=type`
/// since s2w#297) goes through the single-flight gate (s2w#270, [`generation`]): at most one
/// full projection is in flight, shared by every request with the same parameters. The type
/// summary (`lod=type&links=none`) is served alone. A request not answered within
/// [`BODY_YIELD_LIMIT`] gets `503`, the viewer retries.
async fn world(
    State(state): State<QueryState>,
    Path(world): Path<String>,
    Query(p): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let parsed = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        check_branch(p.branch.as_deref())?;
        let params = ViewParams {
            lod: parse_lod(p.lod.as_deref())?,
            focus: parse("focus", p.focus.as_deref())?,
            hops: parse("hops", p.hops.as_deref())?.unwrap_or(1),
            links: parse_links(p.links.as_deref())?,
        };
        // Before any guard, tag or 304: an invalid `links` never answers `Not Modified`.
        check_links(&params)?;
        let at = parse("at", p.at.as_deref())?;
        Ok((at, parse("epoch", p.epoch.as_deref())?, params))
    };
    let (at, epoch, params) = match parsed() {
        Ok(parsed) => parsed,
        Err(error) => return error.into_response(),
    };
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let (answer_tx, answer_rx) = oneshot::channel();
    let key = generation::Key { epoch, at, params };
    let waiter = generation::Waiter::new(if_none_match, answer_tx);
    // Only the type summary (s2w#296; `check_links` allows `links=none` on it alone) is served
    // alone: it is small, and first paint must not wait behind a full projection. Every full
    // view, `lod=type` too (s2w#297), is shared through the queue.
    if params.links == LinkDetail::None {
        let state = state.clone();
        tokio::task::spawn_blocking(move || generation::serve_alone(&state, key, waiter));
    } else if let Err(error) = generation::submit(&state, key, waiter) {
        return error.into_response();
    }
    // Giving up drops the receiver: a queued request is then dropped before it is built.
    match tokio::time::timeout(state.body_wait, answer_rx).await {
        Ok(answer) => world_response(answer),
        Err(_) => QueryError::Unavailable.into_response(),
    }
}

/// The response for `/world`'s answer; a dropped sender means the generation panicked.
fn world_response(
    answer: Result<Result<WorldAnswer, QueryError>, oneshot::error::RecvError>,
) -> Response {
    match answer {
        Ok(Ok(WorldAnswer::NotModified(tag))) => {
            (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response()
        }
        Ok(Ok(WorldAnswer::Body(tag, body))) => (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                ),
                (header::ETAG, tag),
            ],
            axum::body::Body::from_stream(body),
        )
            .into_response(),
        Ok(Err(error)) => error.into_response(),
        // The generation panicked before answering.
        Err(_) => QueryError::Unavailable.into_response(),
    }
}

/// Checks the epoch and the offset (in that order, as every offset read does) and returns the
/// view's tag with the offset it names, so a caller can answer `304` before any projection.
pub(super) fn resolve_offset(
    t: &Timeline,
    epoch: Option<Epoch>,
    at: Option<u64>,
    params: &ViewParams,
) -> Result<(HeaderValue, u64), QueryError> {
    t.check_epoch(epoch)?;
    let offset = at.unwrap_or_else(|| t.head());
    t.check_offset(offset)?;
    Ok((world_etag(t.epoch(), t.hub_cap(), offset, params), offset))
}

/// Serializes `view` into `writer`. A failed write (client gone or stalled) drops `writer`
/// unfinished, which ends the body with an error; there is no one left to report it to.
pub(super) fn write_view(view: &HeadView, mut writer: stream::ChunkWriter) {
    if serde_json::to_writer(&mut writer, view).is_ok() {
        let _ = writer.finish();
    }
}

/// What `/world` answers once the read guard is held: `304`, or the tag and the body stream.
pub(super) enum WorldAnswer {
    NotModified(HeaderValue),
    Body(HeaderValue, stream::ChunkStream),
}

/// `/world`'s entity tag: the view is a pure function of (epoch, fold, offset, params), so equal
/// tags name equal bytes. The epoch names the feed, not the fold, so the tag also carries
/// [`FOLD_VERSION`] and the hub cap: a deploy that changes either under the same mapping must
/// not answer 304 to a page that still holds an old tag. The type summary (`links=none`,
/// s2w#296) adds `-nolinks`; every other tag is unchanged by it.
fn world_etag(epoch: Epoch, hub_cap: u64, offset: u64, params: &ViewParams) -> HeaderValue {
    let lod = match params.lod {
        Lod::Type => "type",
        Lod::Entity => "entity",
    };
    let focus = params
        .focus
        .map_or_else(|| "-".to_owned(), |f| f.to_string());
    let links = match params.links {
        LinkDetail::All => "",
        LinkDetail::None => "-nolinks",
    };
    let tag = format!(
        "\"{epoch}-f{FOLD_VERSION}-c{hub_cap}-{offset}-{lod}-{focus}-{}{links}\"",
        params.hops
    );
    // Hex, digits, letters, dashes and quotes only: always a valid header value.
    HeaderValue::from_str(&tag).unwrap_or_else(|_| HeaderValue::from_static("\"\""))
}

/// Whether an `If-None-Match` value names `tag` (weak comparison, or `*`).
pub(super) fn etag_matches(if_none_match: &str, tag: &HeaderValue) -> bool {
    let tag = tag.to_str().unwrap_or_default();
    if_none_match.split(',').map(str::trim).any(|candidate| {
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == tag
    })
}

/// One world served by this process.
#[derive(Serialize)]
pub struct WorldSummary {
    /// The world's string identifier.
    pub world: String,
    /// The manifest's display name, falling back to the world identifier for legacy worlds.
    pub name: String,
    /// The world's latest fold offset.
    pub head: u64,
    /// Presentation-supplied title override, read fresh; absent when no presentation has been
    /// set. The client's fallback chain (title -> name -> world) is not pre-collapsed here.
    pub title: Option<String>,
    /// Presentation-supplied tagline, read fresh; absent when no presentation has been set.
    pub tagline: Option<String>,
}

#[derive(Serialize)]
struct Worlds {
    worlds: Vec<WorldSummary>,
}

async fn list_worlds(State(state): State<QueryState>) -> Response {
    let run = || -> Result<_, QueryError> {
        let presentation = state.presentation()?;
        state.read(|timeline| {
            let world = state.world().to_owned();
            Ok(Worlds {
                worlds: vec![WorldSummary {
                    name: state
                        .manifest
                        .as_ref()
                        .map_or_else(|| world.clone(), |m| m.name.clone()),
                    world,
                    head: timeline.head(),
                    title: presentation.title.clone(),
                    tagline: presentation.tagline.clone(),
                }],
            })
        })
    };
    run().map(Json).into_response()
}

async fn world_presentation(
    State(state): State<QueryState>,
    Path(world): Path<String>,
) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        state.presentation()
    };
    run().map(Json).into_response()
}

async fn world_proposals(State(state): State<QueryState>, Path(world): Path<String>) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        state.proposals()
    };
    run().map(Json).into_response()
}

async fn world_dashboard(State(state): State<QueryState>, Path(world): Path<String>) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        state.dashboard()
    };
    run().map(Json).into_response()
}

async fn world_sentences(
    State(state): State<QueryState>,
    Path(world): Path<String>,
    Query(p): Query<Params>,
) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        let last = parse::<u64>("last", p.last.as_deref())?.ok_or(QueryError::BadParameter {
            name: "last",
            reason: format!("is required, from 1 to {}", super::sentences::MAX_SENTENCES),
        })?;
        state.sentences(last)
    };
    run().map(Json).into_response()
}

/// One served world branch.
#[derive(Serialize)]
pub struct Branch {
    /// The branch's name; only `actual` exists.
    pub name: &'static str,
    /// The world the branch folds; always 0 today.
    pub world_id: u64,
    /// The branch's latest offset.
    pub head: u64,
    /// The fold version that produced the world.
    pub fold_version: u32,
    /// The in-degree cap the world was folded under.
    pub hub_in_degree_cap: u64,
}

async fn branches(State(state): State<QueryState>, Path(world): Path<String>) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        state.branches()
    };
    run().map(Json).into_response()
}

async fn world_diff(
    State(state): State<QueryState>,
    Path(world): Path<String>,
    Query(p): Query<Params>,
) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        check_branch(p.branch.as_deref())?;
        let from = parse("from", p.from.as_deref())?.unwrap_or(0);
        let to = parse("to", p.to.as_deref())?;
        state.diff(from, to, parse("epoch", p.epoch.as_deref())?)
    };
    run().map(Json).into_response()
}

#[derive(Deserialize)]
struct HistoryPath {
    world: String,
    id: String,
}

async fn history(
    State(state): State<QueryState>,
    Path(HistoryPath { world, id }): Path<HistoryPath>,
    Query(p): Query<Params>,
) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        check_branch(p.branch.as_deref())?;
        let id = id.parse::<u64>().map_err(|e| QueryError::BadParameter {
            name: "id",
            reason: format!("'{id}': {e}"),
        })?;
        let to = parse("to", p.to.as_deref())?;
        state.history(id, to, parse("epoch", p.epoch.as_deref())?)
    };
    run().map(Json).into_response()
}

/// `/worlds/{world}/time`'s answer at one timestamp.
#[derive(Serialize)]
pub struct TimeAt {
    /// The timestamp asked about, in milliseconds.
    pub ts: i64,
    /// The largest offset whose events were received at or before `ts`.
    pub offset: u64,
    /// The history `offset` belongs to.
    pub epoch: Epoch,
}

/// `/worlds/{world}/time`'s answer: one timestamp's offset, or the whole range when no `ts` was
/// given. The two shapes serialize flat, exactly as the two HTTP responses always did.
#[derive(Serialize)]
#[serde(untagged)]
pub enum TimeResult {
    /// The offset at a timestamp.
    At(TimeAt),
    /// The time index's range.
    Range(TimeRange),
}

async fn time(
    State(state): State<QueryState>,
    Path(world): Path<String>,
    Query(p): Query<Params>,
) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        check_branch(p.branch.as_deref())?;
        state.time(
            parse("ts", p.ts.as_deref())?,
            parse("epoch", p.epoch.as_deref())?,
        )
    };
    run().map(Json).into_response()
}

#[derive(Serialize)]
struct Message<'a> {
    offset: u64,
    #[serde(flatten)]
    delta: &'a Delta,
}

/// An SSE message's `id:`: `<epoch>:<offset>`, so a browser's automatic `Last-Event-ID`
/// reconnect carries the history it read (s2w#184).
fn sse_id(epoch: Epoch, offset: u64) -> String {
    format!("{epoch}:{offset}")
}

/// Parses a `Last-Event-ID`: `<epoch>:<offset>` (what this server sends), or a bare `<offset>`
/// (a client that opted out of the epoch check).
fn parse_last_event_id(raw: &str) -> Result<(Option<Epoch>, u64), QueryError> {
    let bad = |reason: String| QueryError::BadParameter {
        name: "Last-Event-ID",
        reason: format!("'{raw}': {reason}"),
    };
    let (epoch, offset) = match raw.split_once(':') {
        Some((epoch, offset)) => (Some(epoch.parse::<Epoch>().map_err(bad)?), offset),
        None => (None, raw),
    };
    let offset = offset.parse::<u64>().map_err(|e| bad(e.to_string()))?;
    Ok((epoch, offset))
}

fn sse_event(epoch: Epoch, offset: u64, delta: &Delta) -> Event {
    let event = Event::default()
        .id(sse_id(epoch, offset))
        .event(delta.kind());
    match event.json_data(Message { offset, delta }) {
        Ok(event) => event,
        Err(e) => Event::default()
            .id(sse_id(epoch, offset))
            .event("error")
            .data(e.to_string()),
    }
}

/// The one SSE event a stream ends with when it cannot continue: `event: error`, data the
/// same `{"error": code, "message": …}` body an HTTP error carries.
fn sse_error(error: &QueryError) -> Event {
    Event::default()
        .event("error")
        .data(error.json_body().to_string())
}

/// SSE: replays strictly after `from` (or `Last-Event-ID`) through `at` and closes, or
/// through the head when `at` is absent, then
/// follows appends. Each message's `id:` is `<epoch>:<offset>`, the offset after its event.
/// `last=N` (s2w#294) is its own anchor: the last N retained events through the head, then close.
/// Every stream's headers carry `S2W-Epoch` and `S2W-Head`, the history and head it resolved.
///
/// The offset comes from `Last-Event-ID` when present, else `from`; the epoch from the
/// header's prefix when present, else `?epoch`. A supplied epoch that is not the served one
/// answers 410 `stale_epoch` before any bounds check. The stream remembers the epoch it
/// started under (supplied or not) and ends with one `stale_epoch` error event if the served
/// history changes under it.
async fn events(
    State(state): State<QueryState>,
    Path(world): Path<String>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> Response {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let start = || events_start(&state, &world, &p, last_event_id.as_deref());
    // Acquire the stream-cap permit before validating (round-1 review finding): a request
    // arriving over the cap pays only the semaphore check. The permit is dropped (freeing the
    // slot) if `start()` then fails validation — no slot is held past this function returning
    // an error response.
    let permit = match sse_cap_guard(state.sse_slots.clone()) {
        Ok(permit) => permit,
        Err(error) => return error.into_response(),
    };
    let cursor = match start() {
        Ok(ok) => ok,
        Err(e) => return e.into_response(),
    };
    let resolved = [
        ("s2w-epoch", cursor.epoch.to_string()),
        ("s2w-head", cursor.head.to_string()),
    ];
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(follow(state, cursor, tx));
    let headers = [(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    )];
    (
        headers,
        resolved,
        Sse::new(CappedStream {
            inner: ReceiverStream::new(rx),
            _permit: permit,
        })
        .keep_alive(KeepAlive::default()),
    )
        .into_response()
}

// Takes one of `slots`' stream permits, or [`QueryError::StreamLimit`] when all are held.
fn sse_cap_guard(
    slots: Arc<tokio::sync::Semaphore>,
) -> Result<tokio::sync::OwnedSemaphorePermit, QueryError> {
    slots
        .try_acquire_owned()
        .map_err(|_| QueryError::StreamLimit)
}

/// The largest `last=`: a tail is a page's evidence seed, not a replay (s2w#294).
const MAX_LAST: u64 = 1000;

/// Validates an `/events` request under one read: the epoch first, then `at >= from`, then
/// `from` against the retained window (decision 0026), then `at` against the head.
///
/// `last=N` resolves in the same read: `at` is the head and `from` is `head - N`, clamped up to
/// the replay base, so a restart from a snapshot returns fewer events instead of 410.
fn events_start(
    state: &QueryState,
    world: &str,
    p: &Params,
    last_event_id: Option<&str>,
) -> Result<Cursor, QueryError> {
    check_world(state, world)?;
    check_branch(p.branch.as_deref())?;
    let query_epoch = parse("epoch", p.epoch.as_deref())?;
    if let Some(last) = parse::<u64>("last", p.last.as_deref())? {
        return events_tail(state, p, last_event_id, query_epoch, last);
    }
    let (epoch, from) = match last_event_id {
        Some(id) => {
            let (epoch, from) = parse_last_event_id(id)?;
            (epoch.or(query_epoch), from)
        }
        None => (query_epoch, parse("from", p.from.as_deref())?.unwrap_or(0)),
    };
    let at = parse("at", p.at.as_deref())?;
    state.read(|t| {
        t.check_epoch(epoch)?;
        if at.is_some_and(|at| at < from) {
            return Err(QueryError::BadParameter {
                name: "at",
                reason: "must be at least from".to_owned(),
            });
        }
        // Bounds check only: `from` must still be inside the replay window (410 otherwise).
        t.events_after(from)?;
        if let Some(at) = at
            && at > t.head()
        {
            return Err(QueryError::OffsetBeyondHead { at, head: t.head() });
        }
        Ok(Cursor {
            epoch: t.epoch(),
            pos: from,
            at,
            head: t.head(),
        })
    })
}

/// `last=N`: one anchor per request, so `from`, `at` and `Last-Event-ID` are refused.
fn events_tail(
    state: &QueryState,
    p: &Params,
    last_event_id: Option<&str>,
    epoch: Option<Epoch>,
    last: u64,
) -> Result<Cursor, QueryError> {
    if !(1..=MAX_LAST).contains(&last) {
        return Err(QueryError::BadParameter {
            name: "last",
            reason: format!("'{last}': must be from 1 to {MAX_LAST}"),
        });
    }
    for (name, present) in [
        ("from", p.from.is_some()),
        ("at", p.at.is_some()),
        ("Last-Event-ID", last_event_id.is_some()),
    ] {
        if present {
            return Err(QueryError::BadParameter {
                name: "last",
                reason: format!("cannot be combined with {name}"),
            });
        }
    }
    state.read(|t| {
        t.check_epoch(epoch)?;
        let head = t.head();
        Ok(Cursor {
            epoch: t.epoch(),
            pos: head.saturating_sub(last).max(t.replay_base()),
            at: Some(head),
            head,
        })
    })
}

/// Where an SSE follower is: the history it started under, the offset it has sent through,
/// the bound it closes at, and the head when it started (the `S2W-Head` header).
struct Cursor {
    epoch: Epoch,
    pos: u64,
    at: Option<u64>,
    head: u64,
}

async fn follow(state: QueryState, cursor: Cursor, tx: mpsc::Sender<Result<Event, Infallible>>) {
    let Cursor {
        epoch, mut pos, at, ..
    } = cursor;
    let mut head = state.head.subscribe();
    loop {
        head.borrow_and_update();
        // Base-relative through `events_after`: after a snapshot restore, index 0 is the
        // base's offset, never offset 0 (decision 0024). The epoch check comes first: after a
        // timeline swap, `pos` names an offset of another history, and continuing would send
        // the new history's events as if they followed the old one's (even from `pos == 0`).
        // Only the deltas are cloned, and only while the read lock is held: `append` waits on it.
        let batch: Vec<Delta> = match state.read(|t| {
            t.check_epoch(Some(epoch))?;
            let after = t.events_after(pos)?;
            let take = at.map_or(after.len(), |at| {
                usize::try_from(at.saturating_sub(pos)).map_or(after.len(), |n| n.min(after.len()))
            });
            Ok(after
                .get(..take)
                .unwrap_or_default()
                .iter()
                .map(|timed| timed.delta.clone())
                .collect())
        }) {
            Ok(batch) => batch,
            Err(error) => {
                // Say why the stream ends: one `error` event with the stable code, never a
                // silent close (the base can move under a follower once snapshots rebase).
                let _ignored = tx.send(Ok(sse_error(&error))).await;
                return;
            }
        };
        // Each event carries the delta it made at append: the follower holds no world.
        for delta in batch {
            pos = pos.saturating_add(1);
            if tx.send(Ok(sse_event(epoch, pos, &delta))).await.is_err() {
                return;
            }
        }
        if at.is_some_and(|at| pos >= at) {
            return;
        }
        tokio::select! {
            changed = head.changed() => if changed.is_err() { return },
            () = tx.closed() => return,
        }
    }
}

// The response body owns the slot, even before its first poll and after headers are sent.
struct CappedStream {
    inner: ReceiverStream<Result<Event, Infallible>>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl tokio_stream::Stream for CappedStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_next(cx)
    }
}

/// One unrouted raw event kept for the sources view, so a viewer can see the stream is alive
/// before any engine claims it.
#[derive(Serialize)]
pub struct RawEventInfo {
    /// The event's log position.
    pub offset: u64,
    /// When the shell received the event, in milliseconds since the Unix epoch.
    pub received_at: i64,
    /// The event's payload, decoded as JSON when it parses and as a lossy string when not.
    pub payload: serde_json::Value,
}

/// `/worlds/{world}/sources`'s answer for one member source: its membership name plus what the
/// bridge has done with its events.
#[derive(Serialize)]
pub struct SourceInfo {
    /// The member's source id.
    pub source: String,
    /// Events of this source the bridge has consumed.
    pub consumed: u64,
    /// Consumed events no engine is routed for.
    pub unrouted: u64,
    /// The most recent unrouted events, most recent first, capped by the bridge.
    pub recent_unrouted: Vec<RawEventInfo>,
    /// Present while a live rebuild refolds this source's world under a newly effective mapping
    /// (s2w#184); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rebuilding: Option<Rebuilding>,
}

/// A live rebuild in progress for one source (s2w#184).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rebuilding {
    /// The identity of the mapping the source's world is being rebuilt under.
    pub identity: String,
    /// The log position the previous world had reached when the rebuild began.
    pub since_position: u64,
}

/// The payload bytes as JSON when they decode, or as a lossy string when they do not: the log
/// never interprets them, so both shapes are legitimate.
fn payload_json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(bytes).into_owned()))
}

async fn sources(
    State(state): State<QueryState>,
    Path(world): Path<String>,
    Query(p): Query<Params>,
) -> Response {
    let run = || -> Result<Vec<SourceInfo>, QueryError> {
        check_world(&state, &world)?;
        state.sources(
            parse("at", p.at.as_deref())?,
            parse("epoch", p.epoch.as_deref())?,
        )
    };
    run().map(Json).into_response()
}

impl QueryState {
    /// Each member source at fold offset `at` (the head when absent) with what the bridge has
    /// done with its events. The route and the MCP `sources` tool both call this.
    ///
    /// # Errors
    /// [`QueryError::StaleEpoch`] (checked first); [`QueryError::OffsetBeyondHead`] if `at` is
    /// past the head; [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn sources(
        &self,
        at: Option<u64>,
        epoch: Option<Epoch>,
    ) -> Result<Vec<SourceInfo>, QueryError> {
        self.read(|timeline| {
            timeline.check_epoch(epoch)?;
            let at = at.unwrap_or_else(|| timeline.head());
            if at > timeline.head() {
                return Err(QueryError::OffsetBeyondHead {
                    at,
                    head: timeline.head(),
                });
            }
            // The join key is the SourceId itself: membership rows and the bridge's counters
            // both name sources by it, so a member the bridge has not read yet reports zeros.
            let stats = self.source_stats.borrow();
            let rebuilding = self.rebuilding.borrow();
            Ok(members_at(&self.membership, at)
                .into_iter()
                .map(|source| {
                    let empty = SourceStats::default();
                    let stats = stats.get(&source).unwrap_or(&empty);
                    SourceInfo {
                        source: source.as_str().to_owned(),
                        consumed: stats.consumed,
                        unrouted: stats.unrouted,
                        recent_unrouted: stats
                            .recent_unrouted
                            .iter()
                            .rev()
                            .map(|stored| RawEventInfo {
                                offset: stored.position.as_u64(),
                                received_at: stored.event.received_at.as_millis(),
                                payload: payload_json(&stored.event.payload),
                            })
                            .collect(),
                        rebuilding: rebuilding.get(&source).cloned(),
                    }
                })
                .collect())
        })
    }
}

#[cfg(test)]
mod membership_tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use s2w_log::{EffectiveFrom, EventLog, SqliteEventLog, SqliteVerdictStore};
    use s2w_model::{Cursor, RawEvent, SourceId};
    use tower::ServiceExt;
    async fn get(app: &Router, path: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    fn sources_at_boundary_tests() {
        crate::tests::run(false, async {
            let dir = crate::tests::TestDirectory::new("sources-http");
            let mut log = SqliteEventLog::open(dir.path()).unwrap();
            let source = SourceId::new("member").unwrap();
            let clock = SourceId::new("clock").unwrap();
            let mut timeline = Timeline::new(3);
            log.bootstrap_source(&source).unwrap();
            for n in 1..=3 {
                log.append(RawEvent {
                    source: clock.clone(),
                    cursor: Cursor::new(vec![n]).unwrap(),
                    received_at: Timestamp::from_millis(i64::from(n)),
                    payload: vec![n],
                })
                .unwrap();
                timeline.append(
                    Timestamp::from_millis(i64::from(n)),
                    WorldEvent::EntityObserved {
                        key: s2w_core::NaturalKey::new(format!("e{n}")),
                        entity_type: "thing".into(),
                        attrs: Default::default(),
                    },
                );
                if n == 2 {
                    log.record_source_removed(&source).unwrap();
                }
            }
            log.record_source_added(
                &source,
                EffectiveFrom::FromCursor(Cursor::new(vec![3]).unwrap()),
            )
            .unwrap();
            // Equal-offset transitions: the last sequence wins, even across a second source.
            let tied = SourceId::new("tied").unwrap();
            log.bootstrap_source(&tied).unwrap();
            log.record_source_removed(&tied).unwrap();
            let manifest = WorldManifest::create_if_absent(
                &mut log,
                "default",
                "Display name",
                Timestamp::from_millis(0),
                &[],
                &[],
            )
            .unwrap();
            assert_eq!(log.membership_at(1).unwrap(), vec![source.clone()]);
            assert!(log.membership_at(2).unwrap().is_empty());
            let app = router(
                QueryState::new(timeline)
                    .with_metadata(Some(manifest), log.membership_history().unwrap()),
            );
            // No bridge runs here, so the member reports zeros alongside its name.
            let member = serde_json::json!([{
                "source": "member",
                "consumed": 0,
                "unrouted": 0,
                "recent_unrouted": []
            }]);
            for (path, expected) in [
                ("/worlds/default/sources?at=0", member.clone()),
                ("/worlds/default/sources?at=1", member.clone()),
                ("/worlds/default/sources?at=2", serde_json::json!([])),
                ("/worlds/default/sources?at=3", member.clone()),
                ("/worlds/default/sources", member.clone()),
            ] {
                assert_eq!(get(&app, path).await, (StatusCode::OK, expected));
            }
            for (path, status, code) in [
                (
                    "/worlds/unknown/sources",
                    StatusCode::NOT_FOUND,
                    "unknown_world",
                ),
                (
                    "/worlds/default/sources?at=4",
                    StatusCode::NOT_FOUND,
                    "offset_beyond_head",
                ),
                (
                    "/worlds/default/sources?at=no",
                    StatusCode::BAD_REQUEST,
                    "bad_parameter",
                ),
            ] {
                let (actual, body) = get(&app, path).await;
                assert_eq!(actual, status);
                assert_eq!(body["error"], code);
            }
            assert_eq!(
                get(&app, "/worlds").await.1["worlds"][0]["name"],
                "Display name"
            );
        });
    }

    /// A bridge that has consumed an unrouted member's events is visible through the route:
    /// per-source counters and the most recent raw events, most recent first, with payloads
    /// decoded as JSON when they parse and kept as text when they do not.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    fn sources_serve_per_source_bridge_consumption() {
        crate::tests::run(false, async {
            let dir = crate::tests::TestDirectory::new("sources-bridge");
            let mut log = SqliteEventLog::open(dir.path()).unwrap();
            let unrouted = SourceId::new("feed.unrouted").unwrap();
            log.bootstrap_source(&unrouted).unwrap();
            for (n, payload) in [r#"{"n":1}"#, "not json", r#"{"n":3}"#]
                .into_iter()
                .enumerate()
            {
                log.append(RawEvent {
                    source: unrouted.clone(),
                    cursor: Cursor::new(vec![u8::try_from(n).unwrap() + 1]).unwrap(),
                    received_at: Timestamp::from_millis(i64::try_from(n).unwrap() + 1),
                    payload: payload.as_bytes().to_vec(),
                })
                .unwrap();
            }
            let manifest = WorldManifest::create_if_absent(
                &mut log,
                "default",
                "Unrouted",
                Timestamp::from_millis(0),
                &[],
                &[],
            )
            .unwrap();
            let state = QueryState::new(Timeline::new(3))
                .with_metadata(Some(manifest), log.membership_history().unwrap());
            let mut bridge = crate::bridge::Bridge::new(
                log,
                SqliteVerdictStore::open(dir.path()).unwrap(),
                crate::bridge::EngineRegistry::with_defaults(),
                state.clone(),
                crate::bridge::BridgeConfig::default(),
            )
            .unwrap();
            bridge.poll_once().unwrap();

            let app = router(state.clone());
            let (status, http_body) = get(&app, "/worlds/default/sources").await;
            let mcp = crate::mcp::WorldMcp::new(state.clone());
            let tool = mcp.sources(rmcp::handler::server::wrapper::Parameters(
                serde_json::from_value(serde_json::json!({"world": "default"})).unwrap(),
            ));
            assert_ne!(tool.is_error, Some(true));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&tool.content[0].as_text().unwrap().text)
                    .unwrap(),
                http_body,
                "the MCP tool and the HTTP route serve the same sources"
            );
            assert_eq!(
                (status, http_body),
                (
                    StatusCode::OK,
                    serde_json::json!([{
                        "source": "feed.unrouted",
                        "consumed": 3,
                        "unrouted": 3,
                        "recent_unrouted": [
                            { "offset": 3, "received_at": 3, "payload": { "n": 3 } },
                            { "offset": 2, "received_at": 2, "payload": "not json" },
                            { "offset": 1, "received_at": 1, "payload": { "n": 1 } },
                        ]
                    }])
                )
            );

            // A live rebuild in progress (s2w#184) is reported on the source it is for, by
            // both surfaces; with none in progress the field is absent (above).
            state_rebuilding(&state, &unrouted);
            let (_, http_body) = get(&app, "/worlds/default/sources").await;
            let tool = mcp.sources(rmcp::handler::server::wrapper::Parameters(
                serde_json::from_value(serde_json::json!({"world": "default"})).unwrap(),
            ));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&tool.content[0].as_text().unwrap().text)
                    .unwrap(),
                http_body
            );
            assert_eq!(
                http_body[0]["rebuilding"],
                serde_json::json!({"identity": "m-1", "since_position": 42})
            );
        });
    }

    fn state_rebuilding(state: &QueryState, source: &SourceId) {
        state.publish_rebuilding(BTreeMap::from([(
            source.clone(),
            Rebuilding {
                identity: "m-1".to_owned(),
                since_position: 42,
            },
        )]));
    }

    /// s2w#259: `serve`'s bridge appends on the current-thread runtime while a `/world` body,
    /// larger than the body channel holds, is still streaming from a blocking thread. The
    /// append must wait for the body without blocking the runtime that drains it; before the
    /// write reservation, the body ended cut short at ~4 MiB (the demo viewer's "Failed to fetch").
    #[test]
    fn a_world_body_is_whole_while_the_runtime_appends() {
        fn observed(n: u32, filler: &str) -> WorldEvent {
            WorldEvent::EntityObserved {
                key: s2w_core::NaturalKey::new(format!("e{n}")),
                entity_type: "thing".into(),
                attrs: BTreeMap::from([(
                    "filler".to_owned(),
                    s2w_model::AttrValue::Str(filler.to_owned()),
                )]),
            }
        }
        const ENTITIES: u32 = 8_000;
        crate::tests::run(false, async {
            let filler = "x".repeat(1024);
            let mut timeline = Timeline::new(3);
            for n in 0..ENTITIES {
                timeline.append(Timestamp::from_millis(i64::from(n)), observed(n, &filler));
            }
            let state = QueryState::new(timeline);
            let response = router(state.clone())
                .oneshot(
                    Request::get("/worlds/default/world")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            // What `local_bridge` does each poll, while the body is in flight.
            let append = async {
                let reservation = state.reserve_write().await;
                state
                    .append(
                        Timestamp::from_millis(i64::from(ENTITIES)),
                        observed(ENTITIES, ""),
                    )
                    .expect("append");
                drop(reservation);
            };
            let (body, ()) = tokio::join!(
                axum::body::to_bytes(response.into_body(), usize::MAX),
                append
            );
            let body = body.expect("the body ends complete, not cut short");
            assert!(
                body.len() > stream::CHUNK_BYTES * stream::CHUNKS_IN_FLIGHT,
                "the body must outgrow the channel to exercise the stall: {} bytes",
                body.len()
            );
            let view: serde_json::Value = serde_json::from_slice(&body).expect("whole JSON");
            assert_eq!(view["offset"], u64::from(ENTITIES));
            assert_eq!(state.bounds().expect("bounds").1, u64::from(ENTITIES) + 1);
        });
    }

    /// s2w#259: a body waiting out one poll's reservation gets in before the next poll's, so a
    /// backfill's back-to-back polls cannot answer every `/world` with a 503.
    #[test]
    fn a_waiting_body_goes_before_the_next_reservation() {
        crate::tests::run(false, async {
            let state = QueryState::new(Timeline::new(3));
            let first = state.reserve_write().await;
            let (tx, rx) = std::sync::mpsc::channel();
            let body = {
                let state = state.clone();
                std::thread::spawn(move || {
                    let guard = state.read_for_body(|| true).expect("read");
                    tx.send(()).expect("send");
                    std::thread::sleep(Duration::from_millis(50));
                    drop(guard);
                })
            };
            while state.bodies_waiting.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            drop(first);
            let second = state.reserve_write().await;
            assert!(rx.try_recv().is_ok(), "the waiting body went first");
            drop(second);
            body.join().expect("body");
            assert_eq!(state.write_reservations.load(Ordering::SeqCst), 0);
            assert_eq!(state.bodies_waiting.load(Ordering::SeqCst), 0);
        });
    }
}
