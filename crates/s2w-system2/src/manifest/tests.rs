use s2w_model::{ManifestOutcome, ManifestProposer, ProposerId};
use serde_json::json;

use super::System2Proposer;
use crate::fixture::{input, manifest_json};
use crate::prompt;
use crate::provider::{Provider, ProviderError, Reply};
use crate::replay::ReplayProvider;

fn reply(text: &str, tokens: u64, latency: u64) -> Reply {
    Reply {
        text: text.to_owned(),
        input_tokens: Some(tokens),
        output_tokens: Some(tokens / 10),
        latency_ms: Some(latency),
        ..Reply::default()
    }
}

fn id() -> ProposerId {
    ProposerId {
        model: "m".to_owned(),
        version: "v".to_owned(),
    }
}

/// A provider that always fails the same way.
struct Failing(ProviderError);

impl Provider for Failing {
    fn complete(&self, _prompt: &str) -> Result<Reply, ProviderError> {
        Err(self.0.clone())
    }
}

#[test]
fn a_valid_first_reply_is_a_manifest_after_one_call() {
    let input = input();
    let good = manifest_json(&input).to_string();
    let mut replay = ReplayProvider::new();
    replay.insert(
        &prompt::manifest_prompt(&input).unwrap(),
        reply(&good, 100, 7),
    );
    let proposer = System2Proposer::new(replay, id());

    let ManifestOutcome::Manifest { manifest, trace } = proposer.propose(&input) else {
        panic!("expected a manifest");
    };
    assert_eq!(manifest.domain.name, "A world");
    assert_eq!(trace.input_tokens, Some(100));
    assert_eq!(trace.output_tokens, Some(10));
    assert_eq!(trace.latency_ms, Some(7));
    assert_eq!(trace.raw.as_deref(), Some(good.as_str()));
    assert_eq!(proposer.provider().calls().len(), 1);
}

#[test]
fn a_fenced_reply_is_read() {
    let input = input();
    let fenced = format!("```json\n{}\n```\n", manifest_json(&input));
    let mut replay = ReplayProvider::new();
    replay.insert(
        &prompt::manifest_prompt(&input).unwrap(),
        reply(&fenced, 1, 1),
    );
    let proposer = System2Proposer::new(replay, id());
    assert!(matches!(
        proposer.propose(&input),
        ManifestOutcome::Manifest { .. }
    ));
}

#[test]
fn a_refused_reply_gets_one_repair_call_and_the_attempt_sums_both() {
    let input = input();
    let mut bad = manifest_json(&input);
    bad["quintessential_projection"]["slots"]["subject_type"] = json!("nothing");
    let bad = bad.to_string();
    let good = manifest_json(&input).to_string();
    let first = prompt::manifest_prompt(&input).unwrap();
    let mut replay = ReplayProvider::new();
    replay.insert(&first, reply(&bad, 100, 7));
    let fault = super::accept(&bad, &input).unwrap_err();
    assert!(fault.starts_with("validator: "), "{fault}");
    let second = prompt::repair_prompt(&first, &bad, &fault).unwrap();
    replay.insert(&second, reply(&good, 200, 5));
    let proposer = System2Proposer::new(replay, id());

    let ManifestOutcome::Manifest { trace, .. } = proposer.propose(&input) else {
        panic!("expected the repaired manifest");
    };
    assert_eq!(trace.input_tokens, Some(300));
    assert_eq!(trace.output_tokens, Some(30));
    assert_eq!(trace.latency_ms, Some(12));
    assert_eq!(trace.raw.as_deref(), Some(good.as_str()));
    assert_eq!(proposer.provider().calls().len(), 2);
}

#[test]
fn two_faults_are_invalid_with_the_last_fault_and_no_third_call() {
    let input = input();
    let first = prompt::manifest_prompt(&input).unwrap();
    let mut replay = ReplayProvider::new();
    replay.insert(&first, reply("not json at all", 1, 1));
    let fault = super::accept("not json at all", &input).unwrap_err();
    assert!(fault.starts_with("not JSON: "), "{fault}");
    let second = prompt::repair_prompt(&first, "not json at all", &fault).unwrap();
    replay.insert(&second, reply(r#"{"domain": 1}"#, 1, 1));
    let proposer = System2Proposer::new(replay, id());

    let ManifestOutcome::Invalid { error, trace } = proposer.propose(&input) else {
        panic!("expected invalid");
    };
    assert!(error.starts_with("decode: "), "{error}");
    assert_eq!(trace.raw.as_deref(), Some(r#"{"domain": 1}"#));
    assert_eq!(trace.input_tokens, Some(2));
    assert_eq!(proposer.provider().calls().len(), 2);
}

#[test]
fn a_provider_failure_is_invalid_with_its_output_and_is_not_repaired() {
    let failure = ProviderError::Timeout {
        secs: 180,
        latency_ms: 180_000,
        stdout: Some("partial".to_owned()),
    };
    let proposer = System2Proposer::new(Failing(failure), id());
    let ManifestOutcome::Invalid { error, trace } = proposer.propose(&input()) else {
        panic!("expected invalid");
    };
    assert_eq!(error, "exec: timed out after 180 s");
    assert_eq!(trace.raw.as_deref(), Some("partial"));
    assert_eq!(trace.latency_ms, Some(180_000));
    assert_eq!(trace.input_tokens, None);
}

#[test]
fn a_failed_repair_call_keeps_the_first_reply_as_raw() {
    let input = input();
    let first = prompt::manifest_prompt(&input).unwrap();
    let mut replay = ReplayProvider::new();
    replay.insert(&first, reply("nope", 1, 1));
    let proposer = System2Proposer::new(replay, id());
    let ManifestOutcome::Invalid { error, trace } = proposer.propose(&input) else {
        panic!("expected invalid");
    };
    assert!(error.starts_with("replay: no recorded reply"), "{error}");
    assert_eq!(trace.raw.as_deref(), Some("nope"));
    assert_eq!(proposer.provider().calls().len(), 2);
}

#[test]
fn an_input_with_no_source_abstains_without_a_call() {
    let mut input = input();
    input.sources.clear();
    let proposer = System2Proposer::new(ReplayProvider::new(), id());
    assert!(matches!(
        proposer.propose(&input),
        ManifestOutcome::Abstain(_)
    ));
    assert!(proposer.provider().calls().is_empty());
}

#[test]
fn the_prompt_hash_is_sixteen_hex_digits_and_the_id_is_configured() {
    let proposer = System2Proposer::new(ReplayProvider::new(), id());
    let hash = proposer.prompt_hash().unwrap();
    assert!(s2w_model::is_hex16(&hash), "{hash}");
    assert_eq!(proposer.prompt_hash(), Some(hash));
    assert_eq!(proposer.id(), id());
}
