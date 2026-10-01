//! The type-level views: the cheap type summary and the full type view.

use std::collections::{BTreeMap, HashMap};

use s2w_core::{EntityId, World};

use super::graph::Graph;
use super::{ACTUAL_BRANCH, Link, Lod, Node, WorldView, node_id};
use crate::query::epoch::Epoch;

/// The type summary (s2w#296, `lod=type&links=none`): the type view's nodes, one per entity type
/// with its count plus one per hub, and no links. It skips the relationship pass, so it costs one
/// pass over entities (and over keys when there is a hub) instead of a map over every relationship.
/// Its node set and counts equal [`world_view`](super::world_view)'s at `lod=type`, and each hub
/// node is the same except `hub_refs`, which is always empty here: filling it needs the
/// relationships. The full type view, `type_view`, starts from these nodes and fills it.
#[must_use]
pub fn type_summary(world: &World) -> WorldView {
    let (graph, counts) = Graph::summary(world);
    type_level_view(world, type_nodes(&graph, counts), BTreeMap::new())
}

/// The type-level nodes of a [`Graph::summary`]: one per hub (with whatever `hub_refs` the
/// graph holds) and one per entity type with its count, in node id order.
fn type_nodes(graph: &Graph<'_>, counts: BTreeMap<&str, u64>) -> Vec<Node> {
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    for (&id, members) in &graph.members {
        let node = graph.entity_node(id, members);
        nodes.insert(node.id().to_owned(), node);
    }
    for (entity_type, count) in counts {
        let id = format!("type:{entity_type}");
        let node = Node::Type {
            id: id.clone(),
            entity_type: entity_type.to_owned(),
            count,
        };
        nodes.insert(id, node);
    }
    nodes.into_values().collect()
}

/// A whole-world `lod=type` view of `nodes` and `links` (keyed source, target, kind).
fn type_level_view(
    world: &World,
    nodes: Vec<Node>,
    links: BTreeMap<(String, String, String), u64>,
) -> WorldView {
    WorldView {
        offset: world.offset(),
        epoch: Epoch::default(),
        branch: ACTUAL_BRANCH,
        fold_version: world.fold_version(),
        hub_in_degree_cap: world.hub_in_degree_cap(),
        lod: Lod::Type,
        focus: None,
        nodes,
        links: links
            .into_iter()
            .map(|((source, target, kind), weight)| Link {
                source,
                target,
                kind,
                weight,
            })
            .collect(),
    }
}

/// The full type view's groups, interned (s2w#325): hub `i` (in id order) is group `i`, and
/// entity types follow in the order met. `memo[i]` is the group of entity `i` after merges.
struct TypeGroups<'w> {
    world: &'w World,
    hubs: Vec<EntityId>,
    types: Vec<&'w str>,
    by_type: HashMap<&'w str, usize>,
    memo: Vec<usize>,
}

impl<'w> TypeGroups<'w> {
    fn new(world: &'w World, hubs: Vec<EntityId>) -> Self {
        let mut groups = Self {
            world,
            hubs,
            types: Vec::new(),
            by_type: HashMap::new(),
            memo: Vec::with_capacity(world.entity_count()),
        };
        // `World` keeps entity `i` at index `i` and `entities()` yields them in that order,
        // so `memo` is indexed by the raw id.
        for (id, _) in world.entities() {
            let group = groups.resolved(world.resolve(id));
            groups.memo.push(group);
        }
        groups
    }

    /// The group of a resolved id: its hub, else its entity type, `untyped` when empty or
    /// unknown (as [`Graph::entity_type`]).
    fn resolved(&mut self, id: EntityId) -> usize {
        if let Ok(hub) = self.hubs.binary_search(&id) {
            return hub;
        }
        let entity_type = match self.world.entity(id) {
            Some(state) if !state.entity_type.is_empty() => state.entity_type.as_str(),
            _ => "untyped",
        };
        let next = self.hubs.len() + self.types.len();
        let types = &mut self.types;
        *self.by_type.entry(entity_type).or_insert_with(|| {
            types.push(entity_type);
            next
        })
    }

    /// The group of a raw id, after merges.
    fn of(&mut self, id: EntityId) -> usize {
        match usize::try_from(id.get())
            .ok()
            .and_then(|index| self.memo.get(index))
        {
            Some(&group) => group,
            None => self.resolved(self.world.resolve(id)),
        }
    }

    /// The group's node id: `e:<hub>` or `type:<entity type>`.
    fn name(&self, group: usize) -> String {
        match self.hubs.get(group) {
            Some(&hub) => node_id(hub),
            None => {
                debug_assert!(group < self.hubs.len() + self.types.len(), "unknown group");
                let entity_type = self
                    .types
                    .get(group - self.hubs.len())
                    .copied()
                    .unwrap_or("untyped");
                format!("type:{entity_type}")
            }
        }
    }
}

/// Link weights by source group, then target group, then kind. A pair holds a few kinds, so
/// a scan beats hashing each link's kind string (s2w#325: hashing was most of the pass). A
/// target is never a hub here, so a row starts at the first type group, `offset`, and holds
/// no slots for the hubs.
struct GroupWeights<'w> {
    offset: usize,
    rows: Vec<Vec<Vec<(&'w str, u64)>>>,
}

impl<'w> GroupWeights<'w> {
    const fn new(offset: usize) -> Self {
        Self {
            offset,
            rows: Vec::new(),
        }
    }

    fn add(&mut self, source: usize, target: usize, kind: &'w str, weight: u64) {
        let column = target.saturating_sub(self.offset);
        if self.rows.len() <= source {
            self.rows.resize_with(source + 1, Vec::new);
        }
        let row = &mut self.rows[source];
        if row.len() <= column {
            row.resize_with(column + 1, Vec::new);
        }
        let kinds = &mut row[column];
        match kinds.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, total)) => *total = total.saturating_add(weight),
            None => kinds.push((kind, weight)),
        }
    }

    /// Every `((source, target, kind), weight)`, in no particular order.
    fn into_links(self) -> impl Iterator<Item = ((usize, usize, &'w str), u64)> {
        let offset = self.offset;
        self.rows
            .into_iter()
            .enumerate()
            .flat_map(move |(source, row)| {
                row.into_iter()
                    .enumerate()
                    .flat_map(move |(column, kinds)| {
                        let target = column + offset;
                        kinds
                            .into_iter()
                            .map(move |(kind, weight)| ((source, target, kind), weight))
                    })
            })
    }
}

/// Link weights between groups (non-hub targets), and the resolved hub edges
/// `(source, hub, kind)` sorted and deduplicated: the set [`Graph::new`] builds as `hub_edges`.
type GroupLinks<'w> = (GroupWeights<'w>, Vec<(EntityId, EntityId, &'w str)>);

/// One pass over the relationships and one over the entities' hub refs, with no string
/// allocation per link. Summing each resolved (source, target, kind) and then each group, both
/// saturating, equals summing each group saturating: the weights are unsigned.
fn group_links<'w>(world: &'w World, groups: &mut TypeGroups<'w>) -> GroupLinks<'w> {
    let mut links = GroupWeights::new(groups.hubs.len());
    let mut hub_edges = Vec::new();
    for (rel, &weight) in world.relationships() {
        let target = groups.of(rel.to);
        if let Some(&hub) = groups.hubs.get(target) {
            hub_edges.push((world.resolve(rel.from), hub, rel.kind.as_str()));
        } else {
            links.add(groups.of(rel.from), target, rel.kind.as_str(), weight);
        }
    }
    for (id, state) in world.entities() {
        for (kind, hub) in state.hub_refs() {
            let hub = world.resolve(hub);
            // As in `Graph::new`: a ref a merge re-resolved off its hub has no hub node.
            if groups.hubs.binary_search(&hub).is_ok() {
                hub_edges.push((world.resolve(id), hub, kind));
            }
        }
    }
    hub_edges.sort_unstable();
    hub_edges.dedup();
    (links, hub_edges)
}

/// The full type view (`lod=type` with links and no focus) without the per-entity [`Graph`]
/// (s2w#325): the type summary's nodes, each hub's `hub_refs`, and the links grouped over
/// interned groups, with strings built once per output key instead of a `format!` per link.
/// Byte-identical to [`graph_view`](super::graph_view) at the same parameters.
pub(super) fn type_view(world: &World) -> WorldView {
    let (mut graph, counts) = Graph::summary(world);
    let mut groups = TypeGroups::new(world, graph.hubs.keys().copied().collect());
    let (links, hub_edges) = group_links(world, &mut groups);
    let mut out: BTreeMap<(String, String, String), u64> = links
        .into_links()
        .map(|((s, t, kind), w)| ((groups.name(s), groups.name(t), kind.to_owned()), w))
        .collect();
    let mut hub_sources: BTreeMap<(usize, EntityId, &str), u64> = BTreeMap::new();
    for &(source, hub, kind) in &hub_edges {
        // The edges are distinct, so a count per key is its count of distinct sources.
        let group = groups.of(source);
        let n = hub_sources.entry((group, hub, kind)).or_insert(0);
        *n = n.saturating_add(1);
        if group < groups.hubs.len() {
            graph.hub_refs.entry(source).or_default().push((kind, hub));
        }
    }
    for ((source, hub, kind), n) in hub_sources {
        out.insert((groups.name(source), node_id(hub), kind.to_owned()), n);
    }
    type_level_view(world, type_nodes(&graph, counts), out)
}
