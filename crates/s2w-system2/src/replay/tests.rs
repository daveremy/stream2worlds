use super::{ReplayError, ReplayProvider};
use crate::provider::{Provider, ProviderError, Reply};

fn reply(text: &str) -> Reply {
    Reply {
        text: text.to_owned(),
        input_tokens: Some(3),
        output_tokens: None,
        latency_ms: Some(9),
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
        ReplayProvider::from_json(&doc(2, &[])),
        Err(ReplayError::Format(2))
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
