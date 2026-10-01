//! The scale baseline (`xtask/scale-baseline.toml`) and its judges (s2w#32, decision 0004).
//!
//! The judges are pure; [`read`], [`growth`] (which reads `origin/main` through `git`) and
//! the runners do I/O. The runners live in `scale_run.rs` (`cargo xtask scale`, fold and parse instructions per event under
//! Valgrind), `scale_mem_check.rs` (the `check` hook, heap bytes per entity) and
//! `discover_volume_run.rs` (`cargo xtask discover-volume`, the discovered-mapping world heap). A measurement
//! that cannot be read is a failure, never a pass.
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::module_size::{git, trailer};

pub(super) mod discover_volume;
mod ir_bench;
mod supply;
pub(super) mod tighten;
pub(super) use discover_volume::{DiscoverVolumeBaseline, VolumeMeasurement};
pub(super) use ir_bench::IrBench;
pub(super) use supply::{Supply, tighten_text};

/// The baseline file, relative to the workspace root.
pub(super) const BASELINE: &str = "xtask/scale-baseline.toml";
/// The recorded fixture the second supply replays (s2w#174), relative to the workspace root.
pub(super) const FIXTURE: &str = "crates/s2w-app/tests/fixtures/recorded-10min.raw.sse";

/// `xtask/scale-baseline.toml`. Every field is required: a renamed or missing key is a parse
/// failure, never a silent default.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Baseline {
    pub(super) tolerance_percent: u64,
    pub(super) set_by: String,
    pub(super) recorded: RecordedFixture,
    pub(super) ir: IrBaseline,
    pub(super) parse: ParseBaseline,
    pub(super) memory: MemoryBaseline,
    pub(super) discover_volume: DiscoverVolumeBaseline,
}

/// `[parse]`: System 1 parse instructions per raw event of the recorded fixture (s2w#166), same
/// CI image, rustc and profile as `[ir]`.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ParseBaseline {
    pub(super) parse_ir_per_event: u64,
    pub(super) events: u64,
}

/// `[recorded]`: the pin on the recorded fixture's bytes (FNV-1a 64, as `s2w_model::Fnv64`
/// computes it and `crates/s2w-app/tests/support/recorded.rs` pins it).
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RecordedFixture {
    pub(super) fixture_fnv1a64: u64,
}

/// `[ir.recorded]`: fold instructions per raw event of the recorded fixture, same CI image.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct IrRecorded {
    pub(super) fold_ir_per_event: u64,
    pub(super) events: u64,
}

/// `[memory.recorded]`: heap bytes per entity after folding the recorded fixture's claims.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct MemoryRecorded {
    pub(super) bytes_per_entity: u64,
    pub(super) bytes_per_relationship_reported: u64,
    pub(super) entities: u64,
    pub(super) relationships: u64,
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
    pub(super) recorded: IrRecorded,
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
    pub(super) recorded: MemoryRecorded,
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
    let sizes = [
        b.ir.events,
        b.memory.entities,
        b.ir.recorded.events,
        b.parse.events,
        b.memory.recorded.entities,
        b.memory.recorded.relationships,
    ];
    if b.ir.profile != "bench" || sizes.contains(&0) {
        return Err(format!(
            "{BASELINE}: [ir] profile must be \"bench\" and every events / entities / relationships count must be > 0; fix the file"
        ));
    }
    b.discover_volume.validate()?;
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

/// Checks the recorded fixture's bytes against `[recorded]` and counts its events (frames with
/// both `id:` and `data:`, by the live SSE framing) against `[ir.recorded] events` and
/// `[parse] events`, the two divisors over this fixture. A changed
/// recording never gets measured: it is human-owned, like a golden file.
pub(super) fn judge_fixture(b: &Baseline, bytes: &[u8]) -> Result<(), String> {
    let actual = s2w_model::Fnv64::new().write(bytes).finish();
    let pinned = b.recorded.fixture_fnv1a64;
    if actual != pinned {
        return Err(format!(
            "recorded fixture: UNKNOWN, {FIXTURE} has FNV-1a 64 {actual:#x}, not [recorded] fixture_fnv1a64 = {pinned:#x}; restore the file (it is human-owned), never re-pin to make a gate pass"
        ));
    }
    let events = s2w_sources::replay_frames(bytes)
        .map_err(|e| format!("recorded fixture: UNKNOWN, {FIXTURE}: {e}"))?
        .len() as u64;
    for (table, pinned) in [
        ("[ir.recorded]", b.ir.recorded.events),
        ("[parse]", b.parse.events),
    ] {
        if events != pinned {
            return Err(format!(
                "recorded fixture: {FIXTURE} holds {events} events but {table} events = {pinned}; the framing changed, so fix it or re-measure and update {BASELINE}"
            ));
        }
    }
    Ok(())
}

/// Judges one benchmark's total instructions against its table (`[ir]`, `[ir.recorded]` or
/// `[parse]`). `Ok` is the report line.
pub(super) fn judge_ir(b: &Baseline, bench: IrBench, total_ir: u64) -> Result<String, String> {
    let name = bench.name();
    let (table, key) = (bench.table(), bench.key());
    if total_ir == 0 {
        return Err(format!(
            "{name}: UNKNOWN (the benchmark reported 0 instructions); fix the bench run, an unknown never passes"
        ));
    }
    let (base, events) = b.ir_gate(bench);
    let per_event = total_ir as f64 / events as f64;
    let set_to = per_event.ceil();
    if base == 0 {
        return Err(format!(
            "{name}: baseline unset: measured {set_to} Ir/event ({total_ir} Ir over {events} events); set {table} {key} = {set_to} and [ir] rustc = the `rustc -V` line in {BASELINE} from the CI job 'scale' (image {}), with a Baseline-growth: s2w#<N> trailer",
            b.ir.ci_image
        ));
    }
    let base_f = base as f64;
    let change = percent(per_event, base_f);
    let tol = b.tolerance_percent as f64;
    if change > tol {
        return Err(format!(
            "{name} regressed {change:+.1}%: measured {per_event:.0} Ir/event vs baseline {base} (tolerance {tol}%); {}, or if the cost is intended raise {table} {key} to {set_to} in {BASELINE}, say why, and add a Baseline-growth: s2w#<N> trailer",
            bench.remedy()
        ));
    }
    // Unlike [memory], nothing lowers [ir] or [parse] automatically: each belongs to the CI image, so a local
    // improvement is only a hint.
    let mut line = format!(
        "{name}: {per_event:.0} Ir/event vs baseline {base} ({change:+.1}%, tolerance {tol}%)"
    );
    if change < -tol {
        line.push_str(&format!(
            "; improved past tolerance, lower {table} {key} to {set_to} to lock it in"
        ));
    }
    Ok(line)
}

/// Judges one supply's memory measurement against its `[memory]` table: report lines, then
/// problems. Target and budget are shared: decision 0004's line applies to real data too.
pub(super) fn judge_memory(
    b: &Baseline,
    supply: Supply,
    m: &MemMeasurement,
) -> (Vec<String>, Vec<String>) {
    let (mem, gate, key) = (&b.memory, b.memory_gate(supply), supply.memory());
    let name = supply.name("bytes/entity");
    let mut report = Vec::new();
    let mut problems = Vec::new();
    if m.bytes_per_entity == 0 || m.entities == 0 {
        problems.push(format!(
            "{name}: UNKNOWN (the memory test measured 0); fix the test, an unknown never passes"
        ));
        return (report, problems);
    }
    let relationships_moved = gate.relationships.is_some_and(|r| r != m.relationships);
    if m.entities != gate.entities || relationships_moved {
        problems.push(format!(
            "{name}: the memory test folded {} entities and {} relationships but {key} pins {} entities{}; {}, so re-measure and update {key} in {BASELINE}",
            m.entities,
            m.relationships,
            gate.entities,
            gate.relationships.map(|r| format!(" and {r} relationships")).unwrap_or_default(),
            supply.changed()
        ));
    }
    let (measured, target) = (
        m.bytes_per_entity as f64,
        mem.target_bytes_per_entity as f64,
    );
    let ratio = measured / target.max(1.0);
    report.push(format!(
        "{name}: {} B = {ratio:.2}x decision 0004's {} B target{}; bytes/relationship {} B over {} relationships (reported, baseline {})",
        m.bytes_per_entity,
        mem.target_bytes_per_entity,
        if ratio > 2.0 { ", beyond its 2x amendment line: interim ceiling only, overshoot tracked in s2w#172" } else { "" },
        m.bytes_per_relationship,
        m.relationships,
        gate.reported
    ));
    if m.bytes_per_entity > mem.budget_bytes_per_entity {
        problems.push(format!(
            "{name} {} B exceeds the hard budget {} B ([memory] budget_bytes_per_entity); shrink the world's per-entity footprint, or get a decision 0004 amendment before raising the budget with a Baseline-growth: s2w#<N> trailer",
            m.bytes_per_entity, mem.budget_bytes_per_entity
        ));
    }
    judge_memory_baseline(b, supply, m, &mut report, &mut problems);
    (report, problems)
}

fn judge_memory_baseline(
    b: &Baseline,
    supply: Supply,
    m: &MemMeasurement,
    report: &mut Vec<String>,
    problems: &mut Vec<String>,
) {
    let (base, key) = (b.memory_gate(supply).bytes_per_entity, supply.memory());
    let name = supply.name("bytes/entity");
    if base == 0 {
        problems.push(format!(
            "{name}: baseline unset: measured {} B; set {key} bytes_per_entity = {} in {BASELINE} with a Baseline-growth: s2w#<N> trailer",
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
            "{name} regressed {change:+.1}%: measured {} B vs baseline {base} B (tolerance {tol}%); shrink the per-entity footprint, or if intended raise {key} bytes_per_entity in {BASELINE}, say why, and add a Baseline-growth: s2w#<N> trailer",
            m.bytes_per_entity
        ));
    } else if change < -tol {
        report.push(format!(
            "{name} improved {change:+.1}% past tolerance; run cargo xtask check --tighten-baseline to lower {key} bytes_per_entity"
        ));
    }
}

/// Guarded keys that are higher than on the base (or new, when there is no base). The events
/// and entities measurement sizes count too: raising one lowers the measured per-unit figure.
pub(super) fn grown_keys(base: Option<&Baseline>, current: &Baseline) -> Vec<&'static str> {
    let keys = |b: &Baseline| {
        let mut keys = vec![
            ("[ir] fold_ir_per_event", b.ir.fold_ir_per_event),
            (
                "[ir.recorded] fold_ir_per_event",
                b.ir.recorded.fold_ir_per_event,
            ),
            ("[parse] parse_ir_per_event", b.parse.parse_ir_per_event),
            ("[memory] bytes_per_entity", b.memory.bytes_per_entity),
            (
                "[memory.recorded] bytes_per_entity",
                b.memory.recorded.bytes_per_entity,
            ),
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
            ("[ir.recorded] events", b.ir.recorded.events),
            ("[parse] events", b.parse.events),
            ("[memory.recorded] entities", b.memory.recorded.entities),
        ];
        keys.extend(b.discover_volume.guarded());
        keys
    };
    let before = base.map(keys);
    keys(current)
        .iter()
        .enumerate()
        .filter(|(i, (_, now))| before.as_ref().is_none_or(|b| b[*i].1 < *now))
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

#[cfg(test)]
mod tests;
