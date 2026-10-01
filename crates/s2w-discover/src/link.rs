//! Stage 6's links (decision 0027, `PROFILER_VERSION` 9): which classes of a 1:1 merge stay key
//! paths and how they link into one survivor.

use crate::flatten::Table;

/// Stage 6's links (decision 0027; `PROFILER_VERSION` 9): the losers of a 1:1 merge stay key
/// paths under the winner's label, and every class but the survivor is linked into it. The
/// survivor is the more specific encoding, the class with the most distinct values in the window,
/// whichever class won the merge: a link joins an absorbed value to the first survivor it
/// co-occurs with, so the side that determines the other must survive. A tie on the most distinct
/// values gives no link, and the losers stay attributes (`PROFILER_VERSION` 8's rule). Returns the
/// loser paths that stay keys and the links as (survivor path, absorbed path).
pub(crate) fn one_to_one_links(
    table: &Table,
    winner: &[usize],
    losers: &[&[usize]],
) -> (Vec<usize>, Vec<(usize, usize)>) {
    if losers.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let distinct = |c: &[usize]| {
        c.iter()
            .map(|&p| table.columns[p].texts.len())
            .max()
            .unwrap_or(0)
    };
    let classes: Vec<&[usize]> = std::iter::once(winner)
        .chain(losers.iter().copied())
        .collect();
    let most = classes.iter().map(|c| distinct(c)).max().unwrap_or(0);
    let mut top = (0..classes.len()).filter(|&i| distinct(classes[i]) == most);
    let (Some(survivor), None) = (top.next(), top.next()) else {
        return (Vec::new(), Vec::new());
    };
    let anchor = |c: &[usize]| link_member(table, c);
    let links = (0..classes.len())
        .filter(|&i| i != survivor)
        .map(|i| (anchor(classes[survivor]), anchor(classes[i])))
        .collect();
    let aliases = losers.iter().flat_map(|c| c.iter().copied()).collect();
    (aliases, links)
}

/// The member of a class a link names: the most carried, then the most distinct. Members of a
/// class are aliases, holding equal values in at least `alias_pct` of events, so any of them
/// names the same entity in those events. A tie that remains takes the first path in table
/// order. Table order is first appearance in the stream and, within one payload, the key order
/// of `serde_json`'s map, which sorts by name; so this tie can be broken by a name. It is
/// deterministic (the same payloads always give the same link) and it is the one exception to
/// "no tie is broken by a name" in this crate (`AGENTS.md`; decision 0022, `PROFILER_VERSION`
/// 9), because a link names exactly one rule per side (decision 0027). Check 12 compares links
/// exactly and passes on the recorded fixture; if a renaming ever flips this tie on a fixture,
/// check 12 fails with a `links` difference, and the fix is to compare a link up to its tied
/// members there, not to change this rule.
fn link_member(table: &Table, class: &[usize]) -> usize {
    let col = |p: usize| &table.columns[p];
    class
        .iter()
        .copied()
        .max_by_key(|&p| (col(p).cells.len(), col(p).texts.len(), std::cmp::Reverse(p)))
        .unwrap_or(0)
}
