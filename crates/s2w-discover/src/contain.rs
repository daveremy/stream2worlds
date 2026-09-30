//! Stage 5b of research 0002 §6: inclusion dependencies between identifier paths (decision 0022,
//! `PROFILER_VERSION` 5). Two paths share one value domain when a real share of one path's values
//! also appears at the other, and each shared value appears at the other path first, in an
//! earlier event. That carry order separates a reference from a chance overlap of two value sets.
//!
//! Every decision reads value equality, counts and stream order, never a name.

use std::collections::BTreeMap;

use s2w_model::FieldPath;

use crate::Config;
use crate::flatten::{Table, pct};
use crate::roles::Role;

/// One measured pair: `referrer` takes its values from `referenced`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Containment {
    /// The path whose values point at the other's.
    pub referrer: FieldPath,
    /// The path the values are carried from.
    pub referenced: FieldPath,
    /// Distinct values the two paths share.
    pub shared: usize,
    /// Share of the referrer's distinct values that also appear at `referenced`, whole percent.
    pub coverage_pct: usize,
    /// Share of the shared values first seen at `referenced` in a strictly earlier event than at
    /// `referrer`, whole percent.
    pub carry_pct: usize,
    /// Whether both thresholds hold, so the two paths key one type.
    pub accepted: bool,
}

/// Every candidate pair with at least `min_support` shared values, in both directions, and the
/// accepted links as unordered `(smaller, larger)` path indices.
pub(crate) fn measure(
    table: &Table,
    roles: &[Role],
    cfg: &Config,
) -> (Vec<Containment>, Vec<(usize, usize)>) {
    let candidates: Vec<usize> = (0..table.paths.len())
        .filter(|&p| matches!(roles[p], Role::Entity | Role::EventId))
        .filter(|&p| table.columns[p].keyable())
        .collect();
    let firsts: Vec<BTreeMap<&str, usize>> = candidates
        .iter()
        .map(|&p| first_events(table, p, cfg))
        .collect();
    let mut measured = Vec::new();
    let mut links = Vec::new();
    for (i, &a) in candidates.iter().enumerate() {
        for (j, &b) in candidates.iter().enumerate() {
            if i == j {
                continue;
            }
            let (from, to) = (&firsts[i], &firsts[j]);
            let mut shared = 0;
            let mut carried = 0;
            for (value, &at_a) in from {
                if let Some(&at_b) = to.get(value) {
                    shared += 1;
                    if at_b < at_a {
                        carried += 1;
                    }
                }
            }
            if shared < cfg.min_support {
                continue;
            }
            let coverage_pct = pct(shared, from.len());
            let carry_pct = pct(carried, shared);
            let accepted = coverage_pct >= cfg.contain_pct && carry_pct >= cfg.carry_pct;
            if accepted {
                links.push((a.min(b), a.max(b)));
            }
            measured.push(Containment {
                referrer: table.paths[a].clone(),
                referenced: table.paths[b].clone(),
                shared,
                coverage_pct,
                carry_pct,
                accepted,
            });
        }
    }
    measured.sort_by(|x, y| (&x.referrer, &x.referenced).cmp(&(&y.referrer, &y.referenced)));
    links.sort_unstable();
    links.dedup();
    (measured, links)
}

/// Each distinct value of path `p` mapped to the first event carrying it, for the first
/// `contain_cap` distinct values in stream order (value ids are assigned in that order).
fn first_events<'t>(table: &'t Table, p: usize, cfg: &Config) -> BTreeMap<&'t str, usize> {
    let column = &table.columns[p];
    let mut firsts = BTreeMap::new();
    for &(event, id) in &column.cells {
        let Some(id) = id else { continue };
        let id = id as usize;
        if id < cfg.contain_cap {
            firsts.entry(column.texts[id].as_str()).or_insert(event);
        }
    }
    firsts
}
