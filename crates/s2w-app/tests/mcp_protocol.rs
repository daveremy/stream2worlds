//! A full MCP handshake, tool discovery and calls over the stdio JSON codec.

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::time::Duration;

    use rmcp::ServiceExt;
    use rmcp::model::{CallToolRequestParams, CallToolResult, ErrorCode};
    use rmcp::service::ServiceError;
    use s2w_app::mcp::WorldMcp;
    use s2w_app::query::{QueryState, Timeline};
    use s2w_core::DEFAULT_HUB_IN_DEGREE_CAP;
    use serde_json::json;

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

    fn text(result: &CallToolResult) -> &str {
        assert!(result.structured_content.is_none());
        assert_eq!(result.content.len(), 1);
        &result.content[0].as_text().unwrap().text
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
    )]
    fn stdio_round_trip_advertises_read_only_tools_and_splits_errors() {
        run(async {
            let state = QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP));
            let server = WorldMcp::new(state.clone());
            // Duplex uses the same newline-delimited JSON transport as stdin/stdout.
            let (server_io, client_io) = tokio::io::duplex(4096);
            let task = tokio::spawn(async move {
                server
                    .serve(server_io)
                    .await
                    .unwrap()
                    .waiting()
                    .await
                    .unwrap();
            });
            // rmcp's client sends initialize and notifications/initialized before returning.
            let client = ().serve(client_io).await.unwrap();
            let tools = client.list_all_tools().await.unwrap();
            let names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
            assert_eq!(
                names,
                [
                    "branches",
                    "dashboard",
                    "entity_history",
                    "proposals_list",
                    "sources",
                    "time",
                    "world_diff",
                    "world_view"
                ]
            );
            for tool in tools {
                let requires_world = tool
                    .input_schema
                    .get("required")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|required| required.iter().any(|name| name == "world"));
                assert!(
                    requires_world,
                    "{} schema does not require world",
                    tool.name
                );
                assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
            }
            let result = client
                .call_tool(
                    CallToolRequestParams::new("branches")
                        .with_arguments(json!({"world":"default"}).as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(false));
            assert_eq!(
                text(&result),
                serde_json::to_string(&state.branches().unwrap()).unwrap()
            );
            let result = client
                .call_tool(
                    CallToolRequestParams::new("entity_history").with_arguments(
                        json!({"world":"default","id":1})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                )
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(true));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(text(&result)).unwrap()["error"],
                "unknown_entity"
            );
            for (tool, args) in [
                ("entity_history", json!({"world":"default"})),
                ("world_view", json!({"world":"default","at":"oops"})),
            ] {
                let error = client
                    .call_tool(
                        CallToolRequestParams::new(tool)
                            .with_arguments(args.as_object().unwrap().clone()),
                    )
                    .await
                    .unwrap_err();
                assert!(
                    matches!(error, ServiceError::McpError(ref data) if data.code == ErrorCode::INVALID_PARAMS),
                    "{error:?}"
                );
            }
            client.cancel().await.unwrap();
            task.await.unwrap();
        });
    }

    #[test]
    fn allowing_decisions_adds_exactly_decision_record_as_the_one_write_tool() {
        run(async {
            let state = QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP));
            let server = WorldMcp::new(state).with_decisions();
            let (server_io, client_io) = tokio::io::duplex(4096);
            let task = tokio::spawn(async move {
                server
                    .serve(server_io)
                    .await
                    .unwrap()
                    .waiting()
                    .await
                    .unwrap();
            });
            let client = ().serve(client_io).await.unwrap();
            let tools = client.list_all_tools().await.unwrap();
            let names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
            assert_eq!(
                names,
                [
                    "branches",
                    "dashboard",
                    "decision_record",
                    "entity_history",
                    "proposals_list",
                    "sources",
                    "time",
                    "world_diff",
                    "world_view"
                ]
            );
            for tool in tools {
                let requires_world = tool
                    .input_schema
                    .get("required")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|required| required.iter().any(|name| name == "world"));
                assert!(
                    requires_world,
                    "{} schema does not require world",
                    tool.name
                );
                let annotations = tool.annotations.unwrap();
                let writes = tool.name == "decision_record";
                assert_eq!(annotations.read_only_hint, Some(!writes), "{}", tool.name);
                if writes {
                    assert_eq!(annotations.destructive_hint, Some(false));
                }
            }
            client.cancel().await.unwrap();
            task.await.unwrap();
        });
    }

    #[test]
    fn omitting_only_world_is_rejected_at_the_protocol_level() {
        run(async {
            let server = WorldMcp::new(QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP)));
            let (server_io, client_io) = tokio::io::duplex(4096);
            let task = tokio::spawn(async move {
                server
                    .serve(server_io)
                    .await
                    .unwrap()
                    .waiting()
                    .await
                    .unwrap();
            });
            let client = ().serve(client_io).await.unwrap();
            let error = client
                .call_tool(
                    CallToolRequestParams::new("entity_history")
                        .with_arguments(json!({"id":1}).as_object().unwrap().clone()),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, ServiceError::McpError(ref data) if data.code == ErrorCode::INVALID_PARAMS),
                "{error:?}"
            );
            client.cancel().await.unwrap();
            task.await.unwrap();
        });
    }
}
