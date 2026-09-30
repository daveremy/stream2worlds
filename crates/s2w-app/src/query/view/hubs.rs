//! Hub aggregates and the entity-node builder both the graph and type paths share.

use std::collections::{BTreeMap, BTreeSet};

use s2w_core::{EntityId, EntityState, World};

use super::{HubRef, Node, node_id};

#[derive(Default)]
pub(in crate::query) struct HubAgg {
    sources: BTreeSet<EntityId>,
    by_kind: BTreeMap<String, u64>,
    last_seen_offset: u64,
}

impl HubAgg {
    /// What a [`Node::Hub`] shows of the aggregate: the source set is only ever counted.
    pub(in crate::query) fn facts(&self) -> HubFacts {
        HubFacts {
            in_degree: u64::try_from(self.sources.len()).unwrap_or(u64::MAX),
            by_kind: self.by_kind.clone(),
            last_seen_offset: self.last_seen_offset,
        }
    }

    /// [`Self::facts`], moving the kind counts instead of cloning them.
    pub(in crate::query) fn into_facts(self) -> HubFacts {
        HubFacts {
            in_degree: u64::try_from(self.sources.len()).unwrap_or(u64::MAX),
            by_kind: self.by_kind,
            last_seen_offset: self.last_seen_offset,
        }
    }
}

/// A hub's [`Node::Hub`] fields beyond the entity's own.
#[derive(Clone)]
pub(in crate::query) struct HubFacts {
    in_degree: u64,
    by_kind: BTreeMap<String, u64>,
    last_seen_offset: u64,
}

/// A resolved entity's parts beyond its state: the node is a [`Node::Hub`] when `hub` is given,
/// else a [`Node::Entity`]. A missing state reads as an empty type and no attributes.
pub(in crate::query) struct NodeParts {
    /// Natural keys that resolve to the entity.
    pub(in crate::query) keys: Vec<String>,
    /// Ids merged into it, excluding itself.
    pub(in crate::query) members: Vec<EntityId>,
    /// Its relationships to hubs.
    pub(in crate::query) hub_refs: Vec<HubRef>,
    /// Present when the entity is a hub.
    pub(in crate::query) hub: Option<HubFacts>,
}

/// A resolved entity's node from its state and graph parts.
pub(in crate::query) fn entity_node(
    id: EntityId,
    state: Option<&EntityState>,
    parts: NodeParts,
) -> Node {
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
pub(super) fn hub_aggregates(world: &World) -> BTreeMap<EntityId, HubAgg> {
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
