//! The Hungarian solver against exhaustive enumeration.

use super::{assign, best};

/// Every partial one-to-one assignment of rows to columns, as sorted `(row, column)` lists.
fn assignments(rows: usize, cols: usize) -> Vec<Vec<(usize, usize)>> {
    fn walk(
        row: usize,
        rows: usize,
        free: &mut Vec<bool>,
        at: &mut Vec<(usize, usize)>,
        out: &mut Vec<Vec<(usize, usize)>>,
    ) {
        if row == rows {
            out.push(at.clone());
            return;
        }
        walk(row + 1, rows, free, at, out);
        for col in 0..free.len() {
            if free[col] {
                free[col] = false;
                at.push((row, col));
                walk(row + 1, rows, free, at, out);
                at.pop();
                free[col] = true;
            }
        }
    }
    let mut out = Vec::new();
    walk(0, rows, &mut vec![true; cols], &mut Vec::new(), &mut out);
    out
}

/// The optimal total and the lexicographically smallest optimal positive-pair list, by brute force.
fn brute(w: &[Vec<usize>]) -> (usize, Vec<(usize, usize)>) {
    let cols = w.first().map_or(0, Vec::len);
    let mut found: Option<(usize, Vec<(usize, usize)>)> = None;
    for pairs in assignments(w.len(), cols) {
        let positive: Vec<(usize, usize)> =
            pairs.into_iter().filter(|(r, c)| w[*r][*c] > 0).collect();
        let total = positive.iter().map(|(r, c)| w[*r][*c]).sum();
        let better = match &found {
            None => true,
            Some((t, p)) => total > *t || (total == *t && positive < *p),
        };
        if better {
            found = Some((total, positive));
        }
    }
    found.unwrap_or_default()
}

/// Every matrix of `rows × cols` cells, each in `0..=2`.
fn matrices(rows: usize, cols: usize) -> impl Iterator<Item = Vec<Vec<usize>>> {
    let cells = u32::try_from(rows * cols).unwrap();
    (0..3usize.pow(cells)).map(move |mut code| {
        (0..rows)
            .map(|_| {
                (0..cols)
                    .map(|_| {
                        let cell = code % 3;
                        code /= 3;
                        cell
                    })
                    .collect()
            })
            .collect()
    })
}

fn check(w: &[Vec<usize>]) {
    let (total, pairs) = brute(w);
    let got = assign(w);
    let signed: Vec<Vec<i64>> = w
        .iter()
        .map(|r| r.iter().map(|x| i64::try_from(*x).unwrap()).collect())
        .collect();
    assert_eq!(
        best(&signed),
        i64::try_from(total).unwrap(),
        "best on {w:?}"
    );
    assert_eq!(got, pairs, "assignment on {w:?}");
}

#[test]
fn agrees_with_enumeration_on_every_small_matrix() {
    for (rows, cols) in [
        (0, 0),
        (1, 1),
        (1, 3),
        (2, 2),
        (2, 3),
        (3, 2),
        (3, 3),
        (2, 4),
        (4, 2),
        (1, 4),
        (4, 1),
    ] {
        for w in matrices(rows, cols) {
            check(&w);
        }
    }
}

#[test]
fn agrees_with_enumeration_on_sampled_four_by_four_matrices() {
    // 3^16 matrices is too many; a fixed linear congruential walk visits 5000 of them.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    for _ in 0..5000 {
        let w: Vec<Vec<usize>> = (0..4)
            .map(|_| {
                (0..4)
                    .map(|_| {
                        state = state
                            .wrapping_mul(6_364_136_223_846_793_005)
                            .wrapping_add(1);
                        usize::try_from((state >> 33) % 3).unwrap()
                    })
                    .collect()
            })
            .collect();
        check(&w);
    }
}

#[test]
fn a_tie_goes_to_the_earlier_pairs() {
    // Diagonal and anti-diagonal both total 2: the diagonal's (0, 0) comes first.
    assert_eq!(assign(&[vec![1, 1], vec![1, 1]]), vec![(0, 0), (1, 1)]);
    // One pair worth 2 against two worth 1 each: equal totals, and (0, 0) comes before (0, 1).
    assert_eq!(assign(&[vec![1, 2], vec![0, 1]]), vec![(0, 0), (1, 1)]);
}

#[test]
fn zero_cells_are_never_aligned() {
    assert_eq!(assign(&[vec![0, 0], vec![0, 3]]), vec![(1, 1)]);
    assert!(assign(&vec![vec![0; 3]; 2]).is_empty());
}
