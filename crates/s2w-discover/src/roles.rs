//! Stages 2 to 4 of research 0002 §6: per-path statistics, the event-type field, and roles with
//! an abstain band. Every statistic reads value equality, presence or stream order, never a
//! value's text or a key's name, so renaming keys and hashing strings leaves it unchanged.

use std::collections::{BTreeMap, BTreeSet};

use crate::Config;
use crate::flatten::{Column, Table, pct};

/// What the profiler decided a path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Fewer than `min_support` events carry it: abstain.
    Sparse,
    /// Floats, nulls or mixed kinds: never a key.
    Other,
    /// One value.
    Constant,
    /// Two values.
    Flag,
    /// Unique per event: names the event, never an entity (research 0002 §3).
    EventId,
    /// Uniqueness in the grey band between an entity id and an event id: abstain.
    GreyUniqueness,
    /// Values repeat only in unbroken runs, like a timestamp or a counter.
    Sequence,
    /// Too few repeated values to test what depends on it: abstain.
    FewGroups,
    /// Something depends on it, but not clearly enough: abstain.
    GreyDependency,
    /// Values repeat, but nothing else is constant under them.
    NoDependents,
    /// An entity identifier: repeated values that other fields are constant under.
    Entity,
}

/// Values that occur at least twice in a column, each with the events that carry it.
pub(crate) fn repeat_groups(column: &Column) -> Vec<Vec<usize>> {
    let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for &(event, id) in &column.cells {
        if let Some(id) = id {
            groups.entry(id).or_default().push(event);
        }
    }
    groups.into_values().filter(|g| g.len() >= 2).collect()
}

/// Share of repeat occurrences, in percent, whose previous occurrence is not the immediately
/// preceding cell: low for timestamps and counters, high for identifiers that interleave.
fn recurrence_pct(column: &Column) -> usize {
    let mut last: BTreeMap<u32, usize> = BTreeMap::new();
    let (mut repeats, mut interleaved) = (0, 0);
    for (cell, &(_, id)) in column.cells.iter().enumerate() {
        let Some(id) = id else { continue };
        if let Some(previous) = last.insert(id, cell) {
            repeats += 1;
            if previous + 1 != cell {
                interleaved += 1;
            }
        }
    }
    pct(interleaved, repeats)
}

/// Whether `a` and `b` hold equal values in at least `alias_pct` of at least `min_support`
/// events that carry both: per-event value equality.
pub(crate) fn aliased(table: &Table, a: usize, b: usize, cfg: &Config) -> bool {
    let (mut both, mut equal) = (0, 0);
    for (event, row) in table.rows.iter().enumerate() {
        if row[a].is_some() && row[b].is_some() {
            both += 1;
            if table.text(event, a) == table.text(event, b) {
                equal += 1;
            }
        }
    }
    both >= cfg.min_support && pct(equal, both) >= cfg.alias_pct
}

/// How `a` behaves under `k`'s repeat groups: all of them, those where `a` is carried at least
/// twice and constant, and the distinct `a` values across those constant groups.
pub(crate) struct Dependency {
    pub(crate) considered: usize,
    pub(crate) constant: usize,
    pub(crate) distinct: usize,
}

impl Dependency {
    pub(crate) fn measure(table: &Table, groups: &[Vec<usize>], a: usize) -> Self {
        let mut values = BTreeSet::new();
        let mut constant = 0;
        for group in groups {
            let seen: Vec<u32> = group.iter().filter_map(|&e| table.rows[e][a]).collect();
            // A dependent missing from a group, or carried once, is evidence of nothing: the
            // group counts against it (every repeat group is in the denominator).
            if seen.len() >= 2 && seen.iter().all(|v| *v == seen[0]) {
                constant += 1;
                values.insert(seen[0]);
            }
        }
        Self {
            considered: groups.len(),
            constant,
            distinct: values.len(),
        }
    }

    /// Share of considered groups where `a` is constant, in percent.
    pub(crate) fn share(&self) -> usize {
        pct(self.constant, self.considered)
    }

    /// Whether `a` separates the groups rather than being one value almost everywhere.
    pub(crate) fn informative(&self) -> bool {
        self.distinct * 2 >= self.constant
    }
}

/// The role of one path from single-column statistics, or `None` when it is a candidate key
/// that needs the dependency test.
pub(crate) fn single_column(column: &Column, cfg: &Config) -> Option<Role> {
    let count = column.cells.len();
    let distinct = column.texts.len();
    Some(if count < cfg.min_support {
        Role::Sparse
    } else if !column.keyable() {
        Role::Other
    } else if distinct == 1 {
        Role::Constant
    } else if distinct == 2 {
        Role::Flag
    } else if pct(distinct, count) >= cfg.event_id_pct {
        Role::EventId
    } else if pct(distinct, count) < cfg.grey_uniqueness_pct
        && recurrence_pct(column) < cfg.min_recurrence_pct
    {
        Role::Sequence
    } else {
        return None;
    })
}

/// The dependency test for a candidate key `k`: the best informative dependent decides. A path
/// in the grey uniqueness band gets the test too, since uniqueness falls as the window grows;
/// it abstains as `GreyUniqueness` unless it passes.
pub(crate) fn dependency_role(table: &Table, k: usize, cfg: &Config) -> Role {
    let column = &table.columns[k];
    let grey = pct(column.texts.len(), column.cells.len()) >= cfg.grey_uniqueness_pct;
    let groups = repeat_groups(column);
    if groups.len() < cfg.min_groups {
        return if grey {
            Role::GreyUniqueness
        } else {
            Role::FewGroups
        };
    }
    let best = (0..table.paths.len())
        .filter(|&a| a != k && candidate_dependent(&table.columns[a], cfg))
        .filter(|&a| !aliased(table, k, a, cfg))
        .map(|a| Dependency::measure(table, &groups, a))
        .filter(Dependency::informative)
        .map(|d| d.share())
        .max()
        .unwrap_or(0);
    if best >= cfg.fd_accept_pct {
        Role::Entity
    } else if grey {
        Role::GreyUniqueness
    } else if best >= cfg.fd_grey_pct {
        Role::GreyDependency
    } else {
        Role::NoDependents
    }
}

/// A path that can be evidence for, or an attribute of, a key: keyable, supported, varying.
pub(crate) fn candidate_dependent(column: &Column, cfg: &Config) -> bool {
    column.keyable() && column.cells.len() >= cfg.min_support && column.texts.len() >= 2
}

/// Stage 3: among always-present paths with a small value set, the one whose values best
/// explain which optional paths an event carries. A tie abstains.
pub(crate) fn event_type(table: &Table, cfg: &Config) -> Option<usize> {
    let optional: Vec<usize> = (0..table.paths.len())
        .filter(|&p| {
            let n = table.columns[p].cells.len();
            table.columns[p].keyable() && n >= cfg.min_support && n < table.events
        })
        .collect();
    let mut scored: Vec<(usize, usize)> = (0..table.paths.len())
        .filter(|&c| {
            let col = &table.columns[c];
            col.cells.len() == table.events
                && col.keyable()
                && (2..=cfg.category_max).contains(&col.texts.len())
        })
        .map(|c| (explained(table, c, &optional), c))
        .filter(|&(score, _)| score > 0)
        .collect();
    scored.sort_unstable_by_key(|s| std::cmp::Reverse(s.0));
    match scored[..] {
        [(top, c), (next, _), ..] if top > next => Some(c),
        [(_, c)] => Some(c),
        _ => None,
    }
}

/// How many optional paths are present in all or none of the events of each value of `c`
/// (within 2% either way).
fn explained(table: &Table, c: usize, optional: &[usize]) -> usize {
    let mut by_value: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for &(event, id) in &table.columns[c].cells {
        if let Some(id) = id {
            by_value.entry(id).or_default().push(event);
        }
    }
    optional
        .iter()
        .filter(|&&p| {
            by_value.values().all(|events| {
                let carried = events
                    .iter()
                    .filter(|&&e| table.rows[e][p].is_some())
                    .count();
                let share = pct(carried, events.len());
                share <= 2 || share >= 98
            })
        })
        .count()
}
