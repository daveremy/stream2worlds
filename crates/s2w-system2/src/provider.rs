//! The provider seam: one prompt in, one reply out.

/// One model reply and what it cost. Every count is `None` unless the provider reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    /// The reply text, as the model produced it.
    pub text: String,
    /// Input tokens, when the provider reports them: tokens read from no cache.
    pub input_tokens: Option<u64>,
    /// Output tokens, when the provider reports them.
    pub output_tokens: Option<u64>,
    /// Wall time of the call, when the provider measured it.
    pub latency_ms: Option<u64>,
    /// Input tokens read from the provider's prompt cache, when it reports them.
    pub cache_read_tokens: Option<u64>,
    /// Input tokens written to the provider's prompt cache, when it reports them.
    pub cache_write_tokens: Option<u64>,
    /// The provider's own cost figure in US dollars, when it reports one. A cross-check only:
    /// spend is computed from tokens and a committed price table, never from this.
    pub cost_usd: Option<f64>,
    /// The model the provider says answered, when it says. Recorded beside the configured
    /// model, never in its place.
    pub model: Option<String>,
}

/// Why a provider call produced no reply.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    /// The command could not be started.
    #[error("exec: could not start the command: {0}")]
    Spawn(String),
    /// The command ran past its time limit and was killed.
    #[error("exec: timed out after {secs} s")]
    Timeout {
        /// The limit, in seconds.
        secs: u64,
        /// Wall time until the kill.
        latency_ms: u64,
        /// The stdout captured before the kill, if any.
        stdout: Option<String>,
    },
    /// The command wrote more than the stdout cap.
    #[error("exec: stdout is over {limit} bytes")]
    StdoutTooLarge {
        /// The cap, in bytes.
        limit: usize,
        /// Wall time of the call.
        latency_ms: u64,
        /// The first `limit` bytes of stdout.
        stdout: String,
    },
    /// The command exited unsuccessfully.
    #[error("exec: exited with {status}: {stderr}")]
    Exit {
        /// The exit status, as the platform prints it.
        status: String,
        /// Its stderr, capped.
        stderr: String,
        /// Wall time of the call.
        latency_ms: u64,
        /// Its stdout, if any.
        stdout: Option<String>,
    },
    /// The command's stdout is not UTF-8.
    #[error("exec: stdout is not UTF-8")]
    NotUtf8 {
        /// Wall time of the call.
        latency_ms: u64,
    },
    /// The command exited, but a process it started (outside its process group) still held
    /// stdout open after the drain grace, so the reply may be incomplete.
    #[error("exec: stdout was still open after the command exited")]
    StdoutHeld {
        /// Wall time of the call.
        latency_ms: u64,
    },
    /// Reading from or writing to the command failed.
    #[error("exec: {0}")]
    Io(String),
    /// The reply format did not hold: the envelope is not JSON, lacks a field, or reports an
    /// error.
    #[error("envelope: {reason}")]
    Envelope {
        /// What was wrong.
        reason: String,
        /// Wall time of the call.
        latency_ms: u64,
        /// The stdout that was parsed.
        stdout: String,
    },
    /// A recorded failure, replayed. It displays as the failure displayed when it was recorded.
    #[error("{error}")]
    Replayed {
        /// The recorded failure's text.
        error: String,
        /// The recorded wall time, if any.
        latency_ms: Option<u64>,
    },
    /// A replay provider holds no reply for this prompt.
    #[error("replay: no recorded reply for prompt {prompt_hash}")]
    NotRecorded {
        /// The prompt's hash.
        prompt_hash: String,
    },
}

impl ProviderError {
    /// The output captured before the failure, persisted as the row's `raw`.
    #[must_use]
    pub fn raw(&self) -> Option<&str> {
        match self {
            Self::Timeout { stdout, .. } | Self::Exit { stdout, .. } => stdout.as_deref(),
            Self::StdoutTooLarge { stdout, .. } | Self::Envelope { stdout, .. } => Some(stdout),
            Self::Spawn(_)
            | Self::Replayed { .. }
            | Self::NotUtf8 { .. }
            | Self::StdoutHeld { .. }
            | Self::Io(_)
            | Self::NotRecorded { .. } => None,
        }
    }

    /// Wall time spent before the failure, when it was measured.
    #[must_use]
    pub const fn latency_ms(&self) -> Option<u64> {
        match self {
            Self::Timeout { latency_ms, .. }
            | Self::StdoutTooLarge { latency_ms, .. }
            | Self::Exit { latency_ms, .. }
            | Self::NotUtf8 { latency_ms }
            | Self::StdoutHeld { latency_ms }
            | Self::Envelope { latency_ms, .. } => Some(*latency_ms),
            Self::Replayed { latency_ms, .. } => *latency_ms,
            Self::Spawn(_) | Self::Io(_) | Self::NotRecorded { .. } => None,
        }
    }
}

/// A model behind one call: the prompt in, the reply out. A provider holds no conversation;
/// a repair call carries its history in the prompt.
pub trait Provider {
    /// Sends `prompt` and returns the reply.
    ///
    /// # Errors
    ///
    /// When no reply was produced; see [`ProviderError`].
    fn complete(&self, prompt: &str) -> Result<Reply, ProviderError>;
}
