//! The provider seam: one prompt in, one reply out.

/// One model reply and what it cost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The reply text, as the model produced it.
    pub text: String,
    /// Input tokens, when the provider reports them.
    pub input_tokens: Option<u64>,
    /// Output tokens, when the provider reports them.
    pub output_tokens: Option<u64>,
    /// Wall time of the call, when the provider measured it.
    pub latency_ms: Option<u64>,
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
    /// Reading from or writing to the command failed.
    #[error("exec: {0}")]
    Io(String),
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
            Self::StdoutTooLarge { stdout, .. } => Some(stdout),
            Self::Spawn(_) | Self::NotUtf8 { .. } | Self::Io(_) | Self::NotRecorded { .. } => None,
        }
    }

    /// Wall time spent before the failure, when it was measured.
    #[must_use]
    pub const fn latency_ms(&self) -> Option<u64> {
        match self {
            Self::Timeout { latency_ms, .. }
            | Self::StdoutTooLarge { latency_ms, .. }
            | Self::Exit { latency_ms, .. }
            | Self::NotUtf8 { latency_ms } => Some(*latency_ms),
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
