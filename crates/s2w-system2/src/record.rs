//! `CallRecord`: one model call as recording format 2 stores it (decision 0032).

use serde::{Deserialize, Serialize};

use crate::provider::{ProviderError, Reply};

/// One call: where it sat in the proposal, the prompt it answered, and what came back. A
/// row of recording format 2 and of a proposal's ledger. Exactly one of `reply` and `error`
/// is set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRecord {
    /// The attempt, from 1.
    pub attempt: u32,
    /// The call within the attempt, from 1: 1 is the first prompt, 2 the repair.
    pub call: u32,
    /// The hash of the prompt as sent, 16 lowercase hex digits: the replay key.
    pub prompt_hash: String,
    /// The reply text, when the call produced one.
    pub reply: Option<String>,
    /// The failure, as it displays, when the call produced no reply.
    pub error: Option<String>,
    /// The model the provider says answered, when it says.
    pub model: Option<String>,
    /// Input tokens read from no cache, when reported.
    pub input_tokens: Option<u64>,
    /// Output tokens, when reported.
    pub output_tokens: Option<u64>,
    /// Input tokens read from the prompt cache, when reported.
    pub cache_read_tokens: Option<u64>,
    /// Input tokens written to the prompt cache, when reported.
    pub cache_write_tokens: Option<u64>,
    /// The provider's own cost figure in US dollars, when reported; a cross-check only.
    pub cost_usd: Option<f64>,
    /// Wall time of the call, when measured.
    pub latency_ms: Option<u64>,
    /// When the call started, in milliseconds since the Unix epoch, when a clock was given.
    pub started_at_ms: Option<u64>,
}

impl CallRecord {
    /// The record of one call that sent a prompt hashed `prompt_hash` and got `result`.
    #[must_use]
    pub fn new(
        (attempt, call): (u32, u32),
        prompt_hash: String,
        result: &Result<Reply, ProviderError>,
        started_at_ms: Option<u64>,
    ) -> Self {
        let empty = Reply::default();
        let (reply, error, usage) = match result {
            Ok(reply) => (Some(reply.text.clone()), None, reply),
            Err(e) => (None, Some(e.to_string()), &empty),
        };
        let latency_ms = match result {
            Ok(reply) => reply.latency_ms,
            Err(e) => e.latency_ms(),
        };
        Self {
            attempt,
            call,
            prompt_hash,
            reply,
            error,
            model: usage.model.clone(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            cost_usd: usage.cost_usd,
            latency_ms,
            started_at_ms,
        }
    }

    /// What a replay of this call answers: the reply, or the failure as it displayed. `None`
    /// when the row sets both or neither.
    #[must_use]
    pub fn replayed(&self) -> Option<Result<Reply, ProviderError>> {
        match (&self.reply, &self.error) {
            (Some(text), None) => Some(Ok(Reply {
                text: text.clone(),
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
                latency_ms: self.latency_ms,
                cache_read_tokens: self.cache_read_tokens,
                cache_write_tokens: self.cache_write_tokens,
                cost_usd: self.cost_usd,
                model: self.model.clone(),
            })),
            (None, Some(error)) => Some(Err(ProviderError::Replayed {
                error: error.clone(),
                latency_ms: self.latency_ms,
            })),
            _ => None,
        }
    }
}
