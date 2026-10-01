//! Edge-type alignment (contract B3 "Relationships"): a maximum-weight assignment of key edge
//! types to predicted edge types, written in-house (no new dependency) and tested against
//! exhaustive enumeration in `tests.rs`.
//!
//! [`best`] is the O(n³) Hungarian algorithm (Kuhn–Munkres with potentials) on the matrix padded
//! to a square with zeros. [`assign`] then breaks ties: it walks the cells in `(row, column)`
//! order and keeps a positive cell when some optimal assignment still holds it, so the result is
//! the optimal assignment whose sorted pair list is lexicographically smallest.

/// The pairs `(row, column)` of a maximum-weight assignment, positive cells only, sorted. Among
/// assignments of equal total, the one whose sorted pair list is lexicographically smallest.
pub(super) fn assign(weights: &[Vec<usize>]) -> Vec<(usize, usize)> {
    let mut w: Vec<Vec<i64>> = weights
        .iter()
        .map(|row| {
            row.iter()
                .map(|x| i64::try_from(*x).unwrap_or(i64::MAX / 4))
                .collect()
        })
        .collect();
    let mut target = best(&w);
    let mut chosen = Vec::new();
    for i in 0..w.len() {
        for j in 0..w[i].len() {
            let weight = w[i][j];
            if weight == 0 {
                continue;
            }
            let forced = without(&w, i, j);
            if weight + best(&forced) == target {
                chosen.push((i, j));
                target -= weight;
                w = forced;
            } else {
                w[i][j] = 0;
            }
        }
    }
    chosen
}

/// `w` with row `i` and column `j` zeroed: neither can take another pair.
fn without(w: &[Vec<i64>], i: usize, j: usize) -> Vec<Vec<i64>> {
    let mut out = w.to_vec();
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            if r == i || c == j {
                *cell = 0;
            }
        }
    }
    out
}

/// The total weight of a maximum-weight assignment of `w` (rectangular; missing cells are 0).
pub(super) fn best(w: &[Vec<i64>]) -> i64 {
    let n = w.len().max(w.iter().map(Vec::len).max().unwrap_or(0));
    let weight = |i: usize, j: usize| w.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0);
    // `owner[j]` is the row (1-based, 0 = none) holding column `j` (1-based; 0 is the root).
    let mut owner = vec![0usize; n + 1];
    let (mut u, mut v) = (vec![0i64; n + 1], vec![0i64; n + 1]);
    for row in 1..=n {
        augment(row, &|i, j| -weight(i, j), &mut owner, &mut u, &mut v);
    }
    (1..=n)
        .filter(|j| owner[*j] > 0)
        .map(|j| weight(owner[j] - 1, j - 1))
        .sum()
}

/// One Hungarian phase: adds `row` to the matching along a shortest augmenting path under the
/// reduced costs, updating the potentials `u` (rows) and `v` (columns).
fn augment(
    row: usize,
    cost: &dyn Fn(usize, usize) -> i64,
    owner: &mut [usize],
    u: &mut [i64],
    v: &mut [i64],
) {
    let n = owner.len() - 1;
    let mut way = vec![0usize; n + 1];
    let mut slack = vec![i64::MAX; n + 1];
    let mut used = vec![false; n + 1];
    owner[0] = row;
    let mut col = 0;
    while owner[col] != 0 {
        used[col] = true;
        let i = owner[col];
        let (mut delta, mut next) = (i64::MAX, 0);
        for j in (1..=n).filter(|j| !used[*j]) {
            let reduced = cost(i - 1, j - 1) - u[i] - v[j];
            if reduced < slack[j] {
                slack[j] = reduced;
                way[j] = col;
            }
            if slack[j] < delta {
                delta = slack[j];
                next = j;
            }
        }
        for j in 0..=n {
            if used[j] {
                u[owner[j]] += delta;
                v[j] -= delta;
            } else {
                slack[j] -= delta;
            }
        }
        col = next;
    }
    while col != 0 {
        let prev = way[col];
        owner[col] = owner[prev];
        col = prev;
    }
}

#[cfg(test)]
mod tests;
