//! The world as a d3 node/link graph at a level of detail, optionally around a focus entity.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use s2w_core::{AttrMap, EntityId, EntityState, World};
use serde::Serialize;
use serde::ser::Serializer;

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
pub(super) struct HubAgg {
    sources: BTreeSet<EntityId>,
    by_kind: BTreeMap<String, u64>,
    last_seen_offset: u64,
}

impl HubAgg {
    /// What a [`Node::Hub`] shows of the aggregate: the source set is only ever counted.
    pub(super) fn facts(&self) -> HubFacts {
        HubFacts {
            in_degree: u64::try_from(self.sources.len()).unwrap_or(u64::MAX),
            by_kind: self.by_kind.clone(),
            last_seen_offset: self.last_seen_offset,
        }
    }

    /// [`Self::facts`], moving the kind counts instead of cloning them.
    pub(super) fn into_facts(self) -> HubFacts {
        HubFacts {
            in_degree: u64::try_from(self.sources.len()).unwrap_or(u64::MAX),
            by_kind: self.by_kind,
            last_seen_offset: self.last_seen_offset,
        }
    }
}

/// A hub's [`Node::Hub`] fields beyond the entity's own.
#[derive(Clone)]
pub(super) struct HubFacts {
    in_degree: u64,
    by_kind: BTreeMap<String, u64>,
    last_seen_offset: u64,
}

/// A resolved entity's parts beyond its state: the node is a [`Node::Hub`] when `hub` is given,
/// else a [`Node::Entity`]. A missing state reads as an empty type and no attributes.
pub(super) struct NodeParts {
    /// Natural keys that resolve to the entity.
    pub(super) keys: Vec<String>,
    /// Ids merged into it, excluding itself.
    pub(super) members: Vec<EntityId>,
    /// Its relationships to hubs.
    pub(super) hub_refs: Vec<HubRef>,
    /// Present when the entity is a hub.
    pub(super) hub: Option<HubFacts>,
}

/// A resolved entity's node from its state and graph parts.
pub(super) fn entity_node(id: EntityId, state: Option<&EntityState>, parts: NodeParts) -> Node {
    let NodeParts {
        keys,
        members,
        hub_refs,
        hub,
    } = parts;
    let entity_type = state.map(|s| s.entity_type.clone()).unwrap_or_default();
    let attrs = state.map(|s| s.attrs.clone()).unwrap_or_default();
    match hub {
        Some(hub) => Node::Hub {
            id: node_id(id),
            entity: id,
            entity_type,
            keys,
            attrs,
            members,
            in_degree: hub.in_degree,
            by_kind: hub.by_kind,
            last_seen_offset: hub.last_seen_offset,
            hub_refs,
        },
        None => Node::Entity {
            id: node_id(id),
            entity: id,
            entity_type,
            keys,
            attrs,
            members,
            hub_refs,
        },
    }
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
pub(super) struct Graph<'w> {
    world: &'w World,
    pub(super) members: BTreeMap<EntityId, Vec<EntityId>>,
    // Key and kind strings borrow from the world: the whole map is built under the read guard,
    // and cloning every string doubled its size (#216). A streamed `/world` copies only what
    // it keeps before releasing the guard (`super::projection`).
    pub(super) keys: BTreeMap<EntityId, Vec<&'w str>>,
    pub(super) hubs: BTreeMap<EntityId, HubAgg>,
    pub(super) links: BTreeMap<(EntityId, EntityId, &'w str), u64>,
    hub_edges: BTreeSet<(EntityId, EntityId, &'w str)>,
    /// `hub_edges` grouped by source as (kind, hub), in `hub_edges` order, so a node's
    /// `hub_refs` is one lookup, not a scan.
    pub(super) hub_refs: BTreeMap<EntityId, Vec<(&'w str, EntityId)>>,
}

impl<'w> Graph<'w> {
    pub(super) fn new(world: &'w World) -> Self {
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
    pub(super) fn subset(
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

    fn entity_node(&self, id: EntityId, members: &[EntityId]) -> Node {
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

/// Projects `world` at `params`. Pure: the HTTP handler, `--json` and MCP all call this. The
/// view's `epoch` is 0 here; [`super::QueryState::view_at`] labels it with the served one.
///
/// # Errors
/// [`QueryError::UnknownEntity`] for an unknown focus, [`QueryError::HopsTooLarge`] past
/// [`MAX_HOPS`], [`QueryError::BadParameter`] for `links=none` with `lod=entity` or a focus.
#[expect(
    clippy::too_many_lines,
    reason = "one sequential pass whose steps share local state; splitting it is a follow-up refactor (s2w#156)"
)]
pub fn world_view(world: &World, params: &ViewParams) -> Result<WorldView, QueryError> {
    check_links(params)?;
    if params.links == LinkDetail::None {
        return Ok(type_summary(world));
    }
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
    WorldView {
        offset: world.offset(),
        epoch: Epoch::default(),
        branch: ACTUAL_BRANCH,
        fold_version: world.fold_version(),
        hub_in_degree_cap: world.hub_in_degree_cap(),
        lod: Lod::Type,
        focus: None,
        nodes: nodes.into_values().collect(),
        links: Vec::new(),
    }
}

/// The decimal digits of an entity id, ordered as its `e:<id>` node id string is: `e:10`
/// sorts before `e:2`. [`world_view`] orders nodes and links by those strings; a streamed view
/// walking `EntityId` order instead would serve different bytes (#216).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct IdDigits {
    digits: [u8; 20],
    len: usize,
}

impl IdDigits {
    pub(super) fn new(id: EntityId) -> Self {
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
pub(super) struct NodeIdRef(pub(super) EntityId);

impl Serialize for NodeIdRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("e:{}", self.0.get()))
    }
}

/// [`Link`]'s wire shape over borrowed parts.
#[derive(Serialize)]
pub(super) struct LinkRef<'a> {
    pub(super) source: NodeIdRef,
    pub(super) target: NodeIdRef,
    pub(super) kind: &'a str,
    pub(super) weight: u64,
}
