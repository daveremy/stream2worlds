//! How an [`ExecProvider`]'s stdout becomes a [`Reply`] (decision 0032).

use std::collections::BTreeMap;

use serde::Deserialize;

use super::ExecProvider;
use crate::provider::{ProviderError, Reply};

/// What the command prints on stdout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReplyFormat {
    /// The reply text and nothing else. Tokens and cost are unknown.
    #[default]
    Text,
    /// The Claude CLI's `--output-format json` envelope: one JSON object holding the reply in
    /// `result`, the token counts in `usage`, and the CLI's cost figure in `total_cost_usd`.
    /// A missing count is an error, never `None`: a ledger that sums `None` as zero under-counts.
    ClaudeJson,
}

impl ExecProvider {
    /// The same provider reading stdout as `format`.
    #[must_use]
    pub const fn with_format(mut self, format: ReplyFormat) -> Self {
        self.format = format;
        self
    }
}

/// The fields of the envelope this build reads. Other fields are ignored: the envelope is a
/// third party's format and grows.
#[derive(Deserialize)]
struct Envelope {
    result: Option<String>,
    #[serde(default)]
    is_error: bool,
    subtype: Option<String>,
    usage: Usage,
    total_cost_usd: f64,
    #[serde(default, rename = "modelUsage")]
    model_usage: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
}

/// `stdout` read as `format`.
pub(super) fn reply(
    format: ReplyFormat,
    stdout: String,
    latency_ms: u64,
) -> Result<Reply, ProviderError> {
    match format {
        ReplyFormat::Text => Ok(Reply {
            text: stdout,
            latency_ms: Some(latency_ms),
            ..Reply::default()
        }),
        ReplyFormat::ClaudeJson => claude_json(stdout, latency_ms),
    }
}

fn claude_json(stdout: String, latency_ms: u64) -> Result<Reply, ProviderError> {
    let fail = |reason: String, stdout: String| ProviderError::Envelope {
        reason,
        latency_ms,
        stdout,
    };
    let envelope: Envelope = match serde_json::from_str(&stdout) {
        Ok(envelope) => envelope,
        Err(e) => return Err(fail(e.to_string(), stdout)),
    };
    if envelope.is_error {
        let subtype = envelope.subtype.as_deref().unwrap_or("none");
        return Err(fail(format!("is_error, subtype {subtype}"), stdout));
    }
    let Some(text) = envelope.result else {
        return Err(fail("no result".to_owned(), stdout));
    };
    if !envelope.total_cost_usd.is_finite() || envelope.total_cost_usd < 0.0 {
        return Err(fail("total_cost_usd is not a cost".to_owned(), stdout));
    }
    // Several keys means the CLI called several models; all are named, sorted.
    let models: Vec<&str> = envelope.model_usage.keys().map(String::as_str).collect();
    let usage = envelope.usage;
    Ok(Reply {
        text,
        input_tokens: Some(usage.input_tokens),
        output_tokens: Some(usage.output_tokens),
        latency_ms: Some(latency_ms),
        cache_read_tokens: Some(usage.cache_read_input_tokens),
        cache_write_tokens: Some(usage.cache_creation_input_tokens),
        cost_usd: Some(envelope.total_cost_usd),
        model: (!models.is_empty()).then(|| models.join(",")),
    })
}

#[cfg(test)]
mod tests;
