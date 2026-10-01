//! The clean-session probe against a private corpus (s2w#371 plan §7, deferred to s2w#373 PR 3):
//! a stand-in private corpus (the synthetic private fixture, pinned as a development corpus) and
//! a fake `claude` that answers the probe with whether anything it can observe (stdin, argv,
//! environment, working directory, `HOME` and both their parents) names the corpus directory, the
//! corpus file or any of the corpus's events.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::super::super::freeze_tests::fixture;
use super::super::super::pins::{DATA, sha256};
use super::super::{commit, now_ms, prices};
use super::{MODEL, PRICES, envelope};

const CORPUS: &str = "private-dev";

#[test]
fn the_probe_session_sees_nothing_of_a_private_corpus() {
    let (root, dir) = fixture("gate3-probe-priv");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let text = fs::read_to_string(repo.join("research/h-measure/private/fixture/synthetic-20.sse"))
        .unwrap();
    let file = format!("{CORPUS}.raw.sse");
    fs::write(dir.join(&file), &text).unwrap();
    let corpora = root.join(DATA).join("corpora.toml");
    let mut pins = fs::read_to_string(&corpora).unwrap();
    pins += &format!(
        "[corpus.{CORPUS}]\nrole = \"development\"\nfile = \"{file}\"\nevents = 20\nsha256 = \"{}\"\n",
        sha256(text.as_bytes())
    );
    fs::write(&corpora, pins).unwrap();
    fs::write(root.join(DATA).join(prices::FILE), PRICES).unwrap();

    let claude = observer(&root, &dir, &text);
    control(&root, &claude, &dir);

    let creds = root.join("credentials.json");
    let expires = now_ms() + 2 * 60 * 60 * 1000;
    fs::write(
        &creds,
        format!("{{\"claudeAiOauth\":{{\"accessToken\":\"a\",\"refreshToken\":\"r\",\"expiresAt\":{expires}}}}}"),
    )
    .unwrap();
    let out = root.join("committed").join("h-s2.private-dev.r1.json");
    let args: Vec<String> = [
        "--corpus",
        CORPUS,
        "--window",
        "20",
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
    // A probe that sees context writes no file (`a_probe_that_sees_context_writes_nothing`),
    // so a leak fails here: print what the observer saw.
    commit(&root, &super::super::super::flags(&args).unwrap()).unwrap_or_else(|e| {
        let seen = fs::read_to_string(root.join("fake/seen.txt")).unwrap_or_default();
        panic!("{e}; the probe session saw: {seen}")
    });
    let doc: serde_json::Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    let seen = fs::read_to_string(root.join("fake/seen.txt")).unwrap();
    assert_eq!(
        doc["probe"]["reply"], "none",
        "the probe session saw: {seen}"
    );
    // The observer ran on the probe and looked at the session's HOME.
    assert!(seen.contains(".credentials.json"), "{seen}");
}

/// Writes the observing fake `claude` under `root` and returns its path: on the probe it replies
/// `none` unless what it can observe names `dir`, the corpus or any line of `text`.
fn observer(root: &Path, dir: &Path, text: &str) -> PathBuf {
    // What would show the session saw the corpus: its directory, its name, any event.
    let fake = root.join("fake");
    fs::create_dir_all(&fake).unwrap();
    let mut needles = vec![dir.display().to_string(), CORPUS.to_owned()];
    needles.extend(
        text.lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter(|data| !data.is_empty())
            .map(str::to_owned),
    );
    assert!(needles.len() > 20, "the fixture's events are needles");
    fs::write(fake.join("needles.txt"), needles.join("\n") + "\n").unwrap();
    fs::write(fake.join("none.json"), envelope("none", 1)).unwrap();
    fs::write(
        fake.join("seen.json"),
        envelope("saw the private corpus", 5),
    )
    .unwrap();
    fs::write(fake.join("other.json"), envelope("no mapping here", 5)).unwrap();
    let f = fake.display();
    let script = format!(
        "#!/bin/sh\ninput=$(cat)\n\
         case \"$input\" in *'This is a check that this session is clean'*) ;; *) cat {f}/other.json; exit 0;; esac\n\
         {{ printf '%s\\n' \"$input\" \"$0\" \"$*\" \"$PWD\"; env; ls -a . .. \"$HOME\" \"$HOME/..\"; find \"$HOME\" -maxdepth 3; }} > {f}/seen.txt 2>&1\n\
         if grep -F -q -f {f}/needles.txt {f}/seen.txt; then cat {f}/seen.json; else cat {f}/none.json; fi\n"
    );
    let claude = fake.join("claude");
    fs::write(&claude, script).unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();

    claude
}

/// The instrument can produce a positive: the same observer, given the corpus directory in its
/// environment, reports it.
fn control(root: &Path, claude: &Path, dir: &Path) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let probe = fs::read_to_string(repo.join("crates/s2w-system2/prompts/clean-session-probe.txt"))
        .unwrap();
    let (home, cwd) = (root.join("control-home"), root.join("control-cwd"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    let mut child = Command::new(claude)
        .env("CORPUS_DIR", dir)
        .env("HOME", &home)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(probe.as_bytes())
        .unwrap();
    let control = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8(control.stdout).unwrap(),
        envelope("saw the private corpus", 5)
    );
}
