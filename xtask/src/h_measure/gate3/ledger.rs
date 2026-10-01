//! The spend ledger and the hard stop: [`BudgetGate`] refuses any call that could take a run past
//! [`CAP_USD`], counting every call already made (the clean-session probe included) at its
//! reported tokens times the price table, and a call that reported no tokens at the estimate it
//! was let through on, never at zero.

use s2w_system2::{CallGate, CallRecord};
use serde::{Deserialize, Serialize};

use super::prices::Price;

/// The most one replicate may spend, probe included (decision 0032).
pub(crate) const CAP_USD: f64 = 5.0;

/// A [`CallGate`] holding the run to [`CAP_USD`]. Its own list of estimates is index-aligned
/// with the calls it let through, because every call it allows produces exactly one record.
pub(crate) struct BudgetGate<'a> {
    price: &'a Price,
    /// What was spent before this gate's first call (the probe, for the mapping gate).
    prior_usd: f64,
    estimates: Vec<f64>,
}

impl<'a> BudgetGate<'a> {
    pub(crate) const fn new(price: &'a Price, prior_usd: f64) -> Self {
        Self {
            price,
            prior_usd,
            estimates: Vec::new(),
        }
    }

    /// What each of `calls` is charged: its reported cost, or its estimate when it reported none.
    pub(crate) fn charged(&self, calls: &[CallRecord]) -> Vec<f64> {
        calls
            .iter()
            .zip(&self.estimates)
            .map(|(call, estimate)| self.price.usd(call).unwrap_or(*estimate))
            .collect()
    }
}

impl CallGate for BudgetGate<'_> {
    fn before_call(&mut self, prompt: &str, calls: &[CallRecord]) -> Result<(), String> {
        let spent = self.prior_usd + self.charged(calls).iter().sum::<f64>();
        let next = self.price.estimate(prompt);
        if spent + next > CAP_USD {
            // Fixed decimals so a replay of the run writes the same failure, byte for byte.
            return Err(format!(
                "budget: spent ${spent:.4}, next call estimated ${next:.4}, cap ${CAP_USD:.2}"
            ));
        }
        self.estimates.push(next);
        Ok(())
    }
}

/// A replicate's spend, as its committed file records it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Spend {
    pub cap_usd: f64,
    /// Every call, the probe first, at the price table.
    pub usd: f64,
    /// The sum of the CLI's own `total_cost_usd` figures: a cross-check, never the ledger.
    pub cli_cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Calls made, the probe included.
    pub calls: usize,
    /// What each call was charged, in order, the probe first.
    pub per_call_usd: Vec<f64>,
}

impl Spend {
    /// The spend of `probe` and then `calls`, each paired with its charge in `charged`.
    pub(crate) fn of(probe: &CallRecord, calls: &[CallRecord], charged: Vec<f64>) -> Self {
        let all: Vec<&CallRecord> = std::iter::once(probe).chain(calls).collect();
        let sum = |field: fn(&CallRecord) -> Option<u64>| all.iter().filter_map(|c| field(c)).sum();
        Self {
            cap_usd: CAP_USD,
            usd: charged.iter().sum(),
            cli_cost_usd: all.iter().filter_map(|c| c.cost_usd).sum(),
            input_tokens: sum(|c| c.input_tokens),
            output_tokens: sum(|c| c.output_tokens),
            cache_read_tokens: sum(|c| c.cache_read_tokens),
            cache_write_tokens: sum(|c| c.cache_write_tokens),
            calls: all.len(),
            per_call_usd: charged,
        }
    }
}
