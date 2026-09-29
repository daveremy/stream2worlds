//! The scale baseline (`xtask/scale-baseline.toml`) and its judges (s2w#32, decision 0004).
//!
//! The judges are pure; [`read`], [`growth`] (which reads `origin/main` through `git`) and
//! the runners do I/O. The runners live in `scale_run.rs` (`cargo xtask scale`, fold instructions per event under
//! Valgrind) and `scale_mem_check.rs` (the `check` hook, heap bytes per entity). A measurement
//! that cannot be read is a failure, never a pass.
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::module_size::{git, trailer};

/// The baseline file, relative to the workspace root.
pub(super) const BASELINE: &str = "xtask/scale-baseline.toml";

/// `xtask/scale-baseline.toml`. Every field is required: a renamed or missing key is a parse
/// failure, never a silent default.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Baseline {
    pub(super) tolerance_percent: u64,
    pub(super) set_by: String,
    pub(super) ir: IrBaseline,
    pub(super) memory: MemoryBaseline,
}

/// `[ir]`: fold instructions per event, owned by the CI image that measured it.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct IrBaseline {
    pub(super) fold_ir_per_event: u64,
    pub(super) ci_image: String,
    pub(super) events: u64,
    pub(super) rustc: String,
    pub(super) profile: String,
}

/// `[memory]`: heap bytes per entity after the fold, allocator-counted.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct MemoryBaseline {
    pub(super) bytes_per_entity: u64,
    pub(super) target_bytes_per_entity: u64,
    pub(super) budget_bytes_per_entity: u64,
    pub(super) bytes_per_relationship_reported: u64,
    pub(super) entities: u64,
}

/// The memory test's JSON line.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct MemMeasurement {
    pub(super) bytes_per_entity: u64,
    pub(super) bytes_per_relationship: u64,
    pub(super) entities: u64,
    pub(super) relationships: u64,
}

/// Parses and sanity-checks the baseline text.
pub(super) fn parse(text: &str) -> Result<Baseline, String> {
    let b: Baseline = toml::from_str(text).map_err(|e| format!("{BASELINE}: {e}"))?;
    if b.tolerance_percent == 0 || b.tolerance_percent > 50 {
        return Err(format!(
            "{BASELINE}: tolerance_percent {} is outside 1..=50; research 0006 §12 sets 5",
            b.tolerance_percent
        ));
    }
    if b.ir.profile != "bench" || b.ir.events == 0 || b.memory.entities == 0 {
        return Err(format!(
            "{BASELINE}: [ir] profile must be \"bench\" and [ir] events / [memory] entities must be > 0; fix the file"
        ));
    }
    Ok(b)
}

/// Reads and parses the baseline under `root`.
pub(super) fn read(root: &Path) -> Result<Baseline, String> {
    let path = root.join(BASELINE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text)
}

/// The last stdout line that parses as JSON, deserialized as `T`. `None` in the error means no
/// line parsed at all (the test or bench did not run, or printed nothing).
pub(super) fn last_json_line<T: DeserializeOwned>(stdout: &str) -> Result<T, String> {
    let value = stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
        .ok_or("no JSON line in its output (did the test or bench actually run?)")?;
    serde_json::from_value(value.clone())
        .map_err(|e| format!("its last JSON line {value} does not have the expected fields: {e}"))
}

/// Total Callgrind `Ir` from a gungraun `summary.json`.
pub(super) fn summary_ir(json: &str) -> Result<u64, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("gungraun summary is not JSON: {e}"))?;
    v.get("profiles")
        .and_then(serde_json::Value::as_array)
        .and_then(|ps| {
            ps.iter()
                .find(|p| p.get("tool").and_then(serde_json::Value::as_str) == Some("Callgrind"))
        })
        .and_then(|p| p.pointer("/data/total/metrics/Ir/values/new"))
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            "gungraun summary has no Callgrind profile with an integer total Ir at data.total.metrics.Ir.values.new; the summary format changed, update scale::summary_ir".to_owned()
        })
}

/// Signed percent change from `base` to `measured`.
fn percent(measured: f64, base: f64) -> f64 {
    (measured - base) / base * 100.0
}

/// Judges the fold's total instructions against `[ir]`. `Ok` is the report line.
pub(super) fn judge_ir(b: &Baseline, total_ir: u64) -> Result<String, String> {
    if total_ir == 0 {
        return Err("fold Ir: UNKNOWN (the benchmark reported 0 instructions); fix the bench run, an unknown never passes".into());
    }
    let per_event = total_ir as f64 / b.ir.events as f64;
    let set_to = per_event.ceil();
    let base = b.ir.fold_ir_per_event;
    if base == 0 {
        return Err(format!(
            "fold Ir: baseline unset: measured {set_to} Ir/event ({total_ir} Ir over {} events); set [ir] fold_ir_per_event = {set_to} and [ir] rustc = the `rustc -V` line in {BASELINE} from the CI job 'scale' (image {}), with a Baseline-growth: s2w#<N> trailer",
            b.ir.events, b.ir.ci_image
        ));
    }
    let base_f = base as f64;
    let change = percent(per_event, base_f);
    let tol = b.tolerance_percent as f64;
    if change > tol {
        return Err(format!(
            "fold Ir regressed {change:+.1}%: measured {per_event:.0} Ir/event vs baseline {base} (tolerance {tol}%); make the fold cheaper, or if the cost is intended raise [ir] fold_ir_per_event to {set_to} in {BASELINE}, say why, and add a Baseline-growth: s2w#<N> trailer"
        ));
    }
    // Unlike [memory], nothing lowers [ir] automatically: it belongs to the CI image, so a local
    // improvement is only a hint.
    let mut line = format!(
        "fold Ir: {per_event:.0} Ir/event vs baseline {base} ({change:+.1}%, tolerance {tol}%)"
    );
    if change < -tol {
        line.push_str(&format!(
            "; improved past tolerance, lower [ir] fold_ir_per_event to {set_to} to lock it in"
        ));
    }
    Ok(line)
}

/// Judges the memory test's measurement against `[memory]`: report lines, then problems.
pub(super) fn judge_memory(b: &Baseline, m: &MemMeasurement) -> (Vec<String>, Vec<String>) {
    let mem = &b.memory;
    let mut report = Vec::new();
    let mut problems = Vec::new();
    if m.bytes_per_entity == 0 || m.entities == 0 {
        problems.push("bytes/entity: UNKNOWN (the memory test measured 0); fix the test, an unknown never passes".into());
        return (report, problems);
    }
    if m.entities != mem.entities {
        problems.push(format!(
            "bytes/entity: the memory test folded {} entities but [memory] entities = {}; the generator changed, so re-measure and update [memory] in {BASELINE}",
            m.entities, mem.entities
        ));
    }
    let (measured, target) = (
        m.bytes_per_entity as f64,
        mem.target_bytes_per_entity as f64,
    );
    let ratio = measured / target.max(1.0);
    report.push(format!(
        "bytes/entity: {} B = {ratio:.2}x decision 0004's {} B target{}; bytes/relationship {} B over {} relationships (reported, baseline {})",
        m.bytes_per_entity,
        mem.target_bytes_per_entity,
        if ratio > 2.0 { ", beyond its 2x amendment line: interim ceiling only, overshoot tracked in s2w#172" } else { "" },
        m.bytes_per_relationship,
        m.relationships,
        mem.bytes_per_relationship_reported
    ));
    if m.bytes_per_entity > mem.budget_bytes_per_entity {
        problems.push(format!(
            "bytes/entity {} B exceeds the hard budget {} B ([memory] budget_bytes_per_entity); shrink the world's per-entity footprint, or get a decision 0004 amendment before raising the budget with a Baseline-growth: s2w#<N> trailer",
            m.bytes_per_entity, mem.budget_bytes_per_entity
        ));
    }
    judge_memory_baseline(b, m, &mut report, &mut problems);
    (report, problems)
}

fn judge_memory_baseline(
    b: &Baseline,
    m: &MemMeasurement,
    report: &mut Vec<String>,
    problems: &mut Vec<String>,
) {
    let base = b.memory.bytes_per_entity;
    if base == 0 {
        problems.push(format!(
            "bytes/entity: baseline unset: measured {} B; set [memory] bytes_per_entity = {} in {BASELINE} with a Baseline-growth: s2w#<N> trailer",
            m.bytes_per_entity, m.bytes_per_entity
        ));
        return;
    }
    let (change, tol) = (
        percent(m.bytes_per_entity as f64, base as f64),
        b.tolerance_percent as f64,
    );
    if change > tol {
        problems.push(format!(
            "bytes/entity regressed {change:+.1}%: measured {} B vs baseline {base} B (tolerance {tol}%); shrink the per-entity footprint, or if intended raise [memory] bytes_per_entity in {BASELINE}, say why, and add a Baseline-growth: s2w#<N> trailer",
            m.bytes_per_entity
        ));
    } else if change < -tol {
        report.push(format!(
            "bytes/entity improved {change:+.1}% past tolerance; run cargo xtask check --tighten-baseline to lower [memory] bytes_per_entity"
        ));
    }
}

/// Guarded keys that are higher than on the base (or new, when there is no base). The two
/// measurement sizes count too: raising either lowers the measured per-unit figure.
pub(super) fn grown_keys(base: Option<&Baseline>, current: &Baseline) -> Vec<&'static str> {
    let keys = |b: &Baseline| {
        [
            ("[ir] fold_ir_per_event", b.ir.fold_ir_per_event),
            ("[memory] bytes_per_entity", b.memory.bytes_per_entity),
            (
                "[memory] target_bytes_per_entity",
                b.memory.target_bytes_per_entity,
            ),
            (
                "[memory] budget_bytes_per_entity",
                b.memory.budget_bytes_per_entity,
            ),
            ("tolerance_percent", b.tolerance_percent),
            // Measurement sizes: a larger run amortises fixed cost, which lowers the measured
            // per-event and per-entity figures without any code change.
            ("[ir] events", b.ir.events),
            ("[memory] entities", b.memory.entities),
        ]
    };
    let before = base.map(keys);
    keys(current)
        .iter()
        .enumerate()
        .filter(|(i, (_, now))| before.is_none_or(|b| b[*i].1 < *now))
        .map(|(_, (name, _))| *name)
        .collect()
}

/// The baseline-growth check: growth against `origin/main` needs a `Baseline-growth: s2w#<N>`
/// trailer in `origin/main..HEAD`. A file absent on the base counts as all growth.
pub(super) fn growth(root: &Path, current: &Baseline) -> Vec<String> {
    let base = git(root, &["show", &format!("origin/main:{BASELINE}")])
        .and_then(|text| parse(&text))
        .map_err(|e| println!("[scale baseline] no base on origin/main ({e}); every guarded value counts as growth"))
        .ok();
    let grown = grown_keys(base.as_ref(), current);
    if grown.is_empty() {
        return Vec::new();
    }
    match git(root, &["log", "origin/main..HEAD", "--pretty=%B"]) {
        Ok(messages) if trailer(&messages) => Vec::new(),
        result => vec![format!(
            "scale baseline grew ({}): add a Baseline-growth: s2w#<N> trailer to a commit in origin/main..HEAD, or undo the growth in {BASELINE}{}",
            grown.join(", "),
            result
                .err()
                .map(|e| format!("; cannot read commit range: {e}"))
                .unwrap_or_default()
        )],
    }
}

/// `--tighten-baseline` for `[memory]`: lowers `bytes_per_entity` and
/// `bytes_per_relationship_reported` to the measurement, never raises either, and keeps every
/// comment. `None` when nothing is lower.
pub(super) fn tighten_text(text: &str, m: &MemMeasurement) -> Option<String> {
    let mut section = String::new();
    let mut changed = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed.to_owned();
        }
        let lowered = (section == "[memory]")
            .then(|| lower_line(trimmed, m))
            .flatten();
        changed |= lowered.is_some();
        out.push(lowered.unwrap_or_else(|| line.to_owned()));
    }
    changed.then(|| out.join("\n") + "\n")
}

fn lower_line(line: &str, m: &MemMeasurement) -> Option<String> {
    let (key, rest) = line.split_once('=')?;
    let key = key.trim();
    let measured = match key {
        "bytes_per_entity" => m.bytes_per_entity,
        "bytes_per_relationship_reported" => m.bytes_per_relationship,
        _ => return None,
    };
    let (value, comment) = rest
        .split_once('#')
        .map_or((rest, None), |(v, c)| (v, Some(c)));
    let current: u64 = value.trim().parse().ok()?;
    (measured > 0 && measured < current).then(|| match comment {
        Some(c) => format!("{key} = {measured} #{c}"),
        None => format!("{key} = {measured}"),
    })
}

#[cfg(test)]
mod tests;
