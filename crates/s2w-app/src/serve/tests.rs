use super::*;
use crate::query::Timeline;
use crate::tests::{TestDirectory, run};
use axum::{Router, body::Body, routing::get};
use s2w_model::Timestamp;
use tower::ServiceExt;

fn state() -> QueryState {
    QueryState::new(Timeline::new(crate::DEFAULT_HUB_IN_DEGREE_CAP))
}

#[test]
fn both_writer_locks_map_to_usage_and_release() {
    let dir = TestDirectory::new("serve-locks");
    let args = ServeArgs {
        uri: "-".to_owned(),
        world: "default".to_owned(),
        log_dir: dir.path().to_owned(),
        port: 0,
        wiki: None,
    };
    let log = SqliteEventLog::open(dir.path()).expect("first event log opens");
    let error = run_serve(state(), args.clone()).expect_err("second event log must fail");
    assert!(
        matches!(error, AppError::Usage(ref m) if m.contains("the event log at") && m.contains("only once per --log-dir"))
    );
    drop(log);
    let log = SqliteEventLog::open(dir.path()).expect("event lock released");
    drop(log);
    let verdicts = SqliteVerdictStore::open(dir.path()).expect("first verdict store opens");
    let error = run_serve(state(), args).expect_err("second verdict store must fail");
    assert!(
        matches!(error, AppError::Usage(ref m) if m.contains("the verdict store at") && m.contains("only once per --log-dir"))
    );
    drop(verdicts);
    SqliteVerdictStore::open(dir.path()).expect("verdict lock released");
    SqliteEventLog::open(dir.path()).expect("failed startup released event lock too");
}

#[test]
fn host_middleware_accepts_only_literal_loopback_hosts() {
    run(false, async {
        let app = Router::new()
            .route("/", get(|| async { StatusCode::NO_CONTENT }))
            .layer(middleware::from_fn(host_allowlist));
        for (host, valid) in [
            ("localhost", true),
            ("localhost:4310", true),
            ("127.0.0.1", true),
            ("127.0.0.1:0", true),
            ("[::1]", true),
            ("[::1]:65535", true),
            ("evil.example", false),
            ("localhost.evil.example", false),
            ("127.0.0.1.evil", false),
            ("localhost@evil", false),
            ("LOCALHOST", false),
            ("localhost:", false),
            ("localhost:abc", false),
            ("localhost:+1", false),
            ("localhost:65536", false),
            ("::1", false),
            ("[::1]:80:90", false),
            ("127.1", false),
            ("localhost,evil", false),
            ("", false),
        ] {
            let request = Request::builder()
                .uri("/")
                .header(HOST, host)
                .body(Body::empty())
                .expect("request");
            let response = app
                .clone()
                .oneshot(request)
                .await
                .expect("middleware response");
            assert_eq!(
                response.status(),
                if valid {
                    StatusCode::NO_CONTENT
                } else {
                    StatusCode::BAD_REQUEST
                },
                "{host}"
            );
        }
        for headers in [
            vec![],
            vec!["localhost", "evil"],
            vec!["localhost", "localhost"],
        ] {
            let mut request = Request::builder().uri("/");
            for host in headers {
                request = request.header(HOST, host);
            }
            assert_eq!(
                app.clone()
                    .oneshot(request.body(Body::empty()).expect("request"))
                    .await
                    .expect("response")
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
    });
}

#[test]
fn early_bridge_exit_is_fatal_and_signals_http_shutdown() {
    run(false, async {
        for result in [
            Ok(()),
            Err(BridgeError::Task("injected failure".to_owned())),
        ] {
            let (tx, rx) = watch::channel(false);
            let drained = std::cell::Cell::new(false);
            let outcome = supervise(
                std::future::pending(),
                async { result },
                async {
                    shutdown_signal(rx).await;
                    drained.set(true);
                    Ok(())
                },
                std::future::pending(),
                tx,
            )
            .await;
            assert!(matches!(outcome, Err(AppError::BridgeStopped(_))));
            assert!(drained.get(), "HTTP must observe shutdown before returning");
        }
    });
}

#[test]
fn an_open_sse_cannot_block_shutdown_past_the_deadline() {
    run(true, async {
        let (tx, _rx) = watch::channel(false);
        let start = tokio::time::Instant::now();
        let outcome = supervise(
            std::future::pending(),
            std::future::pending(),
            std::future::pending(),
            async { Ok(()) },
            tx,
        )
        .await;
        assert!(outcome.is_ok());
        assert_eq!(start.elapsed(), DRAIN_TIMEOUT);
    });
}

#[test]
fn ingestion_reaches_world_over_http_on_an_ephemeral_port() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    run(false, async {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping TCP integration: sandbox denies loopback sockets: {error}");
                return;
            }
            Err(error) => panic!("bind: {error}"),
        };
        let addr = listener.local_addr().expect("bound address");
        let dir = TestDirectory::new("serve-http");
        let log = SqliteEventLog::open(dir.path()).expect("log opens");
        let verdicts = SqliteVerdictStore::open(dir.path()).expect("verdicts open");
        let (events_tx, events_rx) = tokio::sync::mpsc::channel(1);
        let started = Started {
            stream: Box::pin(tokio_stream::wrappers::ReceiverStream::new(events_rx)),
            ends: Ending::AtEndOfInput,
            notes: Vec::new(),
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        let server = serve_live(state(), log, verdicts, started, "stdin", listener, async {
            stop_rx.await.expect("stop signal");
            Ok(())
        });
        let client = async {
            events_tx
                .send(Ok(RawEvent {
                    source: SourceId::new("stdin").expect("source"),
                    cursor: Cursor::new(b"1".to_vec()).expect("cursor"),
                    received_at: Timestamp::from_millis(1),
                    payload:
                        br#"{"EntityObserved":{"key":"live","entity_type":"thing","attrs":{}}}"#
                            .to_vec(),
                }))
                .await
                .expect("send event");
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let mut socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
                    socket
                        .write_all(
                            b"GET /worlds/default/world HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .expect("request");
                    let mut response = String::new();
                    socket
                        .read_to_string(&mut response)
                        .await
                        .expect("response");
                    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
                    if response.contains("live") {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("ingested event should reach /worlds/default/world");
            stop_tx.send(()).expect("stop server");
            // Keep ingestion alive until after stop; otherwise EOF could win the select.
            events_tx
        };
        let (result, _sender) = tokio::join!(server, client);
        result.expect("server shuts down");
    });
}

#[test]
fn shared_log_ingestion_and_bridge_feed_the_http_router_without_sockets() {
    run(false, async {
        let dir = TestDirectory::new("serve-router");
        let shared = Rc::new(RefCell::new(SqliteEventLog::open(dir.path()).expect("log")));
        let mut writer = SharedLogWriter(shared.clone());
        let state = state();
        let config = BridgeConfig {
            batch: 1,
            ..BridgeConfig::default()
        };
        let mut bridge = Bridge::new(
            SharedLogReader {
                log: shared,
                batch: config.batch,
            },
            SqliteVerdictStore::open(dir.path()).expect("verdicts"),
            EngineRegistry::with_defaults(),
            state.clone(),
            config,
        )
        .expect("bridge");
        let events = (1..=2).map(|i| Ok(RawEvent {
            source: SourceId::new("stdin").expect("source"),
            cursor: Cursor::new(vec![i]).expect("cursor"),
            received_at: Timestamp::from_millis(i64::from(i)),
            payload: format!(r#"{{"EntityObserved":{{"key":"live-{i}","entity_type":"thing","attrs":{{}}}}}}"#).into_bytes(),
        }));
        group_commit::pump_events(
            &mut writer,
            Box::pin(tokio_stream::iter(events)),
            "stdin",
            &mut group_commit::HumanReporter,
        )
        .await
        .expect("ingest");
        // More than one batch exercises owned reader lifetimes and the explicit collection cap.
        for _ in 0..2 {
            let report = bridge.poll_once().expect("poll");
            assert!(report.error.is_none());
            assert_eq!(report.stats.consumed, 1);
        }
        assert_eq!(bridge.poll_once().expect("empty poll").stats.consumed, 0);
        let response = router(state)
            .layer(middleware::from_fn(host_allowlist))
            .oneshot(
                Request::builder()
                    .uri("/worlds/default/world")
                    .header(HOST, "localhost:4310")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .expect("body");
        let world: serde_json::Value = serde_json::from_slice(&bytes).expect("world JSON");
        assert_eq!(world["nodes"].as_array().expect("nodes").len(), 2);
        println!(
            "GET /worlds/default/world (in-process HTTP router): {}",
            String::from_utf8_lossy(&bytes)
        );
    });
}

#[test]
fn configured_world_is_the_only_world_served() {
    run(false, async {
        let app = router(state().with_world("foo"));
        for (uri, expected) in [
            ("/worlds/foo/world", StatusCode::OK),
            ("/worlds/default/world", StatusCode::NOT_FOUND),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), expected, "{uri}");
        }
    });
}
