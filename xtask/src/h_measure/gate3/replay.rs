//! `score`'s check of a committed replicate: the transcript is the one committed, the input is
//! what this build derives from the window, and replaying the probe and the transcript through
//! the same code and budget gate gives the recorded result, attempts and spend (for B3, also
//! the recorded fits). An edit to the file, the transcript, the prompts, the profiler or the
//! sample rule refuses.

use std::fs;
use std::path::{Path, PathBuf};

use s2w_discover::Profile;
use s2w_model::MappingInput;
use s2w_system2::{CallRecord, MappingProposer, ReplayProvider, mapping_prompt};
use serde::Serialize;
use serde_json::Value;

use super::super::freeze::Frozen;
use super::super::pins::sha256;
use super::b3;
use super::committed::{
    B3, Budget, Committed, FORMAT, Fit, H_S2, KIND, PROVIDER, Ran, SAMPLE_EVENTS,
    SAMPLE_STRING_CHARS, input, input_hash, propose_h_s2, run, split,
};

/// The transcript beside committed file `file`: `<file minus .json>.transcript.json`.
///
/// # Errors
///
/// `file` does not end in `.json`.
pub(crate) fn transcript_path(file: &Path) -> Result<PathBuf, String> {
    let text = file.to_string_lossy();
    let stem = text
        .strip_suffix(".json")
        .ok_or_else(|| format!("{text}: a committed file's name ends in .json"))?;
    Ok(PathBuf::from(format!("{stem}.transcript.json")))
}

/// Refuses a committed file this build would not have written: another kind, format, arm,
/// provider or sample size, or a budget on an h-s2 file or none on a b3 file.
fn admitted(file: &Path, committed: &Committed) -> Result<(), String> {
    let shown = file.display();
    let constants = (
        committed.kind.as_str(),
        committed.format,
        committed.provider.as_str(),
        committed.sample_events,
        committed.sample_string_chars,
    );
    if constants != (KIND, FORMAT, PROVIDER, SAMPLE_EVENTS, SAMPLE_STRING_CHARS) {
        return Err(format!(
            "{shown}: kind, format, provider or sample size {constants:?} is not what this build commits"
        ));
    }
    match (committed.arm.as_str(), committed.budget.is_some()) {
        (H_S2, false) | (B3, true) => Ok(()),
        (arm, budget) => Err(format!(
            "{shown}: arm {arm:?} with{} a budget is not what this build commits (h-s2 has none, b3 has one)",
            if budget { "" } else { "out" }
        )),
    }
}

/// Refuses `committed` (read from `file`) unless it is what a run of this build wrote, given
/// the profile and window events `score` re-derived for its heuristic. Returns its
/// transcript's calls, every one of which the replay asked for, in order.
///
/// # Errors
///
/// Which part does not reproduce.
pub(crate) fn reproduce(
    file: &Path,
    committed: &Committed,
    profile: &Profile,
    window: &[Value],
) -> Result<Vec<CallRecord>, String> {
    let shown = file.display();
    admitted(file, committed)?;
    let path = transcript_path(file)?;
    let transcript = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if sha256(&transcript) != committed.transcript_sha256 {
        return Err(format!(
            "{}: its sha256 is not the transcript_sha256 {shown} records",
            path.display()
        ));
    }
    let transcript =
        String::from_utf8(transcript).map_err(|e| format!("{}: {e}", path.display()))?;
    let input = input(&committed.heuristic, profile, window, committed.replicate)?;
    let probe = ReplayProvider::from_calls(std::slice::from_ref(&committed.probe))
        .map_err(|e| format!("{shown}: probe: {e}"))?;
    let proposer = MappingProposer::new(
        ReplayProvider::from_json(&transcript).map_err(|e| format!("{}: {e}", path.display()))?,
    );
    let (ran, hash) = match &committed.budget {
        None => {
            let hash = input_hash(&input, &committed.prompt_files_hash)?;
            let (ran, ()) = run(&probe, &committed.price, |prior| {
                propose_h_s2(&proposer, &committed.price, &input, prior)
            })
            .map_err(|e| format!("{shown}: replaying the probe: {e}"))?;
            (ran, hash)
        }
        Some(budget) => replay_b3(
            file,
            committed,
            budget,
            (&input, window),
            (&probe, &proposer),
        )?,
    };
    if hash != committed.input_hash {
        return Err(format!(
            "{shown}: input_hash {} is not the {hash} this build derives from the window: the profiler, the sample rule or the file changed since the run",
            committed.input_hash
        ));
    }
    replayed(file, committed, &ran)?;
    let calls = transcript_calls(&transcript).map_err(|e| format!("{}: {e}", path.display()))?;
    let recorded: Vec<&str> = calls.iter().map(|c| c.prompt_hash.as_str()).collect();
    if proposer.provider().calls() != recorded {
        return Err(format!(
            "{}: replaying it asks {} calls, not the {} it holds: the file or the transcript was edited",
            path.display(),
            proposer.provider().calls().len(),
            recorded.len()
        ));
    }
    Ok(calls)
}

/// The calls a transcript (recording format 2) holds, in order.
fn transcript_calls(text: &str) -> Result<Vec<CallRecord>, String> {
    #[derive(serde::Deserialize)]
    struct Calls {
        calls: Vec<CallRecord>,
    }
    serde_json::from_str::<Calls>(text)
        .map(|c| c.calls)
        .map_err(|e| e.to_string())
}

/// Replays a b3 file's fits within `budget` from the h-s2 `input` its budget was built from:
/// the run, and the input hash it gives.
fn replay_b3(
    file: &Path,
    committed: &Committed,
    budget: &Budget,
    (input, window): (&MappingInput, &[Value]),
    (probe, proposer): (&ReplayProvider, &MappingProposer<ReplayProvider>),
) -> Result<(Ran, String), String> {
    let shown = file.display();
    let prompt_bytes = mapping_prompt(input)
        .map_err(|e| format!("input: {e}"))?
        .len();
    if prompt_bytes != budget.prompt_bytes {
        return Err(format!(
            "{shown}: budget prompt_bytes {} is not the {prompt_bytes} of the h-s2 prompt this build derives: the profiler, the sample rule or the file changed since the run",
            budget.prompt_bytes
        ));
    }
    let sized = b3::Sized {
        h_s2_sha256: budget.h_s2_sha256.clone(),
        input_tokens: budget.input_tokens,
        prompt_bytes,
        raw: b3::raw_events(window)?,
    };
    let target = sized.target(input);
    let (ran, fitted) = run(probe, &committed.price, |prior| {
        b3::propose(proposer, &committed.price, &sized.raw, &target, prior)
    })
    .map_err(|e| format!("{shown}: replaying the probe: {e}"))?;
    if fitted.fits != budget.fits {
        return Err(format!(
            "{shown}: replaying its transcript fits {:?}, not the recorded fits: the file or the transcript was edited",
            fitted.fits
        ));
    }
    let hash = b3::input_hash(input, &fitted.last, &committed.prompt_files_hash)?;
    Ok((ran, hash))
}

/// Refuses a replay whose prompts, result, attempts or spend are not the recorded ones.
fn replayed(file: &Path, committed: &Committed, ran: &Ran) -> Result<(), String> {
    let shown = file.display();
    if ran.outcome.prompt_files_hash != committed.prompt_files_hash {
        return Err(format!(
            "{shown}: prompt_files_hash {} is not this build's {}: score with the build that ran it",
            committed.prompt_files_hash, ran.outcome.prompt_files_hash
        ));
    }
    let (mapping, failure) = split(&ran.outcome.result);
    if (&mapping, &failure, ran.outcome.attempts)
        != (&committed.mapping, &committed.failure, committed.attempts)
    {
        return Err(format!(
            "{shown}: replaying its transcript gives {} after {} attempts, not the recorded result: the file or the transcript was edited",
            failure.as_deref().unwrap_or("a mapping"),
            ran.outcome.attempts
        ));
    }
    if ran.spend != committed.spend {
        return Err(format!(
            "{shown}: replaying it spends {:?}, not the recorded spend {:?}",
            ran.spend, committed.spend
        ));
    }
    Ok(())
}

/// What `score --mapping` reads: a frozen mapping, or a committed gate-3 replicate, which
/// carries the H it started from as `heuristic`, checked exactly as a frozen mapping is.
pub(crate) enum Scored {
    Frozen(Box<Frozen>),
    Committed(Box<Committed>),
}

impl Scored {
    pub(crate) fn read(bytes: &[u8], path: &Path) -> Result<Self, String> {
        let shown = |e: serde_json::Error| format!("{}: {e}", path.display());
        let parsed: Value = serde_json::from_slice(bytes).map_err(shown)?;
        Ok(
            if parsed.get("kind").and_then(Value::as_str) == Some(KIND) {
                Self::Committed(serde_json::from_value(parsed).map_err(shown)?)
            } else {
                Self::Frozen(serde_json::from_value(parsed).map_err(shown)?)
            },
        )
    }

    pub(crate) fn frozen(&self) -> &Frozen {
        match self {
            Self::Frozen(frozen) => frozen,
            Self::Committed(committed) => &committed.heuristic,
        }
    }

    pub(crate) fn committed(&self) -> Option<&Committed> {
        match self {
            Self::Frozen(_) => None,
            Self::Committed(committed) => Some(committed),
        }
    }
}

/// What the score report says about a committed replicate.
#[derive(Serialize)]
pub(crate) struct System2<'a> {
    arm: &'a str,
    replicate: u32,
    model: &'a str,
    failure: Option<&'a str>,
    usd: f64,
    /// B3 only: every sample it sent, the last one's proposal graded.
    #[serde(skip_serializing_if = "Option::is_none")]
    fits: Option<&'a [Fit]>,
}

impl<'a> System2<'a> {
    pub(crate) fn of(committed: &'a Committed) -> Self {
        Self {
            arm: &committed.arm,
            replicate: committed.replicate,
            model: &committed.model,
            failure: committed.failure.as_deref(),
            usd: committed.spend.usd,
            fits: committed.budget.as_ref().map(|b| b.fits.as_slice()),
        }
    }

    /// The report's paragraph on it.
    pub(crate) fn markdown(&self) -> String {
        let result = self.failure.map_or_else(
            || "proposed a mapping".to_owned(),
            |f| format!("failed ({f}; graded as the empty prediction)"),
        );
        let start = match self.fits.and_then(<[Fit]>::last) {
            Some(fit) => format!(
                "saw a raw sample of the window (every k-th event from the first, k = {}, {} events; fits tried: {}) and never the heuristic above, and",
                fit.k,
                fit.events,
                self.fits.map_or(0, <[Fit]>::len)
            ),
            None => "started from the heuristic above and".to_owned(),
        };
        format!(
            "System 2 (arm {}, replicate {}, model {}) {start} {result}, ${:.4} by the price table; its output is what is graded below.\n\n",
            self.arm, self.replicate, self.model, self.usd
        )
    }
}
