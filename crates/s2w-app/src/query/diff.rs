//! The difference between two worlds, at entity level of detail.

use std::collections::BTreeMap;

use s2w_core::{EntityId, World};
use serde::Serialize;

use super::QueryError;
use super::view::{Link, Node, ViewParams, WorldView, world_view};

/// A node or link present in both worlds with different contents.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Changed<T> {
    /// The value in the `from` world.
    pub before: T,
    /// The value in the `to` world.
    pub after: T,
}

/// Added, removed and changed items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Changes<T> {
    /// Present only in `to`.
    pub added: Vec<T>,
    /// Present only in `from`.
    pub removed: Vec<T>,
    /// Present in both, different.
    pub changed: Vec<Changed<T>>,
}

/// One raw merge edge, as stored by the fold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MergeEdge {
    /// The absorbed id.
    pub absorbed: EntityId,
    /// The survivor id the merge named.
    pub survivor: EntityId,
}

/// What changed between two offsets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorldDiff {
    /// The earlier offset.
    pub from: u64,
    /// The later offset.
    pub to: u64,
    /// Entity-level nodes, keyed by node id.
    pub nodes: Changes<Node>,
    /// Links, keyed by source, target and kind.
    pub links: Changes<Link>,
    /// Outstanding merges added (merged) and removed (split).
    pub merges: Changes<MergeEdge>,
}

impl<T> Default for Changes<T> {
    fn default() -> Self {
        Self {
            added: Vec::new(),
            removed: Vec::new(),
            changed: Vec::new(),
        }
    }
}

impl WorldDiff {
    /// The diff of a world with itself, at offset `at`: nothing added, removed or changed.
    /// What [`diff`] returns for two equal worlds, without projecting either.
    #[must_use]
    pub(crate) fn unchanged(at: u64) -> Self {
        Self {
            from: at,
            to: at,
            nodes: Changes::default(),
            links: Changes::default(),
            merges: Changes::default(),
        }
    }
}

/// Compares two keyed maps of borrowed items, cloning only the items that changed (#216).
fn changes<K: Ord, T: Clone + PartialEq>(
    before: &BTreeMap<K, &T>,
    after: &BTreeMap<K, &T>,
) -> Changes<T> {
    let mut out = Changes::default();
    for (k, &a) in after {
        match before.get(k) {
            None => out.added.push(a.clone()),
            Some(&b) if b != a => out.changed.push(Changed {
                before: b.clone(),
                after: a.clone(),
            }),
            Some(_) => {}
        }
    }
    for (k, &b) in before {
        if !after.contains_key(k) {
            out.removed.push(b.clone());
        }
    }
    out
}

fn nodes(v: &WorldView) -> BTreeMap<&str, &Node> {
    v.nodes.iter().map(|n| (n.id(), n)).collect()
}

fn links(v: &WorldView) -> BTreeMap<(&str, &str, &str), &Link> {
    v.links
        .iter()
        .map(|l| ((l.source.as_str(), l.target.as_str(), l.kind.as_str()), l))
        .collect()
}

fn merges(w: &World) -> Vec<MergeEdge> {
    w.merges()
        .iter()
        .map(|(&absorbed, &survivor)| MergeEdge { absorbed, survivor })
        .collect()
}

fn by_absorbed(edges: &[MergeEdge]) -> BTreeMap<EntityId, &MergeEdge> {
    edges.iter().map(|e| (e.absorbed, e)).collect()
}

/// Diffs two worlds at entity level of detail. The two views are compared through borrowed
/// maps; only added, removed and changed items are cloned (#216).
///
/// # Errors
/// None in practice: an unfocused view cannot fail. The `Result` keeps the view's contract.
pub fn diff(from: &World, to: &World) -> Result<WorldDiff, QueryError> {
    let params = ViewParams::default();
    let (a, b) = (world_view(from, &params)?, world_view(to, &params)?);
    let (from_merges, to_merges) = (merges(from), merges(to));
    Ok(WorldDiff {
        from: from.offset(),
        to: to.offset(),
        nodes: changes(&nodes(&a), &nodes(&b)),
        links: changes(&links(&a), &links(&b)),
        merges: changes(&by_absorbed(&from_merges), &by_absorbed(&to_merges)),
    })
}
