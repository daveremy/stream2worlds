//! MCP tools preserve the HTTP query contract, including exact JSON bytes and errors.

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::Request;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::CallToolResult;
    use s2w_app::mcp::WorldMcp;
    use s2w_app::query::{QueryState, Timeline, router};
    use s2w_core::{DEFAULT_HUB_IN_DEGREE_CAP, NaturalKey, WorldEvent};
    use s2w_model::Timestamp;
    use serde_json::{Value, json};
    use tower::ServiceExt as _;

    fn run(f: impl Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(10), f)
                    .await
                    .unwrap()
            });
    }

    fn golden() -> QueryState {
        let events: Vec<WorldEvent> = serde_json::from_str(include_str!(
            "../../s2w-core/tests/fixtures/golden-fold-v1.json"
        ))
        .unwrap();
        let mut timeline = Timeline::new(3);
        for (i, event) in events.into_iter().enumerate() {
            timeline.append(
                Timestamp::from_millis(i64::try_from(i).unwrap() * 1000),
                event,
            );
        }
        QueryState::new(timeline)
    }

    fn empty() -> QueryState {
        QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP))
    }

    fn text(result: &CallToolResult) -> &str {
        assert!(result.structured_content.is_none());
        assert_eq!(result.content.len(), 1);
        &result.content[0].as_text().unwrap().text
    }

    // Direct tool calls exercise typed handlers without needing a protocol request context.
    fn call(server: &WorldMcp, tool: &str, args: Value) -> CallToolResult {
        match tool {
            "world_view" => server.world_view(Parameters(serde_json::from_value(args).unwrap())),
            "world_diff" => server.world_diff(Parameters(serde_json::from_value(args).unwrap())),
            "entity_history" => {
                server.entity_history(Parameters(serde_json::from_value(args).unwrap()))
            }
            "branches" => server.branches(Parameters(serde_json::from_value(args).unwrap())),
            "sources" => server.sources(Parameters(serde_json::from_value(args).unwrap())),
            "time" => server.time(Parameters(serde_json::from_value(args).unwrap())),
            _ => panic!("unexpected tool {tool}"),
        }
    }

    async fn assert_http(
        state: &QueryState,
        uri: &str,
        result: &CallToolResult,
        error: Option<&str>,
    ) {
        let response = router(state.clone())
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().is_success(), error.is_none(), "{uri}");
        assert_eq!(result.is_error.unwrap_or(false), error.is_some(), "{uri}");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(text(result).as_bytes(), bytes.as_ref(), "{uri}");
        if let Some(code) = error {
            assert_eq!(
                serde_json::from_str::<Value>(text(result)).unwrap()["error"],
                code
            );
        }
    }

    #[test]
    fn golden_tools_match_http_bytes() {
        run(async {
            let state = golden();
            let server = WorldMcp::new(state.clone());
            let id = state
                .world_at(None)
                .unwrap()
                .id_of(&NaturalKey::new("user:Alice"))
                .unwrap()
                .get();
            for (tool, uri, args) in [
                (
                    "world_view",
                    "/worlds/default/world".to_owned(),
                    json!({"world":"default"}),
                ),
                (
                    "world_view",
                    "/worlds/default/world?at=14&branch=actual&lod=type".to_owned(),
                    json!({"world":"default","at":14,"branch":"actual","lod":"type"}),
                ),
                (
                    "world_view",
                    format!("/worlds/default/world?focus={id}&hops=2"),
                    json!({"world":"default","focus":id,"hops":2}),
                ),
                (
                    "world_diff",
                    "/worlds/default/diff?from=12&to=14".to_owned(),
                    json!({"world":"default","from":12,"to":14}),
                ),
                (
                    "entity_history",
                    format!("/worlds/default/entity/{id}/history?to=14"),
                    json!({"world":"default","id":id,"to":14}),
                ),
                (
                    "branches",
                    "/worlds/default/branches".to_owned(),
                    json!({"world":"default"}),
                ),
                (
                    "time",
                    "/worlds/default/time".to_owned(),
                    json!({"world":"default"}),
                ),
                (
                    "time",
                    "/worlds/default/time?ts=12500".to_owned(),
                    json!({"world":"default","ts":12500}),
                ),
                (
                    "time",
                    "/worlds/default/time?ts=-1".to_owned(),
                    json!({"world":"default","ts":-1}),
                ),
            ] {
                assert_http(&state, &uri, &call(&server, tool, args), None).await;
            }
        });
    }

    #[test]
    fn every_tool_matches_http_on_an_empty_timeline() {
        run(async {
            let state = empty();
            let server = WorldMcp::new(state.clone());
            for (tool, uri, args, error) in [
                (
                    "world_view",
                    "/worlds/default/world",
                    json!({"world":"default"}),
                    None,
                ),
                (
                    "world_diff",
                    "/worlds/default/diff",
                    json!({"world":"default"}),
                    None,
                ),
                (
                    "entity_history",
                    "/worlds/default/entity/1/history",
                    json!({"world":"default","id":1}),
                    Some("unknown_entity"),
                ),
                (
                    "branches",
                    "/worlds/default/branches",
                    json!({"world":"default"}),
                    None,
                ),
                (
                    "time",
                    "/worlds/default/time",
                    json!({"world":"default"}),
                    None,
                ),
                (
                    "time",
                    "/worlds/default/time?ts=0",
                    json!({"world":"default","ts":0}),
                    None,
                ),
            ] {
                assert_http(&state, uri, &call(&server, tool, args), error).await;
            }
        });
    }

    /// The type summary (s2w#296): `links` has the route's default and values.
    #[test]
    fn world_view_links_match_http_bytes() {
        run(async {
            let state = golden();
            let server = WorldMcp::new(state.clone());
            for (uri, args) in [
                (
                    "/worlds/default/world?lod=type&links=none",
                    json!({"world":"default","lod":"type","links":"none"}),
                ),
                (
                    "/worlds/default/world?at=14&lod=type&links=none",
                    json!({"world":"default","at":14,"lod":"type","links":"none"}),
                ),
                (
                    "/worlds/default/world?at=14&lod=type&links=all",
                    json!({"world":"default","at":14,"lod":"type","links":"all"}),
                ),
            ] {
                let result = call(&server, "world_view", args);
                assert_http(&state, uri, &result, None).await;
            }
        });
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    fn domain_errors_match_http_bytes() {
        run(async {
            let state = golden();
            let server = WorldMcp::new(state.clone());
            for (tool, uri, args, code) in [
                (
                    "world_view",
                    "/worlds/default/world?at=999",
                    json!({"world":"default","at":999}),
                    "offset_beyond_head",
                ),
                (
                    "world_view",
                    "/worlds/default/world?focus=999",
                    json!({"world":"default","focus":999}),
                    "unknown_entity",
                ),
                (
                    "world_view",
                    "/worlds/default/world?focus=1&hops=6",
                    json!({"world":"default","focus":1,"hops":6}),
                    "hops_too_large",
                ),
                (
                    "world_view",
                    "/worlds/default/world?lod=cluster",
                    json!({"world":"default","lod":"cluster"}),
                    "lod_not_yet",
                ),
                (
                    "world_view",
                    "/worlds/default/world?lod=invalid",
                    json!({"world":"default","lod":"invalid"}),
                    "bad_parameter",
                ),
                (
                    "world_view",
                    "/worlds/default/world?links=none",
                    json!({"world":"default","links":"none"}),
                    "bad_parameter",
                ),
                (
                    "world_view",
                    "/worlds/default/world?lod=entity&links=none",
                    json!({"world":"default","lod":"entity","links":"none"}),
                    "bad_parameter",
                ),
                (
                    "world_view",
                    "/worlds/default/world?lod=type&focus=1&links=none",
                    json!({"world":"default","lod":"type","focus":1,"links":"none"}),
                    "bad_parameter",
                ),
                (
                    "world_view",
                    "/worlds/default/world?lod=type&links=some",
                    json!({"world":"default","lod":"type","links":"some"}),
                    "bad_parameter",
                ),
                (
                    "world_view",
                    "/worlds/default/world?branch=future",
                    json!({"world":"default","branch":"future"}),
                    "branch_not_yet",
                ),
                (
                    "world_diff",
                    "/worlds/default/diff?from=999",
                    json!({"world":"default","from":999}),
                    "offset_beyond_head",
                ),
                (
                    "world_diff",
                    "/worlds/default/diff?branch=future",
                    json!({"world":"default","branch":"future"}),
                    "branch_not_yet",
                ),
                (
                    "entity_history",
                    "/worlds/default/entity/999/history",
                    json!({"world":"default","id":999}),
                    "unknown_entity",
                ),
                (
                    "entity_history",
                    "/worlds/default/entity/1/history?branch=future",
                    json!({"world":"default","id":1,"branch":"future"}),
                    "branch_not_yet",
                ),
                (
                    "time",
                    "/worlds/default/time?branch=future",
                    json!({"world":"default","branch":"future"}),
                    "branch_not_yet",
                ),
                // An epoch that is not the served one (a fresh timeline serves 0).
                (
                    "world_view",
                    "/worlds/default/world?at=3&epoch=1111111111111111",
                    json!({"world":"default","at":3,"epoch":"1111111111111111"}),
                    "stale_epoch",
                ),
                (
                    "world_diff",
                    "/worlds/default/diff?from=0&to=3&epoch=1111111111111111",
                    json!({"world":"default","from":0,"to":3,"epoch":"1111111111111111"}),
                    "stale_epoch",
                ),
                (
                    "entity_history",
                    "/worlds/default/entity/1/history?epoch=1111111111111111",
                    json!({"world":"default","id":1,"epoch":"1111111111111111"}),
                    "stale_epoch",
                ),
                (
                    "time",
                    "/worlds/default/time?epoch=1111111111111111",
                    json!({"world":"default","epoch":"1111111111111111"}),
                    "stale_epoch",
                ),
                (
                    "sources",
                    "/worlds/default/sources?epoch=1111111111111111",
                    json!({"world":"default","epoch":"1111111111111111"}),
                    "stale_epoch",
                ),
                (
                    "world_view",
                    "/worlds/default/world?epoch=zz",
                    json!({"world":"default","epoch":"zz"}),
                    "bad_parameter",
                ),
            ] {
                assert_http(&state, uri, &call(&server, tool, args), Some(code)).await;
            }
        });
    }

    #[test]
    fn wrong_world_is_an_error_for_every_tool() {
        let server = WorldMcp::new(golden());
        for (tool, args) in [
            ("world_view", json!({"world":"nope"})),
            ("world_diff", json!({"world":"nope"})),
            ("entity_history", json!({"world":"nope","id":1})),
            ("branches", json!({"world":"nope"})),
            ("sources", json!({"world":"nope"})),
            ("time", json!({"world":"nope"})),
        ] {
            let result = call(&server, tool, args);
            assert_eq!(result.is_error, Some(true), "{tool}");
            assert_eq!(
                serde_json::from_str::<Value>(text(&result)).unwrap()["error"],
                "unknown_world",
                "{tool}"
            );
        }
    }
}
