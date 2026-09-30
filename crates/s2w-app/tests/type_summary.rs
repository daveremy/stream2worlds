//! The type summary (s2w#296): `/world?lod=type&links=none` equals the full type view's nodes
//! and counts without its links, carries its own `-nolinks` tag, and refuses every other shape.

#[cfg(test)]
mod summary {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderMap, Request, StatusCode, header};
    use s2w_app::query::{
        LinkDetail, Lod, QueryState, Timeline, ViewParams, router, type_summary, world_view,
    };
    use s2w_core::{NaturalKey, World, WorldEvent, fold};
    use s2w_model::Timestamp;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    const GOLDEN: &str = include_str!("../../s2w-core/tests/fixtures/golden-fold-v1.json");
    /// The golden log's hub cap: `enwiki` is a hub.
    const CAP: u64 = 3;

    fn events() -> Vec<WorldEvent> {
        serde_json::from_str(GOLDEN).unwrap()
    }

    /// `#[tokio::test]` expands to an `allow(clippy::expect_used)` the workspace forbids.
    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn app() -> Router {
        let mut t = Timeline::new(CAP);
        for (i, e) in events().into_iter().enumerate() {
            t.append(Timestamp::from_millis(i64::try_from(i).unwrap() * 1000), e);
        }
        router(QueryState::new(t))
    }

    const TYPE: ViewParams = ViewParams {
        lod: Lod::Type,
        focus: None,
        hops: 1,
        links: LinkDetail::All,
    };
    const SUMMARY: ViewParams = ViewParams {
        links: LinkDetail::None,
        ..TYPE
    };

    fn relate(from: &str, to: &str) -> WorldEvent {
        WorldEvent::RelationshipObserved {
            from: NaturalKey::new(from),
            to: NaturalKey::new(to),
            kind: "on".to_owned(),
        }
    }

    fn observe(key: &str, entity_type: &str) -> WorldEvent {
        WorldEvent::EntityObserved {
            key: NaturalKey::new(key),
            entity_type: entity_type.to_owned(),
            attrs: std::collections::BTreeMap::new(),
        }
    }

    /// Cap 1: typed users `p1`, `p2` and an untyped `p3`; `a` and `b` each get two distinct
    /// sources and trip the cap, hub `a` relates to hub `b`, and `a2` merges into hub `a` (a
    /// member, and a second key).
    fn hub_world() -> World {
        fold(
            World::with_hub_cap(1),
            &[
                observe("p1", "user"),
                observe("p2", "user"),
                observe("a", "page"),
                observe("a2", "page"),
                relate("p1", "a"),
                relate("p2", "a"),
                relate("p1", "b"),
                relate("p2", "b"),
                relate("a", "b"),
                relate("p3", "p1"),
                WorldEvent::EntitiesMerged {
                    survivor: NaturalKey::new("a"),
                    absorbed: NaturalKey::new("a2"),
                },
            ],
        )
    }

    fn e(world: &World, key: &str) -> String {
        format!("e:{}", world.id_of(&NaturalKey::new(key)).unwrap().get())
    }

    fn json(world: &World, params: &ViewParams) -> Value {
        serde_json::to_value(world_view(world, params).unwrap()).unwrap()
    }

    /// The summary equals the full type view except for its links and its hubs' `hub_refs`.
    /// Returns the full view, for fixture-specific assertions.
    fn assert_equivalent(world: &World) -> Value {
        let mut full = json(world, &TYPE);
        let summary = json(world, &SUMMARY);
        assert_eq!(
            summary,
            serde_json::to_value(type_summary(world)).unwrap(),
            "world_view dispatches links=none to type_summary"
        );
        assert_eq!(summary["links"], json!([]));
        let mut expected = full.clone();
        expected["links"] = json!([]);
        for node in expected["nodes"].as_array_mut().unwrap() {
            assert_ne!(node["kind"], "entity", "the type view has no entity node");
            if node["kind"] == "hub" {
                node["hub_refs"] = json!([]);
            }
        }
        assert_eq!(summary, expected);
        full["links"].take();
        full
    }

    #[test]
    fn the_summary_equals_the_full_type_view_at_every_golden_offset() {
        let log = events();
        let mut hubs = 0;
        for at in 0..=log.len() {
            let world = fold(World::with_hub_cap(CAP), &log[..at]);
            let full = assert_equivalent(&world);
            hubs += full["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|n| n["kind"] == "hub")
                .count();
        }
        assert!(hubs > 0, "the golden log reaches its hub");
    }

    #[test]
    fn a_summary_hub_has_every_field_but_its_hub_refs() {
        let world = hub_world();
        let (a, a2, b) = (e(&world, "a"), e(&world, "a2"), e(&world, "b"));
        let full = assert_equivalent(&world);
        let summary = json(&world, &SUMMARY);
        let node = |view: &Value, id: &str| {
            view["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["id"] == id)
                .cloned()
                .unwrap()
        };
        let (full_a, summary_a) = (node(&full, &a), node(&summary, &a));
        assert_eq!(summary_a["kind"], "hub");
        assert_eq!(node(&summary, &b)["kind"], "hub");
        assert_eq!(full_a["hub_refs"], json!([{ "kind": "on", "hub": b }]));
        assert_eq!(summary_a["hub_refs"], json!([]));
        assert_eq!(summary_a["members"], json!([a2[2..].parse::<u64>().unwrap()]));
        assert_eq!(summary_a["keys"], json!(["a", "a2"]));
        assert_eq!(summary_a["in_degree"], 2);
        // Every non-hub type is counted: p1, p2 as user; p3 untyped.
        assert_eq!(node(&summary, "type:user")["count"], 2);
        assert_eq!(node(&summary, "type:untyped")["count"], 1);
    }

    #[test]
    fn a_world_with_no_hub_has_only_type_nodes() {
        let world = fold(
            World::with_hub_cap(CAP),
            &[observe("x", "t"), relate("x", "y")],
        );
        assert_equivalent(&world);
        let summary = json(&world, &SUMMARY);
        assert_eq!(
            summary["nodes"],
            json!([
                { "kind": "type", "id": "type:t", "entity_type": "t", "count": 1 },
                { "kind": "type", "id": "type:untyped", "entity_type": "untyped", "count": 1 },
            ])
        );
    }

    async fn get_raw(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        let res = app.clone().oneshot(request).await.unwrap();
        let (parts, body) = res.into_parts();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        (parts.status, parts.headers, bytes.to_vec())
    }

    fn get(uri: &str, if_none_match: Option<&str>) -> Request<Body> {
        let mut request = Request::get(uri);
        if let Some(tag) = if_none_match {
            request = request.header(header::IF_NONE_MATCH, tag);
        }
        request.body(Body::empty()).unwrap()
    }

    const FULL_URI: &str = "/worlds/default/world?lod=type";
    const SUMMARY_URI: &str = "/worlds/default/world?lod=type&links=none";

    #[test]
    fn the_summary_is_served_with_its_own_tag() {
        run(async {
            let app = app();
            let head = fold(World::with_hub_cap(CAP), &events());
            let (status, headers, body) = get_raw(&app, get(SUMMARY_URI, None)).await;
            assert_eq!(status, StatusCode::OK);
            let expected = serde_json::to_vec(&world_view(&head, &SUMMARY).unwrap()).unwrap();
            assert_eq!(body, expected);
            let summary_tag = headers[header::ETAG].to_str().unwrap().to_owned();
            assert!(summary_tag.ends_with("-1-nolinks\""), "{summary_tag}");

            let (_, headers, full_body) = get_raw(&app, get(FULL_URI, None)).await;
            let full_tag = headers[header::ETAG].to_str().unwrap().to_owned();
            assert_ne!(summary_tag, full_tag);
            assert!(full_tag.ends_with("-type---1\""), "{full_tag}");
            assert_ne!(body, full_body);

            // `links=all` is the default: the same tag and bytes as no `links`.
            let all = format!("{FULL_URI}&links=all");
            let (_, headers, all_body) = get_raw(&app, get(&all, None)).await;
            assert_eq!(headers[header::ETAG], full_tag.as_str());
            assert_eq!(all_body, full_body);
        });
    }

    #[test]
    fn each_tag_answers_304_only_for_its_own_view() {
        run(async {
            let app = app();
            let (_, headers, summary_body) = get_raw(&app, get(SUMMARY_URI, None)).await;
            let summary_tag = headers[header::ETAG].to_str().unwrap().to_owned();
            let (_, headers, full_body) = get_raw(&app, get(FULL_URI, None)).await;
            let full_tag = headers[header::ETAG].to_str().unwrap().to_owned();

            for (uri, tag) in [(SUMMARY_URI, &summary_tag), (FULL_URI, &full_tag)] {
                let (status, headers, got) = get_raw(&app, get(uri, Some(tag))).await;
                assert_eq!(status, StatusCode::NOT_MODIFIED, "{uri}");
                assert_eq!(headers[header::ETAG], tag.as_str());
                assert!(got.is_empty());
            }
            // The summary's tag on a full request gets the full body, and the reverse.
            let (status, headers, got) = get_raw(&app, get(FULL_URI, Some(&summary_tag))).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers[header::ETAG], full_tag.as_str());
            assert_eq!(got, full_body);
            let (status, headers, got) = get_raw(&app, get(SUMMARY_URI, Some(&full_tag))).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers[header::ETAG], summary_tag.as_str());
            assert_eq!(got, summary_body);
        });
    }

    #[test]
    fn links_none_is_refused_outside_the_type_view_without_focus() {
        run(async {
            let app = app();
            let head = fold(World::with_hub_cap(CAP), &events());
            let alice = head.id_of(&NaturalKey::new("user:Alice")).unwrap().get();
            for uri in [
                "/worlds/default/world?links=none".to_owned(),
                "/worlds/default/world?lod=entity&links=none".to_owned(),
                format!("/worlds/default/world?lod=type&focus={alice}&links=none"),
                "/worlds/default/world?lod=type&links=some".to_owned(),
            ] {
                // `*` matches any tag: a refused request must still never answer 304.
                for inm in [None, Some("*")] {
                    let (status, _, body) = get_raw(&app, get(&uri, inm)).await;
                    assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
                    let body: Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(body["error"], "bad_parameter", "{uri}");
                    assert!(
                        body["message"].as_str().unwrap().contains("links"),
                        "{uri}: {body}"
                    );
                }
            }
            // The pure projection refuses the same shapes.
            for params in [
                ViewParams {
                    lod: Lod::Entity,
                    ..SUMMARY
                },
                ViewParams {
                    focus: Some(alice),
                    ..SUMMARY
                },
            ] {
                let error = world_view(&head, &params).unwrap_err();
                assert_eq!(error.json_body()["error"], "bad_parameter");
            }
        });
    }
}
