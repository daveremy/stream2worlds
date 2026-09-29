//! Refusals of `h-measure score`, on the temporary root of `freeze_tests` (s2w#56 PR 2b).

use std::fs;
use std::path::{Path, PathBuf};

use super::freeze::freeze;
use super::freeze_tests::{KEY, fixture};
use super::pins::{DATA, sha256};
use super::report::{Request, run};

/// A fixture root with a mapping frozen on `dev`. Returns the root, corpus dir and frozen file.
fn frozen(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (root, dir) = fixture(&format!("score-{name}"));
    let out = root.join("frozen.json");
    freeze(&root, &dir, "dev", 3, &out).expect("freezes");
    (root, dir, out)
}

fn score(
    root: &Path,
    dir: &Path,
    frozen: &Path,
    corpus: &str,
    keys: &[&str],
) -> Result<String, String> {
    let keys: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
    let json = root.join("score.json");
    let request = Request {
        frozen,
        corpus,
        keys: &keys,
        dir,
        json: Some(&json),
    };
    run(root, &request)
}

fn refused(got: Result<String, String>, expected: &str) {
    let err = got.expect_err("score should refuse");
    assert!(err.contains(expected), "{err:?} does not say {expected:?}");
}

#[test]
fn score_grades_a_frozen_mapping_and_writes_every_number() {
    let (root, dir, out) = frozen("ok");
    let markdown = score(&root, &dir, &out, "dev", &[KEY]).expect("scores");
    assert!(markdown.contains(KEY), "{markdown}");
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("score.json")).unwrap()).unwrap();
    assert_eq!(json["records"], 3);
    assert_eq!(json["keys"][0]["file"], KEY);
}

#[test]
fn score_refuses_pins_changed_since_the_freeze() {
    let (root, dir, out) = frozen("pins");
    let data = root.join(DATA);
    let bytes = fs::read(data.join(KEY)).unwrap();
    fs::write(data.join("other.json"), &bytes).unwrap();
    let mut rows = fs::read_to_string(data.join("keys.toml")).unwrap();
    rows.push_str(&format!(
        "\n[[key]]\nfile = \"other.json\"\nvariant = \"copy\"\nsha256 = \"{}\"\n",
        sha256(&bytes)
    ));
    fs::write(data.join("keys.toml"), rows).unwrap();
    refused(score(&root, &dir, &out, "dev", &[KEY]), "changed since");
}

#[test]
fn score_refuses_a_mapping_not_frozen_on_the_development_corpus() {
    let (root, dir, out) = frozen("origin");
    let text = fs::read_to_string(&out).unwrap();
    fs::write(
        &out,
        text.replace("\"corpus\": \"dev\"", "\"corpus\": \"held\""),
    )
    .unwrap();
    refused(
        score(&root, &dir, &out, "dev", &[KEY]),
        "not frozen on the pinned development corpus",
    );
}

#[test]
fn score_refuses_a_reserved_corpus() {
    let (root, dir, out) = frozen("reserved");
    refused(score(&root, &dir, &out, "res", &[KEY]), "never scored here");
}

#[test]
fn score_refuses_no_key_and_an_unpinned_key() {
    let (root, dir, out) = frozen("keys");
    refused(score(&root, &dir, &out, "dev", &[]), "at least one --key");
    refused(
        score(&root, &dir, &out, "dev", &["other.json"]),
        "is not pinned",
    );
}

#[test]
fn score_refuses_a_changed_key_before_reading_the_corpus() {
    let (root, dir, out) = frozen("key-hash");
    let path = root.join(DATA).join(KEY);
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b'\n');
    fs::write(&path, bytes).unwrap();
    fs::remove_file(dir.join("c.sse")).unwrap();
    refused(
        score(&root, &dir, &out, "dev", &[KEY]),
        "does not match its pin",
    );
}

#[test]
fn score_refuses_a_corpus_relabelled_since_the_freeze() {
    let (root, dir, out) = frozen("relabel");
    let path = root.join(DATA).join("corpora.toml");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("role = \"reserved\"", "role = \"heldout\""),
    )
    .unwrap();
    refused(score(&root, &dir, &out, "res", &[KEY]), "changed since");
}
