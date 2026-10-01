//! The B3 sampler: the raw events it reads, the k it fits, and the prompt tokens it checks.

use s2w_system2::{CallRecord, raw_mapping_prompt};
use serde_json::json;

use super::{Target, first_prompt_tokens, fit, prompt_tokens, raw_events, refit_from};

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
fn raw_events_are_each_stored_envelope_byte_for_byte() {
    let window = [
        json!({"data": "{\"a\": 1}", "id": "1"}),
        json!({"data": "not json at all", "id": null}),
    ];
    let raw = raw_events(&window).unwrap();
    assert_eq!(
        raw,
        [
            r#"{"data":"{\"a\": 1}","id":"1"}"#,
            r#"{"data":"not json at all","id":null}"#
        ]
    );
    // The bytes the profiler and the executor read.
    for (event, envelope) in raw.iter().zip(&window) {
        assert_eq!(event.as_bytes(), serde_json::to_vec(envelope).unwrap());
    }
}

#[test]
fn refit_shrinks_the_sample_by_the_measured_ratio() {
    // 7% over: stride 100 becomes ceil(100 * 1.07 / 0.97) = 111.
    assert_eq!(refit_from(100, 1070, 1000), 111);
    // Always strictly above k, however small the overrun.
    assert_eq!(refit_from(1, 1001, 1000), 2);
    assert_eq!(refit_from(50, 1000, 1000), 52);
    // Twice the budget is about twice the stride, 3% more.
    assert_eq!(refit_from(10, 2000, 1000), 21);
    assert_eq!(refit_from(3, 10, 0), usize::MAX);
}

/// A model whose prompts read `num / den` tokens a byte, answering one fixed mapping.
struct Dense {
    num: u64,
    den: u64,
    reply: String,
}

impl s2w_system2::Provider for Dense {
    fn complete(&self, prompt: &str) -> Result<s2w_system2::Reply, s2w_system2::ProviderError> {
        Ok(s2w_system2::Reply {
            text: self.reply.clone(),
            input_tokens: Some((prompt.len() as u64 * self.num).div_ceil(self.den)),
            output_tokens: Some(10),
            ..s2w_system2::Reply::default()
        })
    }
}

#[test]
fn a_prompt_7_percent_denser_than_h_s2_converges_within_2_refits() {
    // Many uneven events, as a live stream's are (30 to 130 bytes, in no order), sampled at a
    // stride in the hundreds as in leg D's run (k = 148 to 160), where a step of one barely
    // shrinks the sample: the old refit, k + 1, takes 5 refits here; this one takes 1.
    let events: Vec<String> = (0..30_000_u64)
        .map(|i| {
            // A multiplicative hash, so no stride lines up with a period of the sizes.
            let len = 30 + (i.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) % 101;
            format!("{{\"n\":{i},\"text\":\"{}\"}}", "x".repeat(len as usize))
        })
        .collect();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let reply = std::fs::read_to_string(repo.join(crate::obfuscation_raw::MAPPING)).unwrap();
    let price = super::super::prices::Price {
        input: 2.0,
        output: 10.0,
        cache_write: 4.0,
        cache_read: 0.2,
        source_url: String::new(),
        copied_on: String::new(),
    };
    // The h-s2 prompt is B bytes read as T = 25,000 tokens, and B is exactly the first fit's
    // prompt, as in leg D's dry run (98.5 KB against B = 100 KB). B3's prompts read 7% more
    // tokens a byte than h-s2's, so the first fit reads 26,750 tokens, over 105% of T.
    let (_, _, b) = fit(&events, 1, &target(16_000)).unwrap();
    let mut target = target(b);
    target.input_tokens = 25_000;
    let proposer = s2w_system2::MappingProposer::new(Dense {
        num: 107 * 25_000,
        den: 100 * b as u64,
        reply,
    });
    let (proposed, fitted) = super::propose(&proposer, &price, &events, &target, 0.0);
    assert!(matches!(
        proposed.outcome.result,
        s2w_system2::MappingResult::Mapping(_)
    ));
    let fits = &fitted.fits;
    assert!(
        fits.len() >= 2,
        "the first fit is over: {:?}",
        fits[0].input_tokens
    );
    assert!(
        fits.len() <= 3,
        "{} fits: {:?}",
        fits.len(),
        fits.iter()
            .map(|f| (f.k, f.input_tokens))
            .collect::<Vec<_>>()
    );
    let last = fits.last().unwrap().input_tokens.unwrap();
    assert!(last * 100 <= 25_000 * super::TOLERANCE_PERCENT, "{last}");
    assert!(
        fits.windows(2)
            .all(|w| w[0].k < w[1].k && w[0].events > w[1].events)
    );
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
    let mut repair = record(Some(9), Some(9), Some(9));
    repair.call = 2;
    assert_eq!(
        first_prompt_tokens(&[record(None, None, None), repair]),
        None
    );
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
