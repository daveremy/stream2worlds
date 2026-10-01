use super::{ReplayError, ReplayProvider, hash, recording_json};
use crate::provider::{Provider, ProviderError, Reply};
use crate::record::CallRecord;

fn reply(text: &str) -> Reply {
    Reply {
        text: text.to_owned(),
        input_tokens: Some(3),
        output_tokens: None,
        latency_ms: Some(9),
        ..Reply::default()
    }
}

#[test]
fn a_recorded_prompt_is_answered_and_every_call_is_logged() {
    let mut replay = ReplayProvider::new();
    replay.insert("p1", reply("r1"));
    assert_eq!(replay.complete("p1").unwrap(), reply("r1"));
    let error = replay.complete("p2").unwrap_err();
    assert_eq!(
        error,
        ProviderError::NotRecorded {
            prompt_hash: s2w_model::fnv1a64_hex(b"p2")
        }
    );
    assert_eq!(
        replay.calls(),
        vec![s2w_model::fnv1a64_hex(b"p1"), s2w_model::fnv1a64_hex(b"p2")]
    );
}

#[test]
fn a_recording_round_trips_through_json() {
    let mut replay = ReplayProvider::new();
    replay.insert("p1", reply("r1"));
    replay.insert("p2", reply("r2"));
    let text = replay.to_json().unwrap();
    let back = ReplayProvider::from_json(&text).unwrap();
    assert_eq!(back.complete("p2").unwrap(), reply("r2"));
    assert_eq!(back.to_json().unwrap(), text);
}

#[test]
fn a_malformed_recording_is_refused() {
    let row = |hash: &str| {
        format!(
            r#"{{"prompt_hash":"{hash}","reply":"r","input_tokens":null,"output_tokens":null,"latency_ms":null}}"#
        )
    };
    let good = "0123456789abcdef";
    let doc = |format: u32, rows: &[String]| {
        format!(r#"{{"format":{format},"replies":[{}]}}"#, rows.join(","))
    };
    assert!(ReplayProvider::from_json(&doc(1, &[row(good)])).is_ok());
    assert!(matches!(
        ReplayProvider::from_json(&doc(3, &[])),
        Err(ReplayError::Format(3))
    ));
    assert!(matches!(
        ReplayProvider::from_json(&doc(1, &[row("XYZ")])),
        Err(ReplayError::Hash(_))
    ));
    assert!(matches!(
        ReplayProvider::from_json(&doc(1, &[row(good), row(good)])),
        Err(ReplayError::Duplicate(_))
    ));
    assert!(matches!(
        ReplayProvider::from_json(r#"{"format":1,"replies":[],"extra":1}"#),
        Err(ReplayError::Json(_))
    ));
}

fn record(attempt: u32, prompt: &str, answer: Result<&str, &str>) -> CallRecord {
    let result = match answer {
        Ok(text) => Ok(reply(text)),
        Err(error) => Err(ProviderError::Spawn(error.to_owned())),
    };
    CallRecord::new((attempt, 1), hash(prompt), &result, Some(1_000))
}

#[test]
fn format_2_answers_each_row_once_in_order_failures_included() {
    let calls = [
        record(1, "p", Err("no such file")),
        record(2, "p", Ok("second")),
        record(2, "q", Ok("other")),
    ];
    let replay = ReplayProvider::from_calls(&calls).unwrap();
    assert_eq!(
        replay.complete("p").unwrap_err().to_string(),
        "exec: could not start the command: no such file"
    );
    assert_eq!(replay.complete("p").unwrap(), reply("second"));
    assert!(matches!(
        replay.complete("p"),
        Err(ProviderError::NotRecorded { .. })
    ));
    assert_eq!(replay.complete("q").unwrap(), reply("other"));
}

#[test]
fn format_2_round_trips_through_json_and_format_1_cannot_hold_it() {
    let calls = [record(1, "p", Err("gone")), record(2, "p", Ok("r"))];
    let text = recording_json(&calls).unwrap();
    let back = ReplayProvider::from_json(&text).unwrap();
    assert!(back.complete("p").is_err());
    assert_eq!(back.complete("p").unwrap(), reply("r"));
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["format"], 2);
    assert_eq!(
        parsed["calls"][0]["error"],
        "exec: could not start the command: gone"
    );
    assert_eq!(parsed["calls"][1]["started_at_ms"], 1_000);
    assert!(matches!(back.to_json(), Err(ReplayError::Lossy)));
}

#[test]
fn a_format_2_row_needs_exactly_one_of_reply_and_error() {
    let mut both = record(1, "p", Ok("r"));
    both.error = Some("also".to_owned());
    let mut neither = record(1, "p", Ok("r"));
    neither.reply = None;
    for row in [both, neither] {
        assert!(matches!(
            ReplayProvider::from_calls(&[row]),
            Err(ReplayError::Row(_))
        ));
    }
    let mut bad = record(1, "p", Ok("r"));
    bad.prompt_hash = "XYZ".to_owned();
    assert!(matches!(
        ReplayProvider::from_calls(&[bad]),
        Err(ReplayError::Hash(_))
    ));
}
