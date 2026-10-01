//! `[discover_volume]` (s2w#392; decisions 0022, 0025): the world's exact heap bytes after
//! folding 10^5 fresh-string events of the recorded fixture under the mapping serve's profiler
//! settings discover from its first 10k events. The measurement is `s2w-app`'s
//! `discover_volume_heap` test (dhat), run by `cargo xtask discover-volume`
//! (`discover_volume_run.rs`); the judge here is pure.
//!
//! The fold allocates the same sequence every run, so `heap_bytes` reproduces to the byte; the
//! tolerance absorbs `Vec` doubling steps, not noise. `budget_bytes` is decision 0025's deploy
//! line restated in heap bytes, and fires only if the baseline is ever raised past it.
use serde::Deserialize;

use super::{BASELINE, Baseline, percent};

/// The table's name, as every message spells it.
pub(crate) const TABLE: &str = "[discover_volume]";
/// The gated figure, as every report line names it.
const NAME: &str = "discover_volume heap (fresh)";

/// `[discover_volume]`. Every key is required.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiscoverVolumeBaseline {
    /// The baseline: a measurement more than `tolerance_percent` above it fails.
    pub(crate) heap_bytes: u64,
    /// Hard ceiling, decision 0025's deploy line in heap bytes; above it always fails.
    pub(crate) budget_bytes: u64,
    /// Informational: the plain `discover_volume` target's `VmRSS` delta for the same fold, the
    /// figure decisions 0022 and 0025 quote. Set by hand from a release run of that target.
    pub(crate) rss_bytes_reported: u64,
    /// Pinned: the measurement must agree.
    pub(crate) entities: u64,
    /// Pinned: the measurement must agree.
    pub(crate) relationships: u64,
    /// The events folded; the measurement must agree.
    pub(crate) events: u64,
    /// The profiler's window; the measurement must agree.
    pub(crate) window: u64,
}

impl DiscoverVolumeBaseline {
    /// Refuses a zero anywhere but the informational RSS: a zero size or ceiling cannot gate.
    pub(crate) fn validate(&self) -> Result<(), String> {
        let sizes = [
            self.heap_bytes,
            self.budget_bytes,
            self.entities,
            self.relationships,
            self.events,
            self.window,
        ];
        if sizes.contains(&0) {
            return Err(format!(
                "{BASELINE}: every {TABLE} value except rss_bytes_reported must be > 0; fix the file"
            ));
        }
        Ok(())
    }

    /// The keys whose raising is baseline growth: the gated figure, its ceiling, and the two
    /// measurement sizes (a different run is a different number, not a regression).
    pub(crate) fn guarded(&self) -> [(&'static str, u64); 4] {
        [
            ("[discover_volume] heap_bytes", self.heap_bytes),
            ("[discover_volume] budget_bytes", self.budget_bytes),
            ("[discover_volume] events", self.events),
            ("[discover_volume] window", self.window),
        ]
    }
}

/// The `fold_child` test's JSON line.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct VolumeMeasurement {
    pub(crate) heap_bytes: u64,
    /// dhat's bookkeeping inflates it in the heap target: printed, never judged.
    pub(crate) rss_bytes: u64,
    pub(crate) entities: u64,
    pub(crate) relationships: u64,
    pub(crate) links: u64,
    pub(crate) events: u64,
    pub(crate) window: u64,
    pub(crate) profiler_version: String,
    pub(crate) variant: String,
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
}

/// Judges the measurement against `[discover_volume]`: report lines, then problems.
pub(crate) fn judge(b: &Baseline, m: &VolumeMeasurement) -> (Vec<String>, Vec<String>) {
    let base = &b.discover_volume;
    let (mut report, mut problems) = (Vec::new(), Vec::new());
    if m.heap_bytes == 0 || m.entities == 0 {
        problems.push(format!(
            "{NAME}: UNKNOWN (the test measured 0; is dhat its global allocator?); fix the discover_volume_heap target, an unknown never passes"
        ));
        return (report, problems);
    }
    if m.variant != "fresh" {
        problems.push(format!(
            "{NAME}: the test folded the '{}' variant; the gate reads the fresh upper bound, so run the fresh child",
            m.variant
        ));
        return (report, problems);
    }
    problems.extend(pins(base, m));
    report.push(format!(
        "{NAME}: {} B ({}) vs baseline {} B, budget {} B ({}); {} entities, {} relationships, {} links, profiler version {}; resident {} under dhat (inflated, not judged; {TABLE} rss_bytes_reported {} from the plain target)",
        m.heap_bytes,
        mib(m.heap_bytes),
        base.heap_bytes,
        base.budget_bytes,
        mib(base.budget_bytes),
        m.entities,
        m.relationships,
        m.links,
        m.profiler_version,
        mib(m.rss_bytes),
        mib(base.rss_bytes_reported),
    ));
    if m.heap_bytes > base.budget_bytes {
        problems.push(format!(
            "{NAME} {} B exceeds the hard budget {} B ({TABLE} budget_bytes, decision 0025's deploy line); shrink what the fold keeps, or amend decision 0025 before raising the budget with a Baseline-growth: s2w#<N> trailer",
            m.heap_bytes, base.budget_bytes
        ));
    }
    let (change, tol) = (
        percent(m.heap_bytes as f64, base.heap_bytes as f64),
        b.tolerance_percent as f64,
    );
    if change > tol {
        problems.push(format!(
            "{NAME} regressed {change:+.1}%: measured {} B vs baseline {} B (tolerance {tol}%); find what the profiler or fold change added, or if the cost is intended raise {TABLE} heap_bytes to {} in {BASELINE}, say why, and add a Baseline-growth: s2w#<N> trailer",
            m.heap_bytes, base.heap_bytes, m.heap_bytes
        ));
    } else if change < -tol {
        report.push(format!(
            "{NAME} improved {change:+.1}% past tolerance; run cargo xtask discover-volume --tighten-baseline to lower {TABLE} heap_bytes"
        ));
    } else {
        report.push(format!(
            "{NAME}: {change:+.1}% against the baseline, tolerance {tol}%"
        ));
    }
    (report, problems)
}

/// The measurement's sizes against the table's pins; a moved one means re-measure.
fn pins(base: &DiscoverVolumeBaseline, m: &VolumeMeasurement) -> Vec<String> {
    let sizes = [
        ("events", m.events, base.events),
        ("window", m.window, base.window),
        ("entities", m.entities, base.entities),
        ("relationships", m.relationships, base.relationships),
    ];
    sizes
        .into_iter()
        .filter(|(_, measured, pinned)| measured != pinned)
        .map(|(key, measured, pinned)| {
            format!(
                "{NAME}: the test measured {key} = {measured} but {TABLE} pins {pinned}; the profiler, the fixture, the window or the fold changed, so re-measure and update {TABLE} in {BASELINE} (raising events or window needs a Baseline-growth: s2w#<N> trailer)"
            )
        })
        .collect()
}

/// `cargo xtask discover-volume --tighten-baseline`: lowers `heap_bytes` to the measurement,
/// never raises it, keeps every comment. `rss_bytes_reported` is left alone: the heap target's
/// resident figure is dhat-inflated, so it is not the same quantity. `None` when nothing is lower.
pub(crate) fn tighten_text(text: &str, m: &VolumeMeasurement) -> Option<String> {
    super::tighten::lower_in(text, TABLE, &[("heap_bytes", m.heap_bytes)])
}

#[cfg(test)]
mod tests;
