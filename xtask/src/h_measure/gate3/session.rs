//! The clean session a gate-3 run calls the model in: the Claude CLI, headless, with no tools,
//! no MCP servers and no saved session, under a scratch `HOME` that holds only a copy of the
//! operator's credentials file (decision 0032). The CLI then reads no user `CLAUDE.md`, memory,
//! settings or project files; the clean-session probe checks that from the model's side.
//!
//! Credentials (lifeos#1252): an OAuth refresh token is single-use, so a CLI that refreshed
//! inside the scratch copy would burn the token the operator's own sessions hold. The run is
//! refused unless the access token is valid for at least [`MIN_TOKEN_LIFE_MS`] more, so the CLI
//! never needs to refresh, and a scratch copy that changed anyway is reported loudly. This code
//! never calls a refresh or login endpoint.

use std::ffi::OsString;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use s2w_system2::{ExecLimits, ExecProvider, MAX_ATTEMPTS, ReplyFormat};

use super::ledger::CAP_USD;
use super::prices::ESTIMATED_OUTPUT_TOKENS;

/// One call's wall-clock limit: a mapping reply may run to 16k output tokens.
const CALL_TIMEOUT: Duration = Duration::from_secs(900);

/// The most calls a run makes: the probe, then a first call and a repair per attempt.
const MAX_CALLS: u64 = 1 + 2 * MAX_ATTEMPTS as u64;

/// How long the access token must stay valid when the run starts: every call at its timeout,
/// plus 15 minutes.
pub(crate) const MIN_TOKEN_LIFE_MS: u64 = (MAX_CALLS * CALL_TIMEOUT.as_secs() + 15 * 60) * 1000;

/// Where the CLI reads its credentials, under `HOME`.
const CREDENTIALS: &str = ".claude/.credentials.json";

/// Sessions opened by this process: parallel callers share the pid and can share a clock tick,
/// so the counter is what keeps their scratch `HOME`s apart.
static OPENED: AtomicU64 = AtomicU64::new(0);

/// A scratch `HOME`, removed when dropped.
pub(crate) struct Session {
    home: PathBuf,
    copied: Vec<u8>,
}

impl Session {
    /// A scratch `HOME` holding a copy of `credentials`, after checking its access token stays
    /// valid for [`MIN_TOKEN_LIFE_MS`] past `now_ms`.
    ///
    /// # Errors
    ///
    /// The credentials cannot be read, carry no `claudeAiOauth.expiresAt`, expire too soon, or
    /// the scratch directory cannot be written.
    pub(crate) fn open(credentials: &Path, now_ms: u64) -> Result<Self, String> {
        let shown = credentials.display();
        let copied = fs::read(credentials).map_err(|e| format!("{shown}: {e}"))?;
        let parsed: serde_json::Value =
            serde_json::from_slice(&copied).map_err(|e| format!("{shown}: {e}"))?;
        let expires = parsed["claudeAiOauth"]["expiresAt"]
            .as_u64()
            .ok_or_else(|| {
                format!(
                    "{shown}: no claudeAiOauth.expiresAt; the run needs the CLI's OAuth credentials"
                )
            })?;
        if expires < now_ms.saturating_add(MIN_TOKEN_LIFE_MS) {
            return Err(format!(
                "{shown}: the access token expires in {} min, under the {} min a run needs; the CLI would refresh it inside the scratch copy and burn the single-use refresh token (lifeos#1252). Let a normal session refresh it, then run again",
                expires.saturating_sub(now_ms) / 60_000,
                MIN_TOKEN_LIFE_MS / 60_000
            ));
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let seq = OPENED.fetch_add(1, Ordering::Relaxed);
        let home =
            std::env::temp_dir().join(format!("s2w-gate3-{}-{nanos}-{seq}", std::process::id()));
        let session = Self { home, copied };
        let dir = session.home.join(".claude");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        let file = session.home.join(CREDENTIALS);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .and_then(|mut f| f.write_all(&session.copied))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        Ok(session)
    }

    /// The scratch `HOME`.
    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    /// The provider: `claude` run with [`argv`], its environment exactly this `HOME`, the
    /// caller's `PATH` (the CLI needs its runtime) and the CLI's output limit at
    /// [`ESTIMATED_OUTPUT_TOKENS`] (so no reply costs more than the gate estimated), reading the
    /// CLI's JSON envelope.
    ///
    /// # Errors
    ///
    /// `PATH` is unset, or the provider refuses the command.
    pub(crate) fn provider(&self, claude: &Path, model: &str) -> Result<ExecProvider, String> {
        let path = std::env::var_os("PATH").ok_or("PATH is unset")?;
        let env = vec![
            ("HOME".to_owned(), OsString::from(self.home.as_os_str())),
            ("PATH".to_owned(), path),
            (
                "CLAUDE_CODE_MAX_OUTPUT_TOKENS".to_owned(),
                OsString::from(ESTIMATED_OUTPUT_TOKENS.to_string()),
            ),
        ];
        let limits = ExecLimits {
            timeout: CALL_TIMEOUT,
            ..ExecLimits::default()
        };
        Ok(ExecProvider::new(argv(claude, model), env)
            .map_err(|e| format!("claude provider: {e}"))?
            .with_format(ReplyFormat::ClaudeJson)
            .with_limits(limits))
    }

    /// Whether the CLI rewrote the scratch credentials during the run.
    pub(crate) fn credentials_changed(&self) -> bool {
        fs::read(self.home.join(CREDENTIALS)).map_or(true, |now| now != self.copied)
    }

    /// Copies the scratch credentials to a new file beside `credentials` before the scratch
    /// `HOME` is removed: after a refresh it holds the only refresh token that still works.
    ///
    /// # Errors
    ///
    /// The scratch copy cannot be read or the new file cannot be written.
    pub(crate) fn keep_credentials(&self, credentials: &Path) -> Result<PathBuf, String> {
        let from = self.home.join(CREDENTIALS);
        let bytes = fs::read(&from).map_err(|e| format!("{}: {e}", from.display()))?;
        let name = self.home.file_name().map_or_else(
            || OsString::from("s2w-gate3"),
            std::ffi::OsStr::to_os_string,
        );
        let mut kept = credentials.as_os_str().to_os_string();
        kept.push(".");
        kept.push(name);
        let kept = PathBuf::from(kept);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&kept)
            .and_then(|mut f| f.write_all(&bytes))
            .map_err(|e| format!("{}: {e}", kept.display()))?;
        Ok(kept)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

/// The CLI's arguments: one headless reply, no tools, no MCP servers, no saved session, the
/// JSON envelope, and the CLI's own spend stop at the cap (the gate holds the run to it).
pub(crate) fn argv(claude: &Path, model: &str) -> Vec<String> {
    let cap = format!("{CAP_USD}");
    let rest = [
        "-p",
        "--model",
        model,
        "--tools",
        "",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--output-format",
        "json",
        "--max-budget-usd",
        &cap,
    ];
    std::iter::once(claude.to_string_lossy().into_owned())
        .chain(rest.map(str::to_owned))
        .collect()
}

/// `claude` as given, or found on `PATH`, as an absolute path to a file.
///
/// # Errors
///
/// It is not found, or not a file.
pub(crate) fn claude(given: Option<&str>) -> Result<PathBuf, String> {
    let found = match given {
        Some(path) => PathBuf::from(path),
        None => std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths)
            .map(|dir| dir.join("claude"))
            .find(|candidate| candidate.is_file())
            .ok_or("no `claude` on PATH; pass --claude PATH")?,
    };
    // Absolute, but not resolved: the CLI may be a symlink its launcher relies on.
    let absolute = std::path::absolute(&found).map_err(|e| format!("{}: {e}", found.display()))?;
    if !absolute.is_file() {
        return Err(format!("{} is not a file", absolute.display()));
    }
    Ok(absolute)
}
