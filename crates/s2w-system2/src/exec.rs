//! `ExecProvider`: runs a configured command with the prompt on stdin (decision 0029).
//!
//! The command is an argv array run with no shell. The prompt, which carries untrusted stream
//! text, goes on stdin only, never into argv or the environment. The environment is cleared
//! except for the names the operator passes; the working directory is a fresh empty directory,
//! removed afterwards. stdout and stderr are capped and the call has a time limit. That the
//! command has no tools is the operator's responsibility: s2w cannot verify it.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::provider::{Provider, ProviderError, Reply};

mod format;

pub use format::ReplyFormat;

/// How long the readers may take to finish once the command has exited or been killed. A
/// process the command started can hold its pipes open past that; its reader is abandoned.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// How often the exit status is polled.
const POLL: Duration = Duration::from_millis(20);

/// The limits on one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecLimits {
    /// Wall time before the command is killed.
    pub timeout: Duration,
    /// The stdout cap, in bytes.
    pub stdout_bytes: usize,
    /// The stderr cap, in bytes.
    pub stderr_bytes: usize,
}

impl Default for ExecLimits {
    /// 180 s, 1 MiB of stdout and 64 KiB of stderr.
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(180),
            stdout_bytes: 1 << 20,
            stderr_bytes: 64 << 10,
        }
    }
}

/// Why an [`ExecProvider`] could not be configured.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExecSetupError {
    /// The argv is empty.
    #[error("the command is empty")]
    EmptyCommand,
    /// A variable name is empty or holds `=` or a NUL.
    #[error("{0:?} is not a variable name")]
    BadName(String),
    /// A variable to pass through is not set here.
    #[error("{0} is not set")]
    Unset(String),
}

/// A provider that runs one configured command per call.
#[derive(Clone, Debug)]
pub struct ExecProvider {
    argv: Vec<String>,
    env: Vec<(String, OsString)>,
    limits: ExecLimits,
    format: ReplyFormat,
}

impl ExecProvider {
    /// Runs `argv` with exactly the variables in `env`.
    ///
    /// # Errors
    ///
    /// When `argv` is empty or a name is not a variable name.
    pub fn new(argv: Vec<String>, env: Vec<(String, OsString)>) -> Result<Self, ExecSetupError> {
        if argv.is_empty() {
            return Err(ExecSetupError::EmptyCommand);
        }
        if let Some((name, _)) = env.iter().find(|(name, _)| !valid_name(name)) {
            return Err(ExecSetupError::BadName(name.clone()));
        }
        Ok(Self {
            argv,
            env,
            limits: ExecLimits::default(),
            format: ReplyFormat::Text,
        })
    }

    /// Runs `argv` with the named variables taken from this process's environment.
    ///
    /// # Errors
    ///
    /// When `argv` is empty, a name is not a variable name, or a named variable is not set: a
    /// missing credential would otherwise read as a model failure.
    pub fn inherit(argv: Vec<String>, names: &[String]) -> Result<Self, ExecSetupError> {
        let mut env = Vec::with_capacity(names.len());
        for name in names {
            if !valid_name(name) {
                // Checked before `var_os`, which may panic on an empty name, `=` or NUL.
                return Err(ExecSetupError::BadName(name.clone()));
            }
            let value =
                std::env::var_os(name).ok_or_else(|| ExecSetupError::Unset(name.clone()))?;
            env.push((name.clone(), value));
        }
        Self::new(argv, env)
    }

    /// The same provider with other limits.
    #[must_use]
    pub const fn with_limits(mut self, limits: ExecLimits) -> Self {
        self.limits = limits;
        self
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::new(&self.argv[0]);
        command
            .args(&self.argv[1..])
            .env_clear()
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        command
    }
}

impl Provider for ExecProvider {
    fn complete(&self, prompt: &str) -> Result<Reply, ProviderError> {
        let cwd = EmptyDir::create().map_err(|e| ProviderError::Io(e.to_string()))?;
        let start = Instant::now();
        let mut child = self
            .command(cwd.path())
            .spawn()
            .map_err(|e| ProviderError::Spawn(e.to_string()))?;
        let pipes = match Pipes::start(&mut child, prompt, self.limits) {
            Ok(pipes) => pipes,
            Err(e) => {
                stop(&mut child);
                return Err(e);
            }
        };
        let status = wait(&mut child, start, self.limits.timeout);
        let latency_ms = millis(start.elapsed());
        let (stdout, stderr) = pipes.collect(child.id());
        let run = Run {
            status: status.map_err(|e| ProviderError::Io(e.to_string()))?,
            stdout,
            stderr,
            latency_ms,
        };
        run.classify(self.limits, self.format)
    }
}

/// Waits for `child` until `timeout` after `start`. `None` means it was killed at the limit.
fn wait(child: &mut Child, start: Instant, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {}
            Err(e) => {
                stop(child);
                return Err(e);
            }
        }
        if start.elapsed() >= timeout {
            stop(child);
            return Ok(None);
        }
        thread::sleep(POLL);
    }
}

/// Kills the command's process group and then the command, and reaps it. The group kill comes
/// first, while the unreaped leader still pins its id. The child may have exited between the
/// last poll and the kill; either way it is reaped.
fn stop(child: &mut Child) {
    kill_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

/// Kills the command's process group, so a process it started does not outlive it. Best
/// effort: it runs `/bin/kill`, and does nothing where that is absent.
fn kill_group(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// What one reader captured.
#[derive(Debug, Default)]
struct Captured {
    /// At most the cap.
    bytes: Vec<u8>,
    /// More than the cap was written.
    over: bool,
    /// The reader did not finish: something still holds the pipe open.
    open: bool,
}

/// The three pipe threads of one call.
struct Pipes {
    stdout: Receiver<Captured>,
    stderr: Receiver<Captured>,
}

impl Pipes {
    fn start(child: &mut Child, prompt: &str, limits: ExecLimits) -> Result<Self, ProviderError> {
        let missing = || ProviderError::Io("a pipe was not opened".to_owned());
        let mut stdin = child.stdin.take().ok_or_else(missing)?;
        let stdout = child.stdout.take().ok_or_else(missing)?;
        let stderr = child.stderr.take().ok_or_else(missing)?;
        let bytes = prompt.as_bytes().to_vec();
        // A command that exits without reading its input closes the pipe; that is not a
        // failure of the call, so the write error is dropped.
        thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
        Ok(Self {
            stdout: read_capped(stdout, limits.stdout_bytes),
            stderr: read_capped(stderr, limits.stderr_bytes),
        })
    }

    /// The captured output. When a reader is still blocked after the grace, the command's
    /// process group is killed and the readers get one more grace. The command has been reaped
    /// by now, so that kill is best effort: if every process left in the group has also left
    /// it (`setsid`), the id is free and could in principle name another group.
    fn collect(self, pid: u32) -> (Captured, Captured) {
        let deadline = Instant::now() + DRAIN_GRACE;
        let stdout = recv_by(&self.stdout, deadline);
        let stderr = recv_by(&self.stderr, deadline);
        if stdout.is_some() && stderr.is_some() {
            return (stdout.unwrap_or_default(), stderr.unwrap_or_default());
        }
        kill_group(pid);
        let deadline = Instant::now() + DRAIN_GRACE;
        let late = |rx: &Receiver<Captured>| {
            recv_by(rx, deadline).unwrap_or(Captured {
                open: true,
                ..Captured::default()
            })
        };
        let stdout = stdout.unwrap_or_else(|| late(&self.stdout));
        let stderr = stderr.unwrap_or_else(|| late(&self.stderr));
        (stdout, stderr)
    }
}

/// One reader's result, if it arrives before `deadline`.
fn recv_by(rx: &Receiver<Captured>, deadline: Instant) -> Option<Captured> {
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
}

/// Reads at most `cap` bytes from `pipe` on a thread, then drains the rest so the writer never
/// blocks on a full pipe.
fn read_capped<R: Read + Send + 'static>(mut pipe: R, cap: usize) -> Receiver<Captured> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
        let mut bytes = Vec::new();
        let read = (&mut pipe).take(limit).read_to_end(&mut bytes);
        let over = bytes.len() > cap;
        bytes.truncate(cap);
        if read.is_ok() {
            let _ = io::copy(&mut pipe, &mut io::sink());
        }
        let _ = tx.send(Captured {
            bytes,
            over,
            open: false,
        });
    });
    rx
}

/// One finished call, before it is classified.
struct Run {
    /// `None` when the command was killed at the time limit.
    status: Option<ExitStatus>,
    stdout: Captured,
    stderr: Captured,
    latency_ms: u64,
}

impl Run {
    fn classify(self, limits: ExecLimits, format: ReplyFormat) -> Result<Reply, ProviderError> {
        let latency_ms = self.latency_ms;
        let stdout_text = lossy(&self.stdout.bytes);
        let Some(status) = self.status else {
            return Err(ProviderError::Timeout {
                secs: limits.timeout.as_secs(),
                latency_ms,
                stdout: stdout_text,
            });
        };
        // A failed exit is the more useful diagnosis, so it is reported ahead of the cap.
        if !status.success() {
            return Err(ProviderError::Exit {
                status: status.to_string(),
                stderr: lossy(&self.stderr.bytes).unwrap_or_default(),
                latency_ms,
                stdout: stdout_text,
            });
        }
        if self.stdout.over {
            return Err(ProviderError::StdoutTooLarge {
                limit: limits.stdout_bytes,
                latency_ms,
                stdout: stdout_text.unwrap_or_default(),
            });
        }
        if self.stdout.open {
            return Err(ProviderError::StdoutHeld { latency_ms });
        }
        let text = String::from_utf8(self.stdout.bytes)
            .map_err(|_| ProviderError::NotUtf8 { latency_ms })?;
        format::reply(format, text, latency_ms)
    }
}

/// `bytes` as text for a row's `raw`, or `None` when there are none.
fn lossy(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(bytes).into_owned())
}

fn millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['=', '\0'])
}

/// A fresh, empty working directory, removed on drop.
struct EmptyDir(PathBuf);

impl EmptyDir {
    fn create() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let name = format!(
            "s2w-system2-{}-{}-{nanos}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        // `create_dir`, not `create_dir_all`: an existing directory is an error, never reused.
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for EmptyDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests;
