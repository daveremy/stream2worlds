//! `s2w dashboard propose --system2-*` end to end (s2w#311): a model command that prints a
//! manifest is filed as an accepted row under the configured model, one that prints anything
//! else is a null-manifest row with a reject, and bad flags exit 2 before anything is opened.
//! The "model" is a local `/bin/sh` script; no model is called.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use s2w_app::routes::{ENVELOPE_FORMAT, MappingEnvelope, STREAM_MAPPING_CLASS};
use s2w_log::{
    Actor, Decider, EventLog, LogPosition, NewDecision, NewProposal, Outcome, ProposalStore,
    SqliteEventLog, SqliteProposalStore,
};
use s2w_model::{Cursor, RawEvent, SourceId, StreamMapping, Timestamp};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

const SOURCE: &str = "test.mapped";

fn s2w(args: &[&str]) -> Fallible<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_s2w"))
        .args(args)
        .output()?)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A log holding the sample events for `SOURCE`, and an accepted stream mapping for it.
fn mapped(name: &str) -> Fallible<TestDirectory> {
    let dir = TestDirectory::new(name)?;
    let mut log = SqliteEventLog::open(&dir.0)?;
    let events = include_str!("../../s2w-system1/testdata/raw-sample.jsonl")
        .lines()
        .zip(0_i64..)
        .map(|(line, i)| {
            Ok(RawEvent {
                source: SourceId::new(SOURCE)?,
                cursor: Cursor::new(format!("c-{i}"))?,
                received_at: Timestamp::from_millis(1_000 + i),
                payload: line.as_bytes().to_vec(),
            })
        })
        .collect::<Result<Vec<_>, s2w_model::ModelError>>()?;
    log.append_batch(events)?;
    drop(log);

    let mapping: StreamMapping = serde_json::from_str(include_str!(
        "../../s2w-system1/testdata/sample.mapping.json"
    ))?;
    let mut store = SqliteProposalStore::open(&dir.0)?;
    store.append_proposal(&NewProposal {
        id: "m1".to_owned(),
        class: STREAM_MAPPING_CLASS.to_owned(),
        actor: Actor::Human {
            id: "dave".to_owned(),
        },
        snapshot_offset: LogPosition::from_u64(1).ok_or("position")?,
        payload: serde_json::to_vec(&MappingEnvelope {
            format: ENVELOPE_FORMAT,
            source: SOURCE.to_owned(),
            mapping,
        })?,
        proposed_at_ms: 0,
    })?;
    store.append_decision(&NewDecision {
        proposal_id: "m1".to_owned(),
        decider: Decider::Policy,
        outcome: Outcome::Accept,
        basis: "seeded".to_owned(),
        decided_at_ms: 0,
    })?;
    Ok(dir)
}

/// `dashboard propose --log-dir <dir> --json <extra...>`, parsed; asserts exit 0.
fn propose(dir: &TestDirectory, extra: &[&str]) -> Fallible<Value> {
    let path = dir.path();
    let mut args = vec!["dashboard", "propose", "--log-dir", &path, "--json"];
    args.extend_from_slice(extra);
    let out = s2w(&args)?;
    assert!(out.status.success(), "{}", text(&out.stderr));
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// The deterministic proposer's manifest for this log, as the "model" will print it.
fn fallback_manifest(dir: &TestDirectory) -> Fallible<PathBuf> {
    let report = propose(dir, &["--dry-run"])?;
    let manifest = &report["envelope"]["manifest"];
    assert!(manifest.is_object(), "{report}");
    let file = dir.0.with_extension("manifest.json");
    std::fs::write(&file, serde_json::to_vec(manifest)?)?;
    Ok(file)
}

fn store_entries(dir: &Path) -> Fallible<usize> {
    Ok(std::fs::read_dir(dir)?.count())
}

#[test]
fn a_command_that_prints_a_manifest_is_filed_accepted_under_the_given_model() -> TestResult {
    let dir = mapped("system2-ok")?;
    let manifest = fallback_manifest(&dir)?;
    let manifest = manifest.display().to_string();
    // The script reads the prompt, then prints the manifest file named by `$0`.
    let run = [
        "--system2-model",
        "vendor/model-x/2026-09",
        "--system2-cmd",
        "/bin/sh",
        "-c",
        "/bin/cat >/dev/null; /bin/cat \"$0\"",
        &manifest,
    ];
    let before = store_entries(&dir.0)?;
    let dry = propose(&dir, &[&["--dry-run"][..], &run[..]].concat())?;
    assert_eq!(dry["action"], json!("dry_run"), "{dry}");
    assert_eq!(dry["decision"], json!("accept"), "{dry}");
    assert_eq!(store_entries(&dir.0)?, before, "a dry run writes nothing");

    let report = propose(&dir, &run)?;
    assert_eq!(report["action"], json!("filed"), "{report}");
    assert_eq!(report["actor"], json!("vendor/model-x/2026-09"));
    assert_eq!(report["decision"], json!("accept"), "{report}");

    let again = propose(&dir, &run)?;
    assert_eq!(again["action"], json!("skipped"), "{again}");

    let shown = s2w(&["dashboard", "show", "--log-dir", &dir.path(), "--json"])?;
    let view: Value = serde_json::from_slice(&shown.stdout)?;
    assert_eq!(view["proposal_id"], report["proposal_id"]);
    assert_eq!(
        view["actor"],
        json!({"kind": "agent", "model": "vendor/model-x", "version": "2026-09"})
    );
    Ok(())
}

#[test]
fn a_command_that_prints_no_manifest_is_a_null_row_with_a_reject() -> TestResult {
    let dir = mapped("system2-bad")?;
    let report = propose(
        &dir,
        &[
            "--system2-model",
            "m/1",
            "--system2-env",
            "PATH",
            "--system2-cmd",
            "sh",
            "-c",
            "cat >/dev/null; echo 'no manifest here'",
        ],
    )?;
    assert_eq!(report["action"], json!("filed"), "{report}");
    assert_eq!(report["decision"], json!("reject"), "{report}");
    let basis = report["basis"].as_str().ok_or("basis")?;
    assert!(basis.starts_with("invalid: not JSON"), "{basis}");
    let shown = s2w(&["dashboard", "show", "--log-dir", &dir.path(), "--json"])?;
    let view: Value = serde_json::from_slice(&shown.stdout)?;
    assert_eq!(
        view["manifest"],
        Value::Null,
        "a rejected row is not in effect"
    );
    Ok(())
}

#[test]
fn bad_system2_flags_exit_2_and_an_unset_variable_exits_1_before_anything_opens() -> TestResult {
    let dir = TestDirectory::new("system2-usage")?;
    let path = dir.path();
    for bad in [
        &["--system2-cmd", "/bin/true"][..],
        &["--system2-model", "m/v"][..],
        &["--system2-env", "HOME"][..],
        &["--system2-model", "m", "--system2-cmd", "/bin/true"][..],
    ] {
        let mut args = vec!["dashboard", "propose", "--log-dir", &path];
        args.extend_from_slice(bad);
        let out = s2w(&args)?;
        assert_eq!(out.status.code(), Some(2), "{bad:?}: {}", text(&out.stderr));
    }
    let out = s2w(&[
        "dashboard",
        "propose",
        "--log-dir",
        &path,
        "--json",
        "--system2-model",
        "m/v",
        "--system2-env",
        "S2W_TEST_SURELY_UNSET_VARIABLE",
        "--system2-cmd",
        "/bin/true",
    ])?;
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let body: Value = serde_json::from_slice(&out.stderr)?;
    assert_eq!(body["error"], json!("bad_parameter"), "{body}");
    assert_eq!(store_entries(&dir.0)?, 0, "nothing was opened");
    Ok(())
}

/// A fresh directory under the system temp dir, removed on drop (even when a test panics).
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Fallible<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "s2w-dashboard-cli-{name}-{}-{nanos}",
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
        let _ignored = std::fs::remove_file(self.0.with_extension("manifest.json"));
    }
}
