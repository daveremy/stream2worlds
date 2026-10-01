//! The B3 arm (contract §B1): the same model and task as h-s2, given raw events of the same
//! development window instead of the heuristic's result, sampled by a frozen rule up to the
//! input-token budget the h-s2 replicate's System 2 pass used (decision 0032, dated note
//! 2026-10-01).
//!
//! The rule: the sample is every k-th event of the window from the first, each the stored
//! envelope byte for byte, the record the executor applies a mapping to. k starts at the
//! smallest value whose first prompt is no more bytes than the h-s2 first prompt. When the
//! model reports that the fit's first prompt read more than [`TOLERANCE_PERCENT`] of the h-s2
//! prompt's tokens, the fit is spent and the next one shrinks the sample by the measured ratio
//! ([`refit_from`]). Every fit's calls stay in the transcript and the ledger, under one $5
//! budget.

use s2w_model::{MappingInput, RawMappingInput};
use s2w_system2::{CallGate, CallRecord, MappingProposer, Provider, raw_mapping_prompt};
use serde_json::Value;

use super::committed::{Budget, Fit, Proposed, hash_fields, json};
use super::ledger::BudgetGate;
use super::prices::Price;

/// How far, in percent of the h-s2 prompt's tokens, a fit's first prompt may run over.
pub(crate) const TOLERANCE_PERCENT: u64 = 105;

/// The share, in percent, of the measured ratio a refit aims at: 3% under the h-s2 prompt's
/// tokens, so a refit lands inside [`TOLERANCE_PERCENT`] even when sampling is uneven.
pub(crate) const REFIT_PERCENT: u64 = 97;

/// What a fit is sized against.
pub(crate) struct Target<'a> {
    pub corpus: &'a str,
    pub window: u64,
    pub replicate: u32,
    pub prompt_bytes: usize,
    pub input_tokens: u64,
}

/// A B3 replicate's budget, read from its h-s2 replicate before any call, and the window's
/// raw events it samples.
pub(crate) struct Sized {
    pub h_s2_sha256: String,
    pub input_tokens: u64,
    pub prompt_bytes: usize,
    pub raw: Vec<String>,
}

impl Sized {
    /// What each fit is sized against; `input` is the h-s2 input (its corpus, window and
    /// replicate are this replicate's).
    pub(crate) fn target<'a>(&self, input: &'a MappingInput) -> Target<'a> {
        Target {
            corpus: &input.corpus,
            window: input.window,
            replicate: input.replicate,
            prompt_bytes: self.prompt_bytes,
            input_tokens: self.input_tokens,
        }
    }

    /// The committed budget, with the fits the run made.
    pub(crate) fn budget(self, fits: Vec<Fit>) -> Budget {
        Budget {
            h_s2_sha256: self.h_s2_sha256,
            input_tokens: self.input_tokens,
            prompt_bytes: self.prompt_bytes,
            fits,
        }
    }
}

/// B3's input hash: the h-s2 input its budget was built from, the last raw input it sent, and
/// `prompt_files_hash`.
///
/// # Errors
///
/// An input does not serialize.
pub(crate) fn input_hash(
    h_s2: &MappingInput,
    last: &RawMappingInput,
    prompt_files_hash: &str,
) -> Result<String, String> {
    Ok(hash_fields(&[json(h_s2)?, json(last)?], prompt_files_hash))
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

/// The first prompt tokens a first call (call 1 of an attempt) reported. Every attempt's call 1
/// sends the same first prompt, so a provider failure on attempt 1 falls to attempt 2's; a
/// repair call (call 2) carries a reply too and never sets the count.
pub(crate) fn first_prompt_tokens(calls: &[CallRecord]) -> Option<u64> {
    calls
        .iter()
        .filter(|call| call.call == 1)
        .find_map(prompt_tokens)
}

/// The window's events as the executor reads them: each stored envelope serialized byte for
/// byte as the profiler and the executor read it (`data` still a JSON string, `id` beside it).
///
/// # Errors
///
/// An envelope does not serialize.
pub(crate) fn raw_events(window: &[Value]) -> Result<Vec<String>, String> {
    window
        .iter()
        .enumerate()
        .map(|(n, envelope)| {
            serde_json::to_string(envelope).map_err(|e| format!("window event {n}: {e}"))
        })
        .collect()
}

/// The k a spent fit's refit starts from: the sample shrunk by the measured ratio. A fit at
/// stride k sends about 1/k of the events, so shrinking the sample to `budget / tokens` of
/// itself, times [`REFIT_PERCENT`], is a stride of `ceil(k * tokens / (budget * 0.97))`. It is
/// always above k, so a refit never sends a larger sample, and the stride reaches the one-event
/// sample in finitely many refits.
pub(crate) fn refit_from(k: usize, tokens: u64, budget: u64) -> usize {
    let denominator = u128::from(budget) * u128::from(REFIT_PERCENT);
    if denominator == 0 {
        return usize::MAX;
    }
    let numerator = k as u128 * u128::from(tokens) * 100;
    let next = numerator.div_ceil(denominator);
    usize::try_from(next).unwrap_or(usize::MAX).max(k + 1)
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

/// Why nothing more is proposed: not even one event fits, or the last fit was over the
/// tolerance and its refit's k gives no sample.
fn budget_fit(fits: &[Fit], target: &Target<'_>) -> String {
    fits.last().map_or_else(
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
    )
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
            let reason = budget_fit(&fits, target);
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
        let over = tokens.filter(|&t| {
            u128::from(t) * 100 > u128::from(target.input_tokens) * u128::from(TOLERANCE_PERCENT)
        });
        let Some(read) = over else {
            let proposed = Proposed {
                outcome,
                calls,
                charged,
            };
            return (proposed, Fitted { fits, last: input });
        };
        // A one-event sample is the first event alone for every larger k too: nothing smaller fits.
        from = if input.events.len() <= 1 {
            events.len().saturating_add(1)
        } else {
            // A stride past the window still leaves the one-event sample to try.
            refit_from(k, read, target.input_tokens).min(events.len())
        };
        sent = Some(input);
    }
}

#[cfg(test)]
mod tests;
