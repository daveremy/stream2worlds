//! The query API against the golden log from #9 (cap 3, `enwiki` is a hub).

#[cfg(test)]
mod golden {
    use std::time::Duration;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use s2w_app::query::{QueryState, Timeline, ViewParams, router, world_view};
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

    async fn head_entity_view_serves_the_hub_as_an_aggregate_body() {
        let (_, app) = app();
        let (status, view) = get(&app, "/world").await;
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
        assert_eq!(get(&app, "/world").await.1, view);
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
            let (status, got) = get(&app, &format!("/world?at={at}")).await;
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
        let (_, view) = get(&app, "/world?at=14").await;
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
        let (_, view) = get(&app, "/world").await;
        assert!(node(&view, &format!("e:{alt}")).is_some());
    }

    #[test]
    fn type_lod_aggregates_links_into_the_hub() {
        run(type_lod_aggregates_links_into_the_hub_body());
    }

    async fn type_lod_aggregates_links_into_the_hub_body() {
        let (_, app) = app();
        let (status, view) = get(&app, "/world?lod=type").await;
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

        let (status, view) = get(&app, "/world").await;
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

        let (status, view) = get(&app, "/world?lod=type").await;
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
        let (status, view) = get(&app, &format!("/world?focus={alice}&hops=2")).await;
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

        let (status, err) = get(&app, "/world?focus=999").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(err["error"], "unknown_entity");
        let (status, err) = get(&app, &format!("/world?focus={alice}&hops=6")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "hops_too_large");
    }

    #[test]
    fn typed_errors_for_what_does_not_exist_yet() {
        run(typed_errors_for_what_does_not_exist_yet_body());
    }

    async fn typed_errors_for_what_does_not_exist_yet_body() {
        let (_, app) = app();
        for (uri, status, code) in [
            (
                "/world?branch=fork1",
                StatusCode::NOT_IMPLEMENTED,
                "branch_not_yet",
            ),
            (
                "/world?lod=cluster",
                StatusCode::NOT_IMPLEMENTED,
                "lod_not_yet",
            ),
            (
                "/world?lod=galaxy",
                StatusCode::BAD_REQUEST,
                "bad_parameter",
            ),
            ("/world?at=25", StatusCode::NOT_FOUND, "offset_beyond_head"),
            ("/world?at=-1", StatusCode::BAD_REQUEST, "bad_parameter"),
            (
                "/diff?branch=fork1",
                StatusCode::NOT_IMPLEMENTED,
                "branch_not_yet",
            ),
            (
                "/events?branch=fork1",
                StatusCode::NOT_IMPLEMENTED,
                "branch_not_yet",
            ),
            (
                "/events?from=25",
                StatusCode::NOT_FOUND,
                "offset_beyond_head",
            ),
            (
                "/entity/999/history",
                StatusCode::NOT_FOUND,
                "unknown_entity",
            ),
            ("/time?ts=soon", StatusCode::BAD_REQUEST, "bad_parameter"),
            (
                "/time?branch=fork1",
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
        let (_, branches) = get(&app, "/branches").await;
        assert_eq!(branches[0]["name"], "actual");
        assert_eq!(branches[0]["head"], 24);
        assert_eq!(branches[0]["fold_version"], 1);
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
                        id = Some(v.parse::<u64>().unwrap());
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

    async fn sse_replays_one_typed_delta_per_offset_then_follows_body() {
        let (state, app) = app();
        let body = open_sse(&app, "/events", None).await;
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
        let body = open_sse(&app, "/events?from=0", Some("20")).await;
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

    #[test]
    fn diff_reports_the_merge_and_the_split() {
        run(diff_reports_the_merge_and_the_split_body());
    }

    async fn diff_reports_the_merge_and_the_split_body() {
        let (_, app) = app();
        let (alice, alt) = (id_of("user:Alice"), id_of("user:Alice_alt"));
        let (status, d) = get(&app, "/diff?from=12&to=13").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(d["merges"]["added"][0]["absorbed"], alt);
        assert_eq!(d["merges"]["added"][0]["survivor"], alice);
        assert_eq!(d["nodes"]["removed"][0]["id"], format!("e:{alt}"));
        let (_, d) = get(&app, "/diff?from=21&to=22").await;
        assert_eq!(d["merges"]["removed"][0]["absorbed"], alt);
        assert_eq!(d["nodes"]["added"][0]["id"], format!("e:{alt}"));
        let (_, d) = get(&app, "/diff?from=24").await;
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
        let (status, h) = get(&app, &format!("/entity/{alice}/history")).await;
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
        let (_, h) = get(&app, &format!("/entity/{alice}/history?to=5")).await;
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
            let (status, t) = get(&app, &format!("/time?ts={ts}")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(t["offset"], offset, "ts={ts}");
        }
        let (_, range) = get(&app, "/time").await;
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
        let (_, range) = get(&app, "/time").await;
        assert_eq!(range["clamped"], 1);
        assert_eq!(range["last_ts"], 23_000);
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
