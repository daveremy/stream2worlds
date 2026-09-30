//! A full `/world` view that owns what it serves (s2w#272, decision 0028 part B).
//!
//! [`Projection::capture`] runs under the timeline's read guard and copies out only what the
//! body needs: the header scalars, each kept node's keys and hub references, the kept links
//! with their kinds interned, and each kept node's shared `Arc<EntityState>`. Nothing in it
//! borrows the world, so the caller releases the guard before [`Projection::prepare`] sorts it
//! and before the body is written: the fold never waits on a sort or on a client. The states
//! are immutable behind their `Arc`s (the fold copies on write), so the body still describes the
//! one (epoch, offset) it was captured at.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use s2w_core::{EntityId, EntityState, World};
use serde::Serialize;
use serde::ser::{SerializeSeq, SerializeStruct, Serializer};

use super::QueryError;
use super::epoch::Epoch;
use super::view::{
    ACTUAL_BRANCH, Graph, HubFacts, HubRef, IdDigits, LinkRef, Lod, NodeIdRef, NodeParts,
    ViewParams, WorldView, check_links, entity_node, node_id, world_view,
};

/// A relationship kind's index into [`Entities::kinds`]. The table is sorted, so ids order
/// exactly as the kind strings do.
type KindId = u32;

/// (source, target, kind, weight).
type CapturedLink = (EntityId, EntityId, KindId, u64);

/// What the read guard hands over: a view that borrows nothing, not yet sorted.
pub struct Projection {
    inner: Inner,
}

/// The view [`Projection::prepare`] makes: sorted, owned, ready to serialize. Its bytes equal
/// `serde_json::to_vec` of [`world_view`] with the same epoch. Each [`Node`](super::Node) is
/// built for its own element and dropped, so the body never holds a copy of every entity's
/// attributes.
pub struct HeadView {
    inner: Inner,
}

enum Inner {
    /// `lod=type` and the type summary: small by construction (one node per type or hub).
    Owned(WorldView),
    Entities(Entities),
}

struct Entities {
    offset: u64,
    fold_version: u32,
    hub_in_degree_cap: u64,
    epoch: Epoch,
    focus: Option<u64>,
    /// The world's entity count: resolved ids are below it in a folded world.
    entity_count: usize,
    kinds: Vec<Box<str>>,
    /// Kept resolved entities; in `e:<id>` string order once prepared.
    nodes: Vec<CapturedNode>,
    /// Kept links; in (source, target) string order, then kind, once prepared.
    links: Vec<CapturedLink>,
}

struct CapturedNode {
    id: EntityId,
    members: Vec<EntityId>,
    /// Shared with the world until the fold replaces it.
    state: Option<Arc<EntityState>>,
    keys: Vec<Box<str>>,
    /// (kind, hub) in the graph's order.
    hub_refs: Vec<(KindId, EntityId)>,
    hub: Option<HubFacts>,
}

impl Projection {
    /// Captures the view of `world` at `params`, labelled `epoch`. The only step that reads the
    /// world: run it under the read guard, then release the guard. Every error surfaces here,
    /// before a byte is written, so a caller can still answer with an error status.
    ///
    /// # Errors
    /// As [`world_view`].
    pub fn capture(world: &World, params: &ViewParams, epoch: Epoch) -> Result<Self, QueryError> {
        check_links(params)?;
        let inner = match params.lod {
            // The type view's cost is its build, which needs the world; its body is small.
            Lod::Type => Inner::Owned(WorldView {
                epoch,
                ..world_view(world, params)?
            }),
            Lod::Entity => Inner::Entities(Entities::capture(world, params, epoch)?),
        };
        Ok(Self { inner })
    }

    /// A view already projected, such as a memoised type summary.
    pub(crate) const fn from_view(view: WorldView) -> Self {
        Self {
            inner: Inner::Owned(view),
        }
    }

    /// Sorts the captured view. Reads nothing from the world: run it after releasing the guard.
    #[must_use]
    pub fn prepare(self) -> HeadView {
        let inner = match self.inner {
            Inner::Owned(view) => Inner::Owned(view),
            Inner::Entities(mut entities) => {
                entities.sort();
                Inner::Entities(entities)
            }
        };
        HeadView { inner }
    }
}

impl HeadView {
    /// [`Projection::capture`] then [`Projection::prepare`], for a caller that holds the world
    /// for the whole body anyway.
    ///
    /// # Errors
    /// As [`world_view`].
    pub fn new(world: &World, params: &ViewParams, epoch: Epoch) -> Result<Self, QueryError> {
        Ok(Projection::capture(world, params, epoch)?.prepare())
    }

    /// Entity states this view holds that the world no longer shares: the fold replaced them
    /// after the capture, so each is a second copy alive for as long as this view. Meaningful
    /// for a view of the head only; for a world dropped after the capture (an `at` below the
    /// head, a replaced timeline) it counts every state.
    #[must_use]
    pub fn diverged(&self) -> u64 {
        let Inner::Entities(entities) = &self.inner else {
            return 0;
        };
        let count = entities
            .nodes
            .iter()
            .filter_map(|node| node.state.as_ref())
            .filter(|state| Arc::strong_count(state) == 1)
            .count();
        u64::try_from(count).unwrap_or(u64::MAX)
    }
}

impl Entities {
    fn capture(world: &World, params: &ViewParams, epoch: Epoch) -> Result<Self, QueryError> {
        let graph = Graph::new(world);
        let subset = graph.subset(params)?;
        let keep = |id: &EntityId| subset.as_ref().is_none_or(|s| s.contains(id));
        let Graph {
            members,
            keys,
            mut hubs,
            links,
            hub_refs,
            ..
        } = graph;
        let mut kept_kinds: BTreeSet<&str> = BTreeSet::new();
        for (s, t, kind) in links.keys() {
            if keep(s) && keep(t) {
                kept_kinds.insert(kind);
            }
        }
        for (_, refs) in hub_refs.iter().filter(|(id, _)| keep(id)) {
            kept_kinds.extend(refs.iter().map(|&(kind, _)| kind));
        }
        let kind_ids: BTreeMap<&str, KindId> = kept_kinds
            .iter()
            .zip(0..)
            .map(|(&kind, id)| (kind, id))
            .collect();
        let kind_id = |kind: &str| kind_ids.get(kind).copied().unwrap_or(KindId::MAX);
        let nodes = members
            .into_iter()
            .filter(|(id, _)| keep(id))
            .map(|(id, members)| CapturedNode {
                id,
                members,
                state: world.entity_arc(id).cloned(),
                keys: keys
                    .get(&id)
                    .map(|keys| keys.iter().map(|&k| Box::from(k)).collect())
                    .unwrap_or_default(),
                hub_refs: hub_refs
                    .get(&id)
                    .map(|refs| refs.iter().map(|&(k, hub)| (kind_id(k), hub)).collect())
                    .unwrap_or_default(),
                hub: hubs.remove(&id).as_ref().map(super::view::HubAgg::facts),
            })
            .collect();
        let links = links
            .iter()
            .filter(|((s, t, _), _)| keep(s) && keep(t))
            .map(|(&(s, t, kind), &w)| (s, t, kind_id(kind), w))
            .collect();
        Ok(Self {
            offset: world.offset(),
            fold_version: world.fold_version(),
            hub_in_degree_cap: world.hub_in_degree_cap(),
            epoch,
            focus: params.focus,
            entity_count: world.entity_count(),
            kinds: kept_kinds.into_iter().map(Box::from).collect(),
            nodes,
            links,
        })
    }

    fn sort(&mut self) {
        // Ids are unique, so an unstable sort is deterministic.
        self.nodes
            .sort_unstable_by_key(|node| IdDigits::new(node.id));
        let order: Vec<EntityId> = self.nodes.iter().map(|node| node.id).collect();
        sort_links(&order, self.entity_count, &mut self.links);
    }
}

/// Sorts `links` by (source, target) in `e:<id>` string order, then kind, given the node ids
/// already in that order. Each id's position in `order` is its rank, so the sort compares
/// integers instead of caching two digit strings per link. A link whose endpoint has no rank
/// (only possible in a world whose merges were not produced by the fold) falls back to the
/// digit keys; both orders are the same.
fn sort_links(order: &[EntityId], entity_count: usize, links: &mut [CapturedLink]) {
    let slot = |id: EntityId| usize::try_from(id.get()).ok().filter(|&i| i < entity_count);
    const UNRANKED: u32 = u32::MAX;
    let mut rank = vec![UNRANKED; entity_count];
    let mut ranked = true;
    for (id, position) in order.iter().zip(0..UNRANKED) {
        match slot(*id) {
            Some(i) => rank[i] = position,
            None => ranked = false,
        }
    }
    // Past `u32::MAX - 1` nodes the zip stops early and the rest stay unranked.
    ranked &= order.len() < usize::try_from(UNRANKED).unwrap_or(usize::MAX);
    let rank_of = |id: EntityId| slot(id).map(|i| rank[i]).filter(|&r| r != UNRANKED);
    if ranked
        && links
            .iter()
            .all(|&(s, t, _, _)| rank_of(s).is_some() && rank_of(t).is_some())
    {
        // (source, target, kind) is unique per link, so an unstable sort is deterministic.
        links.sort_unstable_by_key(|&(s, t, kind, _)| (rank_of(s), rank_of(t), kind));
    } else {
        links.sort_by_cached_key(|&(s, t, kind, _)| (IdDigits::new(s), IdDigits::new(t), kind));
    }
}

impl Serialize for HeadView {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.inner {
            Inner::Owned(view) => view.serialize(serializer),
            Inner::Entities(entities) => entities.serialize(serializer),
        }
    }
}

struct Nodes<'a>(&'a Entities);
struct Links<'a>(&'a Entities);

impl Serialize for Entities {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Field for field, in `WorldView`'s declaration order.
        let mut s = serializer.serialize_struct("WorldView", 9)?;
        s.serialize_field("offset", &self.offset)?;
        s.serialize_field("epoch", &self.epoch)?;
        s.serialize_field("branch", ACTUAL_BRANCH)?;
        s.serialize_field("fold_version", &self.fold_version)?;
        s.serialize_field("hub_in_degree_cap", &self.hub_in_degree_cap)?;
        s.serialize_field("lod", &Lod::Entity)?;
        s.serialize_field("focus", &self.focus)?;
        s.serialize_field("nodes", &Nodes(self))?;
        s.serialize_field("links", &Links(self))?;
        s.end()
    }
}

impl Entities {
    fn kind(&self, id: KindId) -> &str {
        usize::try_from(id)
            .ok()
            .and_then(|i| self.kinds.get(i))
            .map_or("", |kind| kind)
    }
}

impl Serialize for Nodes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entities = self.0;
        let mut seq = serializer.serialize_seq(Some(entities.nodes.len()))?;
        for node in &entities.nodes {
            let hub_refs = node
                .hub_refs
                .iter()
                .map(|&(kind, hub)| HubRef {
                    kind: entities.kind(kind).to_owned(),
                    hub: node_id(hub),
                })
                .collect();
            let parts = NodeParts {
                keys: node.keys.iter().map(|k| k.to_string()).collect(),
                members: node.members.clone(),
                hub_refs,
                hub: node.hub.clone(),
            };
            seq.serialize_element(&entity_node(node.id, node.state.as_deref(), parts))?;
        }
        seq.end()
    }
}

impl Serialize for Links<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entities = self.0;
        let mut seq = serializer.serialize_seq(Some(entities.links.len()))?;
        for &(s, t, kind, weight) in &entities.links {
            seq.serialize_element(&LinkRef {
                source: NodeIdRef(s),
                target: NodeIdRef(t),
                kind: entities.kind(kind),
                weight,
            })?;
        }
        seq.end()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;
    use s2w_core::{NaturalKey, WorldEvent};
    use s2w_model::{AttrValue, Timestamp};

    use super::*;
    use crate::query::http::QueryState;
    use crate::query::timeline::Timeline;

    fn observed(key: &str, entity_type: &str, value: &str) -> WorldEvent {
        WorldEvent::EntityObserved {
            key: NaturalKey::new(key),
            entity_type: entity_type.into(),
            attrs: BTreeMap::from([("v".to_owned(), AttrValue::Str(value.to_owned()))]),
        }
    }

    fn related(from: &str, to: &str, kind: &str) -> WorldEvent {
        WorldEvent::RelationshipObserved {
            from: NaturalKey::new(from),
            to: NaturalKey::new(to),
            kind: kind.into(),
        }
    }

    fn body(view: &impl Serialize) -> Vec<u8> {
        serde_json::to_vec(view).expect("serialize")
    }

    /// The view the world at the head serves, labelled with the default epoch.
    fn head_bytes(state: &QueryState, params: &ViewParams) -> Vec<u8> {
        let world = state.world_at(None).expect("head");
        body(&WorldView {
            epoch: Epoch::default(),
            ..world_view(&world, params).expect("view")
        })
    }

    /// The copy-on-write pin: a captured view shares entity states with the world, and the
    /// fold replaces a state rather than mutating it, so appends made after the capture (the
    /// guard is already released) never reach the body.
    #[test]
    fn a_body_is_unchanged_by_appends_made_after_its_capture() {
        let mut timeline = Timeline::new(3);
        let events = (0..12)
            .map(|n| observed(&format!("e{n}"), "thing", "before"))
            .chain((0..11).map(|n| related(&format!("e{n}"), &format!("e{}", n + 1), "next")))
            .chain([related("e0", "e11", "loop")]);
        for (n, event) in (0_i64..).zip(events) {
            timeline.append(Timestamp::from_millis(n), event);
        }
        let state = QueryState::new(timeline);
        let params = ViewParams::default();
        let expected = head_bytes(&state, &params);
        let projection = state
            .with_head(|world, _| Projection::capture(world, &params, Epoch::default()))
            .expect("head")
            .expect("capture");
        let changes = [
            observed("e1", "thing", "after"),
            observed("e2", "other", "after"),
            related("e3", "e9", "late"),
            WorldEvent::EntitiesMerged {
                survivor: NaturalKey::new("e4"),
                absorbed: NaturalKey::new("e5"),
            },
            observed("e12", "thing", "new"),
        ];
        for (n, event) in (100_i64..).zip(changes) {
            state
                .append(Timestamp::from_millis(n), event)
                .expect("append");
        }
        assert_ne!(
            head_bytes(&state, &params),
            expected,
            "the appends change the head's view"
        );
        let view = projection.prepare();
        assert!(view.diverged() > 0, "the fold replaced captured states");
        assert_eq!(body(&view), expected, "the body is the view at its capture");
    }

    fn id(raw: u64) -> EntityId {
        serde_json::from_value(raw.into()).expect("an id")
    }

    fn digit_order(links: &mut [CapturedLink]) {
        links.sort_by_cached_key(|&(s, t, kind, _)| (IdDigits::new(s), IdDigits::new(t), kind));
    }

    proptest! {
        /// The rank sort orders links exactly as the `e:<id>` string sort does, with or
        /// without endpoints the rank table does not cover.
        #[test]
        fn the_rank_sort_matches_the_string_sort(
            raw in prop::collection::btree_set(
                prop_oneof![0_u64..10, 10_u64..100, 100_u64..1_000, 1_000_u64..20_000],
                1..60,
            ),
            picks in prop::collection::vec((any::<prop::sample::Index>(),
                any::<prop::sample::Index>(), 0_u32..3, any::<u64>()), 0..200),
            unranked in any::<bool>(),
        ) {
            let ids: Vec<EntityId> = raw.iter().copied().map(id).collect();
            let mut order = ids.clone();
            order.sort_unstable_by_key(|&id| IdDigits::new(id));
            // Every id below the count is ranked; `unranked` drops the largest from the table.
            let largest = raw.iter().copied().max().unwrap_or(0);
            let entity_count = usize::try_from(largest).expect("small") + usize::from(!unranked);
            let mut seen = BTreeSet::new();
            let mut links: Vec<CapturedLink> = picks
                .iter()
                .map(|(s, t, kind, w)| (*s.get(&ids), *t.get(&ids), *kind, *w))
                .filter(|&(s, t, kind, _)| seen.insert((s, t, kind)))
                .collect();
            let mut expected = links.clone();
            digit_order(&mut expected);
            sort_links(&order, entity_count, &mut links);
            prop_assert_eq!(links, expected);
        }
    }
}
