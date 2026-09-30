//! The dashboard read over HTTP, MCP and the decision service (decision 0029): the route and
//! the MCP tool serve the same bytes, an unknown world is `unknown_world`, and `record_decision`
//! refuses an accept that resolution could never honour.

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use rmcp::ServiceExt;
    use rmcp::model::CallToolRequestParams;
    use s2w_app::mcp::WorldMcp;
    use s2w_app::proposals::{Seat, record_decision};
    use s2w_app::query::{DASHBOARD_MANIFEST_CLASS, QueryError, QueryState, Timeline, router};
    use s2w_app::routes::STREAM_MAPPING_CLASS;
    use s2w_core::DEFAULT_HUB_IN_DEGREE_CAP;
    use s2w_log::{
        Actor, Decider, LogPosition, NewDecision, NewProposal, Outcome, ProposalStore,
        ReadOnlySqliteProposalStore, SqliteProposalStore,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt as _;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const SOURCE: &str = "test.dashboard";
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            loop {
                let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir().join(format!(
                    "s2w-app-dashboard-{label}-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("{error}"),
                }
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn mapping() -> Value {
        json!({ "version": 1, "decode": [],
                "entities": [{ "id": "e", "type_label": "item", "key": [["id"]], "attrs": [] }],
                "relationships": [] })
    }

    fn envelope(manifest: &Value) -> Vec<u8> {
        let error = if manifest.is_null() {
            json!("timeout")
        } else {
            Value::Null
        };
        json!({
            "format": 1, "world": "default", "input_hash": "0123456789abcdef", "attempt": 1,
            "manifest": manifest,
            "provenance": { "prompt_hash": null, "input_tokens": 10, "output_tokens": 20,
                            "latency_ms": 30, "raw": null, "error": error }
        })
        .to_string()
        .into_bytes()
    }

    fn manifest(identity: &str) -> Value {
        json!({
            "built_on": [{ "source": SOURCE, "mapping": identity }],
            "domain": { "name": "A world", "summary": "One sentence about it." },
            "quintessential_projection": { "template": "feed", "rationale": "Activity.", "slots": {} },
            "roles": [{ "id": "r1", "name": "Watcher", "default": true, "questions": [],
                        "projection": { "template": "feed", "slots": {} } }],
            "types": [{ "type": "item", "primary": true }]
        })
    }

    fn propose(
        store: &mut SqliteProposalStore,
        id: &str,
        class: &str,
        payload: Vec<u8>,
    ) -> TestResult {
        store.append_proposal(&NewProposal {
            id: id.to_owned(),
            class: class.to_owned(),
            actor: Actor::Agent {
                model: "m".to_owned(),
                version: "1".to_owned(),
            },
            snapshot_offset: LogPosition::from_u64(1).ok_or("position")?,
            payload,
            proposed_at_ms: 0,
        })?;
        Ok(())
    }

    fn accept(store: &mut SqliteProposalStore, id: &str) -> TestResult {
        store.append_decision(&NewDecision {
            proposal_id: id.to_owned(),
            decider: Decider::Policy,
            outcome: Outcome::Accept,
            basis: "seeded".to_owned(),
            decided_at_ms: 0,
        })?;
        Ok(())
    }

    /// One accepted mapping, one accepted manifest built on it, one null-manifest row.
    fn seed(dir: &Path) -> TestResult {
        let mapping_payload = json!({ "format": 1, "source": SOURCE, "mapping": mapping() });
        let identity = s2w_app::routes::decode_envelope(mapping_payload.to_string().as_bytes())?.2;
        let mut store = SqliteProposalStore::open(dir)?;
        propose(
            &mut store,
            "m",
            STREAM_MAPPING_CLASS,
            mapping_payload.to_string().into_bytes(),
        )?;
        accept(&mut store, "m")?;
        propose(
            &mut store,
            "d",
            DASHBOARD_MANIFEST_CLASS,
            envelope(&manifest(&identity)),
        )?;
        accept(&mut store, "d")?;
        propose(
            &mut store,
            "null",
            DASHBOARD_MANIFEST_CLASS,
            envelope(&Value::Null),
        )?;
        Ok(())
    }

    fn run(f: impl Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(10), f)
                    .await
                    .unwrap();
            });
    }

    async fn http_get(state: QueryState, path: &str) -> (StatusCode, Vec<u8>) {
        let response = router(state)
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, bytes.to_vec())
    }

    #[test]
    fn http_and_mcp_serve_the_same_dashboard_bytes() -> TestResult {
        let dir = TestDirectory::new("parity");
        seed(&dir.0)?;
        let state = QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP)).with_log_dir(&dir.0);
        run(async {
            let (status, http) = http_get(state.clone(), "/worlds/default/dashboard").await;
            assert_eq!(status, StatusCode::OK);
            let view: Value = serde_json::from_slice(&http).unwrap();
            assert_eq!(view["proposal_id"], "d");
            assert_eq!(view["stale"], false);
            assert_eq!(
                view["actor"],
                json!({"kind":"agent","model":"m","version":"1"})
            );
            assert_eq!(view["excluded"][0]["proposal_id"], "null");

            let (status, unknown) = http_get(state.clone(), "/worlds/nope/dashboard").await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            let unknown: Value = serde_json::from_slice(&unknown).unwrap();
            assert_eq!(unknown["error"], "unknown_world");

            let (server_io, client_io) = tokio::io::duplex(1 << 16);
            let server = WorldMcp::new(state);
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
            let result = client
                .call_tool(
                    CallToolRequestParams::new("dashboard")
                        .with_arguments(json!({"world":"default"}).as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(false));
            let text = &result.content[0].as_text().unwrap().text;
            assert_eq!(text.as_bytes(), http.as_slice(), "byte-equal to the route");
            client.cancel().await.unwrap();
            task.await.unwrap();
        });
        Ok(())
    }

    /// [`seed`], with a sentence in the manifest and four logged events, the third without
    /// the key.
    fn seed_sentences(dir: &Path) -> TestResult {
        let mapping_payload = json!({ "format": 1, "source": SOURCE, "mapping": mapping() });
        let identity = s2w_app::routes::decode_envelope(mapping_payload.to_string().as_bytes())?.2;
        let mut store = SqliteProposalStore::open(dir)?;
        propose(
            &mut store,
            "m",
            STREAM_MAPPING_CLASS,
            mapping_payload.to_string().into_bytes(),
        )?;
        accept(&mut store, "m")?;
        let mut with_sentence = manifest(&identity);
        with_sentence["types"][0]["label"] = json!({ "key": 0 });
        with_sentence["events"] = json!([{
            "source": SOURCE,
            "sentence": { "text": "item {0} changed", "fields": [["id"]] }
        }]);
        propose(
            &mut store,
            "d",
            DASHBOARD_MANIFEST_CLASS,
            envelope(&with_sentence),
        )?;
        accept(&mut store, "d")?;
        drop(store);
        let mut log = s2w_log::SqliteEventLog::open(dir)?;
        let payloads = [
            json!({"id": "a1"}),
            json!({"id": "a2"}),
            json!({"other": 1}),
            // Not the first payload again: the log keeps one copy of identical content.
            json!({"id": "a1", "n": 2}),
        ];
        let events = payloads
            .iter()
            .zip(1_u8..)
            .map(|(payload, i)| {
                Ok(s2w_model::RawEvent {
                    source: s2w_model::SourceId::new(SOURCE)?,
                    cursor: s2w_model::Cursor::new(vec![i])?,
                    received_at: s2w_model::Timestamp::from_millis(1_000 + i64::from(i)),
                    payload: payload.to_string().into_bytes(),
                })
            })
            .collect::<Result<Vec<_>, s2w_model::ModelError>>()?;
        s2w_log::EventLog::append_batch(&mut log, events)?;
        Ok(())
    }

    /// One MCP tool call's text over an in-process transport.
    async fn mcp_text(state: QueryState, tool: &'static str, args: Value) -> String {
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        let server = WorldMcp::new(state);
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
        let result = client
            .call_tool(
                CallToolRequestParams::new(tool).with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(false));
        let text = result.content[0].as_text().unwrap().text.clone();
        client.cancel().await.unwrap();
        task.await.unwrap();
        text
    }

    #[test]
    fn sentences_render_the_tail_and_http_and_mcp_serve_the_same_bytes() -> TestResult {
        let dir = TestDirectory::new("sentences");
        seed_sentences(&dir.0)?;
        let state = QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP)).with_log_dir(&dir.0);
        run(async {
            let (status, http) = http_get(state.clone(), "/worlds/default/sentences?last=3").await;
            assert_eq!(status, StatusCode::OK);
            let view: Value = serde_json::from_slice(&http).unwrap();
            let rows = view["rows"].as_array().unwrap();
            let positions: Vec<_> = rows
                .iter()
                .map(|r| r["position"].as_u64().unwrap())
                .collect();
            assert_eq!(positions.len(), 3, "the last 3 of 4: {view}");
            assert!(positions.windows(2).all(|w| w[0] < w[1]), "oldest first");
            let sentences: Vec<_> = rows.iter().map(|r| r["sentence"].clone()).collect();
            assert_eq!(
                sentences,
                vec![
                    json!("item a2 changed"),
                    Value::Null,
                    json!("item a1 changed")
                ]
            );
            assert_eq!(rows[1]["entities"], json!([]));
            let entity = &rows[2]["entities"][0];
            assert_eq!(entity["type"], "item");
            assert_eq!(entity["label"], "a1", "the type row labels by key part 0");
            assert!(
                entity.get("entity").is_none(),
                "the head world is empty: {entity}"
            );
            let key = entity["key"].as_str().unwrap().to_owned();

            // Once the head world holds the key, its id is filled in.
            state
                .append(
                    s2w_model::Timestamp::from_millis(1),
                    s2w_core::WorldEvent::EntityObserved {
                        key: s2w_core::NaturalKey::new(key),
                        entity_type: "item".to_owned(),
                        attrs: std::collections::BTreeMap::new(),
                    },
                )
                .unwrap();
            let (_, http) = http_get(state.clone(), "/worlds/default/sentences?last=3").await;
            let view: Value = serde_json::from_slice(&http).unwrap();
            assert!(view["rows"][2]["entities"][0]["entity"].is_u64(), "{view}");
            assert!(view["rows"][0]["entities"][0].get("entity").is_none());

            for bad in ["last=0", "last=201", "last=x", ""] {
                let (status, body) =
                    http_get(state.clone(), &format!("/worlds/default/sentences?{bad}")).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["error"], "bad_parameter", "{bad}");
            }

            let text = mcp_text(state, "sentences", json!({"world":"default","last":3})).await;
            assert_eq!(text.as_bytes(), http.as_slice(), "byte-equal to the route");
        });
        Ok(())
    }

    #[test]
    fn an_unusable_dashboard_row_can_be_rejected_but_not_accepted() -> TestResult {
        let dir = TestDirectory::new("guard");
        seed(&dir.0)?;
        let mut store = SqliteProposalStore::open(&dir.0)?;
        propose(
            &mut store,
            "junk",
            DASHBOARD_MANIFEST_CLASS,
            b"not json".to_vec(),
        )?;
        drop(store);
        let decisions = || ReadOnlySqliteProposalStore::open(&dir.0).map(|r| r.decisions());
        let before = decisions()??.len();
        let human = Seat::Human {
            reviewer: "dave".to_owned(),
        };
        for (id, expect) in [("null", "manifest is null: timeout"), ("junk", "payload")] {
            for seat in [&human, &Seat::Agent] {
                let error = record_decision(&dir.0, seat, id, Outcome::Accept, "why");
                assert!(
                    matches!(&error, Err(QueryError::BadParameter { name: "proposal", reason }) if reason.contains(expect)),
                    "{id}: {error:?}"
                );
            }
        }
        assert_eq!(decisions()??.len(), before, "a refusal writes nothing");
        record_decision(&dir.0, &human, "null", Outcome::Reject, "why")?;
        record_decision(&dir.0, &human, "d", Outcome::Accept, "why")?;
        assert_eq!(decisions()??.len(), before + 2);
        Ok(())
    }
}
