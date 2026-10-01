//! The B3 arm (contract §B1): the same model and task as h-s2, given raw events of the same
//! development window instead of the heuristic's result, sampled by a frozen rule up to the
//! input-token budget the h-s2 replicate's System 2 pass used (decision 0032, dated note
//! 2026-10-01).
//!
//! The rule: the sample is every k-th event of the window from the first, each its frame's
//! `data` exactly as the stream carried it. k starts at the smallest value whose first prompt
//! is no more bytes than the h-s2 first prompt. When the model reports that the fit's first
//! prompt read more than [`TOLERANCE_PERCENT`] of the h-s2 prompt's tokens, the fit is spent
//! and the next one tries the smallest k above it that fits. Every fit's calls stay in the
//! transcript and the ledger, under one $5 budget.

use s2w_model::RawMappingInput;
use s2w_system2::{CallGate, CallRecord, MappingProposer, Provider, raw_mapping_prompt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::committed::Proposed;
use super::ledger::BudgetGate;
use super::prices::Price;

/// How far, in percent of the h-s2 prompt's tokens, a fit's first prompt may run over.
pub(crate) const TOLERANCE_PERCENT: u64 = 105;

/// What a B3 replicate was sized to, and every sample it tried.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Budget {
    /// sha256 of the h-s2 committed file the budget came from.
    pub h_s2_sha256: String,
    /// The h-s2 first prompt's tokens as the model reported them ([`prompt_tokens`]).
    pub input_tokens: u64,
    /// The h-s2 first prompt's length in bytes, rebuilt from the heuristic.
    pub prompt_bytes: usize,
    /// Each sample sent, in order; the last one's proposal is the result.
    pub fits: Vec<Fit>,
}

/// One sample the arm sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fit {
    pub k: usize,
    pub events: usize,
    pub prompt_bytes: usize,
    /// Calls the fit's proposal made.
    pub calls: usize,
    /// Its first prompt's reported tokens; `None` when no call reported any.
    pub input_tokens: Option<u64>,
}

/// What a fit is sized against.
pub(crate) struct Target<'a> {
    pub corpus: &'a str,
    pub window: u64,
    pub replicate: u32,
    pub prompt_bytes: usize,
    pub input_tokens: u64,
}

/// The tokens a call's prompt was read as: input plus cache read plus cache write (the CLI
/// caches the prompt, so `input_tokens` alone is a handful). `None` when the call reported
/// none, which is a provider failure.
pub(crate) fn prompt_tokens(call: &CallRecord) -> Option<u64> {
    let read = call.cache_read_tokens.unwrap_or(0);
    let written = call.cache_write_tokens.unwrap_or(0);
    call.input_tokens
        .map(|input| input.saturating_add(read).saturating_add(written))
}

/// The first prompt tokens any of `calls` reported. Only a first call can be the first to
/// report: a repair call follows a reply, and a reply always reports its tokens.
pub(crate) fn first_prompt_tokens(calls: &[CallRecord]) -> Option<u64> {
    calls.iter().find_map(prompt_tokens)
}

/// The window's events as the stream carried them: each stored envelope's `data` string.
///
/// # Errors
///
/// An envelope has no `data` string.
pub(crate) fn raw_events(window: &[Value]) -> Result<Vec<String>, String> {
    window
        .iter()
        .enumerate()
        .map(|(n, envelope)| {
            envelope
                .get("data")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("window event {n} has no data string"))
        })
        .collect()
}

/// The input holding every `k`-th of `events` from the first.
fn sample(events: &[String], k: usize, target: &Target<'_>) -> RawMappingInput {
    RawMappingInput {
        corpus: target.corpus.to_owned(),
        window: target.window,
        replicate: target.replicate,
        events: events.iter().step_by(k).cloned().collect(),
    }
}

/// The smallest k from `from` up whose first prompt is at most `target.prompt_bytes`, with
/// its input and prompt length; `None` when not even a one-event sample fits.
pub(crate) fn fit(
    events: &[String],
    from: usize,
    target: &Target<'_>,
) -> Option<(usize, RawMappingInput, usize)> {
    for k in from.max(1)..=events.len() {
        let input = sample(events, k, target);
        let bytes = raw_mapping_prompt(&input).map_or(usize::MAX, |prompt| prompt.len());
        if bytes <= target.prompt_bytes {
            return Some((k, input, bytes));
        }
        if input.events.len() <= 1 {
            // Every larger k samples the same first event alone.
            return None;
        }
    }
    None
}

/// A gate that stops the proposal before its first call, with `reason` as its failure.
struct Stop(String);

impl CallGate for Stop {
    fn before_call(&mut self, _prompt: &str, _calls: &[CallRecord]) -> Result<(), String> {
        Err(self.0.clone())
    }
}

/// The arm's fits and the last input it sent (when nothing fit at all, the one-event input
/// the failure is recorded against).
pub(crate) struct Fitted {
    pub fits: Vec<Fit>,
    pub last: RawMappingInput,
}

/// The B3 proposal: fit, propose, check, and refit until a fit's first prompt is within the
/// budget, a proposal fails, or nothing larger fits. Each fit's calls pass a budget gate seeded
/// with everything spent before them (`prior_usd` is the probe).
pub(crate) fn propose<Q: Provider>(
    proposer: &MappingProposer<Q>,
    price: &Price,
    events: &[String],
    target: &Target<'_>,
    prior_usd: f64,
) -> (Proposed, Fitted) {
    let mut fits: Vec<Fit> = Vec::new();
    let mut calls = Vec::new();
    let mut charged = Vec::new();
    let mut spent = prior_usd;
    let mut from = 1;
    let mut sent: Option<RawMappingInput> = None;
    loop {
        let Some((k, input, prompt_bytes)) = fit(events, from, target) else {
            let reason = fits.last().map_or_else(
                || {
                    format!(
                        "budget-fit: one event's prompt is over the {} bytes of the h-s2 prompt",
                        target.prompt_bytes
                    )
                },
                |last| {
                    format!(
                        "budget-fit: fit k={} read {} prompt tokens, over {TOLERANCE_PERCENT}% of {}, and no larger k fits",
                        last.k,
                        last.input_tokens.unwrap_or(0),
                        target.input_tokens
                    )
                },
            );
            let last = sent.unwrap_or_else(|| sample(events, events.len().max(1), target));
            let outcome = proposer.propose_raw(&last, &mut Stop(reason));
            let proposed = Proposed {
                outcome,
                calls,
                charged,
            };
            return (proposed, Fitted { fits, last });
        };
        let mut gate = BudgetGate::new(price, spent);
        let outcome = proposer.propose_raw(&input, &mut gate);
        let fit_charged = gate.charged(&outcome.calls);
        spent += fit_charged.iter().sum::<f64>();
        let tokens = first_prompt_tokens(&outcome.calls);
        fits.push(Fit {
            k,
            events: input.events.len(),
            prompt_bytes,
            calls: outcome.calls.len(),
            input_tokens: tokens,
        });
        calls.extend(outcome.calls.iter().cloned());
        charged.extend(fit_charged);
        let over = tokens.is_some_and(|t| {
            u128::from(t) * 100 > u128::from(target.input_tokens) * u128::from(TOLERANCE_PERCENT)
        });
        if !over {
            let proposed = Proposed {
                outcome,
                calls,
                charged,
            };
            return (proposed, Fitted { fits, last: input });
        }
        from = k + 1;
        sent = Some(input);
    }
}

#[cfg(test)]
mod tests;
