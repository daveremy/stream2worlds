//! `--tighten-baseline` text rewriting for `xtask/scale-baseline.toml`: lowers named keys of one
//! table to a measurement, never raises one, and keeps every comment and every other line.
use super::{MemMeasurement, Supply};

/// One supply's `[memory]` table: lowers `bytes_per_entity` and
/// `bytes_per_relationship_reported` to the measurement. `None` when nothing is lower.
pub(crate) fn tighten_text(text: &str, supply: Supply, m: &MemMeasurement) -> Option<String> {
    lower_in(
        text,
        supply.memory(),
        &[
            ("bytes_per_entity", m.bytes_per_entity),
            ("bytes_per_relationship_reported", m.bytes_per_relationship),
        ],
    )
}

/// Lowers each `(key, measured)` in `table` (its `[name]` header line, exactly) to `measured`
/// when that is above zero and below the current value. `None` when nothing is lower.
pub(crate) fn lower_in(text: &str, table: &str, values: &[(&str, u64)]) -> Option<String> {
    let mut section = String::new();
    let mut changed = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed.to_owned();
        }
        let lowered = (section == table)
            .then(|| lower_line(trimmed, values))
            .flatten();
        changed |= lowered.is_some();
        out.push(lowered.unwrap_or_else(|| line.to_owned()));
    }
    changed.then(|| out.join("\n") + "\n")
}

fn lower_line(line: &str, values: &[(&str, u64)]) -> Option<String> {
    let (key, rest) = line.split_once('=')?;
    let key = key.trim();
    let measured = values.iter().find(|(k, _)| *k == key)?.1;
    let (value, comment) = rest
        .split_once('#')
        .map_or((rest, None), |(v, c)| (v, Some(c)));
    let current: u64 = value.trim().parse().ok()?;
    (measured > 0 && measured < current).then(|| match comment {
        Some(c) => format!("{key} = {measured} #{c}"),
        None => format!("{key} = {measured}"),
    })
}
