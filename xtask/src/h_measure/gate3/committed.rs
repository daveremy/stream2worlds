//! A committed gate-3 replicate: what `cargo xtask gate3 commit` writes and `score` accepts,
//! and the one function both run, so the live run and its replay are the same code.

use s2w_discover::Profile;
use s2w_model::{Fnv64, HeuristicMapping, MappingInput, StreamMapping};
use s2w_system2::{
    CallRecord, MappingOutcome, MappingProposer, MappingResult, Provider, clean_session_probe,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::freeze::Frozen;
use super::ledger::{BudgetGate, Spend};
use super::prices::Price;

/// The committed file's `kind`, which `score` tells it from a frozen mapping by.
pub(crate) const KIND: &str = "s2w-gate3-committed";

/// The committed file's format.
pub(crate) const FORMAT: u32 = 1;

/// The only arm this build commits ("H plus System 2"); B3 ships with the sampler.
pub(crate) const ARM: &str = "h-s2";

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
    /// every pin, so `score` checks it exactly as it checks a frozen mapping.
    pub heuristic: Frozen,
    pub provider: String,
    /// The model snapshot, as configured.
    pub model: String,
    /// The price row the run was charged by.
    pub price: Price,
    pub sample_events: usize,
    pub sample_string_chars: usize,
    pub prompt_files_hash: String,
    /// The input's canonical JSON and `prompt_files_hash`, length-prefixed, FNV-1a 64.
    pub input_hash: String,
    pub attempts: u32,
    pub mapping: Option<StreamMapping>,
    pub failure: Option<String>,
    /// The clean-session probe's call.
    pub probe: CallRecord,
    pub spend: Spend,
    /// sha256 of the sibling transcript (recording format 2 of the proposal's calls).
    pub transcript_sha256: String,
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
    let skip = window.len().saturating_sub(SAMPLE_EVENTS);
    let sample = window[skip..]
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

/// `input`'s hash with `prompt_files_hash`, built as the app builds a manifest's input hash.
///
/// # Errors
///
/// The input does not serialize.
pub(crate) fn input_hash(input: &MappingInput, prompt_files_hash: &str) -> Result<String, String> {
    let json = serde_json::to_vec(input).map_err(|e| format!("input: {e}"))?;
    let mut hasher = Fnv64::new();
    hasher
        .write_field(&json)
        .write_field(prompt_files_hash.as_bytes());
    Ok(format!("{:016x}", hasher.finish()))
}

/// What one run of the arm produced.
pub(crate) struct Ran {
    pub probe: CallRecord,
    pub outcome: MappingOutcome,
    pub spend: Spend,
}

/// The probe through `probe_provider`, then the proposal through `proposer`, both under the
/// budget gate at `price`. The live run and `score`'s replay both call this.
///
/// # Errors
///
/// The probe failed: the session is not proven clean, so there is no replicate.
pub(crate) fn run<P: Provider, Q: Provider>(
    probe_provider: &P,
    proposer: &MappingProposer<Q>,
    price: &Price,
    input: &MappingInput,
) -> Result<Ran, String> {
    let mut gate = BudgetGate::new(price, 0.0);
    let probe = clean_session_probe(probe_provider, &mut gate).map_err(|(reason, call)| {
        let charged: f64 = call.map_or(0.0, |call| gate.charged(&[*call]).iter().sum());
        format!("{reason} (the probe was charged ${charged:.4})")
    })?;
    let probe_charged = gate.charged(std::slice::from_ref(&probe));
    let mut gate = BudgetGate::new(price, probe_charged.iter().sum());
    let outcome = proposer.propose(input, &mut gate);
    let charged = [probe_charged, gate.charged(&outcome.calls)].concat();
    let spend = Spend::of(&probe, &outcome.calls, charged);
    Ok(Ran {
        probe,
        outcome,
        spend,
    })
}

/// The result as the committed file holds it: `(mapping, failure)`.
pub(crate) fn split(result: &MappingResult) -> (Option<StreamMapping>, Option<String>) {
    match result {
        MappingResult::Mapping(mapping) => (Some(mapping.clone()), None),
        MappingResult::Failure(reason) => (None, Some(reason.clone())),
    }
}
