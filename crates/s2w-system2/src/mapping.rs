//! `MappingProposer`: a stream mapping proposer over a [`Provider`] for the gate-3 comparison
//! (s2w#373, decision 0032). It serves both arms: [`MappingProposer::propose`] reads a
//! [`MappingInput`] ("H plus System 2"), [`MappingProposer::propose_raw`] a
//! [`RawMappingInput`] (B3). Both share one reply format, one repair prompt, one no-match
//! check through [`MappingCheck`] (s2w#409) and one retry policy, and both record every call
//! they make as a [`CallRecord`].

use std::time::{SystemTime, UNIX_EPOCH};

use s2w_model::{MappingInput, RawMappingInput, StreamMapping};
use serde::{Deserialize, Serialize};

use crate::manifest::decode_reply;
use crate::prompt;
use crate::provider::Provider;
use crate::record::CallRecord;
use crate::replay;

/// The most attempts one proposal makes. Only a provider failure starts another attempt.
pub const MAX_ATTEMPTS: u32 = 2;

/// [`CallRecord::call`] of an attempt's first call, its format repair and its no-match repair.
const FIRST_CALL: u32 = 1;
const FORMAT_REPAIR_CALL: u32 = 2;
const NO_MATCH_REPAIR_CALL: u32 = 3;

/// Asked before every call. A budget stop plugs in here.
pub trait CallGate {
    /// `Ok` lets the call with `prompt` go ahead; `Err(reason)` stops the proposal before it
    /// is made, and `reason` becomes the failure. `calls` holds every call made so far, with
    /// its tokens and cost.
    ///
    /// # Errors
    ///
    /// When the call must not be made.
    fn before_call(&mut self, prompt: &str, calls: &[CallRecord]) -> Result<(), String>;
}

/// A gate that lets every call through.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoGate;

impl CallGate for NoGate {
    fn before_call(&mut self, _prompt: &str, _calls: &[CallRecord]) -> Result<(), String> {
        Ok(())
    }
}

/// Asked once per attempt, on the attempt's first reply that decodes and validates, and once
/// more on the no-match repair's mapping (s2w#409). A no-match is a mapping that claims nothing
/// from any of the arm's sampled records; this crate cannot apply a mapping (decision 0001), so
/// the driver supplies the check.
pub trait MappingCheck {
    /// `Ok(true)` when `mapping` matches nothing: the proposer makes one no-match repair call.
    ///
    /// # Errors
    ///
    /// When the check cannot run. The proposal stops before any further call, and the failure
    /// is `check: <reason>`.
    fn no_match(&self, mapping: &StreamMapping) -> Result<bool, String>;
}

/// A check that never finds a no-match, for callers with no sample to check against.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCheck;

impl MappingCheck for NoCheck {
    fn no_match(&self, _mapping: &StreamMapping) -> Result<bool, String> {
        Ok(false)
    }
}

/// What the no-match check found in the final attempt (s2w#409).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoMatch {
    /// Whether the attempt's first mapping that decoded and validated matched nothing; `None`
    /// when no reply decoded and validated, or the check failed.
    pub first: Option<bool>,
    /// No-match repair calls made: 0 or 1.
    pub repair_calls: u32,
    /// Whether the final mapping matches nothing; `None` when the result is a failure.
    pub after: Option<bool>,
}

/// What a proposal ended with: exactly one of a mapping that validates and a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappingResult {
    /// A mapping that decoded and validated.
    Mapping(StreamMapping),
    /// Why there is none: `invalid: <fault>` when a repaired reply failed to decode or
    /// validate, `provider: <error>` when the last attempt's call produced no reply,
    /// `prompt: <error>` when the prompt could not be built, `check: <error>` when the no-match
    /// check could not run, or a gate's reason verbatim.
    Failure(String),
}

/// One proposal: its result and every call made for it.
#[derive(Clone, Debug, PartialEq)]
pub struct MappingOutcome {
    /// The mapping or the failure.
    pub result: MappingResult,
    /// Attempts that made at least one call, from 0 to [`MAX_ATTEMPTS`].
    pub attempts: u32,
    /// Every call, in order, failures included.
    pub calls: Vec<CallRecord>,
    /// The hash of the prompt files behind this arm, 16 lowercase hex digits. It folds into a
    /// committed run's `input_hash`; it is not the replay key, which is each call's
    /// [`CallRecord::prompt_hash`].
    pub prompt_files_hash: String,
    /// The no-match check's findings in the final attempt.
    pub no_match: NoMatch,
}

/// Proposes a stream mapping by asking a model through `P`.
///
/// An attempt is at most three calls. The first call sends the first prompt. When its reply
/// does not decode or validate, one format repair call carries the reply and the fault back. The
/// attempt's first mapping that decodes and validates goes to the [`MappingCheck`]; when it
/// matches nothing, one no-match repair call carries the reply back with the fixed no-match
/// fault, and its mapping is final whether or not it matches. A repaired reply that fails to
/// decode or validate ends the proposal. A provider failure in any call is never repaired: it
/// ends the attempt, and the next attempt starts again from the first prompt, up to
/// [`MAX_ATTEMPTS`].
#[derive(Debug)]
pub struct MappingProposer<P: Provider> {
    provider: P,
    clock: fn() -> Option<u64>,
}

impl<P: Provider> MappingProposer<P> {
    /// A proposer whose calls record their start from the system clock.
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            clock: system_clock,
        }
    }

    /// The same proposer with `clock` giving each call's `started_at_ms`.
    #[must_use]
    pub fn with_clock(mut self, clock: fn() -> Option<u64>) -> Self {
        self.clock = clock;
        self
    }

    /// The provider.
    pub const fn provider(&self) -> &P {
        &self.provider
    }

    /// A mapping for the "H plus System 2" arm.
    pub fn propose(
        &self,
        input: &MappingInput,
        gate: &mut dyn CallGate,
        check: &dyn MappingCheck,
    ) -> MappingOutcome {
        self.run(
            prompt::mapping_prompt(input),
            prompt::mapping_prompt_files_hash(),
            gate,
            check,
        )
    }

    /// A mapping for the B3 arm.
    pub fn propose_raw(
        &self,
        input: &RawMappingInput,
        gate: &mut dyn CallGate,
        check: &dyn MappingCheck,
    ) -> MappingOutcome {
        self.run(
            prompt::raw_mapping_prompt(input),
            prompt::raw_mapping_prompt_files_hash(),
            gate,
            check,
        )
    }

    fn run(
        &self,
        first: Result<String, serde_json::Error>,
        prompt_files_hash: String,
        gate: &mut dyn CallGate,
        check: &dyn MappingCheck,
    ) -> MappingOutcome {
        let first = match first {
            Ok(first) => first,
            Err(e) => {
                return MappingOutcome {
                    result: MappingResult::Failure(format!("prompt: {e}")),
                    attempts: 0,
                    calls: Vec::new(),
                    prompt_files_hash,
                    no_match: NoMatch::default(),
                };
            }
        };
        let mut proposal = Proposal {
            first: &first,
            gate,
            check,
            calls: Vec::new(),
            attempts: 0,
            no_match: NoMatch::default(),
        };
        let result = self.attempts(&mut proposal);
        MappingOutcome {
            result,
            attempts: proposal.attempts,
            calls: proposal.calls,
            prompt_files_hash,
            no_match: proposal.no_match,
        }
    }

    fn attempts(&self, p: &mut Proposal<'_>) -> MappingResult {
        // Set by every attempt that does not end the proposal, so it holds the last one's failure.
        let mut last = String::new();
        for attempt in 1..=MAX_ATTEMPTS {
            p.no_match = NoMatch::default();
            match self.attempt(attempt, p) {
                Ended::Result(result) => return result,
                Ended::Failed(error) => last = error,
            }
        }
        MappingResult::Failure(last)
    }

    /// One attempt: the first call, a format repair when its reply fails, then the check.
    fn attempt(&self, attempt: u32, p: &mut Proposal<'_>) -> Ended {
        let first = p.first;
        let reply = match self.call((attempt, FIRST_CALL), first, p) {
            Called::Reply(text) => text,
            Called::Failed(error) => return Ended::Failed(error),
            Called::Stopped(reason) => return Ended::Result(MappingResult::Failure(reason)),
        };
        let (reply, mapping) = match accept(&reply) {
            Ok(mapping) => (reply, mapping),
            Err(fault) => {
                let text = match self.repair((attempt, FORMAT_REPAIR_CALL), &reply, &fault, p) {
                    Ok(text) => text,
                    Err(ended) => return ended,
                };
                match accept(&text) {
                    Ok(mapping) => (text, mapping),
                    Err(fault) => return Ended::invalid(&fault),
                }
            }
        };
        self.checked(attempt, &reply, mapping, p)
    }

    /// The no-match check on the attempt's first valid `mapping` (from `reply`), and the one
    /// no-match repair call when it matches nothing.
    fn checked(
        &self,
        attempt: u32,
        reply: &str,
        mapping: StreamMapping,
        p: &mut Proposal<'_>,
    ) -> Ended {
        let no_match = match p.check.no_match(&mapping) {
            Ok(no_match) => no_match,
            Err(reason) => return Ended::check(&reason),
        };
        p.no_match.first = Some(no_match);
        if !no_match {
            p.no_match.after = Some(false);
            return Ended::Result(MappingResult::Mapping(mapping));
        }
        let fault = prompt::MAPPING_NO_MATCH;
        let text = match self.repair((attempt, NO_MATCH_REPAIR_CALL), reply, fault, p) {
            Ok(text) => text,
            Err(ended) => return ended,
        };
        let repaired = match accept(&text) {
            Ok(mapping) => mapping,
            Err(fault) => return Ended::invalid(&fault),
        };
        match p.check.no_match(&repaired) {
            Ok(after) => {
                p.no_match.after = Some(after);
                Ended::Result(MappingResult::Mapping(repaired))
            }
            Err(reason) => Ended::check(&reason),
        }
    }

    /// One repair call carrying `reply` and `fault` back after the first prompt: its reply.
    fn repair(
        &self,
        step: (u32, u32),
        reply: &str,
        fault: &str,
        p: &mut Proposal<'_>,
    ) -> Result<String, Ended> {
        let prompt = prompt::mapping_repair_prompt(p.first, reply, fault)
            .map_err(|e| Ended::Result(MappingResult::Failure(format!("prompt: {e}"))))?;
        match self.call(step, &prompt, p) {
            Called::Reply(text) => Ok(text),
            Called::Failed(error) => Err(Ended::Failed(error)),
            Called::Stopped(reason) => Err(Ended::Result(MappingResult::Failure(reason))),
        }
    }

    /// One call, gated, recorded in `p`.
    fn call(&self, (attempt, call): (u32, u32), prompt: &str, p: &mut Proposal<'_>) -> Called {
        if let Err(reason) = p.gate.before_call(prompt, &p.calls) {
            return Called::Stopped(reason);
        }
        p.attempts = attempt;
        if call == NO_MATCH_REPAIR_CALL {
            p.no_match.repair_calls += 1;
        }
        let started_at_ms = (self.clock)();
        let result = self.provider.complete(prompt);
        p.calls.push(CallRecord::new(
            (attempt, call),
            replay::hash(prompt),
            &result,
            started_at_ms,
        ));
        match result {
            Ok(reply) => Called::Reply(reply.text),
            Err(e) => Called::Failed(format!("provider: {e}")),
        }
    }
}

/// One proposal in progress: its first prompt, gate and check, and its calls, attempts and
/// no-match findings so far.
struct Proposal<'a> {
    first: &'a str,
    gate: &'a mut dyn CallGate,
    check: &'a dyn MappingCheck,
    calls: Vec<CallRecord>,
    attempts: u32,
    no_match: NoMatch,
}

/// How an attempt ended.
enum Ended {
    /// The proposal's result.
    Result(MappingResult),
    /// A provider failure: the next attempt starts, if there is one.
    Failed(String),
}

impl Ended {
    fn invalid(fault: &str) -> Self {
        Self::Result(MappingResult::Failure(format!("invalid: {fault}")))
    }

    fn check(reason: &str) -> Self {
        Self::Result(MappingResult::Failure(format!("check: {reason}")))
    }
}

/// What one call came to.
enum Called {
    Reply(String),
    /// The provider produced no reply; the failure text.
    Failed(String),
    /// The gate refused the call; its reason.
    Stopped(String),
}

/// The mapping in `reply`, if it decodes and validates; else the fault.
fn accept(reply: &str) -> Result<StreamMapping, String> {
    let mapping: StreamMapping = decode_reply(reply)?;
    mapping.validate().map_err(|e| format!("validator: {e}"))?;
    Ok(mapping)
}

/// Milliseconds since the Unix epoch, when the system clock reads after it.
pub(crate) fn system_clock() -> Option<u64> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(since.as_millis()).ok()
}

#[cfg(test)]
mod tests;
