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
            "branches" => server.branches(),
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
                ("world_view", "/world".to_owned(), json!({})),
                (
                    "world_view",
                    "/world?at=14&branch=actual&lod=type".to_owned(),
                    json!({"at":14,"branch":"actual","lod":"type"}),
                ),
                (
                    "world_view",
                    format!("/world?focus={id}&hops=2"),
                    json!({"focus":id,"hops":2}),
                ),
                (
                    "world_diff",
                    "/diff?from=12&to=14".to_owned(),
                    json!({"from":12,"to":14}),
                ),
                (
                    "entity_history",
                    format!("/entity/{id}/history?to=14"),
                    json!({"id":id,"to":14}),
                ),
                ("branches", "/branches".to_owned(), json!({})),
                ("time", "/time".to_owned(), json!({})),
                ("time", "/time?ts=12500".to_owned(), json!({"ts":12500})),
                ("time", "/time?ts=-1".to_owned(), json!({"ts":-1})),
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
                ("world_view", "/world", json!({}), None),
                ("world_diff", "/diff", json!({}), None),
                (
                    "entity_history",
                    "/entity/1/history",
                    json!({"id":1}),
                    Some("unknown_entity"),
                ),
                ("branches", "/branches", json!({}), None),
                ("time", "/time", json!({}), None),
                ("time", "/time?ts=0", json!({"ts":0}), None),
            ] {
                assert_http(&state, uri, &call(&server, tool, args), error).await;
            }
        });
    }

    #[test]
    fn domain_errors_match_http_bytes() {
        run(async {
            let state = golden();
            let server = WorldMcp::new(state.clone());
            for (tool, uri, args, code) in [
                (
                    "world_view",
                    "/world?at=999",
                    json!({"at":999}),
                    "offset_beyond_head",
                ),
                (
                    "world_view",
                    "/world?focus=999",
                    json!({"focus":999}),
                    "unknown_entity",
                ),
                (
                    "world_view",
                    "/world?focus=1&hops=6",
                    json!({"focus":1,"hops":6}),
                    "hops_too_large",
                ),
                (
                    "world_view",
                    "/world?lod=cluster",
                    json!({"lod":"cluster"}),
                    "lod_not_yet",
                ),
                (
                    "world_view",
                    "/world?lod=invalid",
                    json!({"lod":"invalid"}),
                    "bad_parameter",
                ),
                (
                    "world_view",
                    "/world?branch=future",
                    json!({"branch":"future"}),
                    "branch_not_yet",
                ),
                (
                    "world_diff",
                    "/diff?from=999",
                    json!({"from":999}),
                    "offset_beyond_head",
                ),
                (
                    "world_diff",
                    "/diff?branch=future",
                    json!({"branch":"future"}),
                    "branch_not_yet",
                ),
                (
                    "entity_history",
                    "/entity/999/history",
                    json!({"id":999}),
                    "unknown_entity",
                ),
                (
                    "entity_history",
                    "/entity/1/history?branch=future",
                    json!({"id":1,"branch":"future"}),
                    "branch_not_yet",
                ),
                (
                    "time",
                    "/time?branch=future",
                    json!({"branch":"future"}),
                    "branch_not_yet",
                ),
            ] {
                assert_http(&state, uri, &call(&server, tool, args), Some(code)).await;
            }
        });
    }
}
