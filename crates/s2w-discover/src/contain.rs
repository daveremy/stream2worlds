//! Stage 5b of research 0002 §6: inclusion dependencies between identifier paths (decision 0022,
//! `PROFILER_VERSION` 5). Two paths share one value domain when a real share of one path's values
//! also appears at the other, and each shared value appears at the other path first, in an
//! earlier event. That carry order separates a reference from a chance overlap of two value sets.
//!
//! Every decision reads value equality, counts and stream order, never a name.

use std::cmp::Ordering;
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
        for (j, &b) in candidates.iter().enumerate().skip(i + 1) {
            let (shared, a_first, b_first) = overlap(&firsts[i], &firsts[j]);
            if shared < cfg.min_support {
                continue;
            }
            // Both directions share `shared`; each takes its own coverage denominator and counts
            // the values the other path carried first.
            for (referrer, referenced, distinct, carried) in [
                (a, b, firsts[i].len(), b_first),
                (b, a, firsts[j].len(), a_first),
            ] {
                let coverage_pct = pct(shared, distinct);
                let carry_pct = pct(carried, shared);
                let accepted = coverage_pct >= cfg.contain_pct && carry_pct >= cfg.carry_pct;
                if accepted {
                    links.push((a, b));
                }
                measured.push(Containment {
                    referrer: table.paths[referrer].clone(),
                    referenced: table.paths[referenced].clone(),
                    shared,
                    coverage_pct,
                    carry_pct,
                    accepted,
                });
            }
        }
    }
    measured.sort_by(|x, y| (&x.referrer, &x.referenced).cmp(&(&y.referrer, &y.referenced)));
    links.sort_unstable();
    links.dedup();
    (measured, links)
}

/// Values two first-event maps share, and how many of them each side carried in a strictly
/// earlier event: `(shared, x_first, y_first)`. One merge walk over the two sorted maps.
fn overlap(x: &BTreeMap<&str, usize>, y: &BTreeMap<&str, usize>) -> (usize, usize, usize) {
    let (mut xs, mut ys) = (x.iter().peekable(), y.iter().peekable());
    let (mut shared, mut x_first, mut y_first) = (0, 0, 0);
    while let (Some(&(vx, &ex)), Some(&(vy, &ey))) = (xs.peek(), ys.peek()) {
        match vx.cmp(vy) {
            Ordering::Less => {
                xs.next();
            }
            Ordering::Greater => {
                ys.next();
            }
            Ordering::Equal => {
                shared += 1;
                match ex.cmp(&ey) {
                    Ordering::Less => x_first += 1,
                    Ordering::Greater => y_first += 1,
                    Ordering::Equal => {}
                }
                xs.next();
                ys.next();
            }
        }
    }
    (shared, x_first, y_first)
}

/// Each distinct value of path `p` mapped to the first event carrying it, for the first
/// `contain_cap` distinct values in stream order. Value ids are assigned in that order, so the
/// first id at the cap means every id below it has been seen.
fn first_events<'t>(table: &'t Table, p: usize, cfg: &Config) -> BTreeMap<&'t str, usize> {
    let column = &table.columns[p];
    let mut firsts = BTreeMap::new();
    for &(event, id) in &column.cells {
        let Some(id) = id else { continue };
        let id = id as usize;
        if id >= cfg.contain_cap {
            break;
        }
        firsts.entry(column.texts[id].as_str()).or_insert(event);
    }
    firsts
}
