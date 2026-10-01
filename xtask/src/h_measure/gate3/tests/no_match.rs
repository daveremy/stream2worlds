//! The no-match check and its one repair call in both arms (s2w#409), with the same fake
//! `claude` as the h-s2 tests: one pair per arm, and gate 3's second dry run reproduced.

use std::fs;

use s2w_model::StreamMapping;
use s2w_system2::MappingCheck as _;
use serde_json::{Value, json};

use super::super::super::freeze::derived;
use super::super::super::pins::Pins;
use super::super::committed::{input, sample_window};
use super::super::no_match::Sample;
use super::super::replay::transcript_path;
use super::b3::{T, b3, cached, calls_made};
use super::{MATCHING_REPLY, NO_MATCH_REPLY, envelope, refused, setup};

/// The `call` of every record in `run`'s transcript.
fn transcript_calls(out: &std::path::Path) -> Vec<u64> {
    let text = fs::read_to_string(transcript_path(out).unwrap()).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    doc["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["call"].as_u64().unwrap())
        .collect()
}

fn mapping(reply: &str) -> StreamMapping {
    serde_json::from_str(reply).unwrap()
}

#[test]
fn h_s2_repairs_a_mapping_that_matches_no_sampled_record_once() {
    let run = setup(
        "nomatch-h-s2",
        &[
            envelope("none", 4),
            envelope(NO_MATCH_REPLY, 900),
            envelope(MATCHING_REPLY, 900),
        ],
    );
    let said = run.commit(&[]).unwrap();
    assert!(said.contains("a mapping, 1 attempts, 3 calls"), "{said}");
    let doc = run.committed();
    assert_eq!(
        doc["no_match"],
        json!({"first": true, "repair_calls": 1, "after": false})
    );
    assert_eq!(
        serde_json::from_value::<StreamMapping>(doc["mapping"].clone()).unwrap(),
        mapping(MATCHING_REPLY)
    );
    // The repair call is charged like any other.
    assert_eq!(doc["spend"]["calls"], 3);
    assert_eq!(doc["spend"]["per_call_usd"].as_array().unwrap().len(), 3);
    assert_eq!(calls_made(&run), 3);
    // The transcript holds the attempt's first call and its no-match repair.
    assert_eq!(transcript_calls(&run.out), [1, 3]);
    let report = run.score().unwrap();
    assert!(
        report.contains("matched no sampled record and was repaired once"),
        "{report}"
    );
    run.edit(
        &run.out,
        "no_match",
        json!({"first": false, "repair_calls": 1, "after": false}),
    );
    refused(run.score(), "not the recorded");
}

#[test]
fn h_s2_makes_no_repair_call_when_the_first_mapping_matches() {
    let run = setup(
        "match-h-s2",
        &[envelope("none", 4), envelope(MATCHING_REPLY, 900)],
    );
    let said = run.commit(&[]).unwrap();
    assert!(said.contains("a mapping, 1 attempts, 2 calls"), "{said}");
    let doc = run.committed();
    assert_eq!(
        doc["no_match"],
        json!({"first": false, "repair_calls": 0, "after": false})
    );
    assert_eq!(calls_made(&run), 2);
    assert_eq!(transcript_calls(&run.out), [1]);
    run.score().unwrap();
}

#[test]
fn b3_repairs_a_mapping_that_matches_no_sampled_record_once() {
    let run = setup(
        "nomatch-b3",
        &[
            envelope("none", 4),
            envelope(MATCHING_REPLY, 900),
            envelope("none", 4),
            cached(NO_MATCH_REPLY, 5431),
            cached(MATCHING_REPLY, 5431),
        ],
    );
    run.commit(&[]).unwrap();
    let (b3, said) = b3(&run, &[]);
    let said = said.unwrap();
    assert!(said.contains("a mapping, 1 attempts, 3 calls"), "{said}");
    let doc = b3.committed();
    assert_eq!(
        doc["no_match"],
        json!({"first": true, "repair_calls": 1, "after": false})
    );
    let fit = &doc["budget"]["fits"][0];
    assert_eq!(
        (fit["calls"].clone(), fit["input_tokens"].clone()),
        (2.into(), T.into())
    );
    assert_eq!(doc["spend"]["calls"], 3);
    assert_eq!(calls_made(&run), 5);
    assert_eq!(transcript_calls(&b3.out), [1, 3]);
    b3.score().unwrap();
    b3.edit(
        &b3.out,
        "no_match",
        json!({"first": true, "repair_calls": 0, "after": true}),
    );
    refused(b3.score(), "not the recorded");
}

#[test]
fn b3_makes_no_repair_call_when_the_first_mapping_matches() {
    let run = setup(
        "match-b3",
        &[
            envelope("none", 4),
            envelope(MATCHING_REPLY, 900),
            envelope("none", 4),
            cached(MATCHING_REPLY, 5431),
        ],
    );
    run.commit(&[]).unwrap();
    let (b3, said) = b3(&run, &[]);
    assert!(said.unwrap().contains("a mapping, 1 attempts, 2 calls"));
    let doc = b3.committed();
    assert_eq!(
        doc["no_match"],
        json!({"first": false, "repair_calls": 0, "after": false})
    );
    assert_eq!(doc["budget"]["fits"][0]["calls"], 1);
    assert_eq!(calls_made(&run), 4);
}

/// Gate 3's second dry run, reproduced: a mapping without its decode step matches no stored
/// record, yet it matches the decoded sample the prompt shows. The check reads the stored
/// records, so it catches the mapping the displayed sample would pass (plan §0).
#[test]
fn a_mapping_without_its_decode_step_matches_no_stored_record() {
    let (root, dir) = super::super::super::freeze_tests::fixture("gate3-nomatch-unit");
    let pins = Pins::load(&root).unwrap();
    let (heuristic, profile, events) = derived(&pins, &dir, "dev", 3).unwrap();
    let stored = Sample::of_values(sample_window(&events)).unwrap();
    assert_eq!(stored.no_match(&mapping(MATCHING_REPLY)), Ok(false));
    assert_eq!(stored.no_match(&mapping(NO_MATCH_REPLY)), Ok(true));
    // The prompt shows each record with the profiler's decode steps applied. The 3-event
    // fixture profiles no decode step, so decode `data` as the live corpora's profiles do.
    let mut decoding = profile;
    decoding.decode = mapping(MATCHING_REPLY).decode;
    let shown = input(&heuristic, &decoding, &events, 1).unwrap().sample;
    let shown = Sample::of_values(&shown).unwrap();
    assert_eq!(shown.no_match(&mapping(NO_MATCH_REPLY)), Ok(false));
    assert_eq!(shown.no_match(&mapping(MATCHING_REPLY)), Ok(true));
    // An empty sample shows nothing to match, so it is not a no-match.
    assert_eq!(
        Sample::of_values(&[])
            .unwrap()
            .no_match(&mapping(NO_MATCH_REPLY)),
        Ok(false)
    );
    let _ = fs::remove_dir_all(root);
}
