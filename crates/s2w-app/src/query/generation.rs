//! Single-flight `/world` generations (s2w#270, PR 2 of s2w#235; full `lod=type` since s2w#297).
//!
//! Every full-view projection holds ~330 MiB while it is alive, and four viewers used to build
//! four at once (s2w#243: a 1.86 GiB peak). Here at most one full projection is in
//! flight per [`QueryState`]: a *generation* takes the read guard once, resolves the epoch, the
//! offset and the `ETag`, answers every subscriber whose `If-None-Match` names that tag with
//! `304` before building anything, builds the projection only if a subscriber is left, and
//! serializes it once into a fan-out writer ([`stream::fan_out`]) that hands each chunk to every
//! subscriber.
//!
//! Requests are grouped by exactly what the `ETag` and the body depend on: the requested epoch,
//! `at`, `lod`, `focus`, `hops` and `links` ([`Key`]); equal keys produce equal bytes. The key is
//! the literal request, so `?at=<head>` and no `at` build separately: that loses sharing, never
//! correctness. A request arriving while a generation runs joins the queued group with its
//! key, or starts one; it never joins the generation already under way, so it never gets half
//! a body. When a generation ends the driver serves the group whose oldest waiter has waited
//! longest, so no key starves.
//!
//! Bounds: at most [`QUEUE_LIMIT`] queued requests (more answer `503`,
//! [`QueryError::WorldQueueFull`]), and the handler gives a request that is not answered within
//! its wait limit (30 s) a `503`. A request whose client left is dropped before its generation
//! starts, and a generation whose clients all left stops waiting for the guard. The driver runs
//! on one blocking thread at a time, and the queue's mutex is never held across a send or the
//! timeline guard.
//!
//! Only the type summary (`lod=type&links=none`, s2w#296) bypasses the queue, so first paint
//! never waits behind a full projection: [`serve_alone`] runs the same generation for one
//! request on its own blocking thread. Full `lod=type` views queue like `lod=entity` (s2w#297).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use tokio::sync::oneshot;

use super::QueryError;
use super::epoch::Epoch;
use super::http::{QueryState, WorldAnswer, etag_matches, resolve_offset, write_view};
use super::projection::Projection;
use super::read_timings::Phases;
use super::stream;
use super::view::{LinkDetail, ViewParams};

/// The most requests queued for a generation at once; one more answers `503`. The same bound
/// as the concurrent `/events` streams.
pub(super) const QUEUE_LIMIT: usize = 32;

/// What a generation's bytes depend on, beyond the served timeline: equal keys, equal bodies.
/// `hops` is part of the `ETag` even without a focus, so it is part of the key too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Key {
    /// The requested epoch, checked under the guard.
    pub(super) epoch: Option<Epoch>,
    /// The requested offset; absent means the head when the generation runs.
    pub(super) at: Option<u64>,
    /// The requested view: level of detail, focus, hops and link detail.
    pub(super) params: ViewParams,
}

/// One `/world` request waiting for a generation: its `If-None-Match` and where its answer goes.
pub(super) struct Waiter {
    if_none_match: Option<String>,
    answer: oneshot::Sender<Result<WorldAnswer, QueryError>>,
    queued: Instant,
}

impl Waiter {
    /// A request queued now.
    pub(super) fn new(
        if_none_match: Option<String>,
        answer: oneshot::Sender<Result<WorldAnswer, QueryError>>,
    ) -> Self {
        Self {
            if_none_match,
            answer,
            queued: Instant::now(),
        }
    }

    /// Whether the client is still waiting: its handler drops the receiver when it gives up.
    fn waiting(&self) -> bool {
        !self.answer.is_closed()
    }

    /// Whether the client already has this tag, so the answer is `304`.
    fn has(&self, tag: &axum::http::HeaderValue) -> bool {
        self.if_none_match
            .as_deref()
            .is_some_and(|inm| etag_matches(inm, tag))
    }

    /// Answers the request; `false` when the client already left, which is not an error.
    fn answer(self, answer: Result<WorldAnswer, QueryError>) -> bool {
        self.answer.send(answer).is_ok()
    }
}

/// Requests with one key, served by one generation.
struct Group {
    key: Key,
    /// In arrival order: the first is the oldest.
    waiters: Vec<Waiter>,
}

/// The queued groups and whether a driver is running.
#[derive(Default)]
struct Queue {
    running: bool,
    groups: Vec<Group>,
}

impl Queue {
    /// Drops the requests whose client left, and the groups left empty.
    fn prune(&mut self) {
        for group in &mut self.groups {
            group.waiters.retain(Waiter::waiting);
        }
        self.groups.retain(|group| !group.waiters.is_empty());
    }

    fn len(&self) -> usize {
        self.groups.iter().map(|group| group.waiters.len()).sum()
    }

    /// Removes the group whose oldest request has waited longest.
    fn take_oldest(&mut self) -> Option<Group> {
        let index = (0..self.groups.len())
            .min_by_key(|&i| self.groups[i].waiters.first().map(|waiter| waiter.queued))?;
        Some(self.groups.remove(index))
    }
}

/// The single-flight gate one [`QueryState`] (and every clone of it) shares.
#[derive(Default)]
pub(crate) struct Generations {
    queue: Mutex<Queue>,
}

impl Generations {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        // A poisoned queue is still consistent: every change to it is one assignment or push.
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Requests queued and not yet taken by a generation.
    #[cfg(test)]
    pub(super) fn queued(&self) -> usize {
        self.lock().len()
    }

    /// Whether a driver is running.
    #[cfg(test)]
    pub(super) fn running(&self) -> bool {
        self.lock().running
    }
}

/// Queues `waiter` for a generation of `key` and starts the driver on a blocking thread unless
/// one is running. Must be called inside a tokio runtime. (If the runtime shuts down before
/// the driver runs, `running` stays set; nothing is served after shutdown anyway.)
///
/// # Errors
/// [`QueryError::WorldQueueFull`] when [`QUEUE_LIMIT`] requests are already queued.
pub(super) fn submit(state: &QueryState, key: Key, waiter: Waiter) -> Result<(), QueryError> {
    let start = {
        let mut queue = state.generations().lock();
        queue.prune();
        if queue.len() >= QUEUE_LIMIT {
            return Err(QueryError::WorldQueueFull);
        }
        match queue.groups.iter_mut().find(|group| group.key == key) {
            Some(group) => group.waiters.push(waiter),
            None => queue.groups.push(Group {
                key,
                waiters: vec![waiter],
            }),
        }
        !std::mem::replace(&mut queue.running, true)
    };
    if start {
        let state = state.clone();
        tokio::task::spawn_blocking(move || drive(&state));
    }
    Ok(())
}

/// Serves one request outside the queue, on the calling (blocking) thread: the type summary.
pub(super) fn serve_alone(state: &QueryState, key: Key, waiter: Waiter) {
    generate(
        state,
        Group {
            key,
            waiters: vec![waiter],
        },
    );
}

/// Runs generations until the queue is empty, oldest waiter first.
fn drive(state: &QueryState) {
    loop {
        let group = {
            let mut queue = state.generations().lock();
            queue.prune();
            match queue.take_oldest() {
                Some(group) => group,
                None => {
                    queue.running = false;
                    return;
                }
            }
        };
        // A panic drops the group's answers unsent, and each of its handlers answers 503; the
        // next group is still served.
        let _ = catch_unwind(AssertUnwindSafe(|| generate(state, group)));
    }
}

/// Serves one group from one read-guard hold: errors and `304`s first, then one projection
/// written once to every subscriber still waiting.
fn generate(state: &QueryState, group: Group) {
    let Group { key, waiters } = group;
    let started = Instant::now();
    let Some(Captured {
        tag,
        projection,
        fresh,
        guarded,
        built,
    }) = capture(state, &key, waiters)
    else {
        return;
    };
    // The guard is released: the fold appends while this view is sorted and written.
    let view = projection.prepare();
    let prepared = Instant::now();
    let (writer, bodies) = stream::fan_out(fresh.len());
    let mut served = 0;
    for (waiter, body) in fresh.into_iter().zip(bodies) {
        // A failed answer drops its body stream, and the writer drops that subscriber. (A
        // handler that times out just after a successful answer still counts as served.)
        if waiter.answer(Ok(WorldAnswer::Body(tag.clone(), body))) {
            served += 1;
        }
    }
    if served == 0 {
        return;
    }
    write_view(&view, writer);
    // Recorded only when a measurement opted in (s2w#243).
    if let Some(timings) = state.timings() {
        timings.record(
            served,
            Phases {
                wait: guarded - started,
                build: built - guarded,
                prepare: prepared - built,
                write: prepared.elapsed(),
            },
            view.diverged(),
        );
    }
}

/// What one read-guard hold hands to the rest of a generation.
struct Captured {
    tag: axum::http::HeaderValue,
    projection: Projection,
    fresh: Vec<Waiter>,
    guarded: Instant,
    built: Instant,
}

/// The guarded part of a generation, and the only one: errors and `304`s are answered here,
/// and the view is captured. The guard is released when this returns, before any sort or write.
fn capture(state: &QueryState, key: &Key, waiters: Vec<Waiter>) -> Option<Captured> {
    let answer_all = |waiters: Vec<Waiter>, error: &QueryError| {
        for waiter in waiters {
            waiter.answer(Err(error.clone()));
        }
    };
    // Stops waiting for the guard once every client has left.
    let timeline = match state.read_for_body(|| waiters.iter().any(Waiter::waiting)) {
        Ok(timeline) => timeline,
        Err(error) => {
            answer_all(waiters, &error);
            return None;
        }
    };
    let guarded = Instant::now();
    let (tag, offset) = match resolve_offset(&timeline, key.epoch, key.at, &key.params) {
        Ok(resolved) => resolved,
        Err(error) => {
            answer_all(waiters, &error);
            return None;
        }
    };
    // 304 before anything is built; a client that left while the guard was awaited is dropped.
    let mut fresh = Vec::with_capacity(waiters.len());
    for waiter in waiters.into_iter().filter(Waiter::waiting) {
        if waiter.has(&tag) {
            waiter.answer(Ok(WorldAnswer::NotModified(tag.clone())));
        } else {
            fresh.push(waiter);
        }
    }
    if fresh.is_empty() {
        return None;
    }
    let projection = if key.params.links == LinkDetail::None {
        // The type summary (s2w#296) comes from the memo when the offset has not moved.
        state
            .summary_at(&timeline, offset)
            .map(Projection::from_view)
    } else {
        timeline
            .world_at(offset)
            .and_then(|world| Projection::capture(&world, &key.params, timeline.epoch()))
    };
    match projection {
        Ok(projection) => Some(Captured {
            tag,
            projection,
            fresh,
            guarded,
            built: Instant::now(),
        }),
        Err(error) => {
            answer_all(fresh, &error);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use axum::Router;
    use axum::body::{Body, Bytes};
    use axum::http::{Request, StatusCode, header};
    use axum::response::Response;
    use s2w_core::{NaturalKey, WorldEvent};
    use s2w_model::{AttrValue, Timestamp};
    use tower::ServiceExt;

    use super::*;
    use crate::query::http::router;
    use crate::query::stream::{CHUNK_BYTES, CHUNKS_IN_FLIGHT};
    use crate::query::timeline::Timeline;

    /// A world of `entities` entities, each carrying `filler` bytes: 8,000 x 1 KiB outgrows
    /// the body channel, so an unread body stalls its generation.
    fn state(entities: u32, filler: usize) -> QueryState {
        let filler = "x".repeat(filler);
        let mut timeline = Timeline::new(3);
        for n in 0..entities {
            timeline.append(
                Timestamp::from_millis(i64::from(n)),
                WorldEvent::EntityObserved {
                    key: NaturalKey::new(format!("e{n}")),
                    entity_type: "thing".into(),
                    attrs: BTreeMap::from([("filler".to_owned(), AttrValue::Str(filler.clone()))]),
                },
            );
        }
        QueryState::new(timeline).with_read_timings()
    }

    fn big() -> QueryState {
        state(8_000, 1024)
    }

    /// `/world` with `query` and an optional `If-None-Match`, on its own task.
    fn get(
        app: &Router,
        query: &str,
        if_none_match: Option<&str>,
    ) -> tokio::task::JoinHandle<Response> {
        let mut request = Request::get(format!("/worlds/default/world{query}"));
        if let Some(tag) = if_none_match {
            request = request.header(header::IF_NONE_MATCH, tag);
        }
        let request = request.body(Body::empty()).expect("request");
        let app = app.clone();
        tokio::spawn(async move { app.oneshot(request).await.expect("infallible") })
    }

    async fn response(task: tokio::task::JoinHandle<Response>) -> Response {
        task.await.expect("request task")
    }

    async fn body(response: Response) -> Result<Bytes, axum::Error> {
        axum::body::to_bytes(response.into_body(), usize::MAX).await
    }

    fn etag(response: &Response) -> String {
        let tag = response.headers().get(header::ETAG).expect("an ETag");
        tag.to_str().expect("ascii").to_owned()
    }

    async fn until(mut done: impl FnMut() -> bool) {
        for _ in 0..10_000 {
            if done() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("the condition never held");
    }

    /// Builds and bodies so far, read once the driver is idle: a generation records after its
    /// last chunk is handed over, which can be after the client has read it.
    async fn builds(state: &QueryState) -> (u64, u64) {
        until(|| !state.generations().running()).await;
        let seen = state.read_timings().expect("opted in");
        (seen.builds, seen.bodies)
    }

    /// Holds a write reservation and starts a blocker request (`hops=2`, its own key) that the
    /// driver takes and parks in `read_for_body`: requests made after this queue behind it.
    async fn park(state: &QueryState, app: &Router) -> tokio::task::JoinHandle<Response> {
        // An earlier generation may still be recording; the blocker must start a fresh driver.
        until(|| !state.generations().running()).await;
        let blocker = get(app, "?hops=2", None);
        until(|| state.generations().running() && state.generations().queued() == 0).await;
        blocker
    }

    /// Two concurrent requests for `query` queue behind a parked blocker and share one build.
    async fn two_identical_requests_share_one_build(query: &str) {
        let state = state(200, 16);
        let app = router(state.clone());
        let reservation = state.reserve_write().await;
        let blocker = park(&state, &app).await;
        let (b, c) = (get(&app, query, None), get(&app, query, None));
        // Both queue behind the blocker: neither is served alone.
        until(|| state.generations().queued() == 2).await;
        drop(reservation);
        assert_eq!(response(blocker).await.status(), StatusCode::OK);
        let (b, c) = (response(b).await, response(c).await);
        assert_eq!((b.status(), c.status()), (StatusCode::OK, StatusCode::OK));
        assert_eq!(etag(&b), etag(&c));
        let (b, c) = (body(b).await.expect("whole"), body(c).await.expect("whole"));
        assert_eq!(b, c, "one generation, one set of bytes");
        // The blocker's generation and one shared generation for both.
        assert_eq!(builds(&state).await, (2, 3));
    }

    #[test]
    fn identical_requests_share_one_build_and_get_identical_bytes() {
        crate::tests::run(false, two_identical_requests_share_one_build(""));
    }

    #[test]
    fn identical_full_type_requests_share_one_build() {
        // Full `lod=type` joins the queue (s2w#297).
        crate::tests::run(false, two_identical_requests_share_one_build("?lod=type"));
    }

    #[test]
    fn a_request_arriving_mid_generation_waits_for_the_next_one() {
        crate::tests::run(false, async {
            let state = big();
            let app = router(state.clone());
            let first = response(get(&app, "", None)).await;
            assert_eq!(first.status(), StatusCode::OK);
            // The first body is unread, so its generation is parked on a full channel.
            let second = get(&app, "", None);
            until(|| state.generations().queued() == 1).await;
            let first = body(first).await.expect("the first body is whole");
            let second = body(response(second).await)
                .await
                .expect("whole, never partial");
            assert!(first.len() > CHUNK_BYTES * CHUNKS_IN_FLIGHT);
            assert_eq!(first, second);
            assert_eq!(
                builds(&state).await,
                (2, 2),
                "the late request got its own generation"
            );
        });
    }

    #[test]
    fn a_stalled_subscriber_is_cut_and_the_other_gets_a_whole_body() {
        crate::tests::run(false, async {
            let state = big();
            let app = router(state.clone());
            let reservation = state.reserve_write().await;
            let blocker = park(&state, &app).await;
            let (stalled, reading) = (get(&app, "", None), get(&app, "", None));
            until(|| state.generations().queued() == 2).await;
            drop(reservation);
            body(response(blocker).await).await.expect("blocker whole");
            let (stalled, reading) = (response(stalled).await, response(reading).await);
            let whole = body(reading)
                .await
                .expect("the reading subscriber gets every byte");
            let view: serde_json::Value = serde_json::from_slice(&whole).expect("whole JSON");
            assert_eq!(view["offset"], 8_000);
            assert!(
                body(stalled).await.is_err(),
                "the stalled body ends with an error"
            );
            assert_eq!(builds(&state).await, (2, 3));
        });
    }

    #[test]
    fn one_generation_answers_304_and_200_by_if_none_match() {
        crate::tests::run(false, async {
            let state = state(200, 16);
            let app = router(state.clone());
            let tag = etag(&response(get(&app, "", None)).await);
            let reservation = state.reserve_write().await;
            let blocker = park(&state, &app).await;
            let (matching, fresh) = (get(&app, "", Some(&tag)), get(&app, "", None));
            until(|| state.generations().queued() == 2).await;
            drop(reservation);
            assert_eq!(response(blocker).await.status(), StatusCode::OK);
            let matching = response(matching).await;
            assert_eq!(matching.status(), StatusCode::NOT_MODIFIED);
            assert_eq!(etag(&matching), tag);
            let fresh = response(fresh).await;
            assert_eq!(fresh.status(), StatusCode::OK);
            assert_eq!(etag(&fresh), tag);
            body(fresh).await.expect("whole");
            // First read, blocker, then one generation that built once for its one 200.
            assert_eq!(builds(&state).await, (3, 3));
        });
    }

    #[test]
    fn a_generation_of_matching_tags_builds_nothing() {
        crate::tests::run(false, async {
            let state = state(200, 16);
            let app = router(state.clone());
            let tag = etag(&response(get(&app, "", None)).await);
            let reservation = state.reserve_write().await;
            until(|| !state.generations().running()).await;
            let blocker = get(&app, "", Some(&tag));
            until(|| state.generations().running() && state.generations().queued() == 0).await;
            let (b, c) = (get(&app, "", Some(&tag)), get(&app, "", Some(&tag)));
            until(|| state.generations().queued() == 2).await;
            drop(reservation);
            for task in [blocker, b, c] {
                assert_eq!(response(task).await.status(), StatusCode::NOT_MODIFIED);
            }
            assert_eq!(
                builds(&state).await,
                (1, 1),
                "only the first read built a view"
            );
        });
    }

    #[test]
    fn the_33rd_queued_request_gets_503() {
        crate::tests::run(false, async {
            let state = state(50, 16);
            let app = router(state.clone());
            let reservation = state.reserve_write().await;
            let blocker = park(&state, &app).await;
            let queued: Vec<_> = (0..QUEUE_LIMIT).map(|_| get(&app, "", None)).collect();
            until(|| state.generations().queued() == QUEUE_LIMIT).await;
            let over = response(get(&app, "", None)).await;
            assert_eq!(over.status(), StatusCode::SERVICE_UNAVAILABLE);
            let error: serde_json::Value =
                serde_json::from_slice(&body(over).await.expect("body")).expect("json");
            assert_eq!(error["error"], "world_queue_full");
            drop(reservation);
            assert_eq!(response(blocker).await.status(), StatusCode::OK);
            for task in queued {
                let queued = response(task).await;
                assert_eq!(queued.status(), StatusCode::OK);
                body(queued).await.expect("whole");
            }
            assert_eq!(
                builds(&state).await,
                (2, 1 + 32),
                "the 32 shared one generation"
            );
        });
    }

    #[test]
    fn a_request_past_the_wait_limit_gets_503_and_is_never_built() {
        crate::tests::run(false, async {
            let state = state(50, 16).with_body_wait(Duration::from_millis(100));
            let app = router(state.clone());
            let reservation = state.reserve_write().await;
            let (taken, queued) = (get(&app, "?hops=2", None), get(&app, "", None));
            for task in [taken, queued] {
                let late = response(task).await;
                assert_eq!(late.status(), StatusCode::SERVICE_UNAVAILABLE);
                let error: serde_json::Value =
                    serde_json::from_slice(&body(late).await.expect("body")).expect("json");
                assert_eq!(error["error"], "unavailable");
            }
            drop(reservation);
            until(|| !state.generations().running()).await;
            assert_eq!(builds(&state).await, (0, 0), "no one was left to build for");
        });
    }

    #[test]
    fn the_summary_is_not_gated_behind_a_stalled_generation() {
        crate::tests::run(false, async {
            let state = big();
            let app = router(state.clone());
            let parked = response(get(&app, "", None)).await;
            assert_eq!(parked.status(), StatusCode::OK);
            // The entity body is unread: its generation holds the read guard on a full channel.
            let started = std::time::Instant::now();
            let summary = response(get(&app, "?lod=type&links=none", None)).await;
            assert_eq!(summary.status(), StatusCode::OK);
            body(summary).await.expect("whole");
            assert_eq!(state.generations().queued(), 0, "the summary never queued");
            assert!(
                started.elapsed() < crate::query::stream::STALL,
                "the type summary waited for the entity generation"
            );
            assert!(
                state.generations().running(),
                "the entity generation is still parked"
            );
            body(parked).await.expect("the parked body is whole");
        });
    }

    /// Appends `count` relationships among the first `entities` entities, one write
    /// reservation each, as the bridge does between polls.
    async fn append_links(state: QueryState, entities: u32, count: u32) {
        for n in 0..count {
            let reservation = state.reserve_write().await;
            let event = WorldEvent::RelationshipObserved {
                from: NaturalKey::new(format!("e{}", n * 7 % entities)),
                to: NaturalKey::new(format!("e{}", n * 13 % entities)),
                kind: "near".into(),
            };
            let at = Timestamp::from_millis(i64::from(entities + n));
            state.append(at, event).expect("append");
            drop(reservation);
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    /// Many readers, prompt and late, across generations while the fold appends (s2w#270's
    /// leg-A panic hunt): every body is whole, bodies under one `ETag` are byte-identical, and
    /// each equals `serde_json::to_vec` of [`world_view`](super::super::view::world_view) at its
    /// offset. Entity ids span one to four digits, so the `e:<id>` string sort sees every key
    /// length.
    #[test]
    fn readers_during_appends_get_whole_bodies_equal_to_the_view() {
        crate::tests::run(false, async {
            const ENTITIES: u32 = 3_000;
            let state = state(ENTITIES, 32);
            let app = router(state.clone());
            let writer = tokio::spawn(append_links(state.clone(), ENTITIES, 300));
            let mut by_tag: BTreeMap<String, Bytes> = BTreeMap::new();
            let mut compared = 0;
            for _ in 0..15 {
                let readers: Vec<_> = (0..8u64)
                    .map(|i| {
                        let app = app.clone();
                        tokio::spawn(async move {
                            let response = response(get(&app, "", None)).await;
                            assert_eq!(response.status(), StatusCode::OK);
                            let tag = etag(&response);
                            // A late reader: its chunks queue while the others are written.
                            tokio::time::sleep(Duration::from_millis(i * 3)).await;
                            (tag, body(response).await.expect("whole"))
                        })
                    })
                    .collect();
                for reader in readers {
                    let (tag, bytes) = reader.await.expect("reader task");
                    if let Some(seen) = by_tag.get(&tag) {
                        assert_eq!(seen, &bytes, "one ETag, one set of bytes");
                        continue;
                    }
                    let view: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
                    let offset = view["offset"].as_u64().expect("offset");
                    // An older offset may have left the history; the head always compares.
                    if let Ok(expected) = state.view_at(Some(offset), None, &ViewParams::default())
                    {
                        let expected = serde_json::to_vec(&expected).expect("serialize");
                        assert!(
                            expected == bytes.as_ref(),
                            "body at {offset} differs from the view"
                        );
                        compared += 1;
                    }
                    by_tag.insert(tag, bytes);
                }
            }
            writer.await.expect("writer task");
            assert!(by_tag.len() > 1, "the fold advanced between generations");
            assert!(
                compared > 0,
                "at least one body was checked against the view"
            );
        });
    }
}
