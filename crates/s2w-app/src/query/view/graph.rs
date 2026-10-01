//! The resolved entity graph and the entity-level (or focused) view over it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use s2w_core::{EntityId, World};

use super::hubs::{HubAgg, NodeParts, entity_node, hub_aggregates};
use super::{ACTUAL_BRANCH, HubRef, Link, Lod, MAX_HOPS, Node, ViewParams, WorldView, node_id};
use crate::query::QueryError;
use crate::query::epoch::Epoch;

/// The resolved entity graph: merges applied at read time (decision 0005, two histories).
pub(in crate::query) struct Graph<'w> {
    world: &'w World,
    pub(in crate::query) members: BTreeMap<EntityId, Vec<EntityId>>,
    // Key and kind strings borrow from the world: the whole map is built under the read guard,
    // and cloning every string doubled its size (#216). A streamed `/world` copies only what
    // it keeps before releasing the guard (`crate::query::projection`).
    pub(in crate::query) keys: BTreeMap<EntityId, Vec<&'w str>>,
    pub(in crate::query) hubs: BTreeMap<EntityId, HubAgg>,
    pub(in crate::query) links: BTreeMap<(EntityId, EntityId, &'w str), u64>,
    hub_edges: BTreeSet<(EntityId, EntityId, &'w str)>,
    /// `hub_edges` grouped by source as (kind, hub), in `hub_edges` order, so a node's
    /// `hub_refs` is one lookup, not a scan.
    pub(in crate::query) hub_refs: BTreeMap<EntityId, Vec<(&'w str, EntityId)>>,
}

impl<'w> Graph<'w> {
    pub(in crate::query) fn new(world: &'w World) -> Self {
        let mut members: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
        for (id, _) in world.entities() {
            let r = world.resolve(id);
            let list = members.entry(r).or_default();
            if r != id {
                list.push(id);
            }
        }
        let mut keys: BTreeMap<EntityId, Vec<&'w str>> = BTreeMap::new();
        for (key, &id) in world.keys() {
            keys.entry(world.resolve(id))
                .or_default()
                .push(key.as_str());
        }
        let hubs = hub_aggregates(world);
        let mut links: BTreeMap<(EntityId, EntityId, &'w str), u64> = BTreeMap::new();
        let mut hub_edges = BTreeSet::new();
        for (rel, &weight) in world.relationships() {
            let (s, t) = (world.resolve(rel.from), world.resolve(rel.to));
            if hubs.contains_key(&t) {
                hub_edges.insert((s, t, rel.kind.as_str()));
            } else {
                let w = links.entry((s, t, rel.kind.as_str())).or_insert(0);
                *w = w.saturating_add(weight);
            }
        }
        for (id, state) in world.entities() {
            for (kind, hub) in state.hub_refs() {
                let h = world.resolve(hub);
                // A hub_ref always names a target that tripped the cap, so `h` is a hub unless
                // a merge re-resolved it; such a ref has no hub node to point at.
                if hubs.contains_key(&h) {
                    hub_edges.insert((world.resolve(id), h, kind));
                }
            }
        }
        let mut hub_refs: BTreeMap<EntityId, Vec<(&'w str, EntityId)>> = BTreeMap::new();
        for &(s, h, kind) in &hub_edges {
            hub_refs.entry(s).or_default().push((kind, h));
        }
        Self {
            world,
            members,
            keys,
            hubs,
            links,
            hub_edges,
            hub_refs,
        }
    }

    /// The part of the graph the type summary reads (s2w#296), with its per-type counts of the
    /// non-hub resolved entities. One pass over entities, and over keys only when there is a
    /// hub; `members` and `keys` hold hub ids only, and there are no links, hub edges or hub
    /// refs, so [`Self::entity_node`] gives each hub `hub_refs: []`.
    pub(super) fn summary(world: &'w World) -> (Self, BTreeMap<&'w str, u64>) {
        let hubs = hub_aggregates(world);
        // A hub is a resolved relationship target, always a minted entity, so seeding from
        // the hubs gives the same hub nodes `Graph::new` finds among the entities.
        let mut members: BTreeMap<EntityId, Vec<EntityId>> =
            hubs.keys().map(|&hub| (hub, Vec::new())).collect();
        let mut counts: BTreeMap<&'w str, u64> = BTreeMap::new();
        for (id, state) in world.entities() {
            let r = world.resolve(id);
            if let Some(list) = members.get_mut(&r) {
                if r != id {
                    list.push(id);
                }
            } else if r == id {
                // A merge target is always a minted entity and merges never cycle, so the
                // resolved ids are exactly the entities that resolve to themselves.
                let entity_type = if state.entity_type.is_empty() {
                    "untyped"
                } else {
                    state.entity_type.as_str()
                };
                let count = counts.entry(entity_type).or_insert(0);
                *count = count.saturating_add(1);
            }
        }
        let mut keys: BTreeMap<EntityId, Vec<&'w str>> = BTreeMap::new();
        if !hubs.is_empty() {
            for (key, &id) in world.keys() {
                let r = world.resolve(id);
                if hubs.contains_key(&r) {
                    keys.entry(r).or_default().push(key.as_str());
                }
            }
        }
        let graph = Self {
            world,
            members,
            keys,
            hubs,
            links: BTreeMap::new(),
            hub_edges: BTreeSet::new(),
            hub_refs: BTreeMap::new(),
        };
        (graph, counts)
    }

    /// The focus neighbourhood, or `None` for the whole world.
    pub(in crate::query) fn subset(
        &self,
        params: &ViewParams,
    ) -> Result<Option<BTreeSet<EntityId>>, QueryError> {
        let Some(raw) = params.focus else {
            return Ok(None);
        };
        if params.hops > MAX_HOPS {
            return Err(QueryError::HopsTooLarge { hops: params.hops });
        }
        let start = self
            .find(raw)
            .ok_or(QueryError::UnknownEntity { id: raw })?;
        Ok(Some(self.neighbourhood(start, params.hops)))
    }

    fn find(&self, raw: u64) -> Option<EntityId> {
        self.world.minted_id(raw).map(|e| self.world.resolve(e))
    }

    /// Undirected BFS from `start`; hubs are included but never expanded.
    fn neighbourhood(&self, start: EntityId, hops: u32) -> BTreeSet<EntityId> {
        let mut adj: BTreeMap<EntityId, BTreeSet<EntityId>> = BTreeMap::new();
        let edges = self
            .links
            .keys()
            .map(|(s, t, _)| (*s, *t))
            .chain(self.hub_edges.iter().map(|(s, h, _)| (*s, *h)));
        for (a, b) in edges {
            adj.entry(a).or_default().insert(b);
            adj.entry(b).or_default().insert(a);
        }
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([(start, 0u32)]);
        while let Some((node, depth)) = queue.pop_front() {
            if depth >= hops || self.hubs.contains_key(&node) {
                continue;
            }
            for &next in adj.get(&node).into_iter().flatten() {
                if seen.insert(next) {
                    queue.push_back((next, depth.saturating_add(1)));
                }
            }
        }
        seen
    }

    fn entity_type(&self, id: EntityId) -> String {
        match self.world.entity(id) {
            Some(state) if !state.entity_type.is_empty() => state.entity_type.clone(),
            _ => "untyped".to_owned(),
        }
    }

    pub(super) fn entity_node(&self, id: EntityId, members: &[EntityId]) -> Node {
        let keys = self
            .keys
            .get(&id)
            .map(|keys| keys.iter().map(|&k| k.to_owned()).collect())
            .unwrap_or_default();
        let hub_refs = self
            .hub_refs
            .get(&id)
            .map(|refs| {
                refs.iter()
                    .map(|&(kind, hub)| HubRef {
                        kind: kind.to_owned(),
                        hub: node_id(hub),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let parts = NodeParts {
            keys,
            members: members.to_vec(),
            hub_refs,
            hub: self.hubs.get(&id).map(HubAgg::facts),
        };
        entity_node(id, self.world.entity(id), parts)
    }
}

/// [`world_view`](super::world_view) over the whole resolved [`Graph`]: `lod=entity`, and
/// `lod=type` with a focus. Without a focus its `lod=type` equals [`type_view`](super::type_view),
/// byte for byte.
pub(super) fn graph_view(world: &World, params: &ViewParams) -> Result<WorldView, QueryError> {
    let graph = Graph::new(world);
    let subset = graph.subset(params)?;
    let keep = |id: &EntityId| subset.as_ref().is_none_or(|s| s.contains(id));

    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mut links: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    match params.lod {
        Lod::Entity => entity_level(&graph, &keep, &mut nodes, &mut links),
        Lod::Type => type_level(&graph, &keep, &mut nodes, &mut links),
    }
    Ok(WorldView {
        offset: world.offset(),
        epoch: Epoch::default(),
        branch: ACTUAL_BRANCH,
        fold_version: world.fold_version(),
        hub_in_degree_cap: world.hub_in_degree_cap(),
        lod: params.lod,
        focus: params.focus,
        nodes: nodes.into_values().collect(),
        links: links
            .into_iter()
            .map(|((source, target, kind), weight)| Link {
                source,
                target,
                kind,
                weight,
            })
            .collect(),
    })
}

/// `lod=entity`: one node per resolved entity in the subset, one link per kept edge.
fn entity_level(
    graph: &Graph<'_>,
    keep: &impl Fn(&EntityId) -> bool,
    nodes: &mut BTreeMap<String, Node>,
    links: &mut BTreeMap<(String, String, String), u64>,
) {
    for (&id, members) in graph.members.iter().filter(|(id, _)| keep(id)) {
        let node = graph.entity_node(id, members);
        nodes.insert(node.id().to_owned(), node);
    }
    for ((s, t, kind), &w) in &graph.links {
        if keep(s) && keep(t) {
            links.insert((node_id(*s), node_id(*t), (*kind).to_owned()), w);
        }
    }
}

/// `lod=type`: non-hub entities collapse into one node per type; hubs stay entity nodes.
fn type_level(
    graph: &Graph<'_>,
    keep: &impl Fn(&EntityId) -> bool,
    nodes: &mut BTreeMap<String, Node>,
    links: &mut BTreeMap<(String, String, String), u64>,
) {
    let group = |id: EntityId| {
        if graph.hubs.contains_key(&id) {
            node_id(id)
        } else {
            format!("type:{}", graph.entity_type(id))
        }
    };
    for (&id, members) in graph.members.iter().filter(|(id, _)| keep(id)) {
        if graph.hubs.contains_key(&id) {
            let node = graph.entity_node(id, members);
            nodes.insert(node.id().to_owned(), node);
        } else {
            let entity_type = graph.entity_type(id);
            let node = nodes
                .entry(format!("type:{entity_type}"))
                .or_insert_with(|| Node::Type {
                    id: format!("type:{entity_type}"),
                    entity_type,
                    count: 0,
                });
            if let Node::Type { count, .. } = node {
                *count = count.saturating_add(1);
            }
        }
    }
    for ((s, t, kind), &w) in &graph.links {
        if keep(s) && keep(t) {
            let total = links
                .entry((group(*s), group(*t), (*kind).to_owned()))
                .or_insert(0);
            *total = total.saturating_add(w);
        }
    }
    type_hub_links(graph, keep, &group, links);
}

/// Hub edges at `lod=type`: one link per (group, hub, kind), weighted by distinct sources.
fn type_hub_links(
    graph: &Graph<'_>,
    keep: &impl Fn(&EntityId) -> bool,
    group: &impl Fn(EntityId) -> String,
    links: &mut BTreeMap<(String, String, String), u64>,
) {
    let mut hub_sources: BTreeMap<(String, String, String), BTreeSet<EntityId>> = BTreeMap::new();
    for (s, h, kind) in &graph.hub_edges {
        if keep(s) && keep(h) {
            hub_sources
                .entry((group(*s), node_id(*h), (*kind).to_owned()))
                .or_default()
                .insert(*s);
        }
    }
    for (key, sources) in hub_sources {
        links.insert(key, u64::try_from(sources.len()).unwrap_or(u64::MAX));
    }
}
