//! `cargo xtask h-measure freeze`: runs `s2w-discover` on the first `window` events of the
//! development corpus and writes what it proposed, with every pin it was frozen under. It first
//! checks every pinned key and the corpus against their sha256 pins. A later `score` finds the
//! pins it uses unchanged and re-runs [`derive`] to prove the file is this command's output.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use s2w_discover::{Config, Discovery, PROFILER_VERSION, Profile, Role as PathRole, discover};
use s2w_model::StreamMapping;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::pins::{Pins, Role, sha256};

/// A frozen mapping: exactly one of `mapping` and `abstain` is set.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Frozen {
    /// The corpus it was discovered on (a `corpora.toml` name).
    pub corpus: String,
    pub corpus_sha256: String,
    /// How many of the corpus's first events the profiler read.
    pub window: usize,
    pub profiler_version: String,
    /// The profiler's thresholds (`Config::default()`), as its `Debug` text.
    pub config: String,
    /// Every pin in `keys.toml` and `corpora.toml` when it was frozen.
    pub pins: BTreeMap<String, String>,
    pub profile: Summary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<StreamMapping>,
    /// The profiler's reason, when it proposed no mapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abstain: Option<String>,
}

/// What `freeze` writes for `window` events of `corpus` under `pins`, after checking the corpus
/// is a development corpus matching its pin. `score` calls it again on a frozen file's recorded
/// inputs and refuses a file that differs, so only a real freeze's output scores (s2w#238).
pub(super) fn derive(
    pins: &Pins,
    dir: &Path,
    corpus: &str,
    window: usize,
) -> Result<Frozen, String> {
    let pin = pins.corpus(corpus)?;
    let window_events = development_window(pins, dir, corpus, window)?;
    let config = Config::default();
    let (profile, discovery) = profiled(&window_events, &config)?;
    let (mapping, abstain) = match discovery {
        Discovery::Mapping(m) => (Some(m), None),
        Discovery::Abstain(reason) => (None, Some(reason)),
    };
    Ok(Frozen {
        corpus: corpus.to_owned(),
        corpus_sha256: pin.sha256.clone(),
        window,
        profiler_version: PROFILER_VERSION.to_owned(),
        config: format!("{config:?}"),
        pins: pins.all(),
        profile: summary(&profile),
        mapping,
        abstain,
    })
}

/// What the profiler measured, reduced to what the report prints.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Summary {
    pub events: usize,
    pub skipped: usize,
    /// The event-type field's path id.
    pub event_type: Option<String>,
    /// Paths the profiler abstained on, by path id, with the abstaining role.
    pub abstained: BTreeMap<String, String>,
}

/// The first `window` events of `corpus`, after checking it is the pinned development corpus:
/// the only corpus the profiler may read, for a freeze or a profile.
fn development_window(
    pins: &Pins,
    dir: &Path,
    corpus: &str,
    window: usize,
) -> Result<Vec<Value>, String> {
    let pin = pins.corpus(corpus)?;
    if pin.role != Role::Development {
        return Err(format!(
            "{corpus} is a {:?} corpus; a mapping is frozen or profiled only on the development corpus",
            pin.role
        ));
    }
    let mut payloads = pins.payloads(dir, corpus)?;
    if window == 0 || window > payloads.len() {
        return Err(format!(
            "--window {window}: must be 1 to {n}, the {corpus} corpus has {n} events",
            n = payloads.len()
        ));
    }
    payloads.truncate(window);
    Ok(payloads)
}

/// `cargo xtask h-measure profile`: the profiler's per-path table (count, distinct values,
/// role) on the first `window` events of the development corpus, as markdown. The disclosed
/// development table a profiler rule is justified on (s2w#250); it never reads another corpus.
pub(crate) fn profile(
    root: &Path,
    dir: &Path,
    corpus: &str,
    window: usize,
) -> Result<String, String> {
    let pins = Pins::load(root)?;
    let pin = pins.corpus(corpus)?;
    let events = development_window(&pins, dir, corpus, window)?;
    let (profile, _) = profiled(&events, &Config::default())?;
    let mut out = format!(
        "# Profile: {corpus} (sha256 {}), first {window} events, profiler {PROFILER_VERSION}\n\n\
         events {}, skipped {}, decode {:?}, event-type field {}\n\n\
         | path | count | distinct | role |\n|---|---|---|---|\n",
        pin.sha256,
        profile.events,
        profile.skipped,
        profile
            .decode
            .iter()
            .map(s2w_discover::rule_id)
            .collect::<Vec<_>>(),
        profile
            .event_type
            .as_ref()
            .map_or_else(|| "none".to_owned(), s2w_discover::rule_id),
    );
    for p in &profile.paths {
        out += &format!(
            "| `{}` | {} | {} | {:?} |\n",
            s2w_discover::rule_id(&p.path),
            p.count,
            p.distinct,
            p.role
        );
    }
    Ok(out)
}

fn summary(profile: &Profile) -> Summary {
    let abstained = profile
        .paths
        .iter()
        .filter(|p| {
            matches!(
                p.role,
                PathRole::Sparse
                    | PathRole::GreyUniqueness
                    | PathRole::FewGroups
                    | PathRole::GreyDependency
            )
        })
        .map(|p| (s2w_discover::rule_id(&p.path), format!("{:?}", p.role)))
        .collect();
    Summary {
        events: profile.events,
        skipped: profile.skipped,
        event_type: profile.event_type.as_ref().map(s2w_discover::rule_id),
        abstained,
    }
}

/// Runs the profiler on stored envelopes, as `serve` hands it stored payloads.
fn profiled(payloads: &[Value], config: &Config) -> Result<(Profile, Discovery), String> {
    let bytes = payloads
        .iter()
        .map(serde_json::to_vec)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let refs: Vec<&[u8]> = bytes.iter().map(Vec::as_slice).collect();
    Ok(discover(&refs, config))
}

fn exists(out: &Path) -> String {
    format!(
        "{} exists; a frozen mapping is never overwritten, freeze to a new file",
        out.display()
    )
}

/// Writes `text` to `out`, refusing an `out` that appeared since `freeze` checked for it.
fn write_new(out: &Path, text: &str) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => exists(out),
            _ => format!("{}: {e}", out.display()),
        })?;
    std::io::Write::write_all(&mut file, text.as_bytes())
        .map_err(|e| format!("{}: {e}", out.display()))
}

/// Freezes a mapping discovered on `corpus` (read from `dir`) to `out`, which must not exist.
pub(crate) fn freeze(
    root: &Path,
    dir: &Path,
    corpus: &str,
    window: usize,
    out: &Path,
) -> Result<String, String> {
    if out.exists() {
        return Err(exists(out));
    }
    let pins = Pins::load(root)?;
    pins.verify_keys(root)?;
    let frozen = derive(&pins, dir, corpus, window)?;
    let text = serde_json::to_string_pretty(&frozen).map_err(|e| e.to_string())? + "\n";
    write_new(out, &text)?;
    let outcome = frozen.abstain.as_ref().map_or_else(
        || {
            format!(
                "{} entity rules",
                frozen.mapping.as_ref().map_or(0, |m| m.entities.len())
            )
        },
        |reason| format!("abstained: {reason}"),
    );
    Ok(format!(
        "froze {} (sha256 {}): profiler {PROFILER_VERSION} on the first {window} events of {corpus}, {outcome}",
        out.display(),
        sha256(text.as_bytes())
    ))
}
