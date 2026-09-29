//! The query API over HTTP (axum), with SSE deltas. Handlers parse parameters and call the
//! pure functions; every error is `{ "error": <code>, "message": <text> }`.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use s2w_core::{FOLD_VERSION, World, WorldEvent};
use s2w_log::{
    MembershipRow, ReadOnlySqliteEventLog, ReadOnlySqliteProposalStore, WorldManifest,
    WorldPresentation, members_at,
};
use s2w_model::{SourceId, Timestamp};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;

use super::QueryError;
use super::delta::{Delta, fold_with_delta};
use super::diff::{WorldDiff, diff};
use super::proposals::{ProposalsView, proposals_view};
use super::timeline::{HistoryEntry, TimeRange, TimedEvent, Timeline};
use super::view::{ACTUAL_BRANCH, Lod, ViewParams, world_view};
use crate::bridge::SourceStats;

/// Shared server state: the timeline and a head-offset signal that wakes SSE subscribers.
#[derive(Clone)]
pub struct QueryState {
    timeline: Arc<RwLock<Timeline>>,
    head: Arc<watch::Sender<u64>>,
    world: Arc<str>,
    sse_slots: Arc<tokio::sync::Semaphore>,
    manifest: Option<Arc<WorldManifest>>,
    membership: Arc<Vec<MembershipRow>>,
    source_stats: Arc<watch::Sender<BTreeMap<SourceId, SourceStats>>>,
    /// The event-log directory, so presentation can be read fresh per request rather than
    /// cached at startup — a live `s2w presentation set` is visible without a restart.
    log_dir: Option<Arc<PathBuf>>,
}

impl QueryState {
    /// Serves `timeline`.
    #[must_use]
    pub fn new(timeline: Timeline) -> Self {
        let (head, _) = watch::channel(timeline.head());
        let (source_stats, _) = watch::channel(BTreeMap::new());
        Self {
            timeline: Arc::new(RwLock::new(timeline)),
            head: Arc::new(head),
            world: Arc::from("default"),
            sse_slots: Arc::new(tokio::sync::Semaphore::new(32)),
            manifest: None,
            membership: Arc::new(Vec::new()),
            source_stats: Arc::new(source_stats),
            log_dir: None,
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
        let Some(reader) = open_proposal_reader(log_dir)? else {
            return Ok(ProposalsView::default());
        };
        Ok(proposals_view(
            &reader.proposal_summaries()?,
            &reader.decisions()?,
        ))
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

    /// Replaces the per-source bridge statistics `/worlds/{world}/sources` serves. The live
    /// bridge publishes after each poll; a process with no bridge (the read-only MCP replay
    /// path) never does, so every source there reports zeros.
    pub(crate) fn publish_source_stats(&self, stats: BTreeMap<SourceId, SourceStats>) {
        self.source_stats.send_replace(stats);
    }

    fn read<T>(&self, f: impl FnOnce(&Timeline) -> Result<T, QueryError>) -> Result<T, QueryError> {
        f(&*self.timeline.read().map_err(|_| QueryError::Unavailable)?)
    }

    /// The world at `at`, or at the head when `at` is absent.
    ///
    /// # Errors
    /// Whatever [`Timeline::world_at`] returns, or [`QueryError::Unavailable`] if the lock was
    /// poisoned.
    pub fn world_at(&self, at: Option<u64>) -> Result<World, QueryError> {
        self.read(|t| t.world_at(at.unwrap_or_else(|| t.head())))
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
    /// Whatever [`Timeline::history`] returns, or [`QueryError::Unavailable`] if the lock was
    /// poisoned.
    pub fn history(&self, id: u64, to: Option<u64>) -> Result<Vec<HistoryEntry>, QueryError> {
        self.read(|t| t.history(id, to.unwrap_or_else(|| t.head())))
    }

    /// What changed between `from` and `to`, or the head when `to` is absent.
    ///
    /// # Errors
    /// [`QueryError::OffsetBeyondHead`] past the head, or [`QueryError::Unavailable`] if the
    /// lock was poisoned.
    pub fn diff(&self, from: u64, to: Option<u64>) -> Result<WorldDiff, QueryError> {
        self.world_at(Some(from))
            .and_then(|a| self.world_at(to).map(|b| (a, b)))
            .and_then(|(a, b)| diff(&a, &b))
    }

    /// The time index at `ts`, or its whole range when `ts` is absent.
    ///
    /// # Errors
    /// [`QueryError::TimeBeforeBase`] for a `ts` inside a restored snapshot;
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn time(&self, ts: Option<i64>) -> Result<TimeResult, QueryError> {
        self.read(|t| {
            Ok(match ts {
                Some(ts) => TimeResult::At(TimeAt {
                    ts,
                    offset: t.offset_at(Timestamp::from_millis(ts))?,
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
            Self::OffsetBeforeBase { .. } | Self::TimeBeforeBase { .. } => StatusCode::GONE,
            Self::BranchNotYet { .. } | Self::LodNotYet { .. } => StatusCode::NOT_IMPLEMENTED,
            Self::BadParameter { .. } | Self::HopsTooLarge { .. } => StatusCode::BAD_REQUEST,
            Self::Unavailable | Self::StreamLimit => StatusCode::SERVICE_UNAVAILABLE,
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
}

fn parse<T: std::str::FromStr>(
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

/// Opens `log_dir`'s proposal store read-only, or `None` when the store file does not exist.
/// The existence check comes first so a read never creates the store.
///
/// # Errors
/// [`QueryError::Storage`] if the store exists but cannot be opened.
pub(crate) fn open_proposal_reader(
    log_dir: &std::path::Path,
) -> Result<Option<ReadOnlySqliteProposalStore>, QueryError> {
    let exists = log_dir
        .join(s2w_log::PROPOSAL_DATABASE_FILE)
        .try_exists()
        .map_err(|error| QueryError::Storage(error.to_string()))?;
    if !exists {
        return Ok(None);
    }
    Ok(Some(ReadOnlySqliteProposalStore::open(log_dir)?))
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

async fn world(
    State(state): State<QueryState>,
    Path(world): Path<String>,
    Query(p): Query<Params>,
) -> Response {
    let run = || -> Result<_, QueryError> {
        check_world(&state, &world)?;
        check_branch(p.branch.as_deref())?;
        let params = ViewParams {
            lod: parse_lod(p.lod.as_deref())?,
            focus: parse("focus", p.focus.as_deref())?,
            hops: parse("hops", p.hops.as_deref())?.unwrap_or(1),
        };
        let at = parse("at", p.at.as_deref())?;
        world_view(&state.world_at(at)?, &params)
    };
    run().map(Json).into_response()
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
        state.diff(from, to)
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
        state.history(id, to)
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
        state.time(parse("ts", p.ts.as_deref())?)
    };
    run().map(Json).into_response()
}

#[derive(Serialize)]
struct Message<'a> {
    offset: u64,
    #[serde(flatten)]
    delta: &'a Delta,
}

fn sse_event(offset: u64, delta: &Delta) -> Event {
    let event = Event::default().id(offset.to_string()).event(delta.kind());
    match event.json_data(Message { offset, delta }) {
        Ok(event) => event,
        Err(e) => Event::default()
            .id(offset.to_string())
            .event("error")
            .data(e.to_string()),
    }
}

/// SSE: replays strictly after `from` (or `Last-Event-ID`) through `at` and closes, or
/// through the head when `at` is absent, then
/// follows appends. Each message's `id:` is the offset after its event.
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
    let start = || -> Result<(u64, World, Option<u64>), QueryError> {
        check_world(&state, &world)?;
        check_branch(p.branch.as_deref())?;
        let from = match last_event_id.as_deref() {
            Some(id) => parse("Last-Event-ID", Some(id))?,
            None => parse("from", p.from.as_deref())?,
        }
        .unwrap_or(0);
        let at = parse("at", p.at.as_deref())?;
        let world = state.read(|t| {
            if let Some(at) = at {
                if at < from {
                    return Err(QueryError::BadParameter {
                        name: "at",
                        reason: "must be at least from".to_owned(),
                    });
                }
                // Validate the bound without folding a second snapshot.
                if at > t.head() {
                    return Err(QueryError::OffsetBeyondHead { at, head: t.head() });
                }
            }
            t.world_at(from)
        })?;
        Ok((from, world, at))
    };
    // Acquire the stream-cap permit BEFORE folding any history (round-1 review finding): a
    // request arriving over the cap should pay only the semaphore check, not the full fold
    // `start()` does under the read lock. The permit is dropped (freeing the slot) if `start()`
    // then fails validation — no slot is held past this function returning an error response.
    let permit = match crate::serve::sse_cap_guard(state.sse_slots.clone()) {
        Ok(permit) => permit,
        Err(error) => return error.into_response(),
    };
    let (from, world, at) = match start() {
        Ok(ok) => ok,
        Err(e) => return e.into_response(),
    };
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(follow(state, from, world, at, tx));
    let headers = [(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    )];
    (
        headers,
        Sse::new(CappedStream {
            inner: ReceiverStream::new(rx),
            _permit: permit,
        })
        .keep_alive(KeepAlive::default()),
    )
        .into_response()
}

async fn follow(
    state: QueryState,
    from: u64,
    mut world: World,
    at: Option<u64>,
    tx: mpsc::Sender<Result<Event, Infallible>>,
) {
    let mut head = state.head.subscribe();
    let mut pos = from;
    loop {
        head.borrow_and_update();
        // Base-relative through `events_after`: after a snapshot restore, index 0 is the
        // base's offset, never offset 0 (decision 0021).
        let batch: Vec<TimedEvent> = match state.read(|t| {
            let after = t.events_after(pos)?;
            let take = at.map_or(after.len(), |at| {
                usize::try_from(at.saturating_sub(pos)).map_or(after.len(), |n| n.min(after.len()))
            });
            Ok(after.get(..take).unwrap_or_default().to_vec())
        }) {
            Ok(batch) => batch,
            Err(_) => return,
        };
        for timed in batch {
            let (next, delta) = fold_with_delta(world, &timed.event);
            world = next;
            pos = pos.saturating_add(1);
            if tx.send(Ok(sse_event(pos, &delta))).await.is_err() {
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
        state.sources(parse("at", p.at.as_deref())?)
    };
    run().map(Json).into_response()
}

impl QueryState {
    /// Each member source at fold offset `at` (the head when absent) with what the bridge has
    /// done with its events. The route and the MCP `sources` tool both call this.
    ///
    /// # Errors
    /// [`QueryError::OffsetBeyondHead`] if `at` is past the head;
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn sources(&self, at: Option<u64>) -> Result<Vec<SourceInfo>, QueryError> {
        self.read(|timeline| {
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
            let mcp = crate::mcp::WorldMcp::new(state);
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
        });
    }
}
