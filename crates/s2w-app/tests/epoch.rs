//! The epoch on the query contract (#184, PR 2b-i): a caller-supplied epoch that is not the
//! served one answers 410 `stale_epoch`, never another world's bytes or a 404, on every read
//! surface; SSE ids carry `<epoch>:<offset>`, and a follower ends with one `stale_epoch`
//! error when the served history is replaced under it.

#[cfg(test)]
mod contract {
    use std::time::Duration;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use s2w_app::query::{Epoch, QueryState, Timeline, router};
    use s2w_core::WorldEvent;
    use s2w_model::Timestamp;
    use serde_json::Value;
    use tokio_stream::StreamExt;
    use tower::ServiceExt;

    const GOLDEN: &str = include_str!("../../s2w-core/tests/fixtures/golden-fold-v1.json");
    const CAP: u64 = 3;
    const E1: Epoch = Epoch(0x1111_1111_1111_1111);
    const E2: Epoch = Epoch(0x2222_2222_2222_2222);
    const E1_HEX: &str = "1111111111111111";
    const E2_HEX: &str = "2222222222222222";

    /// The first `n` golden events, event `i` received at `i * 1000` ms, under `epoch`.
    fn timeline(n: usize, epoch: Epoch) -> Timeline {
        let events: Vec<WorldEvent> = serde_json::from_str(GOLDEN).unwrap();
        let mut t = Timeline::new(CAP).with_epoch(epoch);
        for (i, e) in events.into_iter().take(n).enumerate() {
            t.append(Timestamp::from_millis(i64::try_from(i).unwrap() * 1000), e);
        }
        t
    }

    /// `#[tokio::test]` expands to an `allow(clippy::expect_used)` the workspace forbids.
    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    async fn send(app: &Router, uri: &str, last_event_id: Option<&str>) -> (StatusCode, Body) {
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
        get_with(app, uri, None).await
    }

    async fn get_with(app: &Router, uri: &str, last_event_id: Option<&str>) -> (StatusCode, Value) {
        let (status, body) = send(app, uri, last_event_id).await;
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// Every read surface a client can pin with an epoch, at an offset (10) that exists under
    /// e1 but is beyond the head of the shorter e2 timeline (5).
    fn pinned(epoch: &str) -> Vec<String> {
        let w = "/worlds/default";
        vec![
            format!("{w}/world?at=10&epoch={epoch}"),
            format!("{w}/diff?from=0&to=10&epoch={epoch}"),
            format!("{w}/entity/0/history?to=10&epoch={epoch}"),
            format!("{w}/time?epoch={epoch}"),
            format!("{w}/time?ts=9000&epoch={epoch}"),
            format!("{w}/sources?at=10&epoch={epoch}"),
            format!("{w}/events?from=10&epoch={epoch}"),
        ]
    }

    /// e1 served, then replaced by the shorter e2 history.
    fn swapped() -> Router {
        let state = QueryState::new(timeline(24, E1));
        let app = router(state.clone());
        state.replace_timeline(timeline(5, E2)).unwrap();
        app
    }

    #[test]
    fn a_stale_epoch_is_410_on_every_surface_not_404() {
        run(async {
            let app = swapped();
            for uri in pinned(E1_HEX) {
                let (status, body) = get(&app, &uri).await;
                assert_eq!(status, StatusCode::GONE, "{uri}");
                assert_eq!(body["error"], "stale_epoch", "{uri}");
            }
            let (status, body) =
                get_with(&app, "/worlds/default/events", Some(&format!("{E1_HEX}:10"))).await;
            assert_eq!(status, StatusCode::GONE);
            assert_eq!(body["error"], "stale_epoch");
        });
    }

    #[test]
    fn bare_offsets_opt_out_and_the_served_epoch_is_accepted() {
        run(async {
            let app = swapped();
            // Bare forms behave as before: offset 10 is beyond e2's head.
            let (status, body) = get(&app, "/worlds/default/world?at=10").await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(body["error"], "offset_beyond_head");
            // The served epoch is accepted wherever it is supplied.
            for uri in [
                format!("/worlds/default/world?at=5&epoch={E2_HEX}"),
                format!("/worlds/default/diff?from=0&to=5&epoch={E2_HEX}"),
                format!("/worlds/default/entity/0/history?to=5&epoch={E2_HEX}"),
                format!("/worlds/default/time?epoch={E2_HEX}"),
                format!("/worlds/default/time?ts=3000&epoch={E2_HEX}"),
                format!("/worlds/default/sources?at=5&epoch={E2_HEX}"),
                format!("/worlds/default/events?from=0&at=5&epoch={E2_HEX}"),
            ] {
                let (status, _) = send(&app, &uri, None).await;
                assert_eq!(status, StatusCode::OK, "{uri}");
            }
            let (status, _) = send(
                &app,
                "/worlds/default/events?at=5",
                Some(&format!("{E2_HEX}:0")),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        });
    }

    #[test]
    fn a_malformed_epoch_is_a_bad_parameter() {
        run(async {
            let app = swapped();
            for uri in [
                "/worlds/default/world?epoch=zz",
                "/worlds/default/world?epoch=2222",
                "/worlds/default/time?epoch=22222222222222222",
                "/worlds/default/events?epoch=nope",
            ] {
                let (status, body) = get(&app, uri).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
                assert_eq!(body["error"], "bad_parameter", "{uri}");
            }
            let (status, body) = get_with(&app, "/worlds/default/events", Some("zz:3")).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"], "bad_parameter");
        });
    }

    #[test]
    fn the_epoch_is_a_16_hex_string_in_time_and_world() {
        run(async {
            let app = swapped();
            for uri in [
                "/worlds/default/time",
                "/worlds/default/time?ts=3000",
                "/worlds/default/world",
            ] {
                let (status, body) = get(&app, uri).await;
                assert_eq!(status, StatusCode::OK, "{uri}");
                assert_eq!(body["epoch"], E2_HEX, "{uri}");
            }
        });
    }

    /// Reads SSE frames until the stream ends; returns (id, event, data) per frame.
    async fn frames(body: Body) -> Vec<(Option<String>, String, Value)> {
        let mut stream = body.into_data_stream();
        let mut text = String::new();
        let mut out = Vec::new();
        while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
        {
            text.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
            while let Some(end) = text.find("\n\n") {
                let frame: String = text.drain(..end + 2).collect();
                let (mut id, mut event, mut data) = (None, None, None);
                for line in frame.lines() {
                    if let Some(v) = line.strip_prefix("id: ") {
                        id = Some(v.to_owned());
                    } else if let Some(v) = line.strip_prefix("event: ") {
                        event = Some(v.to_owned());
                    } else if let Some(v) = line.strip_prefix("data: ") {
                        data = Some(serde_json::from_str::<Value>(v).unwrap());
                    }
                }
                if let (Some(event), Some(data)) = (event, data) {
                    out.push((id, event, data));
                }
            }
        }
        out
    }

    /// A follower opened under e1 ends with exactly one `stale_epoch` error once e2 replaces
    /// the history, after the frames it had already been sent.
    async fn follower_ends_on_swap(from: u64, e1_len: usize) {
        let state = QueryState::new(timeline(e1_len, E1));
        let app = router(state.clone());
        let (status, body) = send(
            &app,
            &format!("/worlds/default/events?from={from}&epoch={E1_HEX}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let reader = tokio::spawn(frames(body));
        tokio::time::sleep(Duration::from_millis(50)).await;
        state.replace_timeline(timeline(5, E2)).unwrap();
        let got = reader.await.unwrap();
        let errors: Vec<_> = got.iter().filter(|(_, e, _)| e == "error").collect();
        assert_eq!(errors.len(), 1, "{got:?}");
        assert_eq!(errors[0].2["error"], "stale_epoch");
        assert!(errors[0].0.is_none());
        let (_, last, _) = got.last().unwrap();
        assert_eq!(last, "error", "the error is the last frame");
        for (id, event, _) in &got[..got.len() - 1] {
            assert_ne!(event, "error");
            assert!(id.as_deref().unwrap().starts_with(&format!("{E1_HEX}:")));
        }
    }

    #[test]
    fn an_sse_follower_at_the_head_ends_with_stale_epoch_on_swap() {
        run(follower_ends_on_swap(24, 24));
    }

    #[test]
    fn an_sse_follower_on_an_empty_timeline_ends_with_stale_epoch_on_swap() {
        run(follower_ends_on_swap(0, 0));
    }

    #[test]
    fn sse_ids_carry_the_served_epoch() {
        run(async {
            let app = swapped();
            let (status, body) = send(&app, "/worlds/default/events?from=0&at=5", None).await;
            assert_eq!(status, StatusCode::OK);
            let got = frames(body).await;
            assert_eq!(got.len(), 5);
            for (i, (id, _, _)) in got.iter().enumerate() {
                assert_eq!(id.as_deref().unwrap(), format!("{E2_HEX}:{}", i + 1));
            }
        });
    }
}
