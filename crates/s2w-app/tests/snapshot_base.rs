//! The query contract over a timeline restored from a snapshot (decision 0021): every route
//! that takes an offset answers 410 `offset_before_base` below the base, `/time` reports the
//! base, and the SSE stream after the base carries the same deltas the full history would.

// `allow-unwrap-in-tests` applies inside `#[cfg(test)]` items only.
#[cfg(test)]
mod base {
    use std::time::Duration;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use s2w_app::query::{QueryState, Timeline, router};
    use s2w_core::WorldEvent;
    use s2w_model::Timestamp;
    use serde_json::Value;
    use tokio_stream::StreamExt;
    use tower::ServiceExt;

    const GOLDEN: &str = include_str!("../../s2w-core/tests/fixtures/golden-fold-v1.json");
    const CAP: u64 = 3;
    const BASE: usize = 10;

    fn events() -> Vec<WorldEvent> {
        serde_json::from_str(GOLDEN).unwrap()
    }

    fn append(timeline: &mut Timeline, events: &[WorldEvent], from: usize) {
        for (i, e) in events.iter().enumerate() {
            let ts = i64::try_from(from + i).unwrap() * 1000;
            timeline.append(Timestamp::from_millis(ts), e.clone());
        }
    }

    /// The full timeline and one restored at [`BASE`] with the same tail.
    fn timelines() -> (Timeline, Timeline) {
        let events = events();
        let mut full = Timeline::new(CAP);
        append(&mut full, &events, 0);
        let mut prefix = Timeline::new(CAP);
        append(&mut prefix, &events[..BASE], 0);
        let mut restored = Timeline::from_snapshot(prefix.head_world().clone(), prefix.head_time());
        append(&mut restored, &events[BASE..], BASE);
        (full, restored)
    }

    /// `#[tokio::test]` expands to an `allow(clippy::expect_used)` the workspace forbids.
    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    async fn request(app: &Router, uri: &str, last_event_id: Option<&str>) -> (StatusCode, Body) {
        let mut req = Request::get(uri);
        if let Some(id) = last_event_id {
            req = req.header("last-event-id", id);
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        (res.status(), res.into_body())
    }

    async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
        let (status, body) = request(app, uri, None).await;
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// The first `n` SSE frames as raw `id:`/`data:` text, in order.
    async fn sse_frames(body: Body, n: usize) -> Vec<String> {
        let mut stream = body.into_data_stream();
        let mut text = String::new();
        let mut frames = Vec::new();
        while frames.len() < n {
            let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            text.push_str(std::str::from_utf8(&chunk).unwrap());
            while let Some(end) = text.find("\n\n") {
                let frame: String = text.drain(..end + 2).collect();
                if frame.contains("data: ") {
                    frames.push(frame);
                }
            }
        }
        frames
    }

    #[test]
    fn offsets_below_the_base_answer_410_on_every_route() {
        run(async {
            let (_, restored) = timelines();
            let app = router(QueryState::new(restored));
            let below = BASE - 1;
            for uri in [
                format!("/worlds/default/world?at={below}"),
                format!("/worlds/default/diff?from={below}"),
                format!("/worlds/default/diff?from={BASE}&to={below}"),
                format!("/worlds/default/events?from={below}"),
                "/worlds/default/events".to_owned(),
                format!("/worlds/default/entity/0/history?to={below}"),
                format!("/worlds/default/time?ts={}", (BASE as i64 - 1) * 1000 - 1),
            ] {
                let (status, body) = get(&app, &uri).await;
                assert_eq!(status, StatusCode::GONE, "{uri}");
                assert_eq!(body["error"], "offset_before_base", "{uri}");
            }
            let (status, _) =
                request(&app, "/worlds/default/events", Some(&below.to_string())).await;
            assert_eq!(status, StatusCode::GONE, "Last-Event-ID below the base");
        });
    }

    #[test]
    fn time_reports_the_base_and_the_full_range() {
        run(async {
            let (full, restored) = timelines();
            let (status, time) =
                get(&router(QueryState::new(restored)), "/worlds/default/time").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(time["base"], BASE);
            assert_eq!(time["head"], full.head());
            let full_time = full.time_range();
            assert_eq!(time["first_ts"], full_time.first_ts.unwrap());
            assert_eq!(time["last_ts"], full_time.last_ts.unwrap());
            let (_, fresh) = get(&router(QueryState::new(full)), "/worlds/default/time").await;
            assert_eq!(fresh["base"], 0, "no snapshot: base is 0");
        });
    }

    #[test]
    fn sse_after_the_base_matches_the_full_history() {
        run(async {
            let (full, restored) = timelines();
            let head = usize::try_from(full.head()).unwrap();
            let n = head - BASE;
            let full_app = router(QueryState::new(full));
            let restored_app = router(QueryState::new(restored));
            let uri = format!("/worlds/default/events?from={BASE}&at={head}");
            let (status, body) = request(&restored_app, &uri, None).await;
            assert_eq!(status, StatusCode::OK);
            let got = sse_frames(body, n).await;
            let (_, body) = request(&full_app, &uri, None).await;
            assert_eq!(got, sse_frames(body, n).await);
            assert!(got[0].contains(&format!("id: {}", BASE + 1)), "{}", got[0]);
            // Resuming by Last-Event-ID at the base is served, not gone.
            let (status, _) = request(
                &restored_app,
                &format!("/worlds/default/events?at={head}"),
                Some(&BASE.to_string()),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        });
    }
}
