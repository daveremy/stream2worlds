//! `cargo xtask gate3 commit`: one replicate of the "H plus System 2" arm of gate 3 (s2w#373,
//! decision 0032). It derives H exactly as `h-measure freeze` does, proves the model session
//! clean with the probe, asks the model for a mapping under a $5 budget gate, and writes the
//! committed file and its transcript, both new. The operator commits both before `score`
//! accepts them: the git log is the order proof, as for a frozen mapping.
//!
//! Tests never call a model: they run a fake `claude` that prints recorded envelopes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use s2w_system2::{MappingProposer, recording_json};

use super::freeze::derived;
use super::pins::{DATA, Pins, sha256};
use super::{Flags, corpus_dir, flags, one};

pub(crate) mod committed;
mod ledger;
mod prices;
pub(crate) mod replay;
mod session;

use committed::{
    ARM, Committed, FORMAT, KIND, PROVIDER, SAMPLE_EVENTS, SAMPLE_STRING_CHARS, input, input_hash,
    run as run_arm, split,
};

/// The command's usage line.
pub(crate) const USAGE: &str = "cargo xtask gate3 commit --corpus NAME --window N --replicate K --model SNAPSHOT [--arm h-s2] [--out FILE] [--dir DIR] [--claude PATH] [--credentials PATH]";

/// Where committed replicates go by default, under the measurement's data directory.
const COMMITTED: &str = "committed";

/// Runs `cargo xtask gate3 <args>`.
pub(crate) fn run(root: &Path, args: &[String]) -> ExitCode {
    let result = match args.split_first() {
        Some((verb, rest)) if verb == "commit" => flags(rest).and_then(|f| commit(root, &f)),
        _ => {
            eprintln!("usage: {USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("✗ gate3: {problem}");
            ExitCode::FAILURE
        }
    }
}

/// A parsed number flag.
fn number<T: std::str::FromStr>(flags: &Flags, name: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    let raw = one(flags, name)?;
    raw.parse().map_err(|e| format!("--{name} {raw:?}: {e}"))
}

/// An optional flag's one value.
fn optional<'a>(flags: &'a Flags, name: &str) -> Result<Option<&'a str>, String> {
    flags.get(name).map(|_| one(flags, name)).transpose()
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Writes `text` to a new file `path`.
fn write_new(path: &Path, text: &str) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    std::io::Write::write_all(&mut file, text.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// What `gate3 commit` was asked to do.
struct Request<'a> {
    corpus: &'a str,
    window: usize,
    replicate: u32,
    model: &'a str,
    out: PathBuf,
    transcript: PathBuf,
    credentials: PathBuf,
    claude: PathBuf,
    dir: PathBuf,
}

/// The request in `flags`, refused before any call when an output file exists.
fn request<'a>(root: &Path, flags: &'a Flags) -> Result<Request<'a>, String> {
    const KNOWN: [&str; 9] = [
        "corpus",
        "window",
        "replicate",
        "model",
        "arm",
        "out",
        "dir",
        "claude",
        "credentials",
    ];
    if let Some(name) = flags.keys().find(|n| !KNOWN.contains(&n.as_str())) {
        return Err(format!("commit takes no --{name}; usage: {USAGE}"));
    }
    match optional(flags, "arm")? {
        None | Some(ARM) => {}
        Some("b3") => return Err("--arm b3: the B3 arm ships in PR 3 of s2w#373".to_owned()),
        Some(other) => return Err(format!("--arm {other:?}: the arms are h-s2 and b3")),
    }
    let corpus = one(flags, "corpus")?;
    let replicate: u32 = number(flags, "replicate")?;
    let out = optional(flags, "out")?.map_or_else(
        || {
            root.join(DATA)
                .join(COMMITTED)
                .join(format!("{ARM}.{corpus}.r{replicate}.json"))
        },
        PathBuf::from,
    );
    let transcript = replay::transcript_path(&out)?;
    for path in [&out, &transcript] {
        if path.exists() {
            return Err(format!(
                "{} exists; a committed replicate is never overwritten, use a new --replicate",
                path.display()
            ));
        }
    }
    let credentials = match optional(flags, "credentials")? {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var("HOME").map_err(|e| format!("$HOME: {e}"))?)
            .join(".claude/.credentials.json"),
    };
    Ok(Request {
        corpus,
        window: number(flags, "window")?,
        replicate,
        model: one(flags, "model")?,
        out,
        transcript,
        credentials,
        claude: session::claude(optional(flags, "claude")?)?,
        dir: corpus_dir(flags)?,
    })
}

/// `gate3 commit`, from parsed flags.
pub(crate) fn commit(root: &Path, flags: &Flags) -> Result<String, String> {
    let request = request(root, flags)?;
    let pins = Pins::load(root)?;
    pins.verify_keys(root)?;
    let price = prices::load(root, request.model)?;
    let (heuristic, profile, events) =
        derived(&pins, &request.dir, request.corpus, request.window)?;
    let input = input(&heuristic, &profile, &events, request.replicate)?;
    let session = session::Session::open(&request.credentials, now_ms())?;
    let proposer = MappingProposer::new(session.provider(&request.claude, request.model)?);
    let ran = run_arm(proposer.provider(), &proposer, &price, &input);
    if session.credentials_changed() {
        let kept = session.keep_credentials(&request.credentials).map_or_else(
            |e| format!("it could not be kept ({e})"),
            |path| format!("it is kept at {}", path.display()),
        );
        eprintln!(
            "⚠ gate3: the CLI rewrote the credentials copy in {}: it refreshed the token, so {} may now hold a spent refresh token and the copy holds the live one; {kept}. Check the operator's sessions",
            session.home().display(),
            request.credentials.display()
        );
    }
    drop(session);
    let ran = ran?;
    let transcript = recording_json(&ran.outcome.calls).map_err(|e| e.to_string())? + "\n";
    let (mapping, failure) = split(&ran.outcome.result);
    let committed = Committed {
        kind: KIND.to_owned(),
        format: FORMAT,
        arm: ARM.to_owned(),
        replicate: request.replicate,
        heuristic,
        provider: PROVIDER.to_owned(),
        model: request.model.to_owned(),
        price,
        sample_events: SAMPLE_EVENTS,
        sample_string_chars: SAMPLE_STRING_CHARS,
        input_hash: input_hash(&input, &ran.outcome.prompt_files_hash)?,
        prompt_files_hash: ran.outcome.prompt_files_hash,
        attempts: ran.outcome.attempts,
        mapping,
        failure,
        probe: ran.probe,
        spend: ran.spend,
        transcript_sha256: sha256(transcript.as_bytes()),
    };
    write(&request, &committed, &transcript)
}

/// Writes the transcript, then the committed file; removes the transcript if the second write
/// fails, so a half-written replicate never stays behind.
fn write(request: &Request<'_>, committed: &Committed, transcript: &str) -> Result<String, String> {
    let text = serde_json::to_string_pretty(committed).map_err(|e| e.to_string())? + "\n";
    if let Some(parent) = request.out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    write_new(&request.transcript, transcript)?;
    if let Err(e) = write_new(&request.out, &text) {
        let _ = fs::remove_file(&request.transcript);
        return Err(e);
    }
    Ok(format!(
        "committed {} and {}: {}, {} attempts, {} calls, ${:.4} by the price table (CLI reported ${:.4}). Commit both files before scoring.",
        request.out.display(),
        request.transcript.display(),
        committed.failure.as_deref().unwrap_or("a mapping"),
        committed.attempts,
        committed.spend.calls,
        committed.spend.usd,
        committed.spend.cli_cost_usd
    ))
}

#[cfg(test)]
mod tests;
