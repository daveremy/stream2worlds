//! `ReplayProvider`: recorded replies keyed by prompt hash. A replayed run calls no model.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use s2w_model::{fnv1a64_hex, is_hex16};
use serde::{Deserialize, Serialize};

use crate::provider::{Provider, ProviderError, Reply};

/// The recording file's format number.
const FORMAT: u32 = 1;

/// Why a recording could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    /// The recording is not valid JSON of the expected shape.
    #[error("recording: {0}")]
    Json(#[from] serde_json::Error),
    /// The recording's format number is not one this build reads.
    #[error("recording: format {0} is not {FORMAT}")]
    Format(u32),
    /// A prompt hash is not 16 lowercase hex digits.
    #[error("recording: {0:?} is not a prompt hash")]
    Hash(String),
    /// Two replies share a prompt hash.
    #[error("recording: prompt {0} is recorded twice")]
    Duplicate(String),
}

/// The recording file: `{"format": 1, "replies": [...]}`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recording {
    format: u32,
    replies: Vec<Recorded>,
}

/// One recorded reply.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recorded {
    prompt_hash: String,
    reply: String,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    latency_ms: Option<u64>,
}

/// A provider that answers from recorded replies. A prompt it has no reply for is a
/// [`ProviderError::NotRecorded`], which the proposer persists like any other failure.
#[derive(Debug, Default)]
pub struct ReplayProvider {
    replies: BTreeMap<String, Reply>,
    calls: Mutex<Vec<String>>,
}

impl ReplayProvider {
    /// A provider with no replies.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `reply` as the answer to `prompt`, replacing any earlier one.
    pub fn insert(&mut self, prompt: &str, reply: Reply) {
        self.replies.insert(hash(prompt), reply);
    }

    /// Reads a recording.
    ///
    /// # Errors
    ///
    /// When the text is not a recording, its format is not 1, a hash is malformed, or a hash
    /// appears twice.
    pub fn from_json(text: &str) -> Result<Self, ReplayError> {
        let recording: Recording = serde_json::from_str(text)?;
        if recording.format != FORMAT {
            return Err(ReplayError::Format(recording.format));
        }
        let mut replies = BTreeMap::new();
        for row in recording.replies {
            if !is_hex16(&row.prompt_hash) {
                return Err(ReplayError::Hash(row.prompt_hash));
            }
            let reply = Reply {
                text: row.reply,
                input_tokens: row.input_tokens,
                output_tokens: row.output_tokens,
                latency_ms: row.latency_ms,
            };
            if replies.insert(row.prompt_hash.clone(), reply).is_some() {
                return Err(ReplayError::Duplicate(row.prompt_hash));
            }
        }
        Ok(Self {
            replies,
            calls: Mutex::default(),
        })
    }

    /// The recording, in the format [`ReplayProvider::from_json`] reads, sorted by hash.
    ///
    /// # Errors
    ///
    /// When serialization fails.
    pub fn to_json(&self) -> Result<String, ReplayError> {
        let replies = self
            .replies
            .iter()
            .map(|(prompt_hash, reply)| Recorded {
                prompt_hash: prompt_hash.clone(),
                reply: reply.text.clone(),
                input_tokens: reply.input_tokens,
                output_tokens: reply.output_tokens,
                latency_ms: reply.latency_ms,
            })
            .collect();
        let recording = Recording {
            format: FORMAT,
            replies,
        };
        Ok(serde_json::to_string_pretty(&recording)?)
    }

    /// The prompt hashes asked so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Provider for ReplayProvider {
    fn complete(&self, prompt: &str) -> Result<Reply, ProviderError> {
        let prompt_hash = hash(prompt);
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(prompt_hash.clone());
        self.replies
            .get(&prompt_hash)
            .cloned()
            .ok_or(ProviderError::NotRecorded { prompt_hash })
    }
}

/// The key a prompt is recorded under.
fn hash(prompt: &str) -> String {
    fnv1a64_hex(prompt.as_bytes())
}

#[cfg(test)]
mod tests;
