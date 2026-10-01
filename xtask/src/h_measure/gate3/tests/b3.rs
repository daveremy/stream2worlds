//! `gate3 commit --arm b3` after an h-s2 replicate, its refits, its refusals and its replay
//! under `score`, with the same fake `claude` as the h-s2 tests.

use std::fs;

use super::{MODEL, Run, envelope, mapping_reply, refused, setup};

/// An envelope whose prompt reported `cache_write` cache-write tokens (input 2, cache read 531).
fn cached(result: &str, cache_write: u64) -> String {
    serde_json::json!({
        "type": "result", "subtype": "success", "is_error": false, "result": result,
        "total_cost_usd": 0.03,
        "usage": {"input_tokens": 2, "output_tokens": 900,
                  "cache_creation_input_tokens": cache_write, "cache_read_input_tokens": 531},
        "modelUsage": {MODEL: {}}
    })
    .to_string()
}

/// The h-s2 envelope's first-prompt tokens: 2 + 5431 + 531.
const T: u64 = 5964;

/// The same run as `run`, writing the b3 replicate beside the h-s2 one; `extra` replaces or
/// adds flags.
fn b3(run: &Run, extra: &[(&str, &str)]) -> (Run, Result<String, String>) {
    let out = run.out.with_file_name("b3.dev.r1.json");
    let mut args = run.args.clone();
    let mut set = |name: &str, value: String| {
        let flag = format!("--{name}");
        if let Some(at) = args.iter().position(|a| *a == flag) {
            args[at + 1] = value;
        } else {
            args.extend([flag, value]);
        }
    };
    set("out", out.display().to_string());
    set("arm", "b3".to_owned());
    set("h-s2", run.out.display().to_string());
    for (name, value) in extra {
        set(name, (*value).to_owned());
    }
    let said = super::super::super::flags(&args).and_then(|f| super::commit(&run.root, &f));
    let b3 = Run {
        root: run.root.clone(),
        dir: run.dir.clone(),
        out,
        args,
    };
    (b3, said)
}

fn calls_made(run: &Run) -> u32 {
    fs::read_to_string(run.root.join("fake/count")).map_or(0, |n| n.trim().parse().unwrap())
}

#[test]
fn b3_after_h_s2_is_sized_by_it_and_score_replays_it() {
    let run = setup(
        "b3",
        &[
            envelope("none", 4),
            envelope(&mapping_reply(), 900),
            envelope("none", 4),
            cached(&mapping_reply(), 5431),
        ],
    );
    run.commit(&[]).unwrap();
    let (b3, said) = b3(&run, &[]);
    let said = said.unwrap();
    assert!(said.contains("a mapping, 1 attempts, 2 calls"), "{said}");
    let doc = b3.committed();
    assert_eq!(doc["arm"], "b3");
    let budget = &doc["budget"];
    assert_eq!(budget["input_tokens"], T);
    let h_s2 = fs::read(&run.out).unwrap();
    assert_eq!(budget["h_s2_sha256"], super::super::sha256(&h_s2));
    // The 3-event fixture's raw events fit the h-s2 prompt whole: k = 1.
    assert!(budget["prompt_bytes"].as_u64().unwrap() > 0);
    assert_eq!(
        budget["fits"],
        serde_json::json!([{"k": 1, "events": 3, "prompt_bytes": budget["fits"][0]["prompt_bytes"], "calls": 1, "input_tokens": T}])
    );
    assert!(
        budget["fits"][0]["prompt_bytes"].as_u64().unwrap()
            <= budget["prompt_bytes"].as_u64().unwrap()
    );
    let report = b3.score().unwrap();
    assert!(report.contains("System 2 (arm b3, replicate 1"), "{report}");
    assert!(report.contains("never the heuristic above"), "{report}");
}

#[test]
fn a_fit_over_the_tolerance_is_spent_and_refit_with_a_larger_k() {
    // 2 + 531 + 6000 = 6533 tokens, over 105% of 5964 (6262).
    let run = setup(
        "b3-refit",
        &[
            envelope("none", 4),
            envelope(&mapping_reply(), 900),
            envelope("none", 4),
            cached(&mapping_reply(), 6000),
            cached(&mapping_reply(), 5431),
        ],
    );
    run.commit(&[]).unwrap();
    let (b3, said) = b3(&run, &[]);
    assert!(said.unwrap().contains("a mapping, 1 attempts, 3 calls"));
    let doc = b3.committed();
    let fits = doc["budget"]["fits"].as_array().unwrap();
    let ks: Vec<_> = fits
        .iter()
        .map(|f| (f["k"].clone(), f["events"].clone()))
        .collect();
    assert_eq!(ks, [(1.into(), 3.into()), (2.into(), 2.into())]);
    assert_eq!(fits[0]["input_tokens"], 6533);
    // Every fit's calls are in the transcript and the ledger.
    assert_eq!(doc["spend"]["calls"], 3);
    b3.score().unwrap();
    // A replay that would fit differently refuses.
    let mut budget = doc["budget"].clone();
    budget["fits"][1]["k"] = 3.into();
    b3.edit(&b3.out, "budget", budget);
    refused(b3.score(), "not the recorded fits");
}

#[test]
fn a_replicate_that_never_fits_commits_a_budget_fit_failure() {
    let run = setup(
        "b3-nofit",
        &[
            envelope("none", 4),
            envelope(&mapping_reply(), 900),
            envelope("none", 4),
            cached(&mapping_reply(), 6000),
            cached(&mapping_reply(), 6000),
            cached(&mapping_reply(), 6000),
        ],
    );
    run.commit(&[]).unwrap();
    let (b3, said) = b3(&run, &[]);
    let said = said.unwrap();
    // k = 3 samples the first event alone, as every larger k would: there is nothing to refit.
    assert!(
        said.contains("budget-fit: fit k=3 read 6533 prompt tokens, over 105% of 5964"),
        "{said}"
    );
    let doc = b3.committed();
    assert!(doc["mapping"].is_null());
    assert_eq!(doc["budget"]["fits"].as_array().unwrap().len(), 3);
    assert_eq!(doc["spend"]["calls"], 4);
    let report = b3.score().unwrap();
    assert!(report.contains("failed (budget-fit:"), "{report}");
}

#[test]
fn b3_refusals_before_any_call() {
    let run = setup(
        "b3-refuse",
        &[envelope("none", 4), envelope(&mapping_reply(), 900)],
    );
    refused(b3(&run, &[]).1, "commit that first");
    assert_eq!(calls_made(&run), 0);
    refused(run.commit(&["--h-s2", "x.json"]), "the h-s2 arm takes none");
    run.commit(&[]).unwrap();
    refused(
        b3(&run, &[("replicate", "2")]).1,
        "replicate, model or price",
    );
    let original = fs::read(&run.out).unwrap();
    run.edit(&run.out, "attempts", 2.into());
    refused(b3(&run, &[]).1, "not the recorded result");
    fs::write(&run.out, &original).unwrap();
    // A b3 file never sizes another b3 run.
    run.edit(&run.out, "arm", "b3".into());
    refused(b3(&run, &[]).1, "is not h-s2");
    fs::write(&run.out, &original).unwrap();
    assert_eq!(calls_made(&run), 2);
}

#[test]
fn an_h_s2_replicate_whose_calls_reported_no_tokens_sizes_nothing() {
    // After the probe the fake has no reply: every mapping call is a provider failure.
    let run = setup("b3-notokens", &[envelope("none", 4)]);
    let said = run.commit(&[]).unwrap();
    assert!(!said.contains("a mapping"), "{said}");
    let made = calls_made(&run);
    refused(b3(&run, &[]).1, "no h-s2 call reported tokens");
    assert_eq!(calls_made(&run), made);
}

#[test]
fn score_refuses_an_edited_b3_budget() {
    let run = setup(
        "b3-edit",
        &[
            envelope("none", 4),
            envelope(&mapping_reply(), 900),
            envelope("none", 4),
            cached(&mapping_reply(), 5431),
        ],
    );
    run.commit(&[]).unwrap();
    let (b3, said) = b3(&run, &[]);
    said.unwrap();
    let original = fs::read(&b3.out).unwrap();
    let budget = b3.committed()["budget"].clone();
    let mut edited = budget.clone();
    edited["prompt_bytes"] = 99_999.into();
    b3.edit(&b3.out, "budget", edited);
    refused(b3.score(), "prompt_bytes");
    fs::write(&b3.out, &original).unwrap();
    b3.edit(&b3.out, "budget", serde_json::Value::Null);
    refused(b3.score(), "without a budget");
    fs::write(&b3.out, &original).unwrap();
    b3.edit(&b3.out, "arm", "h-s2".into());
    refused(b3.score(), "with a budget");
    fs::write(&b3.out, &original).unwrap();
    // The h-s2 replicate scores unchanged beside it.
    run.score().unwrap();
}
