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
    /// Values repeat, but nothing else is constant under them clearly enough, and they fail
    /// the second entity test (`recurs` in `roles.rs`).
    NoDependents,
    /// An entity identifier, by either of two tests. The first: repeated values that an
    /// informative field is constant under in at least `fd_accept_pct` of groups. The second
    /// (s2w#250 PR 2), for a key the first neither passes nor abstains on: values that come back
    /// apart across the stream, followed by a field that varies (`spread_groups_pct`,
    /// `spread_window_pct`, `fd_grey_pct`).
    Entity,
    /// Passes an entity test, but at least `type_uniqueness_pct` of its values are new: nearly
    /// every event carrying it would mint a new entity, so the world would grow with every
    /// event (s2w#208). It keys no type; it may still be another type's attribute.
    NearUnique,
    /// Passes an entity test, but has at most `category_max` values and each of its repeated
    /// values decides which optional fields its events carry: it names a kind of event, not a
    /// thing that recurs (s2w#250). It keys no type; it may still be another type's attribute.
    Category,
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

/// A path that follows a key which passed only the second entity test (`recurs`): constant in
/// `share` percent of the key's repeat groups, with `distinct` values across those groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Follower {
    pub(crate) path: usize,
    pub(crate) share: usize,
    pub(crate) distinct: usize,
}

/// The dependency test for a candidate key `k`: the best informative dependent decides, and a
/// key it rejects outright may still pass the second entity test (`recurs`). A path
/// in the grey uniqueness band gets the test too, since uniqueness falls as the window grows;
/// it abstains as `GreyUniqueness` unless it passes. A path that passes at or above
/// `type_uniqueness_pct` is `NearUnique`, not an entity.
///
/// Also returns the followers that passed `k` by the second test, and none when `k` passed the
/// first test or neither: a type whose every key passed only the second test is a leaf in
/// `assemble` (s2w#291).
pub(crate) fn dependency_role(table: &Table, k: usize, cfg: &Config) -> (Role, Vec<Follower>) {
    let column = &table.columns[k];
    let unique = pct(column.texts.len(), column.cells.len());
    let grey = unique >= cfg.grey_uniqueness_pct;
    let groups = repeat_groups(column);
    if groups.len() < cfg.min_groups {
        let role = if grey {
            Role::GreyUniqueness
        } else {
            Role::FewGroups
        };
        return (role, Vec::new());
    }
    let dependents: Vec<(usize, Dependency)> = (0..table.paths.len())
        .filter(|&a| a != k && candidate_dependent(&table.columns[a], cfg))
        .filter(|&a| !aliased(table, k, a, cfg))
        .map(|a| (a, Dependency::measure(table, &groups, a)))
        .collect();
    let best = dependents
        .iter()
        .filter(|(_, d)| d.informative())
        .map(|(_, d)| d.share())
        .max()
        .unwrap_or(0);
    let followers = if best < cfg.fd_grey_pct && !grey {
        recurs(table, k, &groups, &dependents, cfg)
    } else {
        Vec::new()
    };
    let passes = best >= cfg.fd_accept_pct || !followers.is_empty();
    let role = if passes {
        if unique >= cfg.type_uniqueness_pct {
            Role::NearUnique
        } else if column.texts.len() <= cfg.category_max && decides_shape(table, k, &groups, cfg) {
            Role::Category
        } else {
            Role::Entity
        }
    } else if grey {
        Role::GreyUniqueness
    } else if best >= cfg.fd_grey_pct {
        Role::GreyDependency
    } else {
        Role::NoDependents
    };
    (role, followers)
}

/// The second entity test, for a key whose best informative dependent is below `fd_grey_pct`
/// (s2w#250 PR 2): `k` names a thing that recurs across the stream when
/// - its values come back apart: at least `spread_groups_pct` of its repeat groups span, first
///   event to last, at least `spread_window_pct` of the events profiled (a burst, like a
///   request id or a timestamp, spans a moment), and
/// - some other path, which varies, follows it: that path is constant in at least
///   `fd_grey_pct` of `k`'s repeat groups, takes at least `min_groups` values across those
///   groups, and no one of its values is carried by more than half of the events carrying `k`
///   and it (so the constancy is not what a near-constant path gives by chance), and
/// - no such path sees `k` churn (`churns`, s2w#291): under a follower, a key that names a thing
///   comes back to earlier values (one member of a group turns up in it again and again), while a
///   counter or a size of the follower's thing moves on and never returns. Every such follower counts, not the best one: a
///   counter looks stable under a coarse follower whose groups mix many owners.
///
/// Returns the paths that follow `k`, or none when `k` fails.
/// `dependents` are the candidate dependents `dependency_role` measured under `groups`.
/// Stream order, presence and value equality only, never a name or a value's text.
fn recurs(
    table: &Table,
    k: usize,
    groups: &[Vec<usize>],
    dependents: &[(usize, Dependency)],
    cfg: &Config,
) -> Vec<Follower> {
    let apart = groups
        .iter()
        .filter(|g| match g.as_slice() {
            [first, .., last] => (last - first) * 100 >= table.events * cfg.spread_window_pct,
            _ => false,
        })
        .count();
    if pct(apart, groups.len()) < cfg.spread_groups_pct {
        return Vec::new();
    }
    let followers: Vec<Follower> = dependents
        .iter()
        .filter(|&&(a, ref d)| {
            d.share() >= cfg.fd_grey_pct && d.distinct >= cfg.min_groups && varies(table, k, a)
        })
        .map(|&(path, ref d)| Follower {
            path,
            share: d.share(),
            distinct: d.distinct,
        })
        .collect();
    if followers.iter().any(|f| churns(table, k, f.path, cfg)) {
        return Vec::new();
    }
    followers
}

/// Whether `k`'s values move on under `a` and do not come back: in each of `a`'s repeat groups,
/// `k`'s values in stream order (events that do not carry `k` are skipped), each change from one
/// value to the next that has a further value after it counts, and it is superseded when the
/// value it replaced never appears later in that group. At least `churn_pct` of at least
/// `min_support` counted changes, pooled over the groups, are superseded. A group's last change
/// does not count: nothing follows it, so its old value is never seen again whatever `k` is.
fn churns(table: &Table, k: usize, a: usize, cfg: &Config) -> bool {
    let (mut changes, mut superseded) = (0, 0);
    for group in repeat_groups(&table.columns[a]) {
        let seen: Vec<u32> = group.iter().filter_map(|&e| table.rows[e][k]).collect();
        for i in 1..seen.len().saturating_sub(1) {
            if seen[i] != seen[i - 1] {
                changes += 1;
                if !seen[i + 1..].contains(&seen[i - 1]) {
                    superseded += 1;
                }
            }
        }
    }
    changes >= cfg.min_support && pct(superseded, changes) >= cfg.churn_pct
}

/// Whether no one value of `a` is carried by more than half of the events that carry both `k`
/// and `a`.
fn varies(table: &Table, k: usize, a: usize) -> bool {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    let mut both = 0;
    for row in &table.rows {
        if let (Some(_), Some(v)) = (row[k], row[a]) {
            both += 1;
            *counts.entry(v).or_default() += 1;
        }
    }
    let top = counts.values().copied().max().unwrap_or(0);
    both > 0 && top * 2 <= both
}

/// Whether `k`'s values decide the shape of the events that carry it: among those events at
/// least one path is optional (carried by at least `min_support` of them and by more than 2%
/// and fewer than 98% of them), and every repeat group of `k` carries every optional path in
/// all or none of its events, within 2% either way (stage 3's band; `pct` rounds down, so a
/// group of 49 with one stray reads 2%). Only repeat groups count: a value seen once would
/// trivially explain every path. Presence only, never a name or a value's text.
fn decides_shape(table: &Table, k: usize, groups: &[Vec<usize>], cfg: &Config) -> bool {
    let carriers = &table.columns[k].cells;
    let optional: Vec<usize> = (0..table.paths.len())
        .filter(|&p| p != k && table.columns[p].keyable())
        .filter(|&p| {
            let carried = carriers
                .iter()
                .filter(|&&(e, _)| table.rows[e][p].is_some())
                .count();
            carried >= cfg.min_support && !pure(pct(carried, carriers.len()))
        })
        .collect();
    !optional.is_empty()
        && optional.iter().all(|&p| {
            groups.iter().all(|group| {
                let carried = group
                    .iter()
                    .filter(|&&e| table.rows[e][p].is_some())
                    .count();
                pure(pct(carried, group.len()))
            })
        })
}

/// A presence share that is all or none, within 2% either way.
fn pure(share: usize) -> bool {
    share <= 2 || share >= 98
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
                pure(pct(carried, events.len()))
            })
        })
        .count()
}
