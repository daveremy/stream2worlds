//! The public price table a gate-3 run is charged by: `research/h-measure/prices.toml`, one row
//! per pinned model snapshot, in USD per million tokens (contract dated note 2026-09-30: dollars
//! are reported tokens times this table; the CLI's own cost figure is a cross-check only).

use std::collections::BTreeMap;
use std::path::Path;

use s2w_system2::CallRecord;
use serde::{Deserialize, Serialize};

use super::super::pins::toml_file;

/// The price table's file name, under the measurement's data directory.
pub(crate) const FILE: &str = "prices.toml";

/// Output tokens a call is estimated at before it is made: the most a reply may use.
pub(crate) const ESTIMATED_OUTPUT_TOKENS: u64 = 16_384;

/// One `[model."<snapshot>"]` row, as it is copied into a committed file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Price {
    /// USD per million input tokens.
    pub input: f64,
    /// USD per million output tokens.
    pub output: f64,
    /// USD per million cache-write tokens, at the cache duration the CLI uses.
    pub cache_write: f64,
    /// USD per million cache-read tokens.
    pub cache_read: f64,
    /// The public page the row was copied from.
    pub source_url: String,
    /// When it was copied, `YYYY-MM-DD`.
    pub copied_on: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    model: BTreeMap<String, Price>,
}

/// The row for `snapshot`.
///
/// # Errors
///
/// The table does not parse, has no row for `snapshot`, or the row holds a negative or
/// non-finite price.
pub(crate) fn load(root: &Path, snapshot: &str) -> Result<Price, String> {
    let table: Table = toml_file(root, FILE)?;
    let price = table.model.get(snapshot).cloned().ok_or_else(|| {
        format!(
            "{FILE} has no row for model {snapshot:?}; add one copied from the public price page before a run (rows: {:?})",
            table.model.keys().collect::<Vec<_>>()
        )
    })?;
    for (name, rate) in [
        ("input", price.input),
        ("output", price.output),
        ("cache_write", price.cache_write),
        ("cache_read", price.cache_read),
    ] {
        if !rate.is_finite() || rate < 0.0 {
            return Err(format!(
                "{FILE}: model {snapshot:?} {name} = {rate} is not a price"
            ));
        }
    }
    Ok(price)
}

/// A token count as `f64`. A call's count is far below `u32::MAX`; a larger one saturates
/// there, which over-charges it and so never lets a run past the cap.
fn tokens(n: u64) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

impl Price {
    /// What `call` cost by its reported tokens; `None` when it reported none (a failed call).
    pub(crate) fn usd(&self, call: &CallRecord) -> Option<f64> {
        let (input, output) = (call.input_tokens?, call.output_tokens?);
        let cache_write = call.cache_write_tokens.unwrap_or(0);
        let cache_read = call.cache_read_tokens.unwrap_or(0);
        Some(
            (tokens(input) * self.input
                + tokens(output) * self.output
                + tokens(cache_write) * self.cache_write
                + tokens(cache_read) * self.cache_read)
                / 1e6,
        )
    }

    /// The most a call with `prompt` is expected to cost: one input token per 3 prompt bytes,
    /// rounded up, at the higher of the input and cache-write rates (the CLI may cache-write
    /// the prompt), and [`ESTIMATED_OUTPUT_TOKENS`] at the output rate.
    pub(crate) fn estimate(&self, prompt: &str) -> f64 {
        let input = u64::try_from(prompt.len().div_ceil(3)).unwrap_or(u64::MAX);
        let rate = self.input.max(self.cache_write);
        (tokens(input) * rate + tokens(ESTIMATED_OUTPUT_TOKENS) * self.output) / 1e6
    }
}
