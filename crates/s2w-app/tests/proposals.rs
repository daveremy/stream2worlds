//! The proposals view over HTTP and MCP, and the opt-in `decision_record` write tool.

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use rmcp::model::{CallToolRequestParams, CallToolResult, ErrorCode};
    use rmcp::service::{RunningService, ServiceError};
    use rmcp::{RoleClient, ServiceExt};
    use s2w_app::mcp::WorldMcp;
    use s2w_app::query::{
        Epoch, GradeDto, ProposalsView, QueryError, QueryState, Timeline, proposals_view, router,
    };
    use s2w_core::DEFAULT_HUB_IN_DEGREE_CAP;
    use s2w_log::{
        Actor, AppendOutcome, Decider, EventLog, InMemoryEventLog, LogPosition, NewDecision,
        NewProposal, Outcome, ProposalStore, ReadOnlySqliteProposalStore, SqliteProposalStore,
        grade,
    };
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
    use serde_json::{Value, json};
    use tower::ServiceExt as _;

    const STORE_FILE: &str = "proposals.sqlite3";
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            loop {
                let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir().join(format!(
                    "s2w-app-proposals-{label}-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("{error}"),
                }
            }
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn store_file(&self) -> PathBuf {
            self.0.join(STORE_FILE)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    /// A log position; there is no public constructor, so one comes from a real append.
    fn position() -> LogPosition {
        let mut log = InMemoryEventLog::new();
        let event = RawEvent {
            source: SourceId::new("test.proposals").unwrap(),
            cursor: Cursor::new(vec![1]).unwrap(),
            received_at: Timestamp::from_millis(1_000),
            payload: vec![1],
        };
        match log.append(event).unwrap() {
            AppendOutcome::Inserted(position) => position,
            other => panic!("{other:?}"),
        }
    }

    fn human(id: &str) -> Actor {
        Actor::Human { id: id.to_owned() }
    }

    fn agent(version: &str) -> Actor {
        Actor::Agent {
            model: "model-a".to_owned(),
            version: version.to_owned(),
        }
    }

    fn proposal(id: &str, class: &str, actor: Actor) -> NewProposal {
        NewProposal {
            id: id.to_owned(),
            class: class.to_owned(),
            actor,
            snapshot_offset: position(),
            payload: format!("payload-{id}").into_bytes(),
            proposed_at_ms: 5_000,
        }
    }

    fn decision(id: &str, decider: Decider, outcome: Outcome) -> NewDecision {
        NewDecision {
            proposal_id: id.to_owned(),
            decider,
            outcome,
            basis: format!("basis-{id}"),
            decided_at_ms: 6_000,
        }
    }

    /// Several classes and actors (two agent versions), with a human correction and every
    /// decider kind. Returns the open writer so a test can keep holding its lock.
    fn seed(dir: &Path) -> SqliteProposalStore {
        let mut store = SqliteProposalStore::open(dir).unwrap();
        for new in [
            proposal("p1", "class-x", agent("1")),
            proposal("p2", "class-x", agent("1")),
            proposal("p3", "class-x", agent("2")),
            proposal("p4", "class-y", human("h")),
            proposal("p5", "class-y", agent("1")),
        ] {
            store.append_proposal(&new).unwrap();
        }
        for new in [
            decision("p1", Decider::Policy, Outcome::Accept),
            decision("p1", Decider::Human, Outcome::Accept),
            decision("p1", Decider::Human, Outcome::Reject),
            decision("p2", Decider::Evidence, Outcome::Accept),
            decision("p3", Decider::Agent, Outcome::Accept),
            decision("p4", Decider::Policy, Outcome::Reject),
            decision("p5", Decider::Human, Outcome::Accept),
        ] {
            store.append_decision(&new).unwrap();
        }
        store
    }

    fn state_at(dir: &Path) -> QueryState {
        QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP)).with_log_dir(dir)
    }

    #[test]
    fn view_grades_equal_log_grade_over_the_stored_rows() {
        let dir = TestDirectory::new("grades");
        drop(seed(dir.path()));
        let view = state_at(dir.path()).proposals().unwrap();
        let reader = ReadOnlySqliteProposalStore::open(dir.path()).unwrap();
        let summaries = reader.proposal_summaries().unwrap();
        let decisions = reader.decisions().unwrap();
        assert_eq!(view, proposals_view(&summaries, &decisions));
        let expected: Vec<GradeDto> = grade(&summaries, &decisions)
            .iter()
            .map(GradeDto::from)
            .collect();
        assert_eq!(view.grades, expected);
        assert_eq!(view.proposals.len(), 5);
        assert_eq!(view.decisions.len(), 7);
        let json = serde_json::to_value(&view).unwrap();
        assert!(json["proposals"][0].get("payload").is_none(), "{json}");
        assert_eq!(json["proposals"][0]["actor"]["kind"], "agent");
        assert_eq!(json["decisions"][2]["decider"], "human");
        assert_eq!(json["decisions"][2]["outcome"], "reject");
        let first = &json["grades"][0];
        assert_eq!(first["class"], "class-x");
        // p1's latest human review is the correction (reject); p2 has evidence only.
        assert_eq!(
            first["human"],
            json!({"accepted":0,"rejected":1,"fraction":[0,1]})
        );
        assert_eq!(first["policy_applied"]["fraction"], json!([0, 1]));
    }

    #[test]
    fn absent_store_is_an_empty_view_and_creates_no_file() {
        let dir = TestDirectory::new("absent");
        assert_eq!(
            state_at(dir.path()).proposals(),
            Ok(ProposalsView::default())
        );
        assert!(!dir.store_file().exists());
        let no_dir = QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP));
        assert_eq!(no_dir.proposals(), Ok(ProposalsView::default()));
    }

    #[test]
    fn unreadable_store_is_a_storage_error_never_an_empty_view() {
        let dir = TestDirectory::new("corrupt");
        fs::write(dir.store_file(), b"this is not a sqlite database at all").unwrap();
        let error = state_at(dir.path()).proposals().unwrap_err();
        assert!(matches!(error, QueryError::Storage(_)), "{error:?}");
        assert_eq!(error.code(), "storage");
    }

    #[test]
    fn view_reads_while_a_writer_holds_the_lock() {
        let dir = TestDirectory::new("locked-read");
        let writer = seed(dir.path());
        let view = state_at(dir.path()).proposals().unwrap();
        assert_eq!(view.proposals.len(), 5);
        drop(writer);
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

    async fn http_get(state: QueryState, path: &str) -> (StatusCode, Value) {
        let response = router(state)
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[test]
    fn http_route_serves_the_view_and_404s_an_unknown_world() {
        let dir = TestDirectory::new("http");
        drop(seed(dir.path()));
        let state = state_at(dir.path());
        run(async {
            let (status, body) = http_get(state.clone(), "/worlds/default/proposals").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                body,
                serde_json::to_value(state.proposals().unwrap()).unwrap()
            );
            let (status, body) = http_get(state, "/worlds/other/proposals").await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(body["error"], "unknown_world");
        });
    }

    /// s2w#201: a proposal's `snapshot_offset` is an event-log position, not a fold offset, so
    /// it carries no epoch. A rebuild (another epoch over the same log) serves the same bytes.
    #[test]
    fn snapshot_offset_is_a_log_position_with_no_epoch_and_survives_a_rebuild() {
        let dir = TestDirectory::new("epoch-free");
        drop(seed(dir.path()));
        let under = |epoch: u64| {
            QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP).with_epoch(Epoch(epoch)))
                .with_log_dir(dir.path())
        };
        run(async {
            let (_, time_a) = http_get(under(0xa), "/worlds/default/time").await;
            let (_, time_b) = http_get(under(0xb), "/worlds/default/time").await;
            assert_ne!(time_a["epoch"], time_b["epoch"], "two histories");
            let (status, a) = http_get(under(0xa), "/worlds/default/proposals").await;
            assert_eq!(status, StatusCode::OK);
            let (_, b) = http_get(under(0xb), "/worlds/default/proposals").await;
            assert_eq!(
                a, b,
                "the proposals view does not depend on the served epoch"
            );
            // Sets, not Vecs: the assertion is about which keys exist, not serde_json's map order.
            let keys_of = |v: &Value| -> std::collections::BTreeSet<String> {
                v.as_object().unwrap().keys().cloned().collect()
            };
            let set = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect();
            assert_eq!(keys_of(&a), set(&["proposals", "decisions", "grades"]));
            assert_eq!(
                keys_of(&a["proposals"][0]),
                set(&[
                    "seq",
                    "id",
                    "class",
                    "actor",
                    "snapshot_offset",
                    "payload_hash",
                    "proposed_at_ms"
                ]),
                "no epoch next to snapshot_offset"
            );
            assert_eq!(a["proposals"][0]["snapshot_offset"], position().as_u64());
        });
    }

    type Client = RunningService<RoleClient, ()>;

    async fn connect(server: WorldMcp) -> (Client, tokio::task::JoinHandle<()>) {
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        let task = tokio::spawn(async move {
            server
                .serve(server_io)
                .await
                .unwrap()
                .waiting()
                .await
                .unwrap();
        });
        (().serve(client_io).await.unwrap(), task)
    }

    async fn call(client: &Client, tool: &str, args: &Value) -> CallToolResult {
        client
            .call_tool(
                CallToolRequestParams::new(tool.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap()
    }

    fn body(result: &CallToolResult) -> Value {
        serde_json::from_str(&result.content[0].as_text().unwrap().text).unwrap()
    }

    fn record_args(proposal_id: &str, basis: &str) -> Value {
        json!({"world":"default","proposal_id":proposal_id,"outcome":"reject","basis":basis})
    }

    #[test]
    fn decision_record_appends_one_agent_row_and_leaves_proposals_untouched() {
        let dir = TestDirectory::new("record");
        drop(seed(dir.path()));
        let state = state_at(dir.path());
        let before_view = state.proposals().unwrap();
        let reader = ReadOnlySqliteProposalStore::open(dir.path()).unwrap();
        let before_rows = reader.proposals().unwrap();
        let before_decisions = reader.decisions().unwrap();
        run(async {
            let (client, task) = connect(WorldMcp::new(state.clone()).with_decisions()).await;
            let result = call(&client, "decision_record", &record_args("p5", "why")).await;
            assert_eq!(result.is_error, Some(false), "{result:?}");
            let stored = body(&result);
            assert_eq!(stored["decider"], "agent");
            assert_eq!(stored["outcome"], "reject");
            assert_eq!(stored["proposal_id"], "p5");
            let listed = call(&client, "proposals_list", &json!({"world":"default"})).await;
            assert_eq!(
                body(&listed),
                serde_json::to_value(state.proposals().unwrap()).unwrap()
            );
            client.cancel().await.unwrap();
            task.await.unwrap();
        });
        assert_eq!(reader.proposals().unwrap(), before_rows);
        let after = reader.decisions().unwrap();
        assert_eq!(after.len(), before_decisions.len() + 1);
        assert_eq!(after[..before_decisions.len()], before_decisions[..]);
        assert_eq!(after.last().unwrap().decider, Decider::Agent);
        let after_view = state.proposals().unwrap();
        assert_eq!(after_view.proposals, before_view.proposals);
        for (old, new) in before_view.grades.iter().zip(&after_view.grades) {
            assert_eq!(old.human, new.human);
            assert_eq!(old.evidence, new.evidence);
            assert_eq!(old.policy_applied, new.policy_applied);
        }
        let class_y_agent = after_view.grades.last().unwrap();
        assert_eq!(class_y_agent.agent.rejected, 1);
    }

    #[test]
    fn decision_record_refuses_unknown_proposals_without_creating_a_store() {
        let seeded = TestDirectory::new("unknown-seeded");
        drop(seed(seeded.path()));
        let empty = TestDirectory::new("unknown-empty");
        run(async {
            for dir in [&seeded, &empty] {
                let server = WorldMcp::new(state_at(dir.path())).with_decisions();
                let (client, task) = connect(server).await;
                let result = call(&client, "decision_record", &record_args("nope", "why")).await;
                assert_eq!(result.is_error, Some(true));
                assert_eq!(body(&result)["error"], "unknown_proposal");
                client.cancel().await.unwrap();
                task.await.unwrap();
            }
        });
        assert!(!empty.store_file().exists());
        assert!(fs::read_dir(empty.path()).unwrap().next().is_none());
        let reader = ReadOnlySqliteProposalStore::open(seeded.path()).unwrap();
        assert_eq!(reader.decisions().unwrap().len(), 7);
    }

    #[test]
    fn decision_record_reports_a_locked_store_and_rejects_an_empty_basis() {
        let dir = TestDirectory::new("locked-write");
        let writer = seed(dir.path());
        run(async {
            let server = WorldMcp::new(state_at(dir.path())).with_decisions();
            let (client, task) = connect(server).await;
            let result = call(&client, "decision_record", &record_args("p1", "why")).await;
            assert_eq!(result.is_error, Some(true));
            assert_eq!(body(&result)["error"], "store_locked");
            let result = call(&client, "decision_record", &record_args("p1", "  ")).await;
            assert_eq!(result.is_error, Some(true));
            assert_eq!(body(&result)["error"], "bad_parameter");
            client.cancel().await.unwrap();
            task.await.unwrap();
        });
        assert_eq!(writer.decisions().unwrap().len(), 7);
    }

    #[test]
    fn decision_record_rejects_a_blank_proposal_id_before_consulting_the_store() {
        let dir = TestDirectory::new("blank-id");
        run(async {
            let server = WorldMcp::new(state_at(dir.path())).with_decisions();
            let (client, task) = connect(server).await;
            for id in ["", " \t"] {
                let result = call(&client, "decision_record", &record_args(id, "why")).await;
                assert_eq!(result.is_error, Some(true));
                let error = body(&result);
                assert_eq!(error["error"], "bad_parameter", "{error}");
            }
            client.cancel().await.unwrap();
            task.await.unwrap();
        });
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn decision_record_does_not_exist_without_the_option() {
        let dir = TestDirectory::new("absent-tool");
        drop(seed(dir.path()));
        run(async {
            let (client, task) = connect(WorldMcp::new(state_at(dir.path()))).await;
            let error = client
                .call_tool(
                    CallToolRequestParams::new("decision_record")
                        .with_arguments(record_args("p1", "why").as_object().unwrap().clone()),
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
        let reader = ReadOnlySqliteProposalStore::open(dir.path()).unwrap();
        assert_eq!(reader.decisions().unwrap().len(), 7);
    }
}
