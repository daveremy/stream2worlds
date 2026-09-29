//! Refusals of `h-measure freeze` and the pins it checks, on a temporary root (s2w#56 PR 2b).

use std::fs;
use std::path::{Path, PathBuf};

use super::freeze::freeze;
use super::pins::{DATA, sha256};

pub(super) const KEY: &str = "dev-key-v0.json";

/// Three SSE frames, each a small recentchange-shaped payload.
fn corpus_text() -> String {
    (1..=3)
        .map(|i| {
            format!(
                "id: [{{\"offset\":{i}}}]\ndata: {{\"type\":\"edit\",\"title\":\"P{i}\",\"wiki\":\"enwiki\",\"user\":\"U{i}\"}}\n\n"
            )
        })
        .collect()
}

/// A root with one pinned key and four corpora: `dev` (development), `held` (heldout), `res`
/// (reserved) and `short` (development, pinned with the wrong event count). Returns the root
/// and the corpus dir.
pub(super) fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("s2w-h-freeze-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let data = root.join(DATA);
    let corpora = root.join("corpora");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&corpora).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let key = fs::read(repo.join(DATA).join(KEY)).unwrap();
    fs::write(data.join(KEY), &key).unwrap();
    fs::write(
        data.join("keys.toml"),
        format!(
            "[[key]]\nfile = \"{KEY}\"\nvariant = \"base\"\nsha256 = \"{}\"\n",
            sha256(&key)
        ),
    )
    .unwrap();
    let text = corpus_text();
    fs::write(corpora.join("c.sse"), &text).unwrap();
    let hash = sha256(text.as_bytes());
    let entry = |name: &str, role: &str, events: usize| {
        format!(
            "[corpus.{name}]\nrole = \"{role}\"\nfile = \"c.sse\"\nevents = {events}\nsha256 = \"{hash}\"\n\n"
        )
    };
    fs::write(
        data.join("corpora.toml"),
        entry("dev", "development", 3)
            + &entry("held", "heldout", 3)
            + &entry("res", "reserved", 3)
            + &entry("short", "development", 4),
    )
    .unwrap();
    (root, corpora)
}

fn refused(root: &Path, dir: &Path, corpus: &str, out: &Path, expected: &str) {
    let err = freeze(root, dir, corpus, 3, out).expect_err("freeze should refuse");
    assert!(err.contains(expected), "{err:?} does not say {expected:?}");
    assert!(!out.exists() || expected.contains("never overwritten"));
}

#[test]
fn freeze_writes_once_and_records_every_pin() {
    let (root, dir) = fixture("once");
    let out = root.join("frozen.json");
    freeze(&root, &dir, "dev", 3, &out).unwrap();
    let frozen: serde_json::Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(frozen["corpus"], "dev");
    assert!(frozen["pins"][format!("key {KEY}")].is_string());
    assert!(frozen["pins"]["corpus held"].is_string());
    let before = fs::read(&out).unwrap();
    refused(&root, &dir, "dev", &out, "never overwritten");
    assert_eq!(fs::read(&out).unwrap(), before);
}

#[test]
fn freeze_refuses_a_heldout_corpus() {
    let (root, dir) = fixture("heldout");
    refused(
        &root,
        &dir,
        "held",
        &root.join("f.json"),
        "only on the development corpus",
    );
}

#[test]
fn freeze_refuses_an_unknown_corpus() {
    let (root, dir) = fixture("unknown");
    refused(
        &root,
        &dir,
        "nope",
        &root.join("f.json"),
        "no corpus \"nope\"",
    );
}

#[test]
fn freeze_refuses_a_corpus_that_does_not_match_its_pin() {
    let (root, dir) = fixture("corpus-hash");
    fs::write(dir.join("c.sse"), corpus_text() + "\n").unwrap();
    refused(
        &root,
        &dir,
        "dev",
        &root.join("f.json"),
        "does not match its pin",
    );
}

#[test]
fn freeze_refuses_a_wrong_event_count() {
    let (root, dir) = fixture("events");
    refused(
        &root,
        &dir,
        "short",
        &root.join("f.json"),
        "3 events, the pin says 4",
    );
}

#[test]
fn freeze_refuses_a_changed_key() {
    let (root, dir) = fixture("key-hash");
    let path = root.join(DATA).join(KEY);
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b'\n');
    fs::write(&path, bytes).unwrap();
    refused(
        &root,
        &dir,
        "dev",
        &root.join("f.json"),
        "does not match its pin",
    );
}

#[test]
fn freeze_refuses_a_key_pinned_twice() {
    let (root, dir) = fixture("twice");
    let path = root.join(DATA).join("keys.toml");
    let once = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{once}\n{once}")).unwrap();
    refused(
        &root,
        &dir,
        "dev",
        &root.join("f.json"),
        "pins dev-key-v0.json twice",
    );
}

#[test]
fn freeze_refuses_a_window_outside_the_corpus() {
    let (root, dir) = fixture("window");
    for window in [0, 4] {
        let out = root.join(format!("w{window}.json"));
        let err = freeze(&root, &dir, "dev", window, &out).expect_err("window should refuse");
        assert!(err.contains("must be 1 to 3"), "{err:?}");
        assert!(!out.exists());
    }
}

#[test]
fn freeze_records_the_corpus_role_file_and_events() {
    let (root, dir) = fixture("corpus-pin");
    let out = root.join("frozen.json");
    freeze(&root, &dir, "dev", 3, &out).unwrap();
    let frozen: serde_json::Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    let hash = sha256(corpus_text().as_bytes());
    assert_eq!(
        frozen["pins"]["corpus held"],
        format!("Heldout c.sse 3 {hash}")
    );
}

#[test]
fn freeze_refuses_a_missing_key_file() {
    let (root, dir) = fixture("key-missing");
    fs::remove_file(root.join(DATA).join(KEY)).unwrap();
    refused(&root, &dir, "dev", &root.join("f.json"), KEY);
}

#[test]
fn a_flag_is_never_taken_as_the_previous_flags_value() {
    let args: Vec<String> = ["--out", "--dir", "/x"].map(String::from).to_vec();
    let err = super::flags(&args).expect_err("--out has no value");
    assert!(err.contains("--out needs a value"), "{err:?}");
}
