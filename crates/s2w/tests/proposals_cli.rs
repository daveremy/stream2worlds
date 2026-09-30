//! `s2w proposals list|grade|propose|decide` end to end (stream2worlds#185, #309): the reads never create the
//! store, a human decision moves only the human tally, data errors exit 1 with the same JSON body
//! HTTP and MCP serve, and usage errors exit 2 before anything is opened.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use s2w_app::routes::{ENVELOPE_FORMAT, MappingEnvelope, STREAM_MAPPING_CLASS};
use s2w_log::{
    Actor, Decider, LogError, LogPosition, NewDecision, NewProposal, Outcome, ProposalStore,
    SqliteProposalStore,
};
use s2w_model::StreamMapping;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

fn s2w(args: &[&str]) -> Fallible<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_s2w"))
        .args(args)
        .output()?)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn propose(store: &mut SqliteProposalStore, id: &str, class: &str, payload: Vec<u8>) -> TestResult {
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

fn decide(store: &mut SqliteProposalStore, id: &str, decider: Decider) -> TestResult {
    store.append_decision(&NewDecision {
        proposal_id: id.to_owned(),
        decider,
        outcome: Outcome::Accept,
        basis: "seeded".to_owned(),
        decided_at_ms: 0,
    })?;
    Ok(())
}

/// A store holding `p1` (class-x) with an agent accept.
fn seeded(name: &str) -> Fallible<TestDirectory> {
    let dir = TestDirectory::new(name)?;
    let mut store = SqliteProposalStore::open(&dir.0)?;
    propose(&mut store, "p1", "class-x", b"{}".to_vec())?;
    decide(&mut store, "p1", Decider::Agent)?;
    Ok(dir)
}

fn decide_args<'a>(dir: &'a str, id: &'a str, outcome: &'a str) -> Vec<&'a str> {
    vec![
        "proposals",
        "decide",
        "--log-dir",
        dir,
        "--proposal",
        id,
        "--outcome",
        outcome,
        "--basis",
        "looked at it",
        "--reviewer",
        "dave",
    ]
}

/// The (human, agent) `[accepted, rejected]` pairs of the only grade row.
fn human_and_agent(grades: &Value) -> Fallible<(Value, Value)> {
    let grade = grades.get(0).ok_or("no grade row")?;
    let pair = |name: &str| json!([grade[name]["accepted"], grade[name]["rejected"]]);
    Ok((pair("human"), pair("agent")))
}

fn entries(dir: &Path) -> Fallible<usize> {
    Ok(std::fs::read_dir(dir)?.count())
}

#[test]
fn list_on_a_missing_store_is_empty_and_creates_nothing() -> TestResult {
    let dir = TestDirectory::new("empty")?;
    let path = dir.path();
    let json_run = s2w(&["proposals", "list", "--log-dir", &path, "--json"])?;
    assert!(json_run.status.success(), "{}", text(&json_run.stderr));
    let view: Value = serde_json::from_slice(&json_run.stdout)?;
    assert_eq!(
        view,
        json!({"proposals": [], "decisions": [], "grades": []})
    );
    let human = s2w(&["proposals", "list", "--log-dir", &path])?;
    assert!(human.status.success());
    assert_eq!(text(&human.stdout), format!("no proposals in {path}\n"));
    let grade = s2w(&["proposals", "grade", "--log-dir", &path, "--json"])?;
    assert_eq!(serde_json::from_slice::<Value>(&grade.stdout)?, json!([]));
    assert_eq!(entries(&dir.0)?, 0, "a read must never create the store");
    Ok(())
}

#[test]
fn a_human_decide_moves_only_the_human_tally() -> TestResult {
    let dir = seeded("decide")?;
    let path = dir.path();
    let before: Value = serde_json::from_slice(
        &s2w(&["proposals", "grade", "--log-dir", &path, "--json"])?.stdout,
    )?;
    assert_eq!(human_and_agent(&before)?, (json!([0, 0]), json!([1, 0])));

    let mut args = decide_args(&path, "p1", "reject");
    args.push("--json");
    let run = s2w(&args)?;
    assert!(run.status.success(), "{}", text(&run.stderr));
    let recorded: Value = serde_json::from_slice(&run.stdout)?;
    assert_eq!(recorded["decision"]["decider"], "human");
    assert_eq!(recorded["decision"]["outcome"], "reject");
    assert_eq!(recorded["decision"]["basis"], "reviewer=dave; looked at it");
    assert_eq!(recorded["route"], Value::Null);

    let after: Value = serde_json::from_slice(
        &s2w(&["proposals", "grade", "--log-dir", &path, "--json"])?.stdout,
    )?;
    assert_eq!(human_and_agent(&after)?, (json!([0, 1]), json!([1, 0])));

    let listed = text(&s2w(&["proposals", "list", "--log-dir", &path])?.stdout);
    assert!(
        listed.contains("proposal 1 p1 class class-x by agent m@1"),
        "{listed}"
    );
    assert!(
        listed.contains("human reject at ")
            && listed.contains("basis: reviewer=dave; looked at it"),
        "{listed}"
    );
    let graded = text(&s2w(&["proposals", "grade", "--log-dir", &path])?.stdout);
    assert!(
        graded.starts_with("class-x agent m@1: proposed 1, ungraded 0, human 0/1"),
        "{graded}"
    );
    Ok(())
}

#[test]
fn an_unknown_proposal_exits_1_and_creates_nothing() -> TestResult {
    let dir = TestDirectory::new("unknown")?;
    let path = dir.path();
    let mut args = decide_args(&path, "nope", "accept");
    args.push("--json");
    let run = s2w(&args)?;
    assert_eq!(run.status.code(), Some(1));
    let body: Value = serde_json::from_slice(&run.stderr)?;
    assert_eq!(body["error"], "unknown_proposal");
    assert_eq!(body["message"], "no proposal with id 'nope'");
    assert_eq!(
        entries(&dir.0)?,
        0,
        "an unknown id must never create the store"
    );

    let human = s2w(&decide_args(&path, "nope", "accept"))?;
    assert_eq!(human.status.code(), Some(1));
    assert_eq!(
        text(&human.stderr),
        format!(
            "s2w: unknown_proposal: no proposal with id 'nope'. Try: s2w proposals list \
             --log-dir {path}\n"
        )
    );
    Ok(())
}

#[test]
fn a_held_writer_is_store_locked_exit_1() -> TestResult {
    let dir = seeded("locked")?;
    let path = dir.path();
    let writer = retry_until_unlocked(|| SqliteProposalStore::open(&dir.0))?;
    let mut args = decide_args(&path, "p1", "accept");
    args.push("--json");
    let run = s2w(&args)?;
    drop(writer);
    assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
    let body: Value = serde_json::from_slice(&run.stderr)?;
    assert_eq!(body["error"], "store_locked");
    let retried = retry_until_unlocked(|| {
        let run = s2w(&args).map_err(|error| LogError::Io(error.to_string()))?;
        if run.status.code() == Some(1) && text(&run.stderr).contains("store_locked") {
            Err(LogError::Locked)
        } else {
            Ok(run)
        }
    })?;
    assert!(retried.status.success(), "{}", text(&retried.stderr));
    Ok(())
}

#[test]
fn usage_errors_exit_2_before_anything_opens() -> TestResult {
    let dir = TestDirectory::new("usage")?;
    let path = dir.path();
    let mut missing_reviewer = decide_args(&path, "p1", "accept");
    missing_reviewer.truncate(missing_reviewer.len() - 2);
    for args in [
        missing_reviewer,
        decide_args(&path, "p1", "maybe"),
        vec!["proposals", "decide", "--proposal", "p1"],
        vec!["proposals", "list", "--world", "w"],
        vec!["proposals"],
        vec!["proposals", "revoke"],
        vec!["--json", "proposals", "list"],
    ] {
        let run = s2w(&args)?;
        assert_eq!(
            run.status.code(),
            Some(2),
            "{args:?}: {}",
            text(&run.stderr)
        );
    }
    assert_eq!(entries(&dir.0)?, 0);
    Ok(())
}

#[test]
fn a_human_reject_of_the_running_mapping_reports_the_source_unrouted() -> TestResult {
    let dir = TestDirectory::new("route")?;
    let path = dir.path();
    let mapping: StreamMapping = serde_json::from_str(include_str!(
        "../../s2w-system1/testdata/sample.mapping.json"
    ))?;
    let payload = serde_json::to_vec(&MappingEnvelope {
        format: ENVELOPE_FORMAT,
        source: "test.mapped".to_owned(),
        mapping,
    })?;
    let mut store = SqliteProposalStore::open(&dir.0)?;
    propose(&mut store, "m1", STREAM_MAPPING_CLASS, payload)?;
    decide(&mut store, "m1", Decider::Policy)?;
    propose(
        &mut store,
        "bad",
        STREAM_MAPPING_CLASS,
        b"not json".to_vec(),
    )?;
    drop(store);

    let listed = text(&s2w(&["proposals", "list", "--log-dir", &path])?.stdout);
    assert!(
        listed.contains("route: source 'test.mapped' runs mapping ")
            && listed.contains(" from proposal m1"),
        "{listed}"
    );

    let accept_bad = s2w(&decide_args(&path, "bad", "accept"))?;
    assert_eq!(accept_bad.status.code(), Some(1));
    assert!(text(&accept_bad.stderr).starts_with("s2w: bad_parameter: "));

    let run = s2w(&decide_args(&path, "m1", "reject"))?;
    assert!(run.status.success(), "{}", text(&run.stderr));
    let out = text(&run.stdout);
    assert!(out.contains("human reject"), "{out}");
    assert!(
        out.contains("source 'test.mapped' is now unrouted"),
        "{out}"
    );
    Ok(())
}

#[test]
fn a_proposed_mapping_routes_only_once_a_human_accepts_it() -> TestResult {
    let dir = TestDirectory::new("propose")?;
    let path = dir.path();
    let mapping = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../s2w-system1/testdata/sample.mapping.json")
        .display()
        .to_string();
    let propose_args = [
        "proposals",
        "propose",
        "--log-dir",
        &path,
        "--source",
        "test.mapped",
        "--mapping",
        &mapping,
        "--author",
        "dave",
        "--json",
    ];
    let first = s2w(&propose_args)?;
    assert!(first.status.success(), "{}", text(&first.stderr));
    let body: Value = serde_json::from_slice(&first.stdout)?;
    let id = body["proposal"]["id"].as_str().ok_or("id")?.to_owned();
    let identity = body["identity"].as_str().ok_or("identity")?.to_owned();
    assert_eq!(body["proposal"]["class"], json!(STREAM_MAPPING_CLASS));
    assert_eq!(
        body["proposal"]["actor"],
        json!({"kind": "human", "id": "dave"})
    );

    // A re-run is an identical retry: the same row, still one proposal, nothing routed yet.
    let again = s2w(&propose_args)?;
    assert!(again.status.success(), "{}", text(&again.stderr));
    let again: Value = serde_json::from_slice(&again.stdout)?;
    assert_eq!(again["proposal"], body["proposal"]);
    let listed = text(&s2w(&["proposals", "list", "--log-dir", &path])?.stdout);
    assert_eq!(listed.matches("proposal ").count(), 1, "{listed}");
    assert!(
        !listed.contains("route: source 'test.mapped' runs"),
        "{listed}"
    );

    let run = s2w(&decide_args(&path, &id, "accept"))?;
    assert!(run.status.success(), "{}", text(&run.stderr));
    let out = text(&run.stdout);
    assert!(
        out.contains(&format!(
            "source 'test.mapped' now runs mapping {identity} from proposal {id}"
        )),
        "{out}"
    );
    Ok(())
}

#[test]
fn an_invalid_mapping_exits_1_and_creates_nothing() -> TestResult {
    let dir = TestDirectory::new("propose-bad")?;
    let path = dir.path();
    let bad = dir.0.with_extension("bad.json");
    std::fs::write(&bad, b"{\"version\": 1}")?;
    let bad = bad.display().to_string();
    for mapping in [bad.as_str(), "/nonexistent/mapping.json"] {
        let run = s2w(&[
            "proposals",
            "propose",
            "--log-dir",
            &path,
            "--source",
            "test.mapped",
            "--mapping",
            mapping,
            "--author",
            "dave",
        ])?;
        assert_eq!(run.status.code(), Some(1), "{mapping}");
        assert!(
            text(&run.stderr).starts_with("s2w: bad_parameter: "),
            "{}",
            text(&run.stderr)
        );
    }
    std::fs::remove_file(&bad)?;
    assert!(!dir.0.exists() || entries(&dir.0)? == 0);
    Ok(())
}

/// Retries `open` while it returns [`LogError::Locked`], bounded by a short deadline.
///
/// Same guard as s2w-log's test helper (#85): a lock file's `flock` is released only once every
/// duplicate of its descriptor is gone, and a concurrently running test's `Command::spawn`
/// forks this binary, briefly duplicating a descriptor we just dropped. Only wrap a reopen
/// that follows our own drop; never the assertion that a held writer refuses.
fn retry_until_unlocked<T>(mut open: impl FnMut() -> Result<T, LogError>) -> Result<T, LogError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        match open() {
            Err(LogError::Locked) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            result => return result,
        }
    }
}

/// A fresh directory under the system temp dir, removed on drop (even when a test panics).
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Fallible<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "s2w-proposals-cli-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}
