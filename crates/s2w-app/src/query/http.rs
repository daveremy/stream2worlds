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
use super::diff::diff;
use super::timeline::{TimedEvent, Timeline};
use super::view::{ACTUAL_BRANCH, Lod, ViewParams, world_view};

/// Shared server state: the timeline and a head-offset signal that wakes SSE subscribers.
#[derive(Clone)]
pub struct QueryState {
    timeline: Arc<RwLock<Timeline>>,
    head: Arc<watch::Sender<u64>>,
}

impl QueryState {
    /// Serves `timeline`.
    #[must_use]
    pub fn new(timeline: Timeline) -> Self {
        let (head, _) = watch::channel(timeline.head());
        Self {
            timeline: Arc::new(RwLock::new(timeline)),
            head: Arc::new(head),
        }
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
}

impl IntoResponse for QueryError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::OffsetBeyondHead { .. } | Self::UnknownEntity { .. } => StatusCode::NOT_FOUND,
            Self::BranchNotYet { .. } | Self::LodNotYet { .. } => StatusCode::NOT_IMPLEMENTED,
            Self::BadParameter { .. } | Self::HopsTooLarge { .. } => StatusCode::BAD_REQUEST,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        let body = serde_json::json!({ "error": self.code(), "message": self.to_string() });
        (status, Json(body)).into_response()
    }
}

/// The query API's routes over `state`.
pub fn router(state: QueryState) -> Router {
    Router::new()
        .route("/world", get(world))
        .route("/events", get(events))
        .route("/branches", get(branches))
        .route("/diff", get(world_diff))
        .route("/entity/{id}/history", get(history))
        .route("/time", get(time))
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

fn check_branch(branch: Option<&str>) -> Result<(), QueryError> {
    match branch {
        None => Ok(()),
        Some(b) if b == ACTUAL_BRANCH => Ok(()),
        Some(b) => Err(QueryError::BranchNotYet {
            branch: b.to_owned(),
        }),
    }
}

fn parse_lod(raw: Option<&str>) -> Result<Lod, QueryError> {
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

fn world_at(state: &QueryState, at: Option<u64>) -> Result<World, QueryError> {
    state.read(|t| t.world_at(at.unwrap_or_else(|| t.head())))
}

async fn world(State(state): State<QueryState>, Query(p): Query<Params>) -> Response {
    let run = || -> Result<_, QueryError> {
        check_branch(p.branch.as_deref())?;
        let params = ViewParams {
            lod: parse_lod(p.lod.as_deref())?,
            focus: parse("focus", p.focus.as_deref())?,
            hops: parse("hops", p.hops.as_deref())?.unwrap_or(1),
        };
        let at = parse("at", p.at.as_deref())?;
        world_view(&world_at(&state, at)?, &params)
    };
    run().map(Json).into_response()
}

#[derive(Serialize)]
struct Branch {
    name: &'static str,
    world_id: u64,
    head: u64,
    fold_version: u32,
    hub_in_degree_cap: u64,
}

async fn branches(State(state): State<QueryState>) -> Response {
    state
        .read(|t| {
            Ok(vec![Branch {
                name: ACTUAL_BRANCH,
                world_id: 0,
                head: t.head(),
                fold_version: FOLD_VERSION,
                hub_in_degree_cap: t.hub_cap(),
            }])
        })
        .map(Json)
        .into_response()
}

async fn world_diff(State(state): State<QueryState>, Query(p): Query<Params>) -> Response {
    let run = || -> Result<_, QueryError> {
        check_branch(p.branch.as_deref())?;
        let from = parse("from", p.from.as_deref())?.unwrap_or(0);
        let to = parse("to", p.to.as_deref())?;
        diff(&world_at(&state, Some(from))?, &world_at(&state, to)?)
    };
    run().map(Json).into_response()
}

async fn history(
    State(state): State<QueryState>,
    Path(id): Path<String>,
    Query(p): Query<Params>,
) -> Response {
    let run = || -> Result<_, QueryError> {
        check_branch(p.branch.as_deref())?;
        let id = id.parse::<u64>().map_err(|e| QueryError::BadParameter {
            name: "id",
            reason: format!("'{id}': {e}"),
        })?;
        let to = parse("to", p.to.as_deref())?;
        state.read(|t| t.history(id, to.unwrap_or_else(|| t.head())))
    };
    run().map(Json).into_response()
}

#[derive(Serialize)]
struct TimeAt {
    ts: i64,
    offset: u64,
}

async fn time(State(state): State<QueryState>, Query(p): Query<Params>) -> Response {
    if let Err(e) = check_branch(p.branch.as_deref()) {
        return e.into_response();
    }
    match parse::<i64>("ts", p.ts.as_deref()) {
        Err(e) => e.into_response(),
        Ok(Some(ts)) => state
            .read(|t| {
                Ok(TimeAt {
                    ts,
                    offset: t.offset_at(Timestamp::from_millis(ts)),
                })
            })
            .map(Json)
            .into_response(),
        Ok(None) => state.read(|t| Ok(t.time_range())).map(Json).into_response(),
    }
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
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> Response {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let start = || -> Result<(u64, World), QueryError> {
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
