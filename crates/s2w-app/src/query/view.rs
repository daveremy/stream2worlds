//! The world as a d3 node/link graph at a level of detail, optionally around a focus entity.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use s2w_core::{AttrMap, EntityId, World};
use serde::Serialize;
use serde::ser::{SerializeSeq, SerializeStruct, Serializer};

use super::QueryError;
use super::epoch::Epoch;

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
    /// Whether the view carries links. [`LinkDetail::None`] is the type summary
    /// ([`type_summary`]): valid only with [`Lod::Type`] and no `focus`.
    pub links: LinkDetail,
}

impl Default for ViewParams {
    fn default() -> Self {
        Self {
            lod: Lod::Entity,
            focus: None,
            hops: 1,
            links: LinkDetail::All,
        }
    }
}

/// The `links` parameter (s2w#296): every link, or none at all.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum LinkDetail {
    /// The view with its links (the default).
    #[default]
    All,
    /// No links: at `lod=type` the type summary, built without the relationship pass.
    None,
}

/// Refuses `links=none` outside its one valid shape, `lod=type` with no `focus`.
///
/// # Errors
/// [`QueryError::BadParameter`] naming `links`.
pub(crate) fn check_links(params: &ViewParams) -> Result<(), QueryError> {
    if params.links == LinkDetail::All {
        return Ok(());
    }
    let reason = if params.lod != Lod::Type {
        "links=none needs lod=type"
    } else if params.focus.is_some() {
        "links=none cannot be combined with focus"
    } else {
        return Ok(());
    };
    Err(QueryError::BadParameter {
        name: "links",
        reason: reason.to_owned(),
    })
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
    /// The history `offset` belongs to (see [`Epoch`]): pass it back as `epoch` with any offset
    /// read from this view.
    pub epoch: Epoch,
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

/// Every hub's aggregate, keyed by the resolved hub id. Reads `hub_counters` only (one entry per
/// distinct relationship target), never the relationships.
fn hub_aggregates(world: &World) -> BTreeMap<EntityId, HubAgg> {
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
    hubs
}

/// The resolved entity graph: merges applied at read time (decision 0005, two histories).
struct Graph<'w> {
    world: &'w World,
    members: BTreeMap<EntityId, Vec<EntityId>>,
    // Key and kind strings borrow from the world: at the head these maps are built under the
    // read guard for every `/world`, and cloning each string doubled their size (#216).
    keys: BTreeMap<EntityId, Vec<&'w str>>,
    hubs: BTreeMap<EntityId, HubAgg>,
    links: BTreeMap<(EntityId, EntityId, &'w str), u64>,
    hub_edges: BTreeSet<(EntityId, EntityId, &'w str)>,
    /// `hub_edges` grouped by source as (kind, hub), in `hub_edges` order, so a node's
    /// `hub_refs` is one lookup, not a scan.
    hub_refs: BTreeMap<EntityId, Vec<(&'w str, EntityId)>>,
}

impl<'w> Graph<'w> {
    fn new(world: &'w World) -> Self {
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
    fn summary(world: &'w World) -> (Self, BTreeMap<&'w str, u64>) {
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
    fn subset(&self, params: &ViewParams) -> Result<Option<BTreeSet<EntityId>>, QueryError> {
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

    fn entity_node(&self, id: EntityId, members: &[EntityId]) -> Node {
        let state = self.world.entity(id);
        let entity_type = state.map(|s| s.entity_type.clone()).unwrap_or_default();
        let attrs = state.map(|s| s.attrs.clone()).unwrap_or_default();
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
                hub_refs,
            },
            None => Node::Entity {
                id: node_id(id),
                entity: id,
                entity_type,
                keys,
                attrs,
                members: members.to_vec(),
                hub_refs,
            },
        }
    }
}

/// Projects `world` at `params`. Pure: the HTTP handler, `--json` and MCP all call this. The
/// view's `epoch` is 0 here; [`super::QueryState::view_at`] labels it with the served one.
///
/// # Errors
/// [`QueryError::UnknownEntity`] for an unknown focus, [`QueryError::HopsTooLarge`] past
/// [`MAX_HOPS`], [`QueryError::BadParameter`] for `links=none` with `lod=entity` or a focus.
pub fn world_view(world: &World, params: &ViewParams) -> Result<WorldView, QueryError> {
    check_links(params)?;
    if params.links == LinkDetail::None {
        return Ok(type_summary(world));
    }
    if params.lod == Lod::Type && params.focus.is_none() {
        return Ok(type_view(world));
    }
    graph_view(world, params)
}

/// [`world_view`] over the whole resolved [`Graph`]: `lod=entity`, and `lod=type` with a
/// focus. Without a focus its `lod=type` equals [`type_view`], byte for byte.
#[expect(
    clippy::too_many_lines,
    reason = "one sequential pass whose steps share local state; splitting it is a follow-up refactor (s2w#156)"
)]
fn graph_view(world: &World, params: &ViewParams) -> Result<WorldView, QueryError> {
    let graph = Graph::new(world);
    let subset = graph.subset(params)?;
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
                    links.insert((node_id(*s), node_id(*t), (*kind).to_owned()), w);
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
                        .entry((group(*s), group(*t), (*kind).to_owned()))
                        .or_insert(0);
                    *total = total.saturating_add(w);
                }
            }
            let mut hub_sources: BTreeMap<(String, String, String), BTreeSet<EntityId>> =
                BTreeMap::new();
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

/// The type summary (s2w#296, `lod=type&links=none`): the type view's nodes, one per entity
/// type with its count plus one per hub, and no links. It skips the relationship pass, so it
/// costs one pass over entities (and over keys when there is a hub) instead of a map over every
/// relationship. Its node set and counts equal [`world_view`]'s at `lod=type`, and each hub node
/// is the same except `hub_refs`, which is always empty here: filling it needs the
/// relationships.
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

/// Link weights between groups (non-hub targets), and the resolved hub edges
/// `(source, hub, kind)` sorted and deduplicated: the set [`Graph::new`] builds as `hub_edges`.
type GroupLinks<'w> = (
    HashMap<(usize, usize, &'w str), u64>,
    Vec<(EntityId, EntityId, &'w str)>,
);

/// One pass over the relationships and one over the entities' hub refs, with no allocation
/// per link. Summing each resolved (source, target, kind) and then each group, both
/// saturating, equals summing each group saturating: the weights are unsigned.
fn group_links<'w>(world: &'w World, groups: &mut TypeGroups<'w>) -> GroupLinks<'w> {
    let mut links: HashMap<(usize, usize, &'w str), u64> = HashMap::new();
    let mut hub_edges = Vec::new();
    for (rel, &weight) in world.relationships() {
        let target = groups.of(rel.to);
        if let Some(&hub) = groups.hubs.get(target) {
            hub_edges.push((world.resolve(rel.from), hub, rel.kind.as_str()));
        } else {
            let source = groups.of(rel.from);
            let total = links
                .entry((source, target, rel.kind.as_str()))
                .or_insert(0);
            *total = total.saturating_add(weight);
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
/// interned groups, with strings built once per output key. At the recorded backfill the
/// per-link `format!` grouping was ~1 s of a ~1.5 s build. Byte-identical to [`graph_view`]
/// at the same parameters.
fn type_view(world: &World) -> WorldView {
    let (mut graph, counts) = Graph::summary(world);
    let mut groups = TypeGroups::new(world, graph.hubs.keys().copied().collect());
    let (links, hub_edges) = group_links(world, &mut groups);
    let mut out: BTreeMap<(String, String, String), u64> = links
        .into_iter()
        .map(|((s, t, kind), w)| ((groups.name(s), groups.name(t), kind.to_owned()), w))
        .collect();
    let mut hub_sources: BTreeMap<(usize, EntityId, &str), u64> = BTreeMap::new();
    for &(source, hub, kind) in &hub_edges {
        // The edges are distinct, so a count per key is its count of distinct sources.
        let n = hub_sources
            .entry((groups.of(source), hub, kind))
            .or_insert(0);
        *n = n.saturating_add(1);
        if graph.hubs.contains_key(&source) {
            graph.hub_refs.entry(source).or_default().push((kind, hub));
        }
    }
    for ((source, hub, kind), n) in hub_sources {
        out.insert((groups.name(source), node_id(hub), kind.to_owned()), n);
    }
    type_level_view(world, type_nodes(&graph, counts), out)
}

/// The decimal digits of an entity id, ordered as its `e:<id>` node id string is: `e:10`
/// sorts before `e:2`. [`world_view`] orders nodes and links by those strings; a streamed view
/// walking `EntityId` order instead would serve different bytes (#216).
#[derive(Clone, Copy, PartialEq, Eq)]
struct IdDigits {
    digits: [u8; 20],
    len: usize,
}

impl IdDigits {
    fn new(id: EntityId) -> Self {
        let mut rev = [0u8; 20];
        let mut n = id.get();
        let mut len = 0;
        loop {
            rev[len] = b"0123456789"[usize::try_from(n % 10).unwrap_or_default()];
            len += 1;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        let mut digits = [0u8; 20];
        for (d, r) in digits.iter_mut().zip(rev[..len].iter().rev()) {
            *d = *r;
        }
        Self { digits, len }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.digits[..self.len]
    }
}

impl Ord for IdDigits {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl PartialOrd for IdDigits {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// An `e:<id>` node id, written straight into the serializer with no owned string.
struct NodeIdRef(EntityId);

impl Serialize for NodeIdRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("e:{}", self.0.get()))
    }
}

/// [`Link`]'s wire shape over borrowed parts.
#[derive(Serialize)]
struct LinkRef<'a> {
    source: NodeIdRef,
    target: NodeIdRef,
    kind: &'a str,
    weight: u64,
}

/// The view of a borrowed world (the head, or an older world folded for `at`), serialized
/// without building [`WorldView`] (#216): nodes and links are written one at a time, each
/// [`Node`] built for its own element and dropped. Its bytes equal `serde_json::to_vec` of
/// [`world_view`] with the same epoch; the resident cost is the graph index and one id per node
/// and link, not a copy of every entity's attributes.
///
/// `lod=type` is small by construction (one node per type or hub), so it holds the owned
/// [`WorldView`].
pub struct HeadView<'w> {
    inner: HeadInner<'w>,
}

enum HeadInner<'w> {
    Owned(WorldView),
    Entities(EntityStream<'w>),
}

struct EntityStream<'w> {
    graph: Graph<'w>,
    epoch: Epoch,
    focus: Option<u64>,
    /// Resolved entity ids in `e:<id>` string order.
    nodes: Vec<EntityId>,
    /// (source, target, kind, weight) in (source, target) string order, then kind.
    links: Vec<(EntityId, EntityId, &'w str, u64)>,
}

impl<'w> HeadView<'w> {
    /// A view already projected, such as a memoised type summary.
    pub(crate) const fn from_view(view: WorldView) -> Self {
        Self {
            inner: HeadInner::Owned(view),
        }
    }

    /// Prepares the view of `world` at `params`, labelled `epoch`. Every error surfaces here,
    /// before a byte is written, so a caller can still answer with an error status.
    ///
    /// # Errors
    /// As [`world_view`].
    pub fn new(world: &'w World, params: &ViewParams, epoch: Epoch) -> Result<Self, QueryError> {
        check_links(params)?;
        let inner = match params.lod {
            Lod::Type => HeadInner::Owned(WorldView {
                epoch,
                ..world_view(world, params)?
            }),
            Lod::Entity => {
                let graph = Graph::new(world);
                let subset = graph.subset(params)?;
                let keep = |id: &EntityId| subset.as_ref().is_none_or(|s| s.contains(id));
                let mut nodes: Vec<EntityId> = graph
                    .members
                    .keys()
                    .copied()
                    .filter(|id| keep(id))
                    .collect();
                nodes.sort_by_cached_key(|&id| IdDigits::new(id));
                let mut links: Vec<(EntityId, EntityId, &'w str, u64)> = graph
                    .links
                    .iter()
                    .filter(|((s, t, _), _)| keep(s) && keep(t))
                    .map(|(&(s, t, kind), &w)| (s, t, kind, w))
                    .collect();
                links.sort_by_cached_key(|&(s, t, kind, _)| {
                    (IdDigits::new(s), IdDigits::new(t), kind)
                });
                HeadInner::Entities(EntityStream {
                    graph,
                    epoch,
                    focus: params.focus,
                    nodes,
                    links,
                })
            }
        };
        Ok(Self { inner })
    }
}

impl Serialize for HeadView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.inner {
            HeadInner::Owned(view) => view.serialize(serializer),
            HeadInner::Entities(stream) => stream.serialize(serializer),
        }
    }
}

struct StreamNodes<'a, 'w>(&'a EntityStream<'w>);
struct StreamLinks<'a, 'w>(&'a EntityStream<'w>);

impl Serialize for EntityStream<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Field for field, in `WorldView`'s declaration order.
        let world = self.graph.world;
        let mut s = serializer.serialize_struct("WorldView", 9)?;
        s.serialize_field("offset", &world.offset())?;
        s.serialize_field("epoch", &self.epoch)?;
        s.serialize_field("branch", ACTUAL_BRANCH)?;
        s.serialize_field("fold_version", &world.fold_version())?;
        s.serialize_field("hub_in_degree_cap", &world.hub_in_degree_cap())?;
        s.serialize_field("lod", &Lod::Entity)?;
        s.serialize_field("focus", &self.focus)?;
        s.serialize_field("nodes", &StreamNodes(self))?;
        s.serialize_field("links", &StreamLinks(self))?;
        s.end()
    }
}

impl Serialize for StreamNodes<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let graph = &self.0.graph;
        let mut seq = serializer.serialize_seq(Some(self.0.nodes.len()))?;
        for &id in &self.0.nodes {
            let members = graph.members.get(&id).map_or(&[][..], Vec::as_slice);
            seq.serialize_element(&graph.entity_node(id, members))?;
        }
        seq.end()
    }
}

impl Serialize for StreamLinks<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.links.len()))?;
        for &(s, t, kind, weight) in &self.0.links {
            seq.serialize_element(&LinkRef {
                source: NodeIdRef(s),
                target: NodeIdRef(t),
                kind,
                weight,
            })?;
        }
        seq.end()
    }
}

#[cfg(test)]
mod tests {
    use s2w_core::{NaturalKey, World, WorldEvent, fold};

    use super::{Lod, ViewParams, graph_view, type_view};

    const GOLDEN: &str = include_str!("../../../s2w-core/tests/fixtures/golden-fold-v1.json");

    fn observe(key: &str, entity_type: &str) -> WorldEvent {
        WorldEvent::EntityObserved {
            key: NaturalKey::new(key),
            entity_type: entity_type.to_owned(),
            attrs: std::collections::BTreeMap::new(),
        }
    }

    fn relate(from: &str, to: &str, kind: &str) -> WorldEvent {
        WorldEvent::RelationshipObserved {
            from: NaturalKey::new(from),
            to: NaturalKey::new(to),
            kind: kind.to_owned(),
        }
    }

    fn merge(survivor: &str, absorbed: &str) -> WorldEvent {
        WorldEvent::EntitiesMerged {
            survivor: NaturalKey::new(survivor),
            absorbed: NaturalKey::new(absorbed),
        }
    }

    /// Asserts the cheap full type view equals the `Graph` one, as values and as bytes.
    fn assert_same(world: &World, at: usize) {
        let params = ViewParams {
            lod: Lod::Type,
            ..ViewParams::default()
        };
        let (cheap, full) = (type_view(world), graph_view(world, &params).unwrap());
        assert_eq!(cheap, full, "type view differs at {at}");
        assert_eq!(
            serde_json::to_string(&cheap).unwrap(),
            serde_json::to_string(&full).unwrap(),
            "type view bytes differ at {at}"
        );
    }

    #[test]
    fn the_cheap_type_view_equals_the_graph_one_at_every_golden_offset() {
        let log: Vec<WorldEvent> = serde_json::from_str(GOLDEN).unwrap();
        for cap in 1..=3 {
            for at in 0..=log.len() {
                assert_same(&fold(World::with_hub_cap(cap), &log[..at]), at);
            }
        }
    }

    /// Hubs as link sources and targets, a hub relating to a hub, untyped entities, merges that
    /// collapse link endpoints, a merge that absorbs a hub (its refs re-resolve), a relationship
    /// observed before its target trips the cap, and a revoked merge, at every prefix.
    #[test]
    fn the_cheap_type_view_equals_the_graph_one_through_merges_and_hubs() {
        let log = [
            observe("p1", "user"),
            observe("p2", "user"),
            observe("p4", ""),
            observe("a", "page"),
            observe("a2", "page"),
            observe("c", "page"),
            relate("p1", "a", "on"),
            relate("p1", "c", "on"),
            relate("p2", "a", "on"),
            relate("p1", "b", "on"),
            relate("p2", "b", "at"),
            relate("a", "b", "on"),
            relate("a", "p1", "by"),
            relate("p3", "p1", "on"),
            relate("p4", "c", "on"),
            relate("p4", "b", "on"),
            relate("p4", "a", "on"),
            merge("a", "a2"),
            relate("a2", "c", "on"),
            merge("p1", "p2"),
            relate("p2", "c", "on"),
            merge("c", "b"),
            relate("p3", "a2", "on"),
            WorldEvent::MergeRevoked {
                survivor: NaturalKey::new("p1"),
                absorbed: NaturalKey::new("p2"),
            },
            relate("p2", "a", "at"),
        ];
        for cap in 1..=3 {
            for at in 0..=log.len() {
                assert_same(&fold(World::with_hub_cap(cap), &log[..at]), at);
            }
        }
    }
}
