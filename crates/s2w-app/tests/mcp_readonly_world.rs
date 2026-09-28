//! A real MCP query while a separate process holds both SQLite writer locks.

#[cfg(test)]
mod tests {

    use std::collections::BTreeMap;
    use std::env;
    use std::fs;
    use std::future::Future;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use rmcp::ServiceExt;
    use rmcp::model::CallToolRequestParams;
    use s2w_app::mcp::WorldMcp;
    use s2w_app::mcp::replay::{LiveReadOnlyWorld, read_only_world};
    use s2w_core::{AttrValue, NaturalKey, WorldEvent};
    use s2w_log::{
        AppendOutcome, EventLog, SqliteEventLog, SqliteVerdictStore, StoredVerdict, VerdictStore,
    };
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
    use s2w_system1::{Confidence, Verdict};
    use serde_json::json;

    const CHILD_DIRECTORY: &str = "S2W_MCP_READONLY_CHILD_DIRECTORY";
    const EVENT_COUNT: u8 = 3;
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> std::io::Result<Self> {
            loop {
                let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir()
                    .join(format!("s2w-app-{label}-{}-{sequence}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Ok(Self(path)),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn raw(index: u8) -> TestResult<RawEvent> {
        Ok(RawEvent {
            source: SourceId::new("test.mcp-readonly")?,
            cursor: Cursor::new(vec![index])?,
            received_at: Timestamp::from_millis(1_000 + i64::from(index)),
            payload: vec![index],
        })
    }

    fn claim(key: &str, version: &str) -> WorldEvent {
        WorldEvent::EntityObserved {
            key: NaturalKey::new(key),
            entity_type: "fixture".to_owned(),
            attrs: BTreeMap::from([("version".to_owned(), AttrValue::Str(version.to_owned()))]),
        }
    }

    fn encoded(key: &str, version: &str) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&Verdict::Propose {
            claims: vec![claim(key, version)],
            confidence: Confidence::CERTAIN,
        })
    }

    #[test]
    fn readonly_writer_child() -> TestResult {
        let Some(directory) = env::var_os(CHILD_DIRECTORY) else {
            return Ok(());
        };
        let directory = PathBuf::from(directory);
        let mut log = SqliteEventLog::open(&directory)?;
        let mut verdicts = SqliteVerdictStore::open(&directory)?;
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        let mut previous = None;
        for index in 1..=EVENT_COUNT {
            let position = match log.append(raw(index)?)? {
                AppendOutcome::Inserted(position) => position,
                other => return Err(format!("unexpected append outcome: {other:?}").into()),
            };
            let stored = log
                .replay(previous)?
                .next()
                .ok_or("the inserted event was not replayed")??;
            let first_key = if index == 1 { "first" } else { "dedup-low" };
            verdicts.commit_batch(
                &[StoredVerdict {
                    position,
                    event_hash: stored.content_hash,
                    engine: "fixture".to_owned(),
                    version: 1,
                    verdict: encoded(first_key, "v1")?,
                    provenance: None,
                }],
                position,
            )?;
            if index == 2 {
                verdicts.commit_batch(
                    &[StoredVerdict {
                        position,
                        event_hash: stored.content_hash,
                        engine: "fixture".to_owned(),
                        version: 2,
                        verdict: encoded("dedup-high", "v2")?,
                        provenance: None,
                    }],
                    position,
                )?;
            }
            previous = Some(position);
            writeln!(output, "committed {index}")?;
            output.flush()?;
            let mut acknowledgement = [0_u8; 1];
            input.read_exact(&mut acknowledgement)?;
        }
        Ok(())
    }

    fn run(future: impl Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(10), future)
                    .await
                    .unwrap();
            });
    }

    #[test]
    fn mcp_queries_a_snapshot_while_another_process_keeps_writing() -> TestResult {
        if env::var_os(CHILD_DIRECTORY).is_some() {
            return Ok(());
        }
        let directory = TestDirectory::new("mcp-readonly")?;
        let executable = env::current_exe()?;
        let mut child = Command::new(executable)
            .arg("--exact")
            .arg("tests::readonly_writer_child")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CHILD_DIRECTORY, directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let child_stdout = child.stdout.take().ok_or("child stdout unavailable")?;
        let mut child_stdin = child.stdin.take().ok_or("child stdin unavailable")?;
        let mut lines = BufReader::new(child_stdout).lines();
        for expected in 1..=2 {
            loop {
                let line = lines.next().ok_or("child exited before commit marker")??;
                if line.contains("committed ") {
                    assert!(line.ends_with(&format!("committed {expected}")), "{line}");
                    break;
                }
            }
            if expected == 1 {
                child_stdin.write_all(&[1])?;
                child_stdin.flush()?;
            }
        }

        let state = read_only_world(directory.path(), "default", 10_000)?;
        run(async move {
            let server = WorldMcp::new(state);
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
            let result = client
                .call_tool(
                    CallToolRequestParams::new("world_view")
                        .with_arguments(json!({"world":"default"}).as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            let text = &result.content[0].as_text().unwrap().text;
            assert!(text.contains("first"), "{text}");
            assert!(text.contains("dedup-low"), "{text}");
            assert!(!text.contains("dedup-high"), "{text}");
            client.cancel().await.unwrap();
            task.await.unwrap();
        });

        child_stdin.write_all(&[1])?;
        child_stdin.flush()?;
        loop {
            let line = lines.next().ok_or("child exited before final marker")??;
            if line.contains("committed ") {
                assert!(line.ends_with("committed 3"), "{line}");
                break;
            }
        }
        child_stdin.write_all(&[1])?;
        child_stdin.flush()?;
        assert!(child.wait()?.success());
        Ok(())
    }

    /// Acceptance test (stream2worlds#128): world_view results advance while another process
    /// keeps writing the log. Opens `LiveReadOnlyWorld` against the first commit only, then
    /// polls `refresh()` — a bounded retry loop, never a fixed sleep, matching how the real
    /// `s2w mcp --log-dir` refresh thread polls — until the second commit shows up, then
    /// confirms the MCP `world_view` tool serves the advanced snapshot.
    #[test]
    fn mcp_serves_advancing_results_while_another_process_keeps_writing() -> TestResult {
        if env::var_os(CHILD_DIRECTORY).is_some() {
            return Ok(());
        }
        let directory = TestDirectory::new("mcp-live-refresh")?;
        let executable = env::current_exe()?;
        let mut child = Command::new(executable)
            .arg("--exact")
            .arg("tests::readonly_writer_child")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CHILD_DIRECTORY, directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let child_stdout = child.stdout.take().ok_or("child stdout unavailable")?;
        let mut child_stdin = child.stdin.take().ok_or("child stdin unavailable")?;
        let mut lines = BufReader::new(child_stdout).lines();

        // Wait for the first commit before opening, so the live snapshot captures exactly that.
        loop {
            let line = lines
                .next()
                .ok_or("child exited before first commit marker")??;
            if line.contains("committed ") {
                assert!(line.ends_with("committed 1"), "{line}");
                break;
            }
        }
        let (state, mut live) = LiveReadOnlyWorld::open(directory.path(), "default", 10_000)?;
        let world = serde_json::to_string(&state.world_at(None)?)?;
        assert!(world.contains("first"), "{world}");
        assert!(!world.contains("dedup-low"), "{world}");

        // Let the writer commit event 2 (which also commits a second, higher-version verdict
        // batch carrying "dedup-high" — see readonly_writer_child), then poll refresh() with a
        // bounded retry loop until the new claim shows up.
        child_stdin.write_all(&[1])?;
        child_stdin.flush()?;
        loop {
            let line = lines
                .next()
                .ok_or("child exited before second commit marker")??;
            if line.contains("committed ") {
                assert!(line.ends_with("committed 2"), "{line}");
                break;
            }
        }
        let mut advanced = false;
        for _ in 0..50 {
            if live.refresh(&state)? {
                advanced = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            advanced,
            "refresh() should observe the second commit within 5s"
        );

        // Confirm via the actual MCP tool, not just the raw QueryState.
        run(async move {
            let server = WorldMcp::new(state);
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
            let result = client
                .call_tool(
                    CallToolRequestParams::new("world_view")
                        .with_arguments(json!({"world":"default"}).as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            let text = &result.content[0].as_text().unwrap().text;
            assert!(text.contains("first"), "{text}");
            assert!(text.contains("dedup-low"), "{text}");
            assert!(!text.contains("dedup-high"), "{text}");
            client.cancel().await.unwrap();
            task.await.unwrap();
        });

        child_stdin.write_all(&[1])?;
        child_stdin.flush()?;
        loop {
            let line = lines.next().ok_or("child exited before final marker")??;
            if line.contains("committed ") {
                assert!(line.ends_with("committed 3"), "{line}");
                break;
            }
        }
        child_stdin.write_all(&[1])?;
        child_stdin.flush()?;
        assert!(child.wait()?.success());
        Ok(())
    }
}
