//! System 2: asynchronous, budget-capped model passes over world snapshots. Proposals are inert until accepted; stream text is untrusted.
//!
//! [`System2Proposer`] is a dashboard manifest proposer (decision 0029) over a [`Provider`]: the
//! seam between building a prompt and getting a reply. Two providers ship: [`ExecProvider`] runs
//! a configured command with the prompt on stdin, and [`ReplayProvider`] answers from recorded
//! replies keyed by prompt hash, so a run can be replayed without calling a model.

mod exec;
#[cfg(test)]
mod fixture;
mod manifest;
mod mapping;
mod prompt;
mod provider;
mod record;
mod replay;

pub use exec::{ExecLimits, ExecProvider, ExecSetupError, ReplyFormat};
pub use manifest::System2Proposer;
pub use mapping::{CallGate, MAX_ATTEMPTS, MappingOutcome, MappingProposer, MappingResult, NoGate};
pub use provider::{Provider, ProviderError, Reply};
pub use record::CallRecord;
pub use replay::{ReplayError, ReplayProvider, recording_json};
