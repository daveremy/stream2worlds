//! The B3 sampler: the raw events it reads, the k it fits, and the prompt tokens it checks.

use s2w_system2::{CallRecord, raw_mapping_prompt};
use serde_json::json;

use super::{Target, first_prompt_tokens, fit, prompt_tokens, raw_events};

fn events(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("{{\"n\":{i},\"text\":\"{}\"}}", "x".repeat(40)))
        .collect()
}

fn target(prompt_bytes: usize) -> Target<'static> {
    Target {
        corpus: "dev",
        window: 30,
        replicate: 1,
        prompt_bytes,
        input_tokens: 1000,
    }
}

fn record(input: Option<u64>, read: Option<u64>, write: Option<u64>) -> CallRecord {
    CallRecord {
        attempt: 1,
        call: 1,
        prompt_hash: "0".repeat(16),
        reply: Some(String::new()),
        error: None,
        model: None,
        input_tokens: input,
        output_tokens: None,
        cache_read_tokens: read,
        cache_write_tokens: write,
        cost_usd: None,
        latency_ms: None,
        started_at_ms: None,
    }
}

#[test]
fn raw_events_are_each_frames_data_verbatim() {
    let window = [
        json!({"data": "{\"a\": 1}", "id": "1"}),
        json!({"data": "not json at all", "id": null}),
    ];
    assert_eq!(
        raw_events(&window).unwrap(),
        ["{\"a\": 1}", "not json at all"]
    );
    let err = raw_events(&[json!({"id": "1"})]).unwrap_err();
    assert!(err.contains("event 0 has no data string"), "{err}");
}

#[test]
fn fit_is_the_smallest_k_whose_prompt_fits() {
    let events = events(30);
    let bytes = |k: usize| {
        let (_, input, b) = fit(&events, k, &target(usize::MAX)).unwrap();
        assert_eq!(b, raw_mapping_prompt(&input).unwrap().len());
        b
    };
    // Everything fits an unbounded budget at k = 1.
    let (k, input, all) = fit(&events, 1, &target(usize::MAX)).unwrap();
    assert_eq!((k, input.events.len()), (1, 30));
    // A budget between k = 3 and k = 2's prompts fits k = 3: every third event from the first.
    let budget = (bytes(2) + bytes(3)) / 2;
    assert!(bytes(3) < budget && budget < all);
    let (k, input, b) = fit(&events, 1, &target(budget)).unwrap();
    assert_eq!(k, 3);
    assert!(b <= budget);
    assert_eq!(input.events.len(), 10);
    assert_eq!(input.events[..2], [events[0].clone(), events[3].clone()]);
    // The same window and budget always give the same sample.
    assert_eq!(fit(&events, 1, &target(budget)).unwrap().1, input);
    // A smaller budget never gives a smaller k; a refit starts above the last k.
    assert!(fit(&events, 1, &target(bytes(5))).unwrap().0 >= 3);
    assert_eq!(fit(&events, 4, &target(budget)).unwrap().0, 4);
}

#[test]
fn fit_is_none_when_one_event_does_not_fit() {
    let events = events(30);
    let (_, _, one) = fit(&events, 30, &target(usize::MAX)).unwrap();
    assert!(fit(&events, 1, &target(one - 1)).is_none());
    assert_eq!(fit(&events, 1, &target(one)).unwrap().0, 30);
    assert!(fit(&events, 31, &target(usize::MAX)).is_none());
}

#[test]
fn prompt_tokens_are_input_plus_cache() {
    assert_eq!(
        prompt_tokens(&record(Some(2), Some(531), Some(5431))),
        Some(5964)
    );
    assert_eq!(prompt_tokens(&record(Some(7), None, None)), Some(7));
    assert_eq!(prompt_tokens(&record(None, Some(5), Some(5))), None);
    let calls = [record(None, None, None), record(Some(1), Some(2), Some(3))];
    assert_eq!(first_prompt_tokens(&calls), Some(6));
    assert_eq!(first_prompt_tokens(&calls[..1]), None);
}

#[test]
fn propose_commits_a_budget_fit_failure_when_one_event_is_over_the_budget() {
    let price = super::super::prices::Price {
        input: 2.0,
        output: 10.0,
        cache_write: 4.0,
        cache_read: 0.2,
        source_url: String::new(),
        copied_on: String::new(),
    };
    let proposer = s2w_system2::MappingProposer::new(s2w_system2::ReplayProvider::new());
    let events = events(30);
    let (proposed, fitted) = super::propose(&proposer, &price, &events, &target(10), 0.0);
    assert!(proposed.calls.is_empty() && fitted.fits.is_empty());
    assert_eq!(fitted.last.events, [events[0].clone()]);
    match proposed.outcome.result {
        s2w_system2::MappingResult::Failure(reason) => assert!(
            reason.contains("budget-fit: one event's prompt is over the 10 bytes"),
            "{reason}"
        ),
        s2w_system2::MappingResult::Mapping(_) => panic!("nothing fits, so nothing is proposed"),
    }
}
