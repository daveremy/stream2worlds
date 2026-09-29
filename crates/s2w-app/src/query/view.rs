//! The world as a d3 node/link graph at a level of detail, optionally around a focus entity.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use s2w_core::{AttrMap, EntityId, World};
use serde::Serialize;

use super::QueryError;

/// The most hops a focused view may request.
pub const MAX_HOPS: u32 = 5;

/// Level of detail. `cluster` is part of the URL vocabulary but not served yet (decision 0006).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lod {
    /// One node per entity type.
    Type,
    /// One node per (resolved) entity.
    Entity,
}

/// What to project.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ViewParams {
    /// Level of detail.
    pub lod: Lod,
    /// Restrict to the neighbourhood of this entity id (resolved through merges).
    pub focus: Option<u64>,
    /// Neighbourhood radius when `focus` is set, at most [`MAX_HOPS`].
    pub hops: u32,
}

impl Default for ViewParams {
    fn default() -> Self {
        Self {
            lod: Lod::Entity,
            focus: None,
            hops: 1,
        }
    }
}

/// A relationship from a source to a hub, carried on the source instead of as a link.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct HubRef {
    /// The relationship kind.
    pub kind: String,
    /// The hub's node id, `e:<id>`.
    pub hub: String,
}

/// A graph node. Every `id` is a prefixed string (`e:<id>`, `type:<name>`), so d3's `===`
/// comparison never mixes numbers and strings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    /// A resolved entity.
    Entity {
        /// `e:<id>`.
        id: String,
        /// The entity id.
        entity: EntityId,
        /// The latest observed type; empty if only named by relationships.
        entity_type: String,
        /// Natural keys that resolve to this entity, for labels.
        keys: Vec<String>,
        /// Attributes.
        attrs: AttrMap,
        /// Ids merged into this entity, excluding itself.
        members: Vec<EntityId>,
        /// Relationships to hubs, served on the source instead of as links.
        hub_refs: Vec<HubRef>,
    },
    /// An entity past the hub in-degree cap, served as an aggregate at every level of detail:
    /// no per-source link into it is emitted.
    Hub {
        /// `e:<id>`: a hub keeps its entity id, so a URL focused on it survives the trip.
        id: String,
        /// The entity id.
        entity: EntityId,
        /// The latest observed type.
        entity_type: String,
        /// Natural keys that resolve to this entity.
        keys: Vec<String>,
        /// Attributes.
        attrs: AttrMap,
        /// Ids merged into this entity, excluding itself.
        members: Vec<EntityId>,
        /// Distinct (resolved) sources pointing at the hub.
        in_degree: u64,
        /// Observations by relationship kind.
        by_kind: BTreeMap<String, u64>,
        /// The world offset of the latest relationship observed pointing here.
        last_seen_offset: u64,
        /// The hub's own relationships into other hubs, as on [`Node::Entity`]: no link into a
        /// hub is emitted at `lod=entity`, so without this a hub-to-hub edge would vanish there.
        hub_refs: Vec<HubRef>,
    },
    /// Every non-hub entity of one type.
    Type {
        /// `type:<name>`.
        id: String,
        /// The type; `untyped` for entities only named by relationships.
        entity_type: String,
        /// How many entities.
        count: u64,
    },
}

impl Node {
    /// The d3 node id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Entity { id, .. } | Self::Hub { id, .. } | Self::Type { id, .. } => id,
        }
    }
}

/// A d3 link between two node ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Link {
    /// Source node id.
    pub source: String,
    /// Target node id.
    pub target: String,
    /// Relationship kind.
    pub kind: String,
    /// Observation count; for an aggregate link into a hub, the number of distinct sources.
    pub weight: u64,
}

/// A world snapshot in the d3 node/link shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorldView {
    /// The fold offset this view is at.
    pub offset: u64,
    /// The world branch; only `actual` exists.
    pub branch: &'static str,
    /// The fold version that produced the world.
    pub fold_version: u32,
    /// The in-degree cap the world was folded under.
    pub hub_in_degree_cap: u64,
    /// The level of detail.
    pub lod: Lod,
    /// The focus entity id, if any.
    pub focus: Option<u64>,
    /// Nodes, sorted by id.
    pub nodes: Vec<Node>,
    /// Links, sorted by source, target, kind.
    pub links: Vec<Link>,
}

/// The name of the only branch served.
pub const ACTUAL_BRANCH: &str = "actual";

pub(crate) fn node_id(id: EntityId) -> String {
    format!("e:{}", id.get())
}

#[derive(Default)]
struct HubAgg {
    sources: BTreeSet<EntityId>,
    by_kind: BTreeMap<String, u64>,
    last_seen_offset: u64,
}

/// The resolved entity graph: merges applied at read time (decision 0005, two histories).
struct Graph<'w> {
    world: &'w World,
    members: BTreeMap<EntityId, Vec<EntityId>>,
    keys: BTreeMap<EntityId, Vec<String>>,
    hubs: BTreeMap<EntityId, HubAgg>,
    links: BTreeMap<(EntityId, EntityId, String), u64>,
    hub_edges: BTreeSet<(EntityId, EntityId, String)>,
    /// `hub_edges` grouped by source, so a node's `hub_refs` is one lookup, not a scan.
    hub_refs: BTreeMap<EntityId, Vec<HubRef>>,
}

impl<'w> Graph<'w> {
    #[expect(
        clippy::too_many_lines,
        reason = "one sequential pass whose steps share local state; splitting it is a follow-up refactor (s2w#156)"
    )]
    fn new(world: &'w World) -> Self {
        let mut members: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
        for &id in world.entities().keys() {
            let r = world.resolve(id);
            let list = members.entry(r).or_default();
            if r != id {
                list.push(id);
            }
        }
        let mut keys: BTreeMap<EntityId, Vec<String>> = BTreeMap::new();
        for (key, &id) in world.keys() {
            keys.entry(world.resolve(id))
                .or_default()
                .push(key.as_str().to_owned());
        }
        let cap = world.hub_in_degree_cap();
        // A hub is any entity a raw target resolves to whose own counters tripped the cap.
        // `hub_counters` stays keyed by the target resolved at observation time and a merge
        // never rewrites it, so every entry resolving to a hub folds into that hub's aggregate,
        // including sub-cap entries merged in later: otherwise the aggregate under-counts the
        // edges `hub_edges` resolves into it.
        let hub_ids: BTreeSet<EntityId> = world
            .hub_counters()
            .iter()
            .filter(|(_, counters)| u64::try_from(counters.in_degree()).map_or(true, |n| n > cap))
            .map(|(&target, _)| world.resolve(target))
            .collect();
        let mut hubs: BTreeMap<EntityId, HubAgg> = BTreeMap::new();
        for (&target, counters) in world.hub_counters() {
            let hub = world.resolve(target);
            if !hub_ids.contains(&hub) {
                continue;
            }
            let agg = hubs.entry(hub).or_default();
            agg.sources
                .extend(counters.sources.iter().map(|&s| world.resolve(s)));
            for (kind, n) in &counters.by_kind {
                let total = agg.by_kind.entry(kind.clone()).or_insert(0);
                *total = total.saturating_add(*n);
            }
            agg.last_seen_offset = agg.last_seen_offset.max(counters.last_seen_offset);
        }
        let mut links: BTreeMap<(EntityId, EntityId, String), u64> = BTreeMap::new();
        let mut hub_edges = BTreeSet::new();
        for (rel, &weight) in world.relationships() {
            let (s, t) = (world.resolve(rel.from), world.resolve(rel.to));
            if hubs.contains_key(&t) {
                hub_edges.insert((s, t, rel.kind.clone()));
            } else {
                let w = links.entry((s, t, rel.kind.clone())).or_insert(0);
                *w = w.saturating_add(weight);
            }
        }
        for (&id, state) in world.entities() {
            for (kind, &hub) in &state.hub_refs {
                let h = world.resolve(hub);
                // A hub_ref always names a target that tripped the cap, so `h` is a hub unless
                // a merge re-resolved it; such a ref has no hub node to point at.
                if hubs.contains_key(&h) {
                    hub_edges.insert((world.resolve(id), h, kind.clone()));
                }
            }
        }
        let mut hub_refs: BTreeMap<EntityId, Vec<HubRef>> = BTreeMap::new();
        for (s, h, kind) in &hub_edges {
            hub_refs.entry(*s).or_default().push(HubRef {
                kind: kind.clone(),
                hub: node_id(*h),
            });
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

    fn find(&self, raw: u64) -> Option<EntityId> {
        self.world
            .entities()
            .keys()
            .find(|e| e.get() == raw)
            .map(|&e| self.world.resolve(e))
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
        match self.world.entities().get(&id) {
            Some(state) if !state.entity_type.is_empty() => state.entity_type.clone(),
            _ => "untyped".to_owned(),
        }
    }

    fn entity_node(&self, id: EntityId, members: &[EntityId]) -> Node {
        let state = self.world.entities().get(&id);
        let entity_type = state.map(|s| s.entity_type.clone()).unwrap_or_default();
        let attrs = state.map(|s| s.attrs.clone()).unwrap_or_default();
        let keys = self.keys.get(&id).cloned().unwrap_or_default();
        match self.hubs.get(&id) {
            Some(agg) => Node::Hub {
                id: node_id(id),
                entity: id,
                entity_type,
                keys,
                attrs,
                members: members.to_vec(),
                in_degree: u64::try_from(agg.sources.len()).unwrap_or(u64::MAX),
                by_kind: agg.by_kind.clone(),
                last_seen_offset: agg.last_seen_offset,
                hub_refs: self.hub_refs.get(&id).cloned().unwrap_or_default(),
            },
            None => Node::Entity {
                id: node_id(id),
                entity: id,
                entity_type,
                keys,
                attrs,
                members: members.to_vec(),
                hub_refs: self.hub_refs.get(&id).cloned().unwrap_or_default(),
            },
        }
    }
}

/// Projects `world` at `params`. Pure: the HTTP handler, `--json` and MCP all call this.
///
/// # Errors
/// [`QueryError::UnknownEntity`] for an unknown focus, [`QueryError::HopsTooLarge`] past
/// [`MAX_HOPS`].
#[expect(
    clippy::too_many_lines,
    reason = "one sequential pass whose steps share local state; splitting it is a follow-up refactor (s2w#156)"
)]
pub fn world_view(world: &World, params: &ViewParams) -> Result<WorldView, QueryError> {
    let graph = Graph::new(world);
    let subset = match params.focus {
        None => None,
        Some(raw) => {
            if params.hops > MAX_HOPS {
                return Err(QueryError::HopsTooLarge { hops: params.hops });
            }
            let start = graph
                .find(raw)
                .ok_or(QueryError::UnknownEntity { id: raw })?;
            Some(graph.neighbourhood(start, params.hops))
        }
    };
    let keep = |id: &EntityId| subset.as_ref().is_none_or(|s| s.contains(id));

    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mut links: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    match params.lod {
        Lod::Entity => {
            for (&id, members) in graph.members.iter().filter(|(id, _)| keep(id)) {
                let node = graph.entity_node(id, members);
                nodes.insert(node.id().to_owned(), node);
            }
            for ((s, t, kind), &w) in &graph.links {
                if keep(s) && keep(t) {
                    links.insert((node_id(*s), node_id(*t), kind.clone()), w);
                }
            }
        }
        Lod::Type => {
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
                        .entry((group(*s), group(*t), kind.clone()))
                        .or_insert(0);
                    *total = total.saturating_add(w);
                }
            }
            let mut hub_sources: BTreeMap<(String, String, String), BTreeSet<EntityId>> =
                BTreeMap::new();
            for (s, h, kind) in &graph.hub_edges {
                if keep(s) && keep(h) {
                    hub_sources
                        .entry((group(*s), node_id(*h), kind.clone()))
                        .or_default()
                        .insert(*s);
                }
            }
            for (key, sources) in hub_sources {
                links.insert(key, u64::try_from(sources.len()).unwrap_or(u64::MAX));
            }
        }
    }
    Ok(WorldView {
        offset: world.offset(),
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
