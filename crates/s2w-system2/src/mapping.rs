//! `MappingProposer`: a stream mapping proposer over a [`Provider`] for the gate-3 comparison
//! (s2w#373, decision 0032). It serves both arms: [`MappingProposer::propose`] reads a
//! [`MappingInput`] ("H plus System 2"), [`MappingProposer::propose_raw`] a
//! [`RawMappingInput`] (B3). Both share one reply format, one repair prompt and one retry
//! policy, and both record every call they make as a [`CallRecord`].

use std::time::{SystemTime, UNIX_EPOCH};

use s2w_model::{MappingInput, RawMappingInput, StreamMapping};

use crate::manifest::decode_reply;
use crate::prompt;
use crate::provider::Provider;
use crate::record::CallRecord;
use crate::replay;

/// The most attempts one proposal makes. Only a provider failure starts another attempt.
pub const MAX_ATTEMPTS: u32 = 2;

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

/// What a proposal ended with: exactly one of a mapping that validates and a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappingResult {
    /// A mapping that decoded and validated.
    Mapping(StreamMapping),
    /// Why there is none: `invalid: <fault>` when the repaired reply still failed,
    /// `provider: <error>` when the last attempt's call produced no reply, `prompt: <error>`
    /// when the prompt could not be built, or a gate's reason verbatim.
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
}

/// Proposes a stream mapping by asking a model through `P`.
///
/// An attempt is the first call and, when its reply does not decode or validate, one repair
/// call that carries the reply and the fault back. A repaired reply that still fails ends the
/// proposal. A provider failure in either call is never repaired: it ends the attempt, and the
/// next attempt starts again from the first prompt, up to [`MAX_ATTEMPTS`].
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
    pub fn propose(&self, input: &MappingInput, gate: &mut dyn CallGate) -> MappingOutcome {
        self.run(
            prompt::mapping_prompt(input),
            prompt::mapping_prompt_files_hash(),
            gate,
        )
    }

    /// A mapping for the B3 arm.
    pub fn propose_raw(&self, input: &RawMappingInput, gate: &mut dyn CallGate) -> MappingOutcome {
        self.run(
            prompt::raw_mapping_prompt(input),
            prompt::raw_mapping_prompt_files_hash(),
            gate,
        )
    }

    fn run(
        &self,
        first: Result<String, serde_json::Error>,
        prompt_files_hash: String,
        gate: &mut dyn CallGate,
    ) -> MappingOutcome {
        let mut run = Run {
            calls: Vec::new(),
            attempts: 0,
        };
        let result = match first {
            Ok(first) => self.attempts(&first, gate, &mut run),
            Err(e) => MappingResult::Failure(format!("prompt: {e}")),
        };
        MappingOutcome {
            result,
            attempts: run.attempts,
            calls: run.calls,
            prompt_files_hash,
        }
    }

    fn attempts(&self, first: &str, gate: &mut dyn CallGate, run: &mut Run) -> MappingResult {
        // Set by every attempt that does not return, so it holds the last attempt's failure.
        let mut last = String::new();
        for attempt in 1..=MAX_ATTEMPTS {
            let reply = match self.call((attempt, 1), first, gate, run) {
                Called::Reply(text) => text,
                Called::Failed(error) => {
                    last = error;
                    continue;
                }
                Called::Stopped(reason) => return MappingResult::Failure(reason),
            };
            let fault = match accept(&reply) {
                Ok(mapping) => return MappingResult::Mapping(mapping),
                Err(fault) => fault,
            };
            let repair = match prompt::mapping_repair_prompt(first, &reply, &fault) {
                Ok(text) => text,
                Err(e) => return MappingResult::Failure(format!("prompt: {e}")),
            };
            match self.call((attempt, 2), &repair, gate, run) {
                Called::Reply(text) => {
                    return match accept(&text) {
                        Ok(mapping) => MappingResult::Mapping(mapping),
                        Err(fault) => MappingResult::Failure(format!("invalid: {fault}")),
                    };
                }
                Called::Failed(error) => last = error,
                Called::Stopped(reason) => return MappingResult::Failure(reason),
            }
        }
        MappingResult::Failure(last)
    }

    /// One call, gated, recorded in `run`.
    fn call(
        &self,
        (attempt, call): (u32, u32),
        prompt: &str,
        gate: &mut dyn CallGate,
        run: &mut Run,
    ) -> Called {
        if let Err(reason) = gate.before_call(prompt, &run.calls) {
            return Called::Stopped(reason);
        }
        run.attempts = attempt;
        let started_at_ms = (self.clock)();
        let result = self.provider.complete(prompt);
        run.calls.push(CallRecord::new(
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

/// The calls and attempts of one proposal so far.
struct Run {
    calls: Vec<CallRecord>,
    attempts: u32,
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
fn system_clock() -> Option<u64> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(since.as_millis()).ok()
}

#[cfg(test)]
mod tests;
