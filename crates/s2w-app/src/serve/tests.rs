use super::*;
use crate::query::Timeline;
use crate::tests::{TestDirectory, run};
use axum::{Router, body::Body, routing::get};
use s2w_model::{StreamMapping, Timestamp};
use tower::ServiceExt;

#[derive(Debug, PartialEq, Eq)]
enum Report {
    Note(String),
    SourceError(String, bool),
}

#[derive(Default)]
struct TestReporter {
    reports: Vec<Report>,
}

impl Reporter for TestReporter {
    fn flushed(
        &mut self,
        _appended: u64,
        _duplicates: u64,
        _reconnects: u64,
        _cursor: Option<&str>,
    ) {
    }

    fn duplicate(&mut self, _position: u64) {}

    fn note(&mut self, message: &str) {
        self.reports.push(Report::Note(message.to_owned()));
    }

    fn source_error(&mut self, message: &str, retry: bool) {
        self.reports
            .push(Report::SourceError(message.to_owned(), retry));
    }

    fn wants_ticker(&self) -> bool {
        false
    }
}

fn state() -> QueryState {
    QueryState::new(Timeline::new(crate::DEFAULT_HUB_IN_DEGREE_CAP))
}

#[test]
fn startup_notes_preserve_the_source_name() {
    let mut reporter = TestReporter::default();
    report_source_start(
        &mut reporter,
        "wikipedia",
        &["no stored cursor; starting fresh".to_owned()],
    );
    assert_eq!(
        reporter.reports,
        [Report::Note(
            "wikipedia: no stored cursor; starting fresh".to_owned()
        )]
    );
}

#[test]
fn listening_note_uses_the_actual_bound_address() {
    run(false, async {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping TCP integration: sandbox denies loopback sockets: {error}");
                return;
            }
            Err(error) => panic!("bind: {error}"),
        };
        let expected = format!(
            "serving on http://{}",
            listener.local_addr().expect("bound address")
        );
        let mut reporter = TestReporter::default();
        report_listener(&mut reporter, &listener).expect("listener address");
        assert_eq!(reporter.reports, [Report::Note(expected)]);
    });
}

#[test]
fn membership_change_note_preserves_recovery_instructions() {
    let mut reporter = TestReporter::default();
    report_membership_changed(&mut reporter);
    assert_eq!(
        reporter.reports,
        [Report::Note(
            "source membership changed; HTTP remains available; re-add with a cursor and restart to resume"
                .to_owned()
        )]
    );
}

#[test]
fn both_writer_locks_map_to_usage_and_release() {
    let dir = TestDirectory::new("serve-locks");
    let args = ServeArgs {
        uri: "-".to_owned(),
        world: "default".to_owned(),
        log_dir: dir.path().to_owned(),
        port: 0,
        filters: Vec::new(),
        snapshots: SnapshotConfig::default(),
    };
    let log = SqliteEventLog::open(dir.path()).expect("first event log opens");
    let error = run_serve(state(), args.clone(), &mut TestReporter::default())
        .expect_err("second event log must fail");
    assert!(
        matches!(error, AppError::Usage(ref m) if m.contains("the event log at") && m.contains("only once per --log-dir"))
    );
    drop(log);
    let log = SqliteEventLog::open(dir.path()).expect("event lock released");
    drop(log);
    let verdicts = SqliteVerdictStore::open(dir.path()).expect("first verdict store opens");
    let error = run_serve(state(), args, &mut TestReporter::default())
        .expect_err("second verdict store must fail");
    assert!(
        matches!(error, AppError::Usage(ref m) if m.contains("the verdict store at") && m.contains("only once per --log-dir"))
    );
    drop(verdicts);
    SqliteVerdictStore::open(dir.path()).expect("verdict lock released");
    SqliteEventLog::open(dir.path()).expect("failed startup released event lock too");
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
)]
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
            let mut reporter = TestReporter::default();
            let outcome = supervise(
                |_| Box::pin(std::future::pending()),
                async { result },
                async {
                    shutdown_signal(rx).await;
                    drained.set(true);
                    Ok(())
                },
                std::future::pending(),
                tx,
                &mut reporter,
            )
            .await;
            assert!(matches!(outcome, Err(AppError::BridgeStopped(_))));
            assert!(drained.get(), "HTTP must observe shutdown before returning");
            assert_eq!(
                reporter.reports,
                [Report::Note(
                    "shutting down HTTP after a fatal error".to_owned()
                )]
            );
            assert!(
                !reporter.reports.iter().any(|report| matches!(
                    report,
                    Report::Note(message) if message.contains("injected failure")
                )),
                "the fatal error is rendered by the CLI exactly once"
            );
        }
    });
}

#[test]
fn ingestion_reports_source_errors_and_natural_shutdown_through_one_reporter() {
    run(false, async {
        let (tx, rx) = watch::channel(false);
        let mut reporter = TestReporter::default();
        let outcome = supervise(
            |reporter| {
                Box::pin(async move {
                    reporter.source_error("stdin: skipped malformed line", false);
                    Ok(())
                })
            },
            std::future::pending(),
            async {
                shutdown_signal(rx).await;
                Ok(())
            },
            std::future::pending(),
            tx,
            &mut reporter,
        )
        .await;
        assert!(outcome.is_ok());
        assert_eq!(
            reporter.reports,
            [
                Report::SourceError("stdin: skipped malformed line".to_owned(), false),
                Report::Note("ingestion stopped; shutting down HTTP".to_owned())
            ]
        );
    });
}

#[test]
fn an_open_sse_cannot_block_shutdown_past_the_deadline() {
    run(true, async {
        let (tx, _rx) = watch::channel(false);
        let start = tokio::time::Instant::now();
        let mut reporter = TestReporter::default();
        let outcome = supervise(
            |_| Box::pin(std::future::pending()),
            std::future::pending(),
            std::future::pending(),
            async { Ok(()) },
            tx,
            &mut reporter,
        )
        .await;
        assert!(outcome.is_ok());
        assert_eq!(start.elapsed(), DRAIN_TIMEOUT);
        assert_eq!(
            reporter.reports,
            [Report::Note(
                "HTTP drain timed out after 5 seconds; closing remaining connections".to_owned()
            )]
        );
    });
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
)]
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
            sources: vec![SourceId::new("stdin").expect("source")],
            stream: Box::pin(tokio_stream::wrappers::ReceiverStream::new(events_rx)),
            ends: Ending::AtEndOfInput,
            notes: Vec::new(),
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        let mut reporter = TestReporter::default();
        let server = serve_live(
            state(),
            ServeStorage {
                log,
                verdicts,
                registry: EngineRegistry::with_defaults(),
                resume: None,
                snapshots: None,
            },
            started,
            "stdin",
            listener,
            async {
                stop_rx.await.expect("stop signal");
                Ok(())
            },
            &mut reporter,
        );
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
#[expect(
    clippy::too_many_lines,
    reason = "scenario test: setup and assertions read as one sequence, and splitting would hide the shared fixture"
)]
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
fn world_routing_serves_the_shell_for_a_matching_world_and_404s_a_mismatch() {
    run(false, async {
        let full_app = app(state().with_world("foo"));
        let (status, headers, _) = web_response(&full_app, "/w/foo/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            headers
                .get(axum::http::header::CONTENT_TYPE)
                .is_some_and(|value| value.to_str().unwrap_or_default().contains("html")),
            "expected the SPA shell's content-type, got {headers:?}"
        );
        let (status, _, _) = web_response(&full_app, "/w/nope/").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    });
}

#[test]
fn world_and_home_pages_carry_the_content_security_policy() {
    run(false, async {
        let full_app = app(state().with_world("foo"));
        for uri in ["/w/foo/", "/", "/main.js", "/style.css"] {
            let (status, headers, _) = web_response(&full_app, uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            let csp = headers
                .get(axum::http::header::CONTENT_SECURITY_POLICY)
                .unwrap_or_else(|| panic!("{uri} has no CSP: {headers:?}"))
                .to_str()
                .expect("ascii");
            for directive in [
                "img-src 'self' data:",
                "font-src 'self'",
                "connect-src 'self'",
                "style-src 'self' 'unsafe-inline'",
                "script-src 'self'",
            ] {
                assert!(csp.contains(directive), "{uri}: {directive} in {csp}");
            }
            assert!(!csp.contains("script-src 'self' 'unsafe"), "{uri}: {csp}");
        }
    });
}

#[test]
fn bare_root_serves_the_home_dashboard_and_the_world_view_serves_the_shell() {
    run(false, async {
        let full_app = app(state().with_world("foo"));
        let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist");
        let home = std::fs::read(dist.join("home.html")).expect("home.html");
        let shell = std::fs::read(dist.join("index.html")).expect("index.html");
        let (status, _, body) = web_response(&full_app, "/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_ref(), home.as_slice(), "/ is the home page");
        let (status, _, body) = web_response(&full_app, "/w/foo/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body.as_ref(),
            shell.as_slice(),
            "/w/foo/ is the world shell"
        );
        let (status, _, body) = web_response(&full_app, "/home.js").await;
        assert_eq!(status, StatusCode::OK);
        let home_js = std::fs::read(dist.join("home.js")).expect("home.js");
        assert_eq!(body.as_ref(), home_js.as_slice());
        // An unknown path is pinned, not assumed: it must not serve either page as a 200.
        let (status, _, _) = web_response(&full_app, "/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    });
}

#[test]
fn world_routing_redirects_the_no_slash_and_legacy_query_forms() {
    run(false, async {
        let full_app = app(state().with_world("foo"));
        let (status, headers, _) = web_response(&full_app, "/w/foo").await;
        assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            headers.get(axum::http::header::LOCATION).unwrap(),
            "/w/foo/"
        );
        let (status, headers, _) = web_response(&full_app, "/?world=foo&at=5").await;
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(
            headers.get(axum::http::header::LOCATION).unwrap(),
            "/w/foo/?at=5"
        );
        // The no-slash form keeps the whole query string too.
        let (status, headers, _) = web_response(&full_app, "/w/foo?at=5&lod=2").await;
        assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            headers.get(axum::http::header::LOCATION).unwrap(),
            "/w/foo/?at=5&lod=2"
        );
    });
}

#[test]
fn world_names_decode_the_same_way_in_every_url_form() {
    // One rule: percent escapes are UTF-8 bytes; `+` is a space only in a query value.
    assert_eq!(super::percent_decode("caf%C3%A9", false), "café");
    assert_eq!(super::percent_decode("caf%C3%A9", true), "café");
    assert_eq!(super::percent_decode("a+b", false), "a+b");
    assert_eq!(super::percent_decode("a+b", true), "a b");
    assert_eq!(super::percent_decode("100%", false), "100%");
    assert_eq!(super::percent_decode("%zz", false), "%zz");
    assert_eq!(super::percent_decode("%FF", false), "\u{fffd}");
    run(false, async {
        let full_app = app(state().with_world("café"));
        let (status, _, _) = web_response(&full_app, "/w/caf%C3%A9/").await;
        assert_eq!(status, StatusCode::OK);
        let (status, headers, _) = web_response(&full_app, "/?world=caf%C3%A9&at=5").await;
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(
            headers.get(axum::http::header::LOCATION).unwrap(),
            "/w/caf%C3%A9/?at=5"
        );
        let (status, _, _) = web_response(&full_app, "/w/caf%C3%A9x/").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    });
}

async fn web_response(
    app: &Router,
    uri: &str,
) -> (StatusCode, axum::http::HeaderMap, axum::body::Bytes) {
    let response = app
        .clone()
        .oneshot(web_request(uri))
        .await
        .expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, headers, bytes)
}

#[test]
fn presentation_endpoint_serves_the_default_record_and_404s_a_wrong_world() {
    run(false, async {
        let bare_app = router(state().with_world("foo"));
        let response = bare_app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/worlds/foo/presentation")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(
            body,
            serde_json::json!({
                "title": null, "tagline": null, "description": null,
                "palette_light": null, "palette_dark": null, "typefaces": null, "stylesheet": null
            })
        );
        let response = bare_app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/worlds/nope/presentation")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
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

fn web_request(uri: &str) -> Request {
    Request::builder()
        .uri(uri)
        .header(HOST, "localhost:4310")
        .body(Body::empty())
        .expect("request")
}

fn append_entity(state: &QueryState, key: &str) {
    state
        .append(
            Timestamp::from_millis(1),
            s2w_core::WorldEvent::EntityObserved {
                key: s2w_core::NaturalKey::new(key),
                entity_type: "thing".to_owned(),
                attrs: Default::default(),
            },
        )
        .expect("append");
}

#[test]
fn web_assets_and_api_share_the_host_boundary() {
    run(false, async {
        let app = app(state());
        for uri in ["/main.js", "/", "/worlds/default/world"] {
            let mut request = web_request(uri);
            request
                .headers_mut()
                .insert(HOST, "evil.example".parse().expect("header"));
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
        }
        // The committed bundle, served from the binary's embed: an embedded asset carries a
        // content-hash ETag (a disk-loaded one does not) and its bytes are the committed file's.
        for (uri, file, content_type) in [
            ("/", "home.html", "text/html"),
            ("/main.js", "main.js", "javascript"),
        ] {
            let response = app.clone().oneshot(web_request(uri)).await.expect("asset");
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            let headers = response.headers();
            assert!(
                headers["content-type"]
                    .to_str()
                    .expect("type")
                    .contains(content_type),
                "{uri}"
            );
            assert!(
                headers.get("etag").is_some_and(|etag| !etag.is_empty()),
                "{uri} is embedded"
            );
            let body = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
                .await
                .expect("body");
            let committed = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("web/dist")
                    .join(file),
            )
            .expect("committed bundle");
            assert!(!committed.is_empty() && body == committed, "{uri}");
        }
        assert_eq!(
            app.oneshot(web_request("/missing-asset.js"))
                .await
                .expect("404")
                .status(),
            StatusCode::NOT_FOUND
        );
    });
}

#[test]
fn web_origin_guard_checks_scheme_authority_and_port() {
    run(false, async {
        let app = app(state());
        for (host, origin, valid) in [
            ("localhost:4310", None, true),
            ("localhost:4310", Some("http://localhost:4310"), true),
            ("127.0.0.1:4310", Some("http://127.0.0.1:4310"), true),
            ("[::1]:4310", Some("http://[::1]:4310"), true),
            // Any literal loopback name on the served port: the page may be opened by either.
            ("localhost:4310", Some("http://127.0.0.1:4310"), true),
            ("127.0.0.1:4310", Some("http://[::1]:4310"), true),
            ("localhost", Some("http://localhost"), true),
            ("localhost:4310", Some("http://localhost"), false),
            ("localhost", Some("http://localhost:4310"), false),
            ("localhost:4310", Some("http://localhost.evil:4310"), false),
            ("localhost:4310", Some("http://evil.example:4310"), false),
            ("localhost:4310", Some("http://localhost:4311"), false),
            ("localhost:4310", Some("https://localhost:4310"), false),
            ("localhost:4310", Some("http://localhost:4310/"), false),
            ("localhost:4310", Some("null"), false),
        ] {
            let mut request = web_request("/worlds/default/events?at=0");
            request
                .headers_mut()
                .insert(HOST, host.parse().expect("host"));
            if let Some(origin) = origin {
                request
                    .headers_mut()
                    .insert("origin", origin.parse().expect("origin"));
            }
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(
                response.status(),
                if valid {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                },
                "{origin:?}"
            );
        }
        let request = Request::builder()
            .uri("/worlds/default/events")
            .header(HOST, "localhost:4310")
            .header("origin", "http://localhost:4310")
            .header("origin", "http://localhost:4310")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            app.oneshot(request).await.expect("response").status(),
            StatusCode::FORBIDDEN
        );
    });
}

#[test]
fn web_sse_cap_lives_until_response_body_is_dropped() {
    run(false, async {
        let app = app(state());
        let mut open = Vec::new();
        // Keep bodies completely unpolled: owning headers is sufficient to consume a slot.
        for _ in 0..32 {
            let response = app
                .clone()
                .oneshot(web_request("/worlds/default/events"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            open.push(response);
        }
        let response = app
            .clone()
            .oneshot(web_request("/worlds/default/events"))
            .await
            .expect("cap");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("body");
        let error: serde_json::Value = serde_json::from_slice(&body).expect("JSON error");
        assert!(error["error"].is_string() && error["message"].is_string());
        drop(open.pop());
        assert_eq!(
            app.oneshot(web_request("/worlds/default/events"))
                .await
                .expect("freed slot")
                .status(),
            StatusCode::OK
        );
    });
}

#[test]
fn web_pinned_evidence_is_seeded_and_from_is_exclusive() {
    run(false, async {
        let state = state();
        for key in ["first", "second", "third"] {
            append_entity(&state, key);
        }
        let app = app(state);
        let response = app
            .clone()
            .oneshot(web_request("/worlds/default/events?from=1&at=2"))
            .await
            .expect("seed");
        assert_eq!(response.status(), StatusCode::OK);
        let body = tokio::time::timeout(
            Duration::from_secs(1),
            axum::body::to_bytes(response.into_body(), 8192),
        )
        .await
        .expect("bounded stream closes")
        .expect("body");
        let text = String::from_utf8(body.to_vec()).expect("text");
        let messages: Vec<serde_json::Value> = text
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|data| serde_json::from_str(data).expect("delta"))
            .collect();
        assert_eq!(
            messages.len(),
            1,
            "pinned evidence must be nonempty and bounded"
        );
        assert_eq!(messages[0]["offset"], 2, "from is exclusive");
        assert_eq!(messages[0]["type"], "entity");
        let response = app
            .clone()
            .oneshot(web_request("/worlds/default/world?at=2"))
            .await
            .expect("snapshot");
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .expect("body");
        let view: serde_json::Value = serde_json::from_slice(&body).expect("snapshot JSON");
        assert_eq!(view["offset"], messages[0]["offset"]);
        assert_eq!(view["nodes"].as_array().expect("nodes").len(), 2);
        for (query, status) in [
            ("from=2&at=1", StatusCode::BAD_REQUEST),
            ("at=4", StatusCode::NOT_FOUND),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(web_request(&format!("/worlds/default/events?{query}")))
                    .await
                    .expect("error")
                    .status(),
                status
            );
        }
    });
}

#[test]
fn web_live_sse_observes_an_append_after_opening() {
    use tokio_stream::StreamExt;
    run(false, async {
        let state = state();
        append_entity(&state, "first");
        let response = app(state.clone())
            .oneshot(web_request("/worlds/default/events?from=1"))
            .await
            .expect("open");
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();
        append_entity(&state, "second");
        let chunk = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("live delta")
            .expect("chunk")
            .expect("bytes");
        let text = String::from_utf8(chunk.to_vec()).expect("text");
        assert!(text.contains("id: 2\n"), "{text}");
        assert!(!text.contains("id: 1\n"), "exclusive resume: {text}");
    });
}

#[test]
fn serving_start_gate_bootstraps_and_preserves_removal() {
    run(false, async {
        let dir = TestDirectory::new("serve-start-gate");
        let mut log = SqliteEventLog::open(dir.path()).unwrap();
        let source = SourceId::new("stdin").unwrap();
        log.record_source_removed(&source).unwrap();
        let started = resolve("-", &[])
            .unwrap()
            .start(None, &ServingCursors(RefCell::new(&mut log)))
            .await
            .unwrap();
        assert_eq!(started.sources, vec![source]);
        assert_eq!(started.ends, Ending::Never);
        assert!(started.notes[0].contains("removed"));
        assert_eq!(log.membership_history().unwrap().len(), 1);
        // SSE's start gate runs before connecting, and uses the adapter's real hashed ID.
        let started = resolve("http://127.0.0.1:1/events", &[])
            .unwrap()
            .start(None, &ServingCursors(RefCell::new(&mut log)))
            .await
            .unwrap();
        let source = started.sources[0].clone();
        drop(started);
        log.record_source_removed(&source).unwrap();
        let restarted = resolve("http://127.0.0.1:1/events", &[])
            .unwrap()
            .start(None, &ServingCursors(RefCell::new(&mut log)))
            .await
            .unwrap();
        assert!(restarted.notes[0].contains("removed"));
        assert_eq!(restarted.sources, vec![source]);
        assert_eq!(log.membership_history().unwrap().len(), 3);
    });
}

struct ServedRun {
    notes: Vec<String>,
    /// `(base, head)` once every sent event reached the world.
    bounds: (u64, u64),
    /// `GET /worlds/default/world` at that moment.
    world: serde_json::Value,
}

/// What one [`serve_until`] run ingests and routes.
struct Feed {
    source: SourceId,
    /// `(cursor, payload)` per event, in order.
    events: Vec<(u8, Vec<u8>)>,
    /// `None`: resolve routes from the proposal store in the log directory, as `serve` does.
    registry: Option<EngineRegistry>,
    until: Until,
}

/// When a [`serve_until`] run stops.
#[derive(Clone, Copy)]
enum Until {
    /// The world holds this many entities.
    Nodes(usize),
    /// The bridge has consumed this many events of the feed's source in this run.
    Consumed(u64),
}

impl Feed {
    /// `EntityObserved` events `(cursor, key)` from a stdin-like source, default routes.
    fn things(events: &[(u8, &str)], nodes: usize) -> Self {
        Self {
            source: SourceId::new("stdin").expect("source"),
            events: events
                .iter()
                .map(|(cursor, key)| {
                    let payload = format!(
                        r#"{{"EntityObserved":{{"key":"{key}","entity_type":"thing","attrs":{{}}}}}}"#
                    );
                    (*cursor, payload.into_bytes())
                })
                .collect(),
            registry: Some(EngineRegistry::with_defaults()),
            until: Until::Nodes(nodes),
        }
    }
}

/// One `serve` process over `dir`: resolves routes, restores per `config`, ingests `feed`,
/// waits until its stop condition holds, then stops as a signal would. `None` if the sandbox
/// denies loopback sockets.
#[expect(
    clippy::too_many_lines,
    reason = "scenario helper: server and client halves share the fixture and read as one sequence"
)]
async fn serve_until(dir: &TestDirectory, config: SnapshotConfig, feed: Feed) -> Option<ServedRun> {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping TCP integration: sandbox denies loopback sockets: {error}");
            return None;
        }
        Err(error) => panic!("bind: {error}"),
    };
    let log = SqliteEventLog::open(dir.path()).expect("log opens");
    let verdicts = SqliteVerdictStore::open(dir.path()).expect("verdicts open");
    let state = state();
    let mut reporter = TestReporter::default();
    let registry = match feed.registry {
        Some(registry) => registry,
        None => routed_registry(dir.path(), &mut reporter).expect("routes resolve"),
    };
    let (resume, snapshots) = snapshots::prepare(
        &state,
        (&log, &verdicts),
        dir.path(),
        (config, registry.feed_fingerprint()),
        &mut reporter,
    )
    .expect("prepare");
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(8);
    let started = Started {
        sources: vec![feed.source.clone()],
        stream: Box::pin(tokio_stream::wrappers::ReceiverStream::new(events_rx)),
        ends: Ending::AtEndOfInput,
        notes: Vec::new(),
    };
    let (stop_tx, stop_rx) = oneshot::channel();
    let server = serve_live(
        state.clone(),
        ServeStorage {
            log,
            verdicts,
            registry,
            resume,
            snapshots,
        },
        started,
        "stdin",
        listener,
        async {
            stop_rx.await.expect("stop signal");
            Ok(())
        },
        &mut reporter,
    );
    let client = async {
        for (cursor, payload) in feed.events {
            events_tx
                .send(Ok(RawEvent {
                    source: feed.source.clone(),
                    cursor: Cursor::new(vec![cursor]).expect("cursor"),
                    received_at: Timestamp::from_millis(i64::from(cursor)),
                    payload,
                }))
                .await
                .expect("send event");
        }
        let app = router(state.clone()).layer(middleware::from_fn(host_allowlist));
        let world = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                // Read the counter BEFORE the world: the bridge serves a batch's claims before
                // it publishes the batch's counters, so this world includes every counted event.
                let consumed = state.consumed(&feed.source);
                let (status, _, bytes) = web_response(&app, "/worlds/default/world").await;
                assert_eq!(status, StatusCode::OK);
                let world: serde_json::Value = serde_json::from_slice(&bytes).expect("world");
                let done = match feed.until {
                    Until::Nodes(nodes) => {
                        world["nodes"].as_array().is_some_and(|n| n.len() == nodes)
                    }
                    Until::Consumed(count) => consumed >= count,
                };
                if done {
                    break world;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("every event should reach the world");
        let (base, head, _) = state.bounds().expect("bounds");
        stop_tx.send(()).expect("stop server");
        // Keep ingestion alive until after stop; otherwise EOF could win the select.
        (events_tx, world, (base, head))
    };
    let (result, (_sender, world, bounds)) = tokio::join!(server, client);
    result.expect("server shuts down");
    let notes = reporter
        .reports
        .into_iter()
        .filter_map(|report| match report {
            Report::Note(note) => Some(note),
            Report::SourceError(..) => None,
        })
        .collect();
    Some(ServedRun {
        notes,
        bounds,
        world,
    })
}

/// Snapshots on, written only by the final snapshot at stop.
const SNAPSHOT_ON_STOP: SnapshotConfig = SnapshotConfig {
    enabled: true,
    every: 1_000,
    shutdown_min: 1,
};

fn noted(run: &ServedRun, needle: &str) -> bool {
    run.notes.iter().any(|note| note.contains(needle))
}

#[test]
fn a_restarted_serve_resumes_from_its_final_snapshot_and_matches_a_full_replay() {
    run(false, async {
        let dir = TestDirectory::new("serve-snapshot-restart");
        let config = SNAPSHOT_ON_STOP;
        let abc = Feed::things(&[(1, "a"), (2, "b"), (3, "c")], 3);
        let Some(first) = serve_until(&dir, config, abc).await else {
            return;
        };
        assert!(
            !noted(&first, "restored from snapshot"),
            "{:?}",
            first.notes
        );
        let snapshot_offset = first.bounds.1;
        assert_eq!(
            store_offsets(&dir),
            vec![snapshot_offset],
            "the stop wrote one final snapshot at the head"
        );

        let second = serve_until(&dir, config, Feed::things(&[(4, "d"), (5, "e")], 5))
            .await
            .expect("sockets allowed once");
        assert!(
            second
                .notes
                .iter()
                .any(|note| note.starts_with("restored from snapshot")
                    && note.contains(&format!(" at offset {snapshot_offset} "))),
            "{:?}",
            second.notes
        );
        assert_eq!(second.bounds.0, snapshot_offset, "the base is the snapshot");
        assert!(
            second.bounds.1 > snapshot_offset,
            "the tail replayed on top"
        );

        let replayed = serve_until(
            &dir,
            SnapshotConfig {
                enabled: false,
                ..config
            },
            Feed::things(&[], 5),
        )
        .await
        .expect("sockets allowed once");
        assert_eq!(
            replayed.bounds,
            (0, second.bounds.1),
            "--no-snapshot replays everything"
        );
        assert_eq!(
            replayed.world, second.world,
            "restore + tail == full replay"
        );
    });
}

fn store_offsets(dir: &TestDirectory) -> Vec<u64> {
    crate::snapshot::store::list(&crate::snapshot::store::dir(dir.path()))
        .expect("snapshot dir")
        .into_iter()
        .map(|(offset, _)| offset)
        .collect()
}

// Routes from stored stream mappings (decision 0023).

const MAPPED: &str = "test.mapped";
const NO_SNAPSHOT: SnapshotConfig = SnapshotConfig {
    enabled: false,
    ..SNAPSHOT_ON_STOP
};

fn mapping_a() -> StreamMapping {
    serde_json::from_str(include_str!(
        "../../../s2w-system1/testdata/sample.mapping.json"
    ))
    .expect("fixture mapping")
}

/// Mapping A with its first entity rule's type label changed: a different mapping identity
/// that folds a different world from the same events.
fn mapping_b() -> StreamMapping {
    let mut mapping = mapping_a();
    let rule = mapping.entities.first_mut().expect("an entity rule");
    rule.type_label = format!("{}-b", rule.type_label);
    mapping
}

/// Recorded raw events `range` of the fixture sample, cursor = line number from 1.
fn raw_events(range: std::ops::Range<usize>) -> Vec<(u8, Vec<u8>)> {
    include_str!("../../../s2w-system1/testdata/raw-sample.jsonl")
        .lines()
        .enumerate()
        .skip(range.start)
        .take(range.len())
        .map(|(index, line)| {
            (
                u8::try_from(index + 1).expect("small"),
                line.as_bytes().to_vec(),
            )
        })
        .collect()
}

fn mapped(range: std::ops::Range<usize>, registry: Option<EngineRegistry>) -> Feed {
    Feed {
        source: SourceId::new(MAPPED).expect("source"),
        until: Until::Consumed(range.len() as u64),
        events: raw_events(range),
        registry,
    }
}

/// Stores `mapping` for [`MAPPED`] as proposal `id` with a human accept, as a reviewer would.
fn accept_mapping(dir: &TestDirectory, id: &str, mapping: StreamMapping) {
    use s2w_log::{
        Actor, Decider, NewDecision, NewProposal, Outcome, ProposalStore, SqliteProposalStore,
    };
    let envelope = routes::MappingEnvelope {
        format: routes::ENVELOPE_FORMAT,
        source: MAPPED.to_owned(),
        mapping,
    };
    let mut store = SqliteProposalStore::open(dir.path()).expect("proposal store");
    store
        .append_proposal(&NewProposal {
            id: id.to_owned(),
            class: routes::STREAM_MAPPING_CLASS.to_owned(),
            actor: Actor::Human { id: "h".to_owned() },
            snapshot_offset: LogPosition::from_u64(1).expect("position"),
            payload: serde_json::to_vec(&envelope).expect("envelope"),
            proposed_at_ms: 0,
        })
        .expect("proposal");
    store
        .append_decision(&NewDecision {
            proposal_id: id.to_owned(),
            decider: Decider::Human,
            outcome: Outcome::Accept,
            basis: "reviewed".to_owned(),
            decided_at_ms: 0,
        })
        .expect("decision");
}

/// The defaults plus `engine` on [`MAPPED`], built directly rather than from a store.
fn routed_to(engine: Box<dyn s2w_system1::Engine>) -> EngineRegistry {
    let mut registry = EngineRegistry::with_defaults();
    registry
        .register(crate::bridge::Route::Exact(MAPPED.to_owned()), engine)
        .expect("register");
    registry
}

fn engine(mapping: StreamMapping, proposal_id: &str) -> Box<dyn s2w_system1::Engine> {
    Box::new(
        s2w_system1::MappingEngine::new(mapping)
            .expect("valid mapping")
            .with_proposal_id(proposal_id),
    )
}

/// The mutant decision 0023 rules out: a mapping engine under one bare name for every mapping.
struct BareName(Box<dyn s2w_system1::Engine>);

impl s2w_system1::Engine for BareName {
    fn name(&self) -> &str {
        "mapping"
    }
    fn version(&self) -> u32 {
        self.0.version()
    }
    fn evaluate(&self, event: &RawEvent) -> s2w_system1::Verdict {
        self.0.evaluate(event)
    }
    fn provenance(&self) -> Option<Vec<u8>> {
        self.0.provenance()
    }
}

fn node_count(run: &ServedRun) -> usize {
    run.world["nodes"].as_array().map_or(0, Vec::len)
}

#[test]
fn serve_routes_a_source_from_its_accepted_stream_mapping_proposal() {
    run(false, async {
        let stored = TestDirectory::new("serve-routes-stored");
        accept_mapping(&stored, "p-a", mapping_a());
        let Some(from_store) = serve_until(&stored, NO_SNAPSHOT, mapped(0..20, None)).await else {
            return;
        };
        assert!(
            noted(
                &from_store,
                &format!("route: source '{MAPPED}' runs mapping ")
            ) && noted(&from_store, "from proposal p-a"),
            "{:?}",
            from_store.notes
        );
        let direct = TestDirectory::new("serve-routes-direct");
        let registry = routed_to(engine(mapping_a(), "p-a"));
        let direct = serve_until(&direct, NO_SNAPSHOT, mapped(0..20, Some(registry)))
            .await
            .expect("sockets allowed once");
        assert!(node_count(&direct) > 0, "the mapping folds entities");
        assert_eq!(
            from_store.world, direct.world,
            "stored route == direct engine"
        );
    });
}

#[test]
fn a_snapshot_taken_under_another_mapping_is_ignored_and_the_restart_folds_cold() {
    run(false, async {
        let dir = TestDirectory::new("serve-routes-remapped");
        accept_mapping(&dir, "p-a", mapping_a());
        let Some(_first) = serve_until(&dir, SNAPSHOT_ON_STOP, mapped(0..10, None)).await else {
            return;
        };
        assert_eq!(store_offsets(&dir).len(), 1, "a snapshot under mapping A");
        accept_mapping(&dir, "p-b", mapping_b());
        // Ignored, so the restart replays all 20 events, not only the 10 new ones.
        let mut tail = mapped(10..20, None);
        tail.until = Until::Consumed(20);
        let second = serve_until(&dir, SNAPSHOT_ON_STOP, tail)
            .await
            .expect("sockets allowed once");
        assert!(
            noted(&second, "ignoring snapshot") && noted(&second, "different engine routing"),
            "{:?}",
            second.notes
        );
        assert_eq!(second.bounds.0, 0, "no snapshot base");

        let cold = TestDirectory::new("serve-routes-cold-b");
        let registry = routed_to(engine(mapping_b(), "p-b"));
        let cold = serve_until(&cold, NO_SNAPSHOT, mapped(0..20, Some(registry)))
            .await
            .expect("sockets allowed once");
        assert_eq!(
            second.world, cold.world,
            "restart under B == cold fold under B"
        );
    });
}

#[test]
fn a_bare_engine_name_would_restore_a_snapshot_taken_under_another_mapping() {
    run(false, async {
        let bare = |mapping, id| routed_to(Box::new(BareName(engine(mapping, id))));
        let dir = TestDirectory::new("serve-routes-bare");
        let first = mapped(0..10, Some(bare(mapping_a(), "p-a")));
        let Some(_first) = serve_until(&dir, SNAPSHOT_ON_STOP, first).await else {
            return;
        };
        let tail = mapped(10..20, Some(bare(mapping_b(), "p-b")));
        let second = serve_until(&dir, SNAPSHOT_ON_STOP, tail)
            .await
            .expect("sockets allowed once");
        assert!(
            noted(&second, "restored from snapshot"),
            "{:?}",
            second.notes
        );

        let cold = TestDirectory::new("serve-routes-bare-cold");
        let all = mapped(0..20, Some(bare(mapping_b(), "p-b")));
        let cold = serve_until(&cold, NO_SNAPSHOT, all)
            .await
            .expect("sockets allowed once");
        assert_ne!(
            second.world, cold.world,
            "under one bare name, mapping A's snapshot is served as mapping B's world"
        );
    });
}
