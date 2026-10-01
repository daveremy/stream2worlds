//! `ReplayProvider`: recorded replies keyed by prompt hash. A replayed run calls no model.
//!
//! Two recording formats are read. Format 1 holds one reply per prompt, answered as often as
//! it is asked. Format 2 (decision 0032) holds every call of a proposal as a [`CallRecord`],
//! failures included, and is strict: the n-th ask of a prompt gets the n-th row recorded for
//! it, and an ask past the last row is [`ProviderError::NotRecorded`].

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use s2w_model::{fnv1a64_hex, is_hex16};
use serde::{Deserialize, Serialize};

use crate::provider::{Provider, ProviderError, Reply};
use crate::record::CallRecord;

/// The format number of a recording of one reply per prompt.
const FORMAT_REPLIES: u32 = 1;

/// The format number of a recording of every call (decision 0032).
const FORMAT_CALLS: u32 = 2;

/// Why a recording could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    /// The recording is not valid JSON of the expected shape.
    #[error("recording: {0}")]
    Json(#[from] serde_json::Error),
    /// The recording's format number is not one this build reads.
    #[error("recording: format {0} is not {FORMAT_REPLIES} or {FORMAT_CALLS}")]
    Format(u32),
    /// A prompt hash is not 16 lowercase hex digits.
    #[error("recording: {0:?} is not a prompt hash")]
    Hash(String),
    /// Two replies share a prompt hash in a format 1 recording.
    #[error("recording: prompt {0} is recorded twice")]
    Duplicate(String),
    /// A format 2 row sets both or neither of `reply` and `error`.
    #[error("recording: the row for prompt {0} needs exactly one of reply and error")]
    Row(String),
    /// Format 1 cannot hold this provider: it has a failure or several rows for one prompt.
    #[error("recording: format {FORMAT_REPLIES} cannot hold a failure or a repeated prompt")]
    Lossy,
}

/// Just the format number, read first to choose the shape.
#[derive(Deserialize)]
struct Header {
    format: u32,
}

/// The format 1 file: `{"format": 1, "replies": [...]}`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recording {
    format: u32,
    replies: Vec<Recorded>,
}

/// One format 1 reply.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recorded {
    prompt_hash: String,
    reply: String,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    latency_ms: Option<u64>,
}

/// The format 2 file: `{"format": 2, "calls": [...]}`, calls in the order they were made.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Calls {
    format: u32,
    calls: Vec<CallRecord>,
}

/// What replay has served so far.
#[derive(Debug, Default)]
struct Served {
    /// Every prompt hash asked, in order.
    calls: Vec<String>,
    /// Per prompt hash, how many rows have been served (format 2).
    taken: BTreeMap<String, usize>,
}

/// A provider that answers from recorded replies. A prompt it has no reply for is a
/// [`ProviderError::NotRecorded`], which the proposer persists like any other failure.
#[derive(Debug, Default)]
pub struct ReplayProvider {
    answers: BTreeMap<String, Vec<Result<Reply, ProviderError>>>,
    /// Format 2: each row answers once. Otherwise a prompt's one row answers every ask.
    strict: bool,
    served: Mutex<Served>,
}

impl ReplayProvider {
    /// A provider with no replies.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `reply` as the answer to `prompt`, replacing any earlier one.
    pub fn insert(&mut self, prompt: &str, reply: Reply) {
        self.answers.insert(hash(prompt), vec![Ok(reply)]);
    }

    /// Reads a recording, format 1 or 2.
    ///
    /// # Errors
    ///
    /// When the text is not a recording, its format is neither, a hash is malformed, a format 1
    /// hash appears twice, or a format 2 row sets both or neither of reply and error.
    pub fn from_json(text: &str) -> Result<Self, ReplayError> {
        let header: Header = serde_json::from_str(text)?;
        match header.format {
            FORMAT_REPLIES => Self::from_replies(serde_json::from_str(text)?),
            FORMAT_CALLS => {
                let calls: Calls = serde_json::from_str(text)?;
                Self::from_calls(&calls.calls)
            }
            other => Err(ReplayError::Format(other)),
        }
    }

    fn from_replies(recording: Recording) -> Result<Self, ReplayError> {
        let mut answers = BTreeMap::new();
        for row in recording.replies {
            if !is_hex16(&row.prompt_hash) {
                return Err(ReplayError::Hash(row.prompt_hash));
            }
            let reply = Reply {
                text: row.reply,
                input_tokens: row.input_tokens,
                output_tokens: row.output_tokens,
                latency_ms: row.latency_ms,
                ..Reply::default()
            };
            if answers
                .insert(row.prompt_hash.clone(), vec![Ok(reply)])
                .is_some()
            {
                return Err(ReplayError::Duplicate(row.prompt_hash));
            }
        }
        Ok(Self {
            answers,
            ..Self::default()
        })
    }

    /// A strict provider answering from `calls`, as recording format 2 holds them.
    ///
    /// # Errors
    ///
    /// When a hash is malformed or a row sets both or neither of reply and error.
    pub fn from_calls(calls: &[CallRecord]) -> Result<Self, ReplayError> {
        let mut answers: BTreeMap<String, Vec<_>> = BTreeMap::new();
        for row in calls {
            if !is_hex16(&row.prompt_hash) {
                return Err(ReplayError::Hash(row.prompt_hash.clone()));
            }
            let answer = row
                .replayed()
                .ok_or_else(|| ReplayError::Row(row.prompt_hash.clone()))?;
            answers
                .entry(row.prompt_hash.clone())
                .or_default()
                .push(answer);
        }
        Ok(Self {
            answers,
            strict: true,
            ..Self::default()
        })
    }

    /// The format 1 recording [`ReplayProvider::from_json`] reads, sorted by hash.
    ///
    /// # Errors
    ///
    /// When serialization fails, or the provider holds a failure or several rows for one
    /// prompt, which format 1 cannot hold; write those with [`recording_json`].
    pub fn to_json(&self) -> Result<String, ReplayError> {
        let mut replies = Vec::with_capacity(self.answers.len());
        for (prompt_hash, answers) in &self.answers {
            let [Ok(reply)] = answers.as_slice() else {
                return Err(ReplayError::Lossy);
            };
            replies.push(Recorded {
                prompt_hash: prompt_hash.clone(),
                reply: reply.text.clone(),
                input_tokens: reply.input_tokens,
                output_tokens: reply.output_tokens,
                latency_ms: reply.latency_ms,
            });
        }
        let recording = Recording {
            format: FORMAT_REPLIES,
            replies,
        };
        Ok(serde_json::to_string_pretty(&recording)?)
    }

    /// The prompt hashes asked so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.served
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .calls
            .clone()
    }
}

/// `calls` as recording format 2, in the order given.
///
/// # Errors
///
/// When serialization fails.
pub fn recording_json(calls: &[CallRecord]) -> Result<String, ReplayError> {
    let recording = Calls {
        format: FORMAT_CALLS,
        calls: calls.to_vec(),
    };
    Ok(serde_json::to_string_pretty(&recording)?)
}

impl Provider for ReplayProvider {
    fn complete(&self, prompt: &str) -> Result<Reply, ProviderError> {
        let prompt_hash = hash(prompt);
        let mut served = self.served.lock().unwrap_or_else(PoisonError::into_inner);
        served.calls.push(prompt_hash.clone());
        let rows = self.answers.get(&prompt_hash);
        let row = if self.strict {
            let taken = served.taken.entry(prompt_hash.clone()).or_default();
            let row = rows.and_then(|rows| rows.get(*taken));
            *taken += 1;
            row
        } else {
            rows.and_then(|rows| rows.first())
        };
        row.cloned()
            .unwrap_or(Err(ProviderError::NotRecorded { prompt_hash }))
    }
}

/// The key a prompt is recorded under.
pub(crate) fn hash(prompt: &str) -> String {
    fnv1a64_hex(prompt.as_bytes())
}

#[cfg(test)]
mod tests;
