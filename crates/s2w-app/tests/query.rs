//! The query API against the golden log from #9 (cap 3, `enwiki` is a hub).

#[cfg(test)]
mod golden {
    use std::time::Duration;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderMap, Request, StatusCode, header};
    use s2w_app::query::{Lod, QueryState, Timeline, ViewParams, diff, router, world_view};
    use s2w_core::{NaturalKey, World, WorldEvent, fold};
    use s2w_model::Timestamp;
    use serde_json::Value;
    use tokio_stream::StreamExt;
    use tower::ServiceExt;

    const GOLDEN: &str = include_str!("../../s2w-core/tests/fixtures/golden-fold-v1.json");
    const CAP: u64 = 3;

    fn events() -> Vec<WorldEvent> {
        serde_json::from_str(GOLDEN).unwrap()
    }

    /// The golden log with event `i` received at `i * 1000` ms.
    fn timeline() -> Timeline {
        let mut t = Timeline::new(CAP);
        for (i, e) in events().into_iter().enumerate() {
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

    fn app() -> (QueryState, Router) {
        let state = QueryState::new(timeline());
        (state.clone(), router(state))
    }

    async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
        let res = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn get_status(app: &Router, uri: &str) -> StatusCode {
        app.clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    fn id_of(key: &str) -> u64 {
        let world = fold(World::with_hub_cap(CAP), &events());
        world.id_of(&NaturalKey::new(key)).unwrap().get()
    }

    fn node<'a>(view: &'a Value, id: &str) -> Option<&'a Value> {
        view["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
    }

    #[test]
    fn head_entity_view_serves_the_hub_as_an_aggregate() {
        run(head_entity_view_serves_the_hub_as_an_aggregate_body());
    }

    #[expect(
        clippy::cognitive_complexity,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    async fn head_entity_view_serves_the_hub_as_an_aggregate_body() {
        let (_, app) = app();
        let (status, view) = get(&app, "/worlds/default/world").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["offset"], 24);
        assert_eq!(view["branch"], "actual");
        assert_eq!(view["hub_in_degree_cap"], 3);
        let enwiki = format!("e:{}", id_of("enwiki"));
        let hub = node(&view, &enwiki).unwrap();
        assert_eq!(hub["kind"], "hub");
        assert_eq!(hub["in_degree"], 5);
        assert_eq!(hub["by_kind"]["on"], 6);
        // No per-source link into the hub at any lod.
        for link in view["links"].as_array().unwrap() {
            assert_ne!(link["target"], enwiki.as_str());
        }
        // Sources carry the hub as a hub_ref instead: pages observed before and after the trip.
        for page in ["page:Rust", "page:Cargo", "page:Clippy"] {
            let n = node(&view, &format!("e:{}", id_of(page))).unwrap();
            assert_eq!(n["hub_refs"][0]["hub"], enwiki.as_str(), "{page}");
        }
        // Every id is a prefixed string.
        for n in view["nodes"].as_array().unwrap() {
            assert!(n["id"].as_str().unwrap().starts_with("e:"));
        }
        // Deterministic bytes.
        assert_eq!(get(&app, "/worlds/default/world").await.1, view);
    }

    #[test]
    fn every_offset_matches_the_pure_projection() {
        run(every_offset_matches_the_pure_projection_body());
    }

    async fn every_offset_matches_the_pure_projection_body() {
        let (_, app) = app();
        let log = events();
        for at in 0..=log.len() {
            let world = fold(World::with_hub_cap(CAP), &log[..at]);
            let expected =
                serde_json::to_value(world_view(&world, &ViewParams::default()).unwrap()).unwrap();
            let (status, got) = get(&app, &format!("/worlds/default/world?at={at}")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(got, expected, "at={at}");
        }
    }

    #[test]
    fn merges_resolve_at_read_time() {
        run(merges_resolve_at_read_time_body());
    }

    async fn merges_resolve_at_read_time_body() {
        let (_, app) = app();
        let (alice, alt) = (id_of("user:Alice"), id_of("user:Alice_alt"));
        // Offset 14: Alice_alt was merged under Alice at 13, so it is a member, not a node, and
        // its observation at 14 landed on Alice. Its attributes from before the merge (12) stay
        // on Alice_alt: a merge moves nothing (decision 0005).
        let (_, view) = get(&app, "/worlds/default/world?at=14").await;
        assert!(node(&view, &format!("e:{alt}")).is_none());
        let a = node(&view, &format!("e:{alice}")).unwrap();
        assert_eq!(a["members"], serde_json::json!([alt]));
        assert_eq!(
            a["keys"],
            serde_json::json!(["user:Alice", "user:Alice_alt"])
        );
        assert_eq!(a["attrs"]["tz"]["Str"], "UTC");
        assert!(a["attrs"].get("edits").is_none());
        // Head: the merge was revoked at 22, so Alice_alt is its own node again.
        let (_, view) = get(&app, "/worlds/default/world").await;
        assert!(node(&view, &format!("e:{alt}")).is_some());
    }

    #[test]
    fn type_lod_aggregates_links_into_the_hub() {
        run(type_lod_aggregates_links_into_the_hub_body());
    }

    async fn type_lod_aggregates_links_into_the_hub_body() {
        let (_, app) = app();
        let (status, view) = get(&app, "/worlds/default/world?lod=type").await;
        assert_eq!(status, StatusCode::OK);
        let page = node(&view, "type:page").unwrap();
        // The hub (type wiki) is its own node, never counted in a type bucket.
        assert!(node(&view, "type:wiki").is_none());
        // Only page:Rust was observed with a type; the other four pages are only named by
        // relationships.
        assert_eq!(page["count"], 1);
        assert_eq!(node(&view, "type:untyped").unwrap()["count"], 4);
        let enwiki = format!("e:{}", id_of("enwiki"));
        let into_hub: Vec<&Value> = view["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["target"] == enwiki.as_str())
            .collect();
        // Pages (typed and untyped) point at the hub; weight is distinct sources per group.
        let total: u64 = into_hub.iter().map(|l| l["weight"].as_u64().unwrap()).sum();
        assert_eq!(total, 5);
    }

    #[test]
    fn a_sub_cap_target_merged_into_the_hub_folds_into_its_aggregate() {
        run(a_sub_cap_target_merged_into_the_hub_folds_into_its_aggregate_body());
    }

    /// `enwiki_mirror` collects three `on` edges (not past the cap of 3, so it is not a hub on
    /// its own), then is merged into `enwiki`. The hub's aggregate must count those sources,
    /// agreeing with the per-source hub_refs and the lod=type aggregate link weights.
    #[expect(
        clippy::cognitive_complexity,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    async fn a_sub_cap_target_merged_into_the_hub_folds_into_its_aggregate_body() {
        let mut log = events();
        for page in ["page:Docs", "page:Book", "page:Rust"] {
            log.push(WorldEvent::RelationshipObserved {
                from: NaturalKey::new(page),
                to: NaturalKey::new("enwiki_mirror"),
                kind: "on".to_owned(),
            });
        }
        log.push(WorldEvent::EntitiesMerged {
            survivor: NaturalKey::new("enwiki"),
            absorbed: NaturalKey::new("enwiki_mirror"),
        });
        let mut t = Timeline::new(CAP);
        for (i, e) in log.iter().cloned().enumerate() {
            t.append(Timestamp::from_millis(i64::try_from(i).unwrap() * 1000), e);
        }
        let app = router(QueryState::new(t));
        let world = fold(World::with_hub_cap(CAP), &log);
        let key_id = |k: &str| format!("e:{}", world.id_of(&NaturalKey::new(k)).unwrap().get());
        let enwiki = key_id("enwiki");

        let (status, view) = get(&app, "/worlds/default/world").await;
        assert_eq!(status, StatusCode::OK);
        let hub = node(&view, &enwiki).unwrap();
        assert_eq!(hub["kind"], "hub");
        // The golden five, plus Docs and Book; Rust was already a source.
        assert_eq!(hub["in_degree"], 7);
        assert_eq!(hub["by_kind"]["on"], 9);
        assert_eq!(
            hub["members"][0],
            world
                .id_of(&NaturalKey::new("enwiki_mirror"))
                .unwrap()
                .get()
        );
        // No per-source link into the merged hub; the merged-in sources carry a hub_ref.
        for link in view["links"].as_array().unwrap() {
            assert_ne!(link["target"], enwiki.as_str());
        }
        let with_ref = view["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| {
                n["hub_refs"]
                    .as_array()
                    .is_some_and(|r| r.iter().any(|h| h["hub"] == enwiki.as_str()))
            })
            .count();
        assert_eq!(with_ref, 7);
        for page in ["page:Docs", "page:Book"] {
            let n = node(&view, &key_id(page)).unwrap();
            assert_eq!(n["hub_refs"][0]["hub"], enwiki.as_str(), "{page}");
        }

        let (status, view) = get(&app, "/worlds/default/world?lod=type").await;
        assert_eq!(status, StatusCode::OK);
        let total: u64 = view["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["target"] == enwiki.as_str())
            .map(|l| l["weight"].as_u64().unwrap())
            .sum();
        assert_eq!(total, 7);
        assert_eq!(node(&view, &enwiki).unwrap()["in_degree"], 7);
    }

    #[test]
    fn focus_limits_to_the_neighbourhood_and_never_expands_a_hub() {
        run(focus_limits_to_the_neighbourhood_and_never_expands_a_hub_body());
    }

    async fn focus_limits_to_the_neighbourhood_and_never_expands_a_hub_body() {
        let (_, app) = app();
        let alice = id_of("user:Alice");
        let (status, view) =
            get(&app, &format!("/worlds/default/world?focus={alice}&hops=2")).await;
        assert_eq!(status, StatusCode::OK);
        let ids: Vec<&str> = view["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect();
        // Alice -> page:Rust -> enwiki (hub, not expanded), so no other page appears.
        assert!(ids.contains(&format!("e:{}", id_of("page:Rust")).as_str()));
        assert!(ids.contains(&format!("e:{}", id_of("enwiki")).as_str()));
        assert!(!ids.contains(&format!("e:{}", id_of("page:Cargo")).as_str()));

        let (status, err) = get(&app, "/worlds/default/world?focus=999").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(err["error"], "unknown_entity");
        let (status, err) = get(&app, &format!("/worlds/default/world?focus={alice}&hops=6")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "hops_too_large");
    }

    #[test]
    fn typed_errors_for_what_does_not_exist_yet() {
        run(typed_errors_for_what_does_not_exist_yet_body());
    }

    #[expect(
        clippy::too_many_lines,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    async fn typed_errors_for_what_does_not_exist_yet_body() {
        let (_, app) = app();
        for (uri, status, code) in [
            (
                "/worlds/default/world?branch=fork1",
                StatusCode::NOT_IMPLEMENTED,
                "branch_not_yet",
            ),
            (
                "/worlds/default/world?lod=cluster",
                StatusCode::NOT_IMPLEMENTED,
                "lod_not_yet",
            ),
            (
                "/worlds/default/world?lod=galaxy",
                StatusCode::BAD_REQUEST,
                "bad_parameter",
            ),
            (
                "/worlds/default/world?at=25",
                StatusCode::NOT_FOUND,
                "offset_beyond_head",
            ),
            (
                "/worlds/default/world?at=-1",
                StatusCode::BAD_REQUEST,
                "bad_parameter",
            ),
            (
                "/worlds/default/diff?branch=fork1",
                StatusCode::NOT_IMPLEMENTED,
                "branch_not_yet",
            ),
            (
                "/worlds/default/events?branch=fork1",
                StatusCode::NOT_IMPLEMENTED,
                "branch_not_yet",
            ),
            (
                "/worlds/default/events?from=25",
                StatusCode::NOT_FOUND,
                "offset_beyond_head",
            ),
            (
                "/worlds/default/entity/999/history",
                StatusCode::NOT_FOUND,
                "unknown_entity",
            ),
            (
                "/worlds/default/time?ts=soon",
                StatusCode::BAD_REQUEST,
                "bad_parameter",
            ),
            (
                "/worlds/default/time?branch=fork1",
                StatusCode::NOT_IMPLEMENTED,
                "branch_not_yet",
            ),
        ] {
            let (got, body) = get(&app, uri).await;
            assert_eq!(got, status, "{uri}");
            assert_eq!(body["error"], code, "{uri}");
            assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()));
        }
        // The only branch is listed.
        let (_, branches) = get(&app, "/worlds/default/branches").await;
        assert_eq!(branches[0]["name"], "actual");
        assert_eq!(branches[0]["head"], 24);
        assert_eq!(branches[0]["fold_version"], 1);
    }

    #[test]
    fn worlds_lists_the_configured_world_and_head() {
        run(async {
            let (_, app) = app();
            let (status, body) = get(&app, "/worlds").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                body,
                serde_json::json!({
                    "worlds": [{"world": "default", "name": "default", "head": 24,
                        "title": null, "tagline": null}]
                })
            );
        });
    }

    #[test]
    fn wrong_world_is_a_typed_not_found_for_every_scoped_route() {
        run(async {
            let (_, app) = app();
            for uri in [
                "/worlds/nope/world",
                "/worlds/nope/events",
                "/worlds/nope/branches",
                "/worlds/nope/diff",
                "/worlds/nope/entity/1/history",
                "/worlds/nope/time",
            ] {
                let (status, body) = get(&app, uri).await;
                assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
                assert_eq!(body["error"], "unknown_world", "{uri}");
                assert!(
                    body["message"]
                        .as_str()
                        .is_some_and(|message| !message.is_empty()),
                    "{uri}"
                );
            }
        });
    }

    #[test]
    fn unscoped_routes_do_not_exist() {
        run(async {
            let (_, app) = app();
            for uri in [
                "/world",
                "/events",
                "/branches",
                "/diff",
                "/entity/1/history",
                "/time",
            ] {
                assert_eq!(get_status(&app, uri).await, StatusCode::NOT_FOUND, "{uri}");
            }
        });
    }

    /// Reads SSE frames until `n` messages arrive; returns (id, event, data) triples.
    async fn read_sse(body: Body, n: usize) -> Vec<(u64, String, Value)> {
        let mut stream = body.into_data_stream();
        let mut text = String::new();
        let mut out = Vec::new();
        while out.len() < n {
            let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            text.push_str(std::str::from_utf8(&chunk).unwrap());
            while let Some(end) = text.find("\n\n") {
                let frame: String = text.drain(..end + 2).collect();
                let (mut id, mut event, mut data) = (None, None, None);
                for line in frame.lines() {
                    if let Some(v) = line.strip_prefix("id: ") {
                        // `<epoch>:<offset>`; this timeline serves epoch 0.
                        let (epoch, offset) = v.split_once(':').unwrap();
                        assert_eq!(epoch, "0000000000000000");
                        id = Some(offset.parse::<u64>().unwrap());
                    } else if let Some(v) = line.strip_prefix("event: ") {
                        event = Some(v.to_owned());
                    } else if let Some(v) = line.strip_prefix("data: ") {
                        data = Some(serde_json::from_str::<Value>(v).unwrap());
                    }
                }
                if let (Some(id), Some(event), Some(data)) = (id, event, data) {
                    out.push((id, event, data));
                }
            }
        }
        out
    }

    async fn open_sse(app: &Router, uri: &str, last_event_id: Option<&str>) -> Body {
        let mut req = Request::get(uri);
        if let Some(id) = last_event_id {
            req = req.header("last-event-id", id);
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-accel-buffering"], "no");
        res.into_body()
    }

    #[test]
    fn sse_replays_one_typed_delta_per_offset_then_follows() {
        run(sse_replays_one_typed_delta_per_offset_then_follows_body());
    }

    #[expect(
        clippy::cognitive_complexity,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    async fn sse_replays_one_typed_delta_per_offset_then_follows_body() {
        let (state, app) = app();
        let body = open_sse(&app, "/worlds/default/events", None).await;
        let msgs = read_sse(body, 24).await;
        let ids: Vec<u64> = msgs.iter().map(|m| m.0).collect();
        assert_eq!(ids, (1..=24).collect::<Vec<_>>());
        let kind = |offset: usize| msgs[offset - 1].1.as_str();
        assert_eq!(kind(1), "entity");
        assert_eq!(kind(4), "link");
        assert_eq!(msgs[4].2["weight"], 2);
        // Cargo is the fourth distinct source into enwiki: the cap (3) trips at offset 9.
        assert_eq!(kind(9), "hub_ref");
        assert_eq!(msgs[8].2["tripped"], true);
        assert_eq!(msgs[9].2["tripped"], false);
        assert_eq!(kind(13), "merge");
        assert_eq!(msgs[12].2["survivor"], id_of("user:Alice"));
        assert_eq!(kind(16), "noop");
        assert_eq!(kind(20), "merge");
        assert_eq!(kind(21), "noop");
        assert_eq!(kind(22), "split");
        assert_eq!(msgs[21].2["absorbed"], id_of("user:Alice_alt"));
        assert_eq!(kind(24), "noop");
        for (id, _, data) in &msgs {
            assert_eq!(data["offset"], *id);
        }

        // Resume after offset 20 with Last-Event-ID, then receive a live append.
        let body = open_sse(&app, "/worlds/default/events?from=0", Some("20")).await;
        let appended = state
            .append(
                Timestamp::from_millis(99_000),
                WorldEvent::EntityObserved {
                    key: NaturalKey::new("user:Carol"),
                    entity_type: "user".to_owned(),
                    attrs: std::collections::BTreeMap::new(),
                },
            )
            .unwrap();
        assert_eq!(appended, 25);
        let msgs = read_sse(body, 5).await;
        let ids: Vec<u64> = msgs.iter().map(|m| m.0).collect();
        assert_eq!(ids, vec![21, 22, 23, 24, 25]);
        assert_eq!(msgs[4].1, "entity");
        assert_eq!(msgs[4].2["minted"], true);
    }

    async fn get_raw(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        let res = app.clone().oneshot(request).await.unwrap();
        let (parts, body) = res.into_parts();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        (parts.status, parts.headers, bytes.to_vec())
    }

    #[test]
    fn streamed_world_is_byte_identical_to_the_pure_projection() {
        run(streamed_world_is_byte_identical_to_the_pure_projection_body());
    }

    /// `/world` streams the view node by node (#216). Its bytes must equal
    /// `serde_json::to_vec(&world_view(..))` at every offset, level of detail and focus the
    /// projection tests use, in particular the node order: by `e:<id>` STRING, so `e:10` comes
    /// before `e:2`, which this log exercises (asserted below).
    async fn streamed_world_is_byte_identical_to_the_pure_projection_body() {
        let log = extended_log();
        let mut timeline = Timeline::new(CAP);
        for (i, e) in log.iter().enumerate() {
            timeline.append(
                Timestamp::from_millis(i64::try_from(i).unwrap() * 1000),
                e.clone(),
            );
        }
        let app = router(QueryState::new(timeline));
        let (alice, enwiki) = (id_of("user:Alice"), id_of("enwiki"));
        let focuses = [
            (None, 1),
            (Some(alice), 0),
            (Some(alice), 1),
            (Some(alice), 2),
            (Some(enwiki), 1),
            (Some(999), 1),
            (Some(alice), 6),
        ];
        let mut string_order_differs = false;
        for at in 0..=log.len() {
            let world = fold(World::with_hub_cap(CAP), &log[..at]);
            for lod in [Lod::Entity, Lod::Type] {
                for (focus, hops) in focuses {
                    let params = ViewParams { lod, focus, hops };
                    string_order_differs |= streamed_matches(&app, &world, at, &params).await;
                }
            }
        }
        assert!(
            string_order_differs,
            "no view put a longer id before a shorter one: the string-order trap is untested"
        );
    }

    /// The golden log mints fewer than ten ids; extra users relating to the hub and to Alice
    /// push ids past 9, so `e:10` and `e:9` share one view.
    fn extended_log() -> Vec<WorldEvent> {
        let mut log = events();
        for i in 0..8 {
            log.push(WorldEvent::RelationshipObserved {
                from: NaturalKey::new(format!("user:Extra{i}")),
                to: NaturalKey::new(if i % 2 == 0 { "enwiki" } else { "user:Alice" }),
                kind: "edit".to_owned(),
            });
        }
        log
    }

    /// Asserts `/world` at `at` and `params` answers exactly `world_view`'s bytes, or its error.
    /// True when the view's node order differs from numeric id order.
    async fn streamed_matches(app: &Router, world: &World, at: usize, params: &ViewParams) -> bool {
        let lod = if params.lod == Lod::Entity {
            "entity"
        } else {
            "type"
        };
        let mut uri = format!(
            "/worlds/default/world?at={at}&lod={lod}&hops={}",
            params.hops
        );
        if let Some(focus) = params.focus {
            uri.push_str(&format!("&focus={focus}"));
        }
        let (status, _, got) = get_raw(app, Request::get(&uri).body(Body::empty()).unwrap()).await;
        match world_view(world, params) {
            Ok(view) => {
                assert_eq!(status, StatusCode::OK, "{uri}");
                let expected = serde_json::to_vec(&view).unwrap();
                assert_eq!(
                    String::from_utf8(got).unwrap(),
                    String::from_utf8(expected).unwrap(),
                    "{uri}"
                );
                let ids: Vec<u64> = view
                    .nodes
                    .iter()
                    .filter_map(|n| n.id().strip_prefix("e:")?.parse().ok())
                    .collect();
                ids.windows(2).any(|w| w[0] > w[1])
            }
            Err(error) => {
                let got: Value = serde_json::from_slice(&got).unwrap();
                assert!(status.is_client_error(), "{uri}: {status}");
                assert_eq!(got, error.json_body(), "{uri}");
                false
            }
        }
    }

    #[test]
    fn world_answers_304_to_a_matching_etag() {
        run(world_answers_304_to_a_matching_etag_body());
    }

    #[expect(
        clippy::cognitive_complexity,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    async fn world_answers_304_to_a_matching_etag_body() {
        let (state, app) = app();
        let uri = "/worlds/default/world";
        let plain = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();
        let conditional = |uri: &str, tag: &str| {
            Request::get(uri)
                .header(header::IF_NONE_MATCH, tag)
                .body(Body::empty())
                .unwrap()
        };
        let (status, headers, body) = get_raw(&app, plain(uri)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        let tag = headers[header::ETAG].to_str().unwrap().to_owned();
        assert!(!body.is_empty());

        for inm in [
            tag.clone(),
            format!("W/{tag}"),
            format!("\"x\", {tag}"),
            "*".to_owned(),
        ] {
            let (status, headers, body) = get_raw(&app, conditional(uri, &inm)).await;
            assert_eq!(status, StatusCode::NOT_MODIFIED, "{inm}");
            assert_eq!(headers[header::ETAG], tag.as_str());
            assert!(body.is_empty());
        }
        // Another level of detail is another view.
        let (status, headers, _) =
            get_raw(&app, conditional(&format!("{uri}?lod=type"), &tag)).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(headers[header::ETAG], tag.as_str());
        // A stale epoch is refused before the tag is compared.
        let stale = format!("{uri}?epoch=00000000000000ff");
        let (status, _, _) = get_raw(&app, conditional(&stale, &tag)).await;
        assert_eq!(status, StatusCode::GONE);
        // The head moves: the old tag no longer matches.
        state
            .append(
                Timestamp::from_millis(99_000),
                WorldEvent::EntityObserved {
                    key: NaturalKey::new("user:Zed"),
                    entity_type: "user".to_owned(),
                    attrs: std::collections::BTreeMap::new(),
                },
            )
            .unwrap();
        let (status, headers, _) = get_raw(&app, conditional(uri, &tag)).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(headers[header::ETAG], tag.as_str());
    }

    #[test]
    fn every_diff_matches_the_pure_diff() {
        run(every_diff_matches_the_pure_diff_body());
    }

    /// `/diff` borrows the head and short-circuits `from == to` (#216): every pair, the
    /// equal ones included, must still answer exactly what the pure diff of two folds does.
    async fn every_diff_matches_the_pure_diff_body() {
        let (_, app) = app();
        let log = events();
        let worlds: Vec<World> = (0..=log.len())
            .map(|at| fold(World::with_hub_cap(CAP), &log[..at]))
            .collect();
        for (from, a) in worlds.iter().enumerate() {
            for (to, b) in worlds.iter().enumerate() {
                let expected = serde_json::to_value(diff(a, b).unwrap()).unwrap();
                let (status, got) =
                    get(&app, &format!("/worlds/default/diff?from={from}&to={to}")).await;
                assert_eq!(status, StatusCode::OK, "from={from} to={to}");
                assert_eq!(got, expected, "from={from} to={to}");
            }
        }
        // An equal pair past the head is still refused, exactly as `/world` refuses it.
        let past = log.len() + 1;
        let (status, got) = get(&app, &format!("/worlds/default/diff?from={past}&to={past}")).await;
        let (world_status, world_err) =
            get(&app, &format!("/worlds/default/world?at={past}")).await;
        assert_eq!(status, world_status);
        assert_eq!(got, world_err);
    }

    #[test]
    fn diff_reports_the_merge_and_the_split() {
        run(diff_reports_the_merge_and_the_split_body());
    }

    async fn diff_reports_the_merge_and_the_split_body() {
        let (_, app) = app();
        let (alice, alt) = (id_of("user:Alice"), id_of("user:Alice_alt"));
        let (status, d) = get(&app, "/worlds/default/diff?from=12&to=13").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(d["merges"]["added"][0]["absorbed"], alt);
        assert_eq!(d["merges"]["added"][0]["survivor"], alice);
        assert_eq!(d["nodes"]["removed"][0]["id"], format!("e:{alt}"));
        let (_, d) = get(&app, "/worlds/default/diff?from=21&to=22").await;
        assert_eq!(d["merges"]["removed"][0]["absorbed"], alt);
        assert_eq!(d["nodes"]["added"][0]["id"], format!("e:{alt}"));
        let (_, d) = get(&app, "/worlds/default/diff?from=24").await;
        assert_eq!(d["from"], 24);
        assert_eq!(d["to"], 24);
        assert!(d["nodes"]["changed"].as_array().unwrap().is_empty());
    }

    #[test]
    fn entity_history_includes_aliases_while_merged() {
        run(entity_history_includes_aliases_while_merged_body());
    }

    async fn entity_history_includes_aliases_while_merged_body() {
        let (_, app) = app();
        let alice = id_of("user:Alice");
        let (status, h) = get(&app, &format!("/worlds/default/entity/{alice}/history")).await;
        assert_eq!(status, StatusCode::OK);
        let offsets: Vec<u64> = h
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["offset"].as_u64().unwrap())
            .collect();
        // 3: observed; 4, 5: edited Rust; 13: merge; 14: Alice_alt observed while merged into
        // Alice; 15: Alice_alt edited Serde while merged; 20: Alice merged under Bob; 22: split.
        assert_eq!(offsets, vec![3, 4, 5, 13, 14, 15, 20, 22]);
        let (_, h) = get(
            &app,
            &format!("/worlds/default/entity/{alice}/history?to=5"),
        )
        .await;
        assert_eq!(h.as_array().unwrap().len(), 3);
    }

    #[test]
    fn time_maps_timestamps_to_offsets() {
        run(time_maps_timestamps_to_offsets_body());
    }

    async fn time_maps_timestamps_to_offsets_body() {
        let (state, app) = app();
        for (ts, offset) in [
            (-1, 0),
            (0, 1),
            (500, 1),
            (1000, 2),
            (23_000, 24),
            (1_000_000, 24),
        ] {
            let (status, t) = get(&app, &format!("/worlds/default/time?ts={ts}")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(t["offset"], offset, "ts={ts}");
        }
        let (_, range) = get(&app, "/worlds/default/time").await;
        assert_eq!(range["head"], 24);
        assert_eq!(range["first_ts"], 0);
        assert_eq!(range["last_ts"], 23_000);
        assert_eq!(range["clamped"], 0);
        // An out-of-order timestamp is clamped, never refused.
        let late = WorldEvent::EntityObserved {
            key: NaturalKey::new("user:Dan"),
            entity_type: "user".to_owned(),
            attrs: std::collections::BTreeMap::new(),
        };
        assert_eq!(state.append(Timestamp::from_millis(5), late).unwrap(), 25);
        let (_, range) = get(&app, "/worlds/default/time").await;
        assert_eq!(range["clamped"], 1);
        assert_eq!(range["last_ts"], 23_000);
    }

    /// `append_batch` (s2w#216) is `append` per event under one lock: same head, same world at
    /// every offset, same `/time` bounds; an empty batch changes nothing.
    #[test]
    fn append_batch_matches_append_per_event() {
        run(async {
            let batched = QueryState::new(Timeline::new(CAP));
            let stamped = events()
                .into_iter()
                .enumerate()
                .map(|(i, e)| (Timestamp::from_millis(i64::try_from(i).unwrap() * 1000), e));
            assert_eq!(batched.append_batch(stamped).unwrap(), 24);
            assert_eq!(batched.append_batch(Vec::new()).unwrap(), 24);
            let batched = router(batched);
            let single = router(QueryState::new(timeline()));
            for uri in [
                "/worlds/default/time".to_owned(),
                "/worlds/default/world".to_owned(),
            ]
            .into_iter()
            .chain((0..=24).map(|at| format!("/worlds/default/world?at={at}")))
            {
                assert_eq!(get(&batched, &uri).await, get(&single, &uri).await, "{uri}");
            }
        });
    }
}

/// A hub relating to another hub (#42): not in the golden log, so a small in-test world.
#[cfg(test)]
mod hub_to_hub {
    use s2w_app::query::{Lod, ViewParams, world_view};
    use s2w_core::{NaturalKey, World, WorldEvent, fold};
    use serde_json::Value;

    fn relate(from: &str, to: &str) -> WorldEvent {
        WorldEvent::RelationshipObserved {
            from: NaturalKey::new(from),
            to: NaturalKey::new(to),
            kind: "on".to_owned(),
        }
    }

    /// Cap 1: `a` and `b` each get two distinct sources and trip, then hub `a` relates to hub `b`.
    fn world() -> World {
        fold(
            World::with_hub_cap(1),
            &[
                relate("p1", "a"),
                relate("p2", "a"),
                relate("p1", "b"),
                relate("p2", "b"),
                relate("a", "b"),
            ],
        )
    }

    fn e(world: &World, key: &str) -> String {
        format!("e:{}", world.id_of(&NaturalKey::new(key)).unwrap().get())
    }

    fn view(world: &World, lod: Lod) -> Value {
        let params = ViewParams {
            lod,
            ..ViewParams::default()
        };
        serde_json::to_value(world_view(world, &params).unwrap()).unwrap()
    }

    fn node<'a>(view: &'a Value, id: &str) -> &'a Value {
        view["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap()
    }

    #[test]
    fn entity_lod_serves_a_hubs_own_hub_ref() {
        let world = world();
        let (a, b) = (e(&world, "a"), e(&world, "b"));
        let view = view(&world, Lod::Entity);
        let hub_a = node(&view, &a);
        assert_eq!(hub_a["kind"], "hub");
        assert_eq!(node(&view, &b)["kind"], "hub");
        assert_eq!(
            hub_a["hub_refs"],
            serde_json::json!([{ "kind": "on", "hub": b }])
        );
        // Still no link into a hub at lod=entity: the edge is carried by hub_refs alone.
        for link in view["links"].as_array().unwrap() {
            assert_ne!(link["target"], b.as_str());
        }
    }

    #[test]
    fn type_lod_shows_the_same_edge_as_a_link() {
        let world = world();
        let (a, b) = (e(&world, "a"), e(&world, "b"));
        let view = view(&world, Lod::Type);
        let hub_to_hub: Vec<&Value> = view["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["source"] == a.as_str() && l["target"] == b.as_str())
            .collect();
        assert_eq!(hub_to_hub.len(), 1);
        assert_eq!(hub_to_hub[0]["kind"], "on");
        assert_eq!(hub_to_hub[0]["weight"], 1);
    }
}
