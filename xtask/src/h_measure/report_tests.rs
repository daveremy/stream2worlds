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

/// Pins a copy of the fixture key as `other.json` and a heldout corpus `later` on the
/// fixture's bytes, as rows added after a freeze.
fn add_rows(root: &Path) {
    let data = root.join(DATA);
    let bytes = fs::read(data.join(KEY)).unwrap();
    fs::write(data.join("other.json"), &bytes).unwrap();
    let mut rows = fs::read_to_string(data.join("keys.toml")).unwrap();
    rows.push_str(&format!(
        "\n[[key]]\nfile = \"other.json\"\nvariant = \"copy\"\nsha256 = \"{}\"\n",
        sha256(&bytes)
    ));
    fs::write(data.join("keys.toml"), rows).unwrap();
    let corpora = data.join("corpora.toml");
    let mut rows = fs::read_to_string(&corpora).unwrap();
    let hash = sha256(&fs::read(root.join("corpora").join("c.sse")).unwrap());
    rows.push_str(&format!(
        "[corpus.later]\nrole = \"heldout\"\nfile = \"c.sse\"\nevents = 3\nsha256 = \"{hash}\"\n"
    ));
    fs::write(corpora, rows).unwrap();
}

/// Rewrites one field of the frozen file.
fn edit(out: &Path, field: &str, value: serde_json::Value) {
    let mut frozen: serde_json::Value = serde_json::from_slice(&fs::read(out).unwrap()).unwrap();
    frozen[field] = value;
    fs::write(out, serde_json::to_vec_pretty(&frozen).unwrap()).unwrap();
}

#[test]
fn score_ignores_pin_rows_added_after_the_freeze() {
    let (root, dir, out) = frozen("added");
    add_rows(&root);
    score(&root, &dir, &out, "dev", &[KEY]).expect("scores the freeze corpus");
    score(&root, &dir, &out, "later", &[KEY]).expect("scores a corpus pinned later");
}

#[test]
fn score_refuses_a_key_pinned_after_the_freeze() {
    let (root, dir, out) = frozen("late-key");
    add_rows(&root);
    refused(
        score(&root, &dir, &out, "dev", &["other.json"]),
        "key other.json was not pinned when",
    );
}

#[test]
fn score_refuses_a_changed_freeze_corpus_pin() {
    let (root, dir, out) = frozen("dev-pin");
    let path = root.join(DATA).join("corpora.toml");
    let text = fs::read_to_string(&path).unwrap();
    let dev = "[corpus.dev]\nrole = \"development\"\nfile = \"c.sse\"\nevents = 3";
    assert!(text.contains(dev));
    fs::write(
        &path,
        text.replace(dev, &dev.replace("events = 3", "events = 4")),
    )
    .unwrap();
    refused(
        score(&root, &dir, &out, "held", &[KEY]),
        "corpus dev: its row in keys.toml or corpora.toml changed or was removed since",
    );
}

#[test]
fn score_refuses_a_changed_scored_corpus_pin_the_freeze_recorded() {
    let (root, dir, out) = frozen("held-pin");
    let path = root.join(DATA).join("corpora.toml");
    let text = fs::read_to_string(&path).unwrap();
    let held = "[corpus.held]\nrole = \"heldout\"\nfile = \"c.sse\"\nevents = 3";
    assert!(text.contains(held));
    fs::write(
        &path,
        text.replace(held, &held.replace("events = 3", "events = 4")),
    )
    .unwrap();
    refused(
        score(&root, &dir, &out, "held", &[KEY]),
        "corpus held: its row in keys.toml or corpora.toml changed or was removed since",
    );
}

#[test]
fn score_refuses_a_scored_key_repinned_since_the_freeze() {
    let (root, dir, out) = frozen("key-repin");
    let data = root.join(DATA);
    let mut bytes = fs::read(data.join(KEY)).unwrap();
    bytes.push(b'\n');
    fs::write(data.join(KEY), &bytes).unwrap();
    let keys = fs::read_to_string(data.join("keys.toml")).unwrap();
    let old = keys.split('"').nth(5).unwrap().to_owned();
    fs::write(data.join("keys.toml"), keys.replace(&old, &sha256(&bytes))).unwrap();
    refused(
        score(&root, &dir, &out, "dev", &[KEY]),
        &format!("key {KEY}: its row in keys.toml or corpora.toml changed or was removed since"),
    );
}

#[test]
fn score_refuses_a_freeze_that_did_not_record_its_corpus_pin() {
    let (root, dir, out) = frozen("no-dev-pin");
    let mut frozen: serde_json::Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    frozen["pins"].as_object_mut().unwrap().remove("corpus dev");
    fs::write(&out, serde_json::to_vec_pretty(&frozen).unwrap()).unwrap();
    refused(
        score(&root, &dir, &out, "dev", &[KEY]),
        "corpus dev was not pinned when",
    );
}

#[test]
fn score_refuses_a_frozen_file_freeze_did_not_write() {
    let (root, dir, out) = frozen("forged");
    let pristine = fs::read(&out).unwrap();
    let forged = "not what freeze writes";
    edit(&out, "abstain", "a reason freeze never gave".into());
    refused(score(&root, &dir, &out, "dev", &[KEY]), forged);
    fs::write(&out, &pristine).unwrap();
    edit(&out, "window", 2.into());
    refused(score(&root, &dir, &out, "dev", &[KEY]), forged);
    fs::write(&out, &pristine).unwrap();
    let mut frozen: serde_json::Value = serde_json::from_slice(&pristine).unwrap();
    frozen["profile"]["skipped"] = 1.into();
    fs::write(&out, serde_json::to_vec_pretty(&frozen).unwrap()).unwrap();
    refused(score(&root, &dir, &out, "dev", &[KEY]), forged);
    fs::write(&out, &pristine).unwrap();
    score(&root, &dir, &out, "dev", &[KEY]).expect("the untouched file scores");
}

#[test]
fn score_refuses_a_freeze_from_another_profiler_build() {
    let (root, dir, out) = frozen("build");
    let pristine = fs::read(&out).unwrap();
    edit(&out, "profiler_version", "0".into());
    refused(
        score(&root, &dir, &out, "dev", &[KEY]),
        "score with the build that froze it",
    );
    fs::write(&out, &pristine).unwrap();
    edit(&out, "config", "Config { min_events: 1 }".into());
    refused(
        score(&root, &dir, &out, "dev", &[KEY]),
        "score with the build that froze it",
    );
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
    refused(score(&root, &dir, &out, "dev", &[KEY, KEY]), "given twice");
}

#[test]
fn score_refuses_a_changed_key_before_reading_the_corpus() {
    let (root, dir, out) = frozen("key-hash");
    let path = root.join(DATA).join(KEY);
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b'\n');
    fs::write(&path, bytes).unwrap();
    // `c.sse` is also the freeze corpus: the key check precedes the freeze re-run.
    fs::remove_file(dir.join("c.sse")).unwrap();
    refused(
        score(&root, &dir, &out, "dev", &[KEY]),
        "does not match its pin",
    );
}

/// Rewrites one corpus row of the fixture's `corpora.toml` after the freeze.
fn edit_corpus(root: &Path, from: &str, to: &str) {
    let path = root.join(DATA).join("corpora.toml");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{text}");
    fs::write(&path, text.replace(from, to)).unwrap();
}

const RES: &str = "[corpus.res]\nrole = \"reserved\"\nfile = \"c.sse\"\nevents = 3";

#[test]
fn score_accepts_a_corpus_opened_since_the_freeze() {
    let (root, dir, out) = frozen("opened");
    edit_corpus(&root, RES, &RES.replace("reserved", "heldout"));
    let markdown = score(&root, &dir, &out, "res", &[KEY]).expect("scores an opened span");
    assert!(markdown.contains(KEY), "{markdown}");
}

#[test]
fn score_refuses_an_opened_corpus_whose_pin_also_changed() {
    let (root, dir, out) = frozen("opened-sha");
    let hash = sha256(fs::read(dir.join("c.sse")).unwrap().as_slice());
    let res = format!("{RES}\nsha256 = \"{hash}\"");
    let opened = res
        .replace("reserved", "heldout")
        .replace(&hash, &"0".repeat(64));
    edit_corpus(&root, &res, &opened);
    refused(
        score(&root, &dir, &out, "res", &[KEY]),
        "corpus res: its row in keys.toml or corpora.toml changed or was removed since",
    );
}

#[test]
fn score_refuses_a_development_corpus_relabelled_heldout_since_the_freeze() {
    // `short` is a development span the mapping's author could have seen; relabelling it
    // held out after the freeze is not an opening. Its pin refuses before its bytes are read.
    let (root, dir, out) = frozen("dev-to-held");
    let short = "[corpus.short]\nrole = \"development\"";
    edit_corpus(&root, short, &short.replace("development", "heldout"));
    refused(
        score(&root, &dir, &out, "short", &[KEY]),
        "corpus short: its row in keys.toml or corpora.toml changed or was removed since",
    );
}

#[test]
fn score_refuses_a_reserved_corpus_relabelled_development_since_the_freeze() {
    let (root, dir, out) = frozen("res-to-dev");
    edit_corpus(&root, RES, &RES.replace("reserved", "development"));
    refused(
        score(&root, &dir, &out, "res", &[KEY]),
        "corpus res: its row in keys.toml or corpora.toml changed or was removed since",
    );
}

#[test]
fn score_marks_a_corpus_with_the_frozen_bytes_in_sample_under_any_name() {
    // `held` pins the same file as `dev` in the fixture.
    let (root, dir, out) = frozen("same-bytes");
    let markdown = score(&root, &dir, &out, "held", &[KEY]).expect("scores");
    assert!(markdown.contains("**In sample**"), "{markdown}");
}

#[test]
fn score_grades_a_heldout_corpus_out_of_sample() {
    let (root, dir) = fixture("score-heldout");
    let text: String = (1..=2)
        .map(|i| {
            format!(
                "id: [{{\"offset\":{i}}}]\ndata: {{\"type\":\"edit\",\"title\":\"Q{i}\",\"wiki\":\"dewiki\",\"user\":\"V{i}\"}}\n\n"
            )
        })
        .collect();
    fs::write(dir.join("h.sse"), &text).unwrap();
    let manifest = root.join(DATA).join("corpora.toml");
    let mut rows = fs::read_to_string(&manifest).unwrap();
    rows.push_str(&format!(
        "[corpus.other]\nrole = \"heldout\"\nfile = \"h.sse\"\nevents = 2\nsha256 = \"{}\"\n",
        sha256(text.as_bytes())
    ));
    fs::write(&manifest, rows).unwrap();
    let out = root.join("frozen.json");
    freeze(&root, &dir, "dev", 3, &out).expect("freezes");
    let markdown = score(&root, &dir, &out, "other", &[KEY]).expect("scores");
    assert!(!markdown.contains("In sample"), "{markdown}");
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("score.json")).unwrap()).unwrap();
    assert_eq!(json["records"], 2);
}

#[test]
fn score_grades_an_abstained_freeze_as_the_empty_prediction() {
    // The fixture corpus has 3 events, under the profiler's minimum, so the freeze abstains.
    let (root, dir, out) = frozen("abstain");
    let markdown = score(&root, &dir, &out, "dev", &[KEY]).expect("scores");
    assert!(
        markdown.contains("abstained: 3 events, fewer than"),
        "{markdown}"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("score.json")).unwrap()).unwrap();
    let micro = &json["keys"][0]["grade"]["mapping"]["micro"];
    assert!(micro["precision"].is_null(), "{micro}");
    assert_eq!(micro["recall"], 0.0);
}

/// Replaces the fixture corpus with `n` frames, enough for the profiler to propose a mapping,
/// and re-pins every corpus row to it.
fn large_corpus(root: &Path, dir: &Path, n: usize) {
    let text: String = (1..=n)
        .map(|i| {
            format!(
                "id: [{{\"offset\":{i}}}]\ndata: {{\"type\":\"edit\",\"title\":\"P{}\",\"ns\":\"n{}\",\"user\":\"U{}\"}}\n\n",
                i % 60,
                i % 60 / 2,
                i % 13
            )
        })
        .collect();
    let old = sha256(&fs::read(dir.join("c.sse")).unwrap());
    fs::write(dir.join("c.sse"), &text).unwrap();
    let path = root.join(DATA).join("corpora.toml");
    let rows = fs::read_to_string(&path)
        .unwrap()
        .replace(&old, &sha256(text.as_bytes()))
        .replace("events = 3", &format!("events = {n}"));
    fs::write(&path, rows).unwrap();
}

#[test]
fn score_accepts_a_real_mapping_freeze_and_refuses_an_edited_one() {
    let (root, dir) = fixture("score-mapping");
    large_corpus(&root, &dir, 1200);
    let out = root.join("frozen.json");
    freeze(&root, &dir, "dev", 1200, &out).expect("freezes");
    let pristine = fs::read(&out).unwrap();
    let mut frozen: serde_json::Value = serde_json::from_slice(&pristine).unwrap();
    assert!(frozen["mapping"].is_object(), "{frozen}");
    score(&root, &dir, &out, "held", &[KEY]).expect("a real freeze scores");
    let entities = frozen["mapping"]["entities"].as_array_mut().unwrap();
    assert!(!entities.is_empty(), "{frozen}");
    entities.pop();
    fs::write(&out, serde_json::to_vec_pretty(&frozen).unwrap()).unwrap();
    refused(
        score(&root, &dir, &out, "held", &[KEY]),
        "not what freeze writes",
    );
}

#[test]
fn score_names_the_freeze_re_run_when_its_recorded_window_is_impossible() {
    let (root, dir, out) = frozen("window-0");
    edit(&out, "window", 0.into());
    refused(
        score(&root, &dir, &out, "dev", &[KEY]),
        "re-running the freeze recorded in",
    );
}
