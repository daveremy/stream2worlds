//! `gate3 commit` and its replay under `score`, on the temporary root of `freeze_tests`, with a
//! fake `claude` that prints recorded-shape envelopes (no live model, no key).

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use s2w_system2::{CallGate as _, CallRecord};

use super::super::freeze_tests::{KEY, fixture};
use super::super::pins::DATA;
use super::super::report::{Request, run as score};
use super::ledger::{BudgetGate, CAP_USD};
use super::prices::{self, Price};
use super::session::{MIN_TOKEN_LIFE_MS, Session, argv};
use super::{commit, now_ms};

const MODEL: &str = "claude-sonnet-5-5";

const PRICES: &str = "[model.\"claude-sonnet-5-5\"]\ninput = 2.0\noutput = 10.0\ncache_write = 4.0\ncache_read = 0.2\nsource_url = \"https://example.invalid/pricing\"\ncopied_on = \"2026-09-30\"\n";

fn price() -> Price {
    Price {
        input: 2.0,
        output: 10.0,
        cache_write: 4.0,
        cache_read: 0.2,
        source_url: String::new(),
        copied_on: String::new(),
    }
}

/// A Claude CLI envelope replying `result` with these token counts.
fn envelope(result: &str, output_tokens: u64) -> String {
    serde_json::json!({
        "type": "result", "subtype": "success", "is_error": false, "result": result,
        "total_cost_usd": 0.0218742,
        "usage": {"input_tokens": 2, "output_tokens": output_tokens,
                  "cache_creation_input_tokens": 5431, "cache_read_input_tokens": 531},
        "modelUsage": {MODEL: {}}
    })
    .to_string()
}

fn record(input: Option<u64>, output: Option<u64>) -> CallRecord {
    CallRecord {
        attempt: 1,
        call: 1,
        prompt_hash: "0".repeat(16),
        reply: Some(String::new()),
        error: None,
        model: None,
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: None,
        cache_write_tokens: None,
        cost_usd: None,
        latency_ms: None,
        started_at_ms: None,
    }
}

/// A fixture root with a price table, credentials valid for two hours, and a fake `claude`
/// that answers the n-th call with `replies[n]`. The fake exits 1 unless its arguments are the
/// clean-session argv, its cwd is empty and its `HOME` holds only the credentials file.
struct Run {
    root: PathBuf,
    dir: PathBuf,
    out: PathBuf,
    args: Vec<String>,
}

fn setup(name: &str, replies: &[String]) -> Run {
    let (root, dir) = fixture(&format!("gate3-{name}"));
    fs::write(root.join(DATA).join(prices::FILE), PRICES).unwrap();
    let fake = root.join("fake");
    fs::create_dir_all(&fake).unwrap();
    for (n, reply) in replies.iter().enumerate() {
        fs::write(fake.join(format!("reply-{}.json", n + 1)), reply).unwrap();
    }
    let creds = root.join("credentials.json");
    let expires = now_ms() + 2 * 60 * 60 * 1000;
    fs::write(
        &creds,
        format!("{{\"claudeAiOauth\":{{\"accessToken\":\"a\",\"refreshToken\":\"r\",\"expiresAt\":{expires}}}}}"),
    )
    .unwrap();
    let expected = argv(Path::new("x"), MODEL)[1..].join(" ");
    let f = fake.display();
    let script = format!(
        "#!/bin/sh\ncat >/dev/null\n\
         [ -z \"$(ls -A .)\" ] || {{ echo cwd not empty >&2; exit 1; }}\n\
         [ \"$(cd \"$HOME\" && find . | sort | tr '\\n' ' ')\" = '. ./.claude ./.claude/.credentials.json ' ] || {{ echo HOME not clean >&2; exit 1; }}\n\
         [ \"$*\" = '{expected}' ] || {{ echo argv \"$*\" >&2; exit 1; }}\n\
         [ \"$CLAUDE_CODE_MAX_OUTPUT_TOKENS\" = 16384 ] || {{ echo output limit not set >&2; exit 1; }}\n\
         n=$(cat {f}/count 2>/dev/null || echo 0); n=$((n+1)); echo $n > {f}/count\n\
         cat {f}/reply-$n.json\n"
    );
    let claude = fake.join("claude");
    fs::write(&claude, script).unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
    let out = root.join("committed").join("h-s2.dev.r1.json");
    let args = [
        "--corpus",
        "dev",
        "--window",
        "3",
        "--replicate",
        "1",
        "--model",
        MODEL,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .chain([
        "--out".to_owned(),
        out.display().to_string(),
        "--dir".to_owned(),
        dir.display().to_string(),
        "--claude".to_owned(),
        claude.display().to_string(),
        "--credentials".to_owned(),
        creds.display().to_string(),
    ])
    .collect();
    Run {
        root,
        dir,
        out,
        args,
    }
}

impl Run {
    fn commit(&self, extra: &[&str]) -> Result<String, String> {
        let mut args = self.args.clone();
        args.extend(extra.iter().map(|s| (*s).to_owned()));
        commit(&self.root, &super::super::flags(&args)?)
    }

    fn score(&self) -> Result<String, String> {
        let keys = [KEY.to_owned()];
        let json = self.root.join("score.json");
        let request = Request {
            frozen: &self.out,
            corpus: "held",
            keys: &keys,
            dir: &self.dir,
            json: Some(&json),
        };
        score(&self.root, &request)
    }

    fn edit(&self, path: &Path, field: &str, value: serde_json::Value) {
        let mut doc: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        doc[field] = value;
        fs::write(path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    }

    fn committed(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(&self.out).unwrap()).unwrap()
    }
}

/// A mapping that matches the fixture's stored events (`freeze_tests::corpus_text`): the
/// envelope's `data` is a JSON string, so it decodes `data` first (s2w#409).
const MATCHING_REPLY: &str = r#"{"version":2,"decode":[["data"]],"entities":[{"id":"page","type_label":"page","key":[["data","wiki"],["data","title"]],"attrs":[]},{"id":"user","type_label":"user","key":[["data","wiki"],["data","user"]],"attrs":[]}],"relationships":[]}"#;

/// [`MATCHING_REPLY`] without its decode step, the shape of gate 3's second dry run: valid, and
/// it matches no stored event, because `data` is a string there.
const NO_MATCH_REPLY: &str = r#"{"version":2,"decode":[],"entities":[{"id":"page","type_label":"page","key":[["data","wiki"],["data","title"]],"attrs":[]},{"id":"user","type_label":"user","key":[["data","wiki"],["data","user"]],"attrs":[]}],"relationships":[]}"#;

fn refused<T: std::fmt::Debug>(got: Result<T, String>, expected: &str) {
    let err = got.expect_err("should refuse");
    assert!(err.contains(expected), "{err:?} does not say {expected:?}");
}

#[test]
fn commit_writes_both_files_and_score_replays_them() {
    let run = setup("ok", &[envelope("none", 4), envelope(MATCHING_REPLY, 900)]);
    let said = run.commit(&[]).unwrap();
    assert!(said.contains("a mapping, 1 attempts, 2 calls"), "{said}");
    let doc = run.committed();
    assert_eq!(doc["kind"], "s2w-gate3-committed");
    assert_eq!(doc["heuristic"]["corpus"], "dev");
    assert!(doc["failure"].is_null() && doc["mapping"].is_object());
    assert_eq!(doc["probe"]["reply"], "none");
    assert_eq!(doc["spend"]["calls"], 2);
    let per_call = doc["spend"]["per_call_usd"].as_array().unwrap();
    assert!((per_call[0].as_f64().unwrap() - 0.021_874_2).abs() < 1e-12);
    let report = run.score().unwrap();
    assert!(
        report.contains("System 2 (arm h-s2, replicate 1"),
        "{report}"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(run.root.join("score.json")).unwrap()).unwrap();
    assert_eq!(json["system2"]["arm"], "h-s2");
    // Replaying again needs no output files to be absent: score never writes them.
    refused(run.commit(&[]), "never overwritten");
}

#[test]
fn score_refuses_an_edited_committed_file_or_transcript() {
    let run = setup(
        "edit",
        &[envelope("none", 4), envelope(MATCHING_REPLY, 900)],
    );
    run.commit(&[]).unwrap();
    let original = fs::read(&run.out).unwrap();
    run.edit(&run.out, "attempts", 2.into());
    refused(run.score(), "not the recorded result");
    fs::write(&run.out, &original).unwrap();
    run.edit(&run.out, "input_hash", "0000000000000000".into());
    refused(run.score(), "input_hash");
    fs::write(&run.out, &original).unwrap();
    // A recorded spend the replay does not recompute.
    let mut spend = run.committed()["spend"].clone();
    spend["usd"] = (spend["usd"].as_f64().unwrap() + 0.01).into();
    run.edit(&run.out, "spend", spend);
    refused(run.score(), "not the recorded spend");
    fs::write(&run.out, &original).unwrap();
    let transcript = super::replay::transcript_path(&run.out).unwrap();
    let mut text = fs::read_to_string(&transcript).unwrap();
    text.push(' ');
    fs::write(&transcript, text).unwrap();
    refused(run.score(), "transcript_sha256");
}

#[test]
fn a_budget_stop_commits_the_failure_and_scores_the_empty_mapping() {
    // After the probe, the mapping call replies no mapping and reports 600k output tokens ($6):
    // the repair call is refused before it is made.
    let run = setup(
        "budget",
        &[envelope("none", 1), envelope("no mapping here", 600_000)],
    );
    let said = run.commit(&[]).unwrap();
    assert!(said.contains("budget: spent $6.0"), "{said}");
    let doc = run.committed();
    assert!(doc["mapping"].is_null());
    assert_eq!(doc["spend"]["calls"], 2);
    let report = run.score().unwrap();
    assert!(report.contains("failed (budget: spent $6.0"), "{report}");
}

#[test]
fn a_probe_that_sees_context_writes_nothing() {
    let run = setup("probe", &[envelope("CLAUDE.md: be terse", 1)]);
    refused(run.commit(&[]), "reported context besides the probe");
    assert!(!run.out.exists());
    assert!(!super::replay::transcript_path(&run.out).unwrap().exists());
}

#[test]
fn commit_refusals_before_any_call() {
    let run = setup("refuse", &[]);
    refused(run.commit(&["--arm", "b4"]), "the arms are h-s2 and b3");
    refused(
        run.commit(&["--temperature", "1"]),
        "takes no --temperature",
    );
    let mut held = run.args.clone();
    held[1] = "held".to_owned();
    refused(
        commit(&run.root, &super::super::flags(&held).unwrap()),
        "only on the development corpus",
    );
    refused(run.commit(&["--model", "x"]), "given more than once");
    fs::create_dir_all(run.out.parent().unwrap()).unwrap();
    fs::write(&run.out, "").unwrap();
    refused(run.commit(&[]), "never overwritten");
    // The fake was never called.
    assert!(!run.root.join("fake/count").exists());
}

#[test]
fn prices_load_and_refuse() {
    let (root, _) = fixture("gate3-prices");
    let file = root.join(DATA).join(prices::FILE);
    fs::write(&file, PRICES).unwrap();
    assert_eq!(prices::load(&root, MODEL).unwrap().cache_write, 4.0);
    refused(prices::load(&root, "claude-other"), "no row for model");
    fs::write(&file, PRICES.replace("input = 2.0", "input = -2.0")).unwrap();
    refused(prices::load(&root, MODEL), "is not a price");
    fs::write(&file, PRICES.replace("cache_read = 0.2\n", "")).unwrap();
    refused(prices::load(&root, MODEL), "cache_read");
    // The committed table has the row the recorded envelope was charged by.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert_eq!(prices::load(&repo, MODEL).unwrap().input, 2.0);
}

#[test]
fn usd_is_tokens_times_the_table() {
    let mut call = record(Some(2), Some(4));
    call.cache_read_tokens = Some(531);
    call.cache_write_tokens = Some(5431);
    // The recorded envelope's own total_cost_usd.
    assert!((price().usd(&call).unwrap() - 0.021_874_2).abs() < 1e-12);
    assert_eq!(price().usd(&record(None, None)), None);
    // 30 bytes: 15 input tokens plus the system allowance at the cache-write rate, 16384 output.
    let estimate = price().estimate(&"x".repeat(30));
    assert!((estimate - ((15.0 + 8_192.0) * 4.0 + 16_384.0 * 10.0) / 1e6).abs() < 1e-12);
}

#[test]
fn the_gate_charges_every_call_and_stops_at_the_cap() {
    let price = price();
    let mut gate = BudgetGate::new(&price, 1.0);
    gate.before_call("p", &[]).unwrap();
    // A failed call (no tokens) is charged its estimate, never zero.
    let failed = record(None, None);
    let estimate = price.estimate("p");
    assert_eq!(gate.charged(std::slice::from_ref(&failed)), vec![estimate]);
    gate.before_call("p", std::slice::from_ref(&failed))
        .unwrap();
    // A call that reported $4 output puts the next estimate past the cap.
    let huge = record(Some(0), Some(400_000));
    let err = gate.before_call("p", &[failed, huge]).unwrap_err();
    assert!(err.starts_with("budget: spent $5.1"), "{err}");
    assert!(err.ends_with(&format!("cap ${CAP_USD:.2}")), "{err}");
}

#[test]
fn the_session_holds_only_fresh_credentials_and_is_removed() {
    let dir = std::env::temp_dir().join(format!("s2w-gate3-creds-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let creds = dir.join("c.json");
    let now = now_ms();
    let write = |expires: u64| {
        fs::write(
            &creds,
            format!("{{\"claudeAiOauth\":{{\"expiresAt\":{expires}}}}}"),
        )
        .unwrap();
    };
    write(now + MIN_TOKEN_LIFE_MS - 60_000);
    refused(Session::open(&creds, now).map(|_| ()), "lifeos#1252");
    fs::write(&creds, "{}").unwrap();
    refused(Session::open(&creds, now).map(|_| ()), "expiresAt");
    write(now + MIN_TOKEN_LIFE_MS + 60_000);
    let session = Session::open(&creds, now).unwrap();
    let home = session.home().to_owned();
    assert_eq!(
        fs::read(home.join(".claude/.credentials.json")).unwrap(),
        fs::read(&creds).unwrap()
    );
    assert_eq!(fs::read_dir(&home).unwrap().count(), 1);
    assert!(!session.credentials_changed());
    fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
    assert!(session.credentials_changed());
    // The rewritten copy outlives the session, beside the operator's file.
    let kept = session.keep_credentials(&creds).unwrap();
    assert!(kept.starts_with(&dir));
    assert_eq!(fs::read(&kept).unwrap(), b"{}");
    // A 90-minute guard: five calls at the 15-minute timeout, plus 15 minutes.
    assert_eq!(MIN_TOKEN_LIFE_MS, 90 * 60 * 1000);
    drop(session);
    assert!(!home.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn argv_is_the_clean_session_command() {
    assert_eq!(
        argv(Path::new("/bin/claude"), MODEL),
        [
            "/bin/claude",
            "-p",
            "--model",
            MODEL,
            "--tools",
            "",
            "--strict-mcp-config",
            "--no-session-persistence",
            "--output-format",
            "json",
            "--max-budget-usd",
            "5",
        ]
    );
}

mod b3;
mod b3_view;
mod no_match;
mod private_probe;

#[test]
fn parallel_sessions_get_distinct_homes() {
    let dir = std::env::temp_dir().join(format!("s2w-gate3-distinct-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let creds = dir.join("c.json");
    let now = now_ms();
    let expires = now + MIN_TOKEN_LIFE_MS + 60_000;
    fs::write(
        &creds,
        format!("{{\"claudeAiOauth\":{{\"expiresAt\":{expires}}}}}"),
    )
    .unwrap();
    let sessions: Vec<Session> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    (0..16)
                        .map(|_| Session::open(&creds, now).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    let mut homes: Vec<&Path> = sessions.iter().map(Session::home).collect();
    homes.sort();
    homes.dedup();
    assert_eq!(homes.len(), 8 * 16);
    drop(sessions);
    let _ = fs::remove_dir_all(&dir);
}
