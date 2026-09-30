//! `/events?last=N` (s2w#294): the last N retained events through the head as a finite SSE
//! response, and the `S2W-Epoch`/`S2W-Head` headers every `/events` response carries.

// `allow-unwrap-in-tests` applies inside `#[cfg(test)]` items only.
#[cfg(test)]
mod last {
    use std::time::Duration;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderMap, Request, StatusCode};
    use s2w_app::query::{Epoch, QueryState, Timeline, router};
    use s2w_core::{NaturalKey, WorldEvent};
    use s2w_model::Timestamp;
    use serde_json::Value;
    use tokio_stream::StreamExt;
    use tower::ServiceExt;

    const GOLDEN: &str = include_str!("../../s2w-core/tests/fixtures/golden-fold-v1.json");
    const CAP: u64 = 3;
    const EPOCH: Epoch = Epoch(0x2222_2222_2222_2222);
    const EPOCH_HEX: &str = "2222222222222222";

    fn events() -> Vec<WorldEvent> {
        serde_json::from_str(GOLDEN).unwrap()
    }

    /// The first `n` golden events, event `i` received at `i * 1000` ms, under [`EPOCH`].
    fn timeline(n: usize) -> Timeline {
        let mut t = Timeline::new(CAP).with_epoch(EPOCH);
        for (i, e) in events().into_iter().take(n).enumerate() {
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

    async fn send(
        app: &Router,
        uri: &str,
        last_event_id: Option<&str>,
    ) -> (StatusCode, HeaderMap, Body) {
        let mut req = Request::get(uri);
        if let Some(id) = last_event_id {
            req = req.header("last-event-id", id);
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (parts, body) = res.into_parts();
        (parts.status, parts.headers, body)
    }

    /// The whole body of a finite stream; fails if it does not close within 5 s.
    async fn to_end(body: Body) -> String {
        let bytes = tokio::time::timeout(
            Duration::from_secs(5),
            axum::body::to_bytes(body, usize::MAX),
        )
        .await
        .expect("a finite stream closes")
        .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// The `data:` frames of an SSE body, in order.
    fn frames(text: &str) -> Vec<&str> {
        text.split("\n\n")
            .filter(|f| f.contains("data: "))
            .collect()
    }

    fn resolved(headers: &HeaderMap) -> (&str, u64) {
        (
            headers["s2w-epoch"].to_str().unwrap(),
            headers["s2w-head"].to_str().unwrap().parse().unwrap(),
        )
    }

    async fn error_of(
        app: &Router,
        uri: &str,
        last_event_id: Option<&str>,
    ) -> (StatusCode, String) {
        let (status, _, body) = send(app, uri, last_event_id).await;
        let body: Value = serde_json::from_str(&to_end(body).await).unwrap();
        (status, body["error"].as_str().unwrap().to_owned())
    }

    #[test]
    fn last_matches_from_head_minus_n_through_head_and_closes() {
        run(async {
            let app = router(QueryState::new(timeline(24)));
            let (status, headers, body) = send(&app, "/worlds/default/events?last=5", None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(resolved(&headers), (EPOCH_HEX, 24));
            let got = to_end(body).await;
            let (_, _, body) = send(&app, "/worlds/default/events?from=19&at=24", None).await;
            let want = to_end(body).await;
            assert_eq!(frames(&got), frames(&want));
            assert_eq!(frames(&got).len(), 5);
            assert!(
                frames(&got)[0].contains(&format!("id: {EPOCH_HEX}:20\n")),
                "{got}"
            );
            // More than the log holds: every event from offset 0.
            let (_, _, body) = send(&app, "/worlds/default/events?last=1000", None).await;
            assert_eq!(frames(&to_end(body).await).len(), 24);
        });
    }

    #[test]
    fn last_clamps_to_the_replay_base_instead_of_410() {
        run(async {
            let mut capped = Timeline::new(CAP).with_epoch(EPOCH).with_history_cap(8);
            for (i, e) in events().into_iter().enumerate() {
                capped.append(Timestamp::from_millis(i64::try_from(i).unwrap() * 1000), e);
            }
            let (base, head) = (capped.replay_base(), capped.head());
            assert!(base > 0, "the cap dropped events");
            let app = router(QueryState::new(capped));
            let (status, headers, body) =
                send(&app, "/worlds/default/events?last=1000", None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(resolved(&headers).1, head);
            let got = to_end(body).await;
            let uri = format!("/worlds/default/events?from={base}&at={head}");
            let (_, _, body) = send(&app, &uri, None).await;
            assert_eq!(frames(&got), frames(&to_end(body).await));
            assert_eq!(frames(&got).len(), usize::try_from(head - base).unwrap());
        });
    }

    #[test]
    fn an_empty_world_answers_an_empty_tail_with_its_epoch_and_head() {
        run(async {
            let app = router(QueryState::new(timeline(0)));
            let (status, headers, body) = send(&app, "/worlds/default/events?last=500", None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(resolved(&headers), (EPOCH_HEX, 0));
            assert!(frames(&to_end(body).await).is_empty());
        });
    }

    #[test]
    fn every_events_response_carries_epoch_and_head() {
        run(async {
            let state = QueryState::new(timeline(10));
            let app = router(state.clone());
            for (uri, id) in [
                ("/worlds/default/events?from=2&at=4", None),
                ("/worlds/default/events?at=6", Some("3")),
                ("/worlds/default/events", None),
                ("/worlds/default/events?from=10", None),
            ] {
                let (status, headers, _) = send(&app, uri, id).await;
                assert_eq!(status, StatusCode::OK, "{uri}");
                assert_eq!(resolved(&headers), (EPOCH_HEX, 10), "{uri}");
            }
            // The live stream reports the head it resolved, then follows past it.
            let (_, headers, body) = send(&app, "/worlds/default/events?from=10", None).await;
            assert_eq!(resolved(&headers).1, 10);
            state
                .append(
                    Timestamp::from_millis(99_000),
                    WorldEvent::EntityObserved {
                        key: NaturalKey::new("user:Carol"),
                        entity_type: "user".to_owned(),
                        attrs: std::collections::BTreeMap::new(),
                    },
                )
                .unwrap();
            let mut stream = body.into_data_stream();
            let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let text = std::str::from_utf8(&chunk).unwrap();
            assert!(text.contains(&format!("id: {EPOCH_HEX}:11\n")), "{text}");
        });
    }

    #[test]
    fn a_last_tail_closes_rather_than_following() {
        run(async {
            let state = QueryState::new(timeline(10));
            let app = router(state.clone());
            let (_, _, body) = send(&app, "/worlds/default/events?last=3", None).await;
            state
                .append(
                    Timestamp::from_millis(99_000),
                    WorldEvent::EntityObserved {
                        key: NaturalKey::new("user:Carol"),
                        entity_type: "user".to_owned(),
                        attrs: std::collections::BTreeMap::new(),
                    },
                )
                .unwrap();
            let got = to_end(body).await;
            let ids: Vec<&str> = frames(&got)
                .iter()
                .filter_map(|f| f.lines().find_map(|l| l.strip_prefix("id: ")))
                .collect();
            let want: Vec<String> = (8..=10).map(|o| format!("{EPOCH_HEX}:{o}")).collect();
            assert_eq!(ids, want);
        });
    }

    #[test]
    fn last_is_its_own_anchor_and_bounded() {
        run(async {
            let app = router(QueryState::new(timeline(10)));
            for (uri, id) in [
                ("/worlds/default/events?last=0", None),
                ("/worlds/default/events?last=1001", None),
                ("/worlds/default/events?last=-1", None),
                ("/worlds/default/events?last=x", None),
                ("/worlds/default/events?last=3&from=2", None),
                ("/worlds/default/events?last=3&at=5", None),
                ("/worlds/default/events?last=3", Some("4")),
                (
                    "/worlds/default/events?last=3",
                    Some(&format!("{EPOCH_HEX}:4")),
                ),
            ] {
                let (status, error) = error_of(&app, uri, id).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{uri} {id:?}");
                assert_eq!(error, "bad_parameter", "{uri} {id:?}");
            }
            let (status, _, _) = send(&app, "/worlds/default/events?last=1000", None).await;
            assert_eq!(status, StatusCode::OK);
        });
    }

    #[test]
    fn last_checks_a_supplied_epoch() {
        run(async {
            let app = router(QueryState::new(timeline(10)));
            let (status, error) = error_of(
                &app,
                "/worlds/default/events?last=3&epoch=1111111111111111",
                None,
            )
            .await;
            assert_eq!(status, StatusCode::GONE);
            assert_eq!(error, "stale_epoch");
            let uri = format!("/worlds/default/events?last=3&epoch={EPOCH_HEX}");
            let (status, _, body) = send(&app, &uri, None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(frames(&to_end(body).await).len(), 3);
        });
    }
}
