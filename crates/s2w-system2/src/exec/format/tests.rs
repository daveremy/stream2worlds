use super::{ReplyFormat, reply};
use crate::ExecProvider;
use crate::provider::{Provider, ProviderError};

/// A recorded Claude CLI JSON envelope (print mode, JSON output), trimmed, with a stand-in
/// result.
const ENVELOPE: &str = include_str!("../../../testdata/claude-envelope.json");

fn reason(result: Result<crate::Reply, ProviderError>) -> String {
    match result {
        Err(ProviderError::Envelope { reason, stdout, .. }) => {
            assert!(!stdout.is_empty());
            reason
        }
        other => panic!("not an envelope error: {other:?}"),
    }
}

fn edited(edit: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut value: serde_json::Value = serde_json::from_str(ENVELOPE).unwrap();
    edit(&mut value);
    value.to_string()
}

fn remove(value: &mut serde_json::Value, field: &str) {
    value.as_object_mut().unwrap().remove(field);
}

#[test]
fn text_is_the_default_and_reports_no_tokens() {
    let reply = reply(ReplyFormat::default(), "hi".to_owned(), 7).unwrap();
    assert_eq!(reply.text, "hi");
    assert_eq!(reply.latency_ms, Some(7));
    assert_eq!((reply.input_tokens, reply.cost_usd), (None, None));
}

#[cfg(unix)]
#[test]
fn claude_json_reads_the_reply_tokens_cache_cost_and_model_from_the_command() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/claude-envelope.json");
    let argv = vec!["/bin/cat".to_owned(), path.to_owned()];
    let reply = ExecProvider::new(argv, vec![])
        .unwrap()
        .with_format(ReplyFormat::ClaudeJson)
        .complete("the prompt")
        .unwrap();
    assert_eq!(reply.text, r#"{"version": 2}"#);
    assert_eq!(reply.input_tokens, Some(2));
    assert_eq!(reply.output_tokens, Some(4));
    assert_eq!(reply.cache_read_tokens, Some(531));
    assert_eq!(reply.cache_write_tokens, Some(5431));
    assert_eq!(reply.cost_usd, Some(0.021_874_2));
    assert_eq!(reply.model.as_deref(), Some("claude-sonnet-5-5"));
    assert!(reply.latency_ms.is_some());
}

#[test]
fn a_missing_count_is_an_error_never_a_null() {
    for field in [
        "input_tokens",
        "output_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ] {
        let text = edited(|v| remove(&mut v["usage"], field));
        let why = reason(reply(ReplyFormat::ClaudeJson, text, 1));
        assert!(why.contains(field), "{field}: {why}");
    }
    for field in ["usage", "total_cost_usd"] {
        let text = edited(|v| remove(v, field));
        let why = reason(reply(ReplyFormat::ClaudeJson, text, 1));
        assert!(why.contains(field), "{field}: {why}");
    }
}

#[test]
fn an_error_envelope_is_reported_before_a_missing_result() {
    let text = edited(|v| {
        v["is_error"] = true.into();
        v["subtype"] = "error_max_turns".into();
        remove(v, "result");
    });
    let why = reason(reply(ReplyFormat::ClaudeJson, text, 1));
    assert_eq!(why, "is_error, subtype error_max_turns");
}

#[test]
fn plain_text_a_missing_result_and_a_negative_cost_are_errors() {
    let why = reason(reply(ReplyFormat::ClaudeJson, "just text".to_owned(), 1));
    assert!(why.contains("expected"), "{why}");
    let text = edited(|v| remove(v, "result"));
    assert_eq!(reason(reply(ReplyFormat::ClaudeJson, text, 1)), "no result");
    let text = edited(|v| v["total_cost_usd"] = (-1.0).into());
    assert_eq!(
        reason(reply(ReplyFormat::ClaudeJson, text, 1)),
        "total_cost_usd is not a cost"
    );
}

#[test]
fn several_models_are_all_named_in_order() {
    let text = edited(|v| v["modelUsage"]["a-small-model"] = serde_json::json!({}));
    let reply = reply(ReplyFormat::ClaudeJson, text, 1).unwrap();
    assert_eq!(
        reply.model.as_deref(),
        Some("a-small-model,claude-sonnet-5-5")
    );
}
