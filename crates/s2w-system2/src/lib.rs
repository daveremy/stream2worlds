//! System 2: asynchronous, budget-capped model passes over world snapshots. Proposals are inert until accepted; stream text is untrusted.
//!
//! [`System2Proposer`] is a dashboard manifest proposer (decision 0029) over a [`Provider`]: the
//! seam between building a prompt and getting a reply. Two providers ship: [`ExecProvider`] runs
//! a configured command with the prompt on stdin, and [`ReplayProvider`] answers from recorded
//! replies keyed by prompt hash, so a run can be replayed without calling a model.
//!
//! [`MappingProposer`] proposes a stream mapping for either arm of the gate-3 comparison
//! (decision 0032) under a [`CallGate`], and records every call as a [`CallRecord`].
//! [`clean_session_probe`] is the first call of a gate-3 run: it proves the session sees nothing
//! besides the model's built-in system prompt.

mod exec;
#[cfg(test)]
mod fixture;
mod manifest;
mod mapping;
mod probe;
mod prompt;
mod provider;
mod record;
mod replay;

pub use exec::{ExecLimits, ExecProvider, ExecSetupError, ReplyFormat};
pub use manifest::System2Proposer;
pub use mapping::{CallGate, MAX_ATTEMPTS, MappingOutcome, MappingProposer, MappingResult, NoGate};
pub use probe::{PROBE_CALL, clean_session_probe};
pub use provider::{Provider, ProviderError, Reply};
pub use record::CallRecord;
pub use replay::{ReplayError, ReplayProvider, recording_json};
