//! A committed gate-3 replicate: what `cargo xtask gate3 commit` writes and `score` accepts,
//! and the one function both run, so the live run and its replay are the same code.

use s2w_discover::Profile;
use s2w_model::{Fnv64, HeuristicMapping, MappingInput, StreamMapping};
use s2w_system2::{
    CallRecord, MappingOutcome, MappingProposer, MappingResult, NoMatch, Provider,
    clean_session_probe,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::freeze::Frozen;
use super::ledger::{BudgetGate, Spend};
use super::no_match::Sample;
use super::prices::Price;

/// The committed file's `kind`, which `score` tells it from a frozen mapping by.
pub(crate) const KIND: &str = "s2w-gate3-committed";

/// The committed file's format.
pub(crate) const FORMAT: u32 = 1;

/// The "H plus System 2" arm.
pub(crate) const H_S2: &str = "h-s2";

/// The raw-sample baseline arm (contract §B1), sized by the h-s2 replicate it follows.
pub(crate) const B3: &str = "b3";

/// The provider every committed run calls.
pub(crate) const PROVIDER: &str = "claude-cli";

/// The newest events of the window the input samples, and the longest string kept in each.
/// Frozen after the gate-3 measurement that fixes them (ruling 5 on s2w#373).
pub(crate) const SAMPLE_EVENTS: usize = 60;
pub(crate) const SAMPLE_STRING_CHARS: usize = 200;

/// One replicate. Exactly one of `mapping` and `failure` is set.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Committed {
    pub kind: String,
    pub format: u32,
    pub arm: String,
    pub replicate: u32,
    /// What `freeze` writes for the same corpus and window: the H the arm starts from, with
    /// every pin, so `score` checks it exactly as it checks a frozen mapping. B3 carries it for
    /// its pins and its budget (the h-s2 prompt is built from it) and never sends it.
    pub heuristic: Frozen,
    pub provider: String,
    /// The model snapshot, as configured.
    pub model: String,
    /// The price row the run was charged by.
    pub price: Price,
    /// The h-s2 input's sample rule; for B3, the rule behind the prompt its budget comes from.
    pub sample_events: usize,
    pub sample_string_chars: usize,
    pub prompt_files_hash: String,
    /// The input's canonical JSON and `prompt_files_hash`, length-prefixed, FNV-1a 64. For B3,
    /// the h-s2 input, the last raw input and `prompt_files_hash`.
    pub input_hash: String,
    pub attempts: u32,
    pub mapping: Option<StreamMapping>,
    pub failure: Option<String>,
    /// The clean-session probe's call.
    pub probe: CallRecord,
    pub spend: Spend,
    /// The no-match check's findings (s2w#409): for B3, the last fit's proposal.
    pub no_match: NoMatch,
    /// sha256 of the sibling transcript (recording format 2 of the proposal's calls).
    pub transcript_sha256: String,
    /// B3 only: the budget it was sized to and every sample it tried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
}

/// What a B3 replicate was sized to, and every sample it tried.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Budget {
    /// sha256 of the h-s2 committed file the budget came from.
    pub h_s2_sha256: String,
    /// The h-s2 first prompt's tokens as the model reported them (`b3::prompt_tokens`).
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

/// The arm's input, built from one profiler run over the window (`freeze::derived`).
///
/// # Errors
///
/// The heuristic mapping does not validate (it always does: the profiler's output is checked).
pub(crate) fn input(
    frozen: &Frozen,
    profile: &Profile,
    window: &[Value],
    replicate: u32,
) -> Result<MappingInput, String> {
    let wide = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    let heuristic = frozen
        .mapping
        .as_ref()
        .map(|mapping| {
            mapping
                .identity()
                .map(|identity| HeuristicMapping {
                    identity,
                    mapping: mapping.clone(),
                })
                .map_err(|e| format!("heuristic mapping: {e}"))
        })
        .transpose()?;
    let sample = sample_window(window)
        .iter()
        .filter_map(|payload| {
            let bytes = serde_json::to_vec(payload).ok()?;
            s2w_discover::manifest::sample_event(&bytes, &profile.decode, SAMPLE_STRING_CHARS)
        })
        .collect();
    Ok(MappingInput {
        corpus: frozen.corpus.clone(),
        window: wide(frozen.window),
        replicate,
        events_read: wide(profile.events),
        heuristic,
        decode: profile.decode.clone(),
        event_type: profile.event_type.clone(),
        paths: s2w_discover::manifest::path_stats(profile),
        sample,
    })
}

/// The newest [`SAMPLE_EVENTS`] events of `window`, as stored: the records the h-s2 input's
/// sample is built from, and the records its no-match check reads.
pub(crate) fn sample_window(window: &[Value]) -> &[Value] {
    &window[window.len().saturating_sub(SAMPLE_EVENTS)..]
}

/// `input`'s hash with `prompt_files_hash`, built as the app builds a manifest's input hash.
///
/// # Errors
///
/// The input does not serialize.
pub(crate) fn input_hash(input: &MappingInput, prompt_files_hash: &str) -> Result<String, String> {
    Ok(hash_fields(&[json(input)?], prompt_files_hash))
}

/// `value`'s canonical JSON (struct field order), as `input_hash` hashes it.
///
/// # Errors
///
/// The value does not serialize.
pub(crate) fn json<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|e| format!("input: {e}"))
}

/// FNV-1a 64 over `fields` then `prompt_files_hash`, each length-prefixed.
pub(crate) fn hash_fields(fields: &[Vec<u8>], prompt_files_hash: &str) -> String {
    let mut hasher = Fnv64::new();
    for field in fields {
        hasher.write_field(field);
    }
    hasher.write_field(prompt_files_hash.as_bytes());
    format!("{:016x}", hasher.finish())
}

/// What an arm's proposal produced: its result (`outcome`), every call it made, in order
/// (`outcome.calls` holds only the last proposal's), and what each call was charged.
pub(crate) struct Proposed {
    pub outcome: MappingOutcome,
    pub calls: Vec<CallRecord>,
    pub charged: Vec<f64>,
}

/// What one run of an arm produced.
pub(crate) struct Ran {
    pub probe: CallRecord,
    pub outcome: MappingOutcome,
    pub calls: Vec<CallRecord>,
    pub spend: Spend,
}

/// The probe through `probe_provider` under the budget gate at `price`, then the arm's
/// proposal, given what the probe was charged. The live run and `score`'s replay both call
/// this; `X` is whatever else the arm reports (B3: its fits).
///
/// # Errors
///
/// The probe failed: the session is not proven clean, so there is no replicate.
pub(crate) fn run<P: Provider, X>(
    probe_provider: &P,
    price: &Price,
    propose: impl FnOnce(f64) -> (Proposed, X),
) -> Result<(Ran, X), String> {
    let mut gate = BudgetGate::new(price, 0.0);
    let probe = clean_session_probe(probe_provider, &mut gate).map_err(|(reason, call)| {
        let charged: f64 = call.map_or(0.0, |call| gate.charged(&[*call]).iter().sum());
        format!("{reason} (the probe was charged ${charged:.4})")
    })?;
    let probe_charged = gate.charged(std::slice::from_ref(&probe));
    let (proposed, extra) = propose(probe_charged.iter().sum());
    let charged = [probe_charged, proposed.charged].concat();
    let spend = Spend::of(&probe, &proposed.calls, charged);
    let ran = Ran {
        probe,
        outcome: proposed.outcome,
        calls: proposed.calls,
        spend,
    };
    Ok((ran, extra))
}

/// The h-s2 arm's proposal: one `propose` under a gate seeded with `prior_usd`, its no-match
/// check on `sample` (the [`sample_window`] of the window `input` was built from).
pub(crate) fn propose_h_s2<Q: Provider>(
    proposer: &MappingProposer<Q>,
    price: &Price,
    input: &MappingInput,
    sample: &Sample,
    prior_usd: f64,
) -> (Proposed, ()) {
    let mut gate = BudgetGate::new(price, prior_usd);
    let outcome = proposer.propose(input, &mut gate, sample);
    let charged = gate.charged(&outcome.calls);
    let proposed = Proposed {
        calls: outcome.calls.clone(),
        outcome,
        charged,
    };
    (proposed, ())
}

/// The result as the committed file holds it: `(mapping, failure)`.
pub(crate) fn split(result: &MappingResult) -> (Option<StreamMapping>, Option<String>) {
    match result {
        MappingResult::Mapping(mapping) => (Some(mapping.clone()), None),
        MappingResult::Failure(reason) => (None, Some(reason.clone())),
    }
}
