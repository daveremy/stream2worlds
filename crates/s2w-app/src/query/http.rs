//! The query API over HTTP (axum), with SSE deltas. Handlers parse parameters and call the
//! pure functions; every error is `{ "error": <code>, "message": <text> }`.

use std::convert::Infallible;
use std::sync::{Arc, RwLock};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use s2w_core::{FOLD_VERSION, World, WorldEvent};
use s2w_model::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;

use super::QueryError;
use super::delta::{Delta, fold_with_delta};
use super::diff::{WorldDiff, diff};
use super::timeline::{HistoryEntry, TimeRange, TimedEvent, Timeline};
use super::view::{ACTUAL_BRANCH, Lod, ViewParams, world_view};

/// Shared server state: the timeline and a head-offset signal that wakes SSE subscribers.
#[derive(Clone)]
pub struct QueryState {
    timeline: Arc<RwLock<Timeline>>,
    head: Arc<watch::Sender<u64>>,
    world: Arc<str>,
}

impl QueryState {
    /// Serves `timeline`.
    #[must_use]
    pub fn new(timeline: Timeline) -> Self {
        let (head, _) = watch::channel(timeline.head());
        Self {
            timeline: Arc::new(RwLock::new(timeline)),
            head: Arc::new(head),
            world: Arc::from("default"),
        }
    }

    /// Configures the string identifier this process serves.
    #[must_use]
    pub fn with_world(mut self, world: impl Into<Arc<str>>) -> Self {
        self.world = world.into();
        self
    }

    /// The string identifier of the world this process serves.
    #[must_use]
    pub fn world(&self) -> &str {
        &self.world
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
    /// [`QueryError::Unavailable`] if the lock was poisoned.
    pub fn time(&self, ts: Option<i64>) -> Result<TimeResult, QueryError> {
        self.read(|t| {
            Ok(match ts {
                Some(ts) => TimeResult::At(TimeAt {
                    ts,
                    offset: t.offset_at(Timestamp::from_millis(ts)),
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
            | Self::UnknownWorld { .. } => StatusCode::NOT_FOUND,
            Self::BranchNotYet { .. } | Self::LodNotYet { .. } => StatusCode::NOT_IMPLEMENTED,
            Self::BadParameter { .. } | Self::HopsTooLarge { .. } => StatusCode::BAD_REQUEST,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
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
    /// The world's display name. Until manifests exist, this deliberately reuses `world`.
    pub name: String,
    /// The world's latest fold offset.
    pub head: u64,
}

#[derive(Serialize)]
struct Worlds {
    worlds: Vec<WorldSummary>,
}

async fn list_worlds(State(state): State<QueryState>) -> Response {
    let run = || -> Result<_, QueryError> {
        state.read(|timeline| {
            let world = state.world().to_owned();
            Ok(Worlds {
                worlds: vec![WorldSummary {
                    name: world.clone(),
                    world,
                    head: timeline.head(),
                }],
            })
        })
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

/// SSE: replays one delta per offset from `from` (or `Last-Event-ID`) to the head, then
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
    let start = || -> Result<(u64, World), QueryError> {
        check_world(&state, &world)?;
        check_branch(p.branch.as_deref())?;
        let from = match last_event_id.as_deref() {
            Some(id) => parse("Last-Event-ID", Some(id))?,
            None => parse("from", p.from.as_deref())?,
        }
        .unwrap_or(0);
        Ok((from, state.read(|t| t.world_at(from))?))
    };
    let (from, world) = match start() {
        Ok(ok) => ok,
        Err(e) => return e.into_response(),
    };
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(follow(state, from, world, tx));
    let headers = [(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    )];
    (
        headers,
        Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()),
    )
        .into_response()
}

async fn follow(
    state: QueryState,
    from: u64,
    mut world: World,
    tx: mpsc::Sender<Result<Event, Infallible>>,
) {
    let mut head = state.head.subscribe();
    let mut pos = from;
    loop {
        head.borrow_and_update();
        let batch: Vec<TimedEvent> = match state.read(|t| {
            Ok(usize::try_from(pos)
                .ok()
                .and_then(|p| t.events().get(p..))
                .map(<[TimedEvent]>::to_vec)
                .unwrap_or_default())
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
        tokio::select! {
            changed = head.changed() => if changed.is_err() { return },
            () = tx.closed() => return,
        }
    }
}
