//! `System2Proposer`: a dashboard manifest proposer over a [`Provider`] (decision 0029).

use s2w_model::{
    DashboardManifest, ManifestInput, ManifestOutcome, ManifestProposer, ProposerId, ProposerTrace,
};

use crate::prompt;
use crate::provider::{Provider, ProviderError, Reply};

/// Proposes a manifest by asking a model through `P`.
///
/// One attempt is at most two calls: the first reply and, when it is not a manifest that
/// validates, one repair call that carries the reply and the fault back. The attempt's
/// tokens and latency are summed over its calls, and `raw` is the last reply. A provider
/// failure (a timeout, an exec failure) is not repaired: the model did not answer.
#[derive(Debug)]
pub struct System2Proposer<P: Provider> {
    provider: P,
    id: ProposerId,
    prompt_hash: String,
}

impl<P: Provider> System2Proposer<P> {
    /// A proposer recorded as `id` (from configuration, never from a reply).
    pub fn new(provider: P, id: ProposerId) -> Self {
        Self {
            provider,
            id,
            prompt_hash: prompt::prompt_hash(),
        }
    }

    /// The provider.
    pub const fn provider(&self) -> &P {
        &self.provider
    }

    /// One call, folded into `trace`.
    fn call(&self, prompt: &str, trace: &mut ProposerTrace) -> Result<String, ProviderError> {
        match self.provider.complete(prompt) {
            Ok(reply) => {
                add_reply(trace, &reply);
                trace.raw = Some(reply.text.clone());
                Ok(reply.text)
            }
            Err(error) => {
                trace.latency_ms = sum(trace.latency_ms, error.latency_ms());
                // A failed repair call keeps the first reply unless it captured output.
                if let Some(raw) = error.raw() {
                    trace.raw = Some(raw.to_owned());
                }
                Err(error)
            }
        }
    }
}

impl<P: Provider> ManifestProposer for System2Proposer<P> {
    fn id(&self) -> ProposerId {
        self.id.clone()
    }

    fn prompt_hash(&self) -> Option<String> {
        Some(self.prompt_hash.clone())
    }

    fn propose(&self, input: &ManifestInput) -> ManifestOutcome {
        if input.sources.is_empty() {
            return ManifestOutcome::Abstain("no mapped source".to_owned());
        }
        let mut trace = ProposerTrace::default();
        let first = match prompt::manifest_prompt(input) {
            Ok(text) => text,
            Err(e) => return invalid(format!("prompt: {e}"), trace),
        };
        let reply = match self.call(&first, &mut trace) {
            Ok(text) => text,
            Err(e) => return invalid(e.to_string(), trace),
        };
        let fault = match accept(&reply, input) {
            Ok(manifest) => return manifest_outcome(manifest, trace),
            Err(fault) => fault,
        };
        let second = match prompt::repair_prompt(&first, &reply, &fault) {
            Ok(text) => text,
            Err(e) => return invalid(format!("prompt: {e}"), trace),
        };
        match self.call(&second, &mut trace) {
            Ok(text) => match accept(&text, input) {
                Ok(manifest) => manifest_outcome(manifest, trace),
                Err(fault) => invalid(fault, trace),
            },
            Err(e) => invalid(e.to_string(), trace),
        }
    }
}

/// The manifest in `reply`, if it decodes and validates against `input`; else the fault.
fn accept(reply: &str, input: &ManifestInput) -> Result<DashboardManifest, String> {
    let body = unfence(reply);
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
    let manifest: DashboardManifest =
        serde_json::from_value(value).map_err(|e| format!("decode: {e}"))?;
    manifest
        .validate(&input.context())
        .map_err(|e| format!("validator: {e}"))?;
    Ok(manifest)
}

/// `reply` without surrounding whitespace and one surrounding code fence, if it has one.
fn unfence(reply: &str) -> &str {
    let trimmed = reply.trim();
    let Some(inner) = trimmed
        .strip_prefix("```")
        .and_then(|rest| rest.strip_suffix("```"))
    else {
        return trimmed;
    };
    // The opening fence's line may name a language; the body starts on the next line.
    inner
        .split_once('\n')
        .map_or(inner, |(_, body)| body)
        .trim()
}

fn manifest_outcome(manifest: DashboardManifest, trace: ProposerTrace) -> ManifestOutcome {
    ManifestOutcome::Manifest {
        manifest: Box::new(manifest),
        trace,
    }
}

const fn invalid(error: String, trace: ProposerTrace) -> ManifestOutcome {
    ManifestOutcome::Invalid { error, trace }
}

fn add_reply(trace: &mut ProposerTrace, reply: &Reply) {
    trace.input_tokens = sum(trace.input_tokens, reply.input_tokens);
    trace.output_tokens = sum(trace.output_tokens, reply.output_tokens);
    trace.latency_ms = sum(trace.latency_ms, reply.latency_ms);
}

/// The sum of the known values; `None` only when neither is known.
fn sum(total: Option<u64>, more: Option<u64>) -> Option<u64> {
    match (total, more) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests;
