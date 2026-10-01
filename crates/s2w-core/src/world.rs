//! The world state and the fold over [`WorldEvent`]s.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attr_map::AttrMap;
use crate::event::{AttrValue, EntityId, NaturalKey, WorldEvent};

/// The version of the fold's semantics, stamped into every [`World`] it creates.
///
/// Bump it when a change to [`fold_one`] would fold the same log to a different world.
pub const FOLD_VERSION: u32 = 1;

/// FNV-1a 64 over the bytes of the human-owned golden fixtures, `tests/fixtures/golden-fold-v1.json`
/// followed by `tests/fixtures/golden-fold-v1.snapshot.json` (decision 0024).
///
/// A world snapshot is valid only for the fold that wrote it. A human-approved fold change edits
/// the golden snapshot, which changes this value (`tests/fixture_hash.rs` fails until it is
/// updated), so stored world snapshots invalidate even when [`FOLD_VERSION`] was not bumped.
pub const FOLD_FIXTURE_HASH: u64 = 0x4c02_9a2f_b104_1c8a;

/// The default in-degree cap: past this many distinct sources, a relationship to an entity
/// becomes an attribute of the source instead of an edge (research 0006, item 7).
pub const DEFAULT_HUB_IN_DEGREE_CAP: u64 = 10_000;

/// Which world a state belongs to. Only the actual world exists today; branches are a later
/// gate, and the field exists now so they need no wire-format migration.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorldId(u64);

impl WorldId {
    /// The world as the log says it is.
    pub const ACTUAL: Self = Self(0);
}

/// What the fold knows about one entity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityState {
    /// The type from the latest observation; empty if the entity was only named by a
    /// relationship.
    pub entity_type: String,
    /// Attributes; each key holds its latest observed value.
    pub attrs: AttrMap,
    /// Relationships this entity has to hub entities, by kind, kept as attributes because the
    /// target is past the in-degree cap. The value is the hub's id. `None` when there are none
    /// (nearly every entity), never `Some` of an empty map; on the wire it is a plain map.
    #[serde(with = "crate::wire::hub_refs")]
    #[expect(
        clippy::box_collection,
        reason = "8 B inline for the empty case, not 24 (s2w#190)"
    )]
    hub_refs: Option<Box<BTreeMap<String, EntityId>>>,
}

// One entity's inline cost, times every entity (s2w#190). A new field must earn its bytes.
const _: () = assert!(size_of::<EntityState>() <= 56);

impl EntityState {
    /// Relationships to hub entities, `(kind, hub id)` in kind order.
    pub fn hub_refs(&self) -> impl Iterator<Item = (&str, EntityId)> {
        self.hub_refs
            .iter()
            .flat_map(|refs| refs.iter())
            .map(|(kind, &hub)| (kind.as_str(), hub))
    }

    /// The hub this entity points at under `kind`, if the relationship was folded into a ref.
    #[must_use]
    pub fn hub_ref(&self, kind: &str) -> Option<EntityId> {
        self.hub_refs.as_ref()?.get(kind).copied()
    }

    /// Whether this entity holds any hub ref.
    #[must_use]
    pub const fn has_hub_refs(&self) -> bool {
        self.hub_refs.is_some()
    }

    /// Whether [`observe`](Self::observe) with these arguments would change this state. Exact:
    /// `false` only when the write would leave the state `==` to what it was.
    fn observation_changes(&self, entity_type: &str, attrs: &BTreeMap<String, AttrValue>) -> bool {
        self.entity_type != entity_type || self.attrs.changes(attrs)
    }

    /// An entity observation: the latest type wins, and each attribute holds its latest value.
    fn observe(&mut self, entity_type: &str, attrs: &BTreeMap<String, AttrValue>) {
        entity_type.clone_into(&mut self.entity_type);
        self.attrs.extend_from_map(attrs);
    }
}

/// A materialized edge. Endpoints are the ids resolved when the relationship was observed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Relationship {
    /// The source endpoint.
    pub from: EntityId,
    /// The target endpoint.
    pub to: EntityId,
    /// The relationship kind.
    pub kind: String,
}

/// Counters for one relationship target, kept whether or not its edges are materialized.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubCounters {
    /// Distinct source ids seen pointing here. Its length is the in-degree the cap tests.
    pub sources: BTreeSet<EntityId>,
    /// Observations by relationship kind (a repeated relationship counts every time).
    pub by_kind: BTreeMap<String, u64>,
    /// The world offset after the latest relationship observed pointing here.
    pub last_seen_offset: u64,
}

impl HubCounters {
    /// The number of distinct sources seen pointing here.
    #[must_use]
    pub fn in_degree(&self) -> usize {
        self.sources.len()
    }
}

/// The world: everything the fold has concluded from the events so far.
///
/// Fields are read through accessors so only the fold can change them. The whole state
/// serializes, so a fold can resume from a serialized prefix (decision 0005).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    world_id: WorldId,
    offset: u64,
    fold_version: u32,
    hub_in_degree_cap: u64,
    next_entity_id: u64,
    keys: BTreeMap<NaturalKey, EntityId>,
    merges: BTreeMap<EntityId, EntityId>,
    /// Indexed by id: ids are dense `0..len` (minted in order by `mint`, never deleted).
    /// Each state sits behind an `Arc`, so a cloned world shares every state until one side
    /// writes it, and a write copies only that entity (`Arc::make_mut`; decision 0028).
    #[serde(with = "crate::wire::entities")]
    entities: Vec<Arc<EntityState>>,
    #[serde(with = "crate::wire::relationships")]
    relationships: BTreeMap<Relationship, u64>,
    hub_counters: BTreeMap<EntityId, HubCounters>,
}

impl Default for World {
    fn default() -> Self {
        Self::with_hub_cap(DEFAULT_HUB_IN_DEGREE_CAP)
    }
}

impl World {
    /// An empty world with a given hub in-degree cap. Tests and the golden fixture use a small
    /// cap so a hub fits in a human-readable log; production uses [`World::default`].
    #[must_use]
    pub fn with_hub_cap(cap: u64) -> Self {
        Self {
            world_id: WorldId::ACTUAL,
            offset: 0,
            fold_version: FOLD_VERSION,
            hub_in_degree_cap: cap,
            next_entity_id: 0,
            keys: BTreeMap::new(),
            merges: BTreeMap::new(),
            entities: Vec::new(),
            relationships: BTreeMap::new(),
            hub_counters: BTreeMap::new(),
        }
    }

    /// Which world this is.
    #[must_use]
    pub const fn world_id(&self) -> WorldId {
        self.world_id
    }

    /// How many [`WorldEvent`]s have been folded, no-ops included. Not a log position.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// The [`FOLD_VERSION`] of the fold that created this world.
    #[must_use]
    pub const fn fold_version(&self) -> u32 {
        self.fold_version
    }

    /// The in-degree past which relationships to an entity become attributes.
    #[must_use]
    pub const fn hub_in_degree_cap(&self) -> u64 {
        self.hub_in_degree_cap
    }

    /// Natural key to id. Write-once: an entry is never removed or changed.
    #[must_use]
    pub const fn keys(&self) -> &BTreeMap<NaturalKey, EntityId> {
        &self.keys
    }

    /// Outstanding merges, absorbed id to survivor id, exactly as each merge named them.
    #[must_use]
    pub const fn merges(&self) -> &BTreeMap<EntityId, EntityId> {
        &self.merges
    }

    /// Every entity ever minted, in id order. Merges never delete one.
    pub fn entities(&self) -> impl ExactSizeIterator<Item = (EntityId, &EntityState)> {
        self.entities
            .iter()
            .enumerate()
            .map(|(index, state)| (id_at(index), &**state))
    }

    /// The entity minted as `id`, if it has been.
    #[must_use]
    pub fn entity(&self, id: EntityId) -> Option<&EntityState> {
        self.entity_arc(id).map(|state| &**state)
    }

    /// The shared handle to the entity minted as `id`, if it has been. A reader that clones it
    /// keeps that state as it is now: the fold never mutates a state another handle holds, it
    /// copies it first (decision 0028).
    #[must_use]
    pub fn entity_arc(&self, id: EntityId) -> Option<&Arc<EntityState>> {
        self.entities.get(index_of(id)?)
    }

    /// How many entities have been minted.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// `raw` as an id, if an entity was minted with it (an O(1) range check).
    #[must_use]
    pub fn minted_id(&self, raw: u64) -> Option<EntityId> {
        usize::try_from(raw)
            .is_ok_and(|index| index < self.entities.len())
            .then(|| EntityId::new(raw))
    }

    /// The id the next minted entity will get.
    #[must_use]
    pub const fn next_entity_id(&self) -> u64 {
        self.next_entity_id
    }

    /// Copies the state first if another handle shares it. Call it only for a write that
    /// changes the state: a no-op write through it would still copy (decision 0028).
    fn entity_mut(&mut self, id: EntityId) -> Option<&mut EntityState> {
        self.entities.get_mut(index_of(id)?).map(Arc::make_mut)
    }

    /// Materialized relationships and how many times each was observed.
    #[must_use]
    pub const fn relationships(&self) -> &BTreeMap<Relationship, u64> {
        &self.relationships
    }

    /// Per-target counters, including targets past the hub cap.
    #[must_use]
    pub const fn hub_counters(&self) -> &BTreeMap<EntityId, HubCounters> {
        &self.hub_counters
    }

    /// The id a key was minted as, if the key has been seen.
    #[must_use]
    pub fn id_of(&self, key: &NaturalKey) -> Option<EntityId> {
        self.keys.get(key).copied()
    }

    /// Follows merges from `id` to the entity it is currently part of.
    ///
    /// The walk is capped at `merges.len()` steps. The fold never stores a cycle, but a
    /// deserialized world is not trusted to be well formed, and a read must terminate.
    #[must_use]
    pub fn resolve(&self, id: EntityId) -> EntityId {
        let mut current = id;
        for _ in 0..self.merges.len() {
            match self.merges.get(&current) {
                Some(&next) => current = next,
                None => break,
            }
        }
        current
    }

    /// The key's id, minting one on first mention.
    ///
    /// `None` only for a deserialized world whose id counter is not its entity count: the id is
    /// the index `push` gives, so such a world mints nothing (the fold stays total, and every
    /// mint in one event fails alike, so none leaves half its state). The id space itself (the
    /// `checked_add`) cannot run out first: that needs 2^64 entities in memory.
    fn mint(&mut self, key: &NaturalKey) -> Option<EntityId> {
        if let Some(&id) = self.keys.get(key) {
            return Some(id);
        }
        if index_of(EntityId::new(self.next_entity_id)) != Some(self.entities.len()) {
            return None;
        }
        let id = EntityId::new(self.next_entity_id);
        self.next_entity_id = self.next_entity_id.checked_add(1)?;
        self.keys.insert(key.clone(), id);
        self.entities.push(Arc::default());
        Some(id)
    }

    fn observe_entity(
        &mut self,
        key: &NaturalKey,
        entity_type: &str,
        attrs: &BTreeMap<String, AttrValue>,
    ) {
        let Some(id) = self.mint(key) else {
            return;
        };
        let target = self.resolve(id);
        // Always minted in a fold-produced world; a snapshot pointing past its entities no-ops.
        let Some(state) = self.entity(target) else {
            return;
        };
        // A write that changes nothing must not reach `entity_mut`, which copies a shared state.
        if !state.observation_changes(entity_type, attrs) {
            return;
        }
        if let Some(state) = self.entity_mut(target) {
            state.observe(entity_type, attrs);
        }
    }

    fn observe_relationship(&mut self, from: &NaturalKey, to: &NaturalKey, kind: &str) {
        // Both mints succeed or both fail (see `mint`), so a failure leaves no half state.
        let (Some(from_id), Some(to_id)) = (self.mint(from), self.mint(to)) else {
            return;
        };
        let from_r = self.resolve(from_id);
        let to_r = self.resolve(to_id);

        let cap = self.hub_in_degree_cap;
        let offset = self.offset;
        let counters = self.hub_counters.entry(to_r).or_default();
        counters.sources.insert(from_r);
        let count = counters.by_kind.entry(kind.to_owned()).or_insert(0);
        // Bounded by `offset`, itself a u64, so this cannot saturate in practice.
        *count = count.saturating_add(1);
        counters.last_seen_offset = offset;
        let over_cap = u64::try_from(counters.sources.len()).map_or(true, |n| n > cap);

        if over_cap {
            // Skip a ref the entity already holds, so the write does not copy a shared state.
            let held = self
                .entity(from_r)
                .is_some_and(|state| state.hub_ref(kind) == Some(to_r));
            if !held && let Some(state) = self.entity_mut(from_r) {
                state
                    .hub_refs
                    .get_or_insert_with(Box::default)
                    .insert(kind.to_owned(), to_r);
            }
        } else {
            let weight = self
                .relationships
                .entry(Relationship {
                    from: from_r,
                    to: to_r,
                    kind: kind.to_owned(),
                })
                .or_insert(0);
            *weight = weight.saturating_add(1);
        }
    }

    fn merge(&mut self, survivor: &NaturalKey, absorbed: &NaturalKey) {
        let (Some(s), Some(a)) = (self.id_of(survivor), self.id_of(absorbed)) else {
            return; // Unknown key: nothing to merge.
        };
        if self.merges.contains_key(&a) {
            return; // One outstanding merge per absorbed id: revoke it first.
        }
        // `a` has no outgoing edge, so it resolves to itself. If the survivor already resolves
        // to `a` this is a self-merge, an existing merge, or a cycle: all no-ops.
        if self.resolve(s) == self.resolve(a) {
            return;
        }
        self.merges.insert(a, s);
    }

    fn revoke(&mut self, survivor: &NaturalKey, absorbed: &NaturalKey) {
        let (Some(s), Some(a)) = (self.id_of(survivor), self.id_of(absorbed)) else {
            return;
        };
        // Only the exact raw edge a merge recorded can be revoked, never a resolved one.
        if self.merges.get(&a) == Some(&s) {
            self.merges.remove(&a);
        }
    }
}

/// Folds one event into the world. Pure and total: it never panics, and an event it cannot
/// apply (an unknown key, a cyclic merge, an exhausted id space) is a no-op that still counts
/// towards [`World::offset`]. The one exception: once `offset` is `u64::MAX`, events are
/// dropped without advancing it.
#[must_use]
pub fn fold_one(world: World, event: &WorldEvent) -> World {
    let mut world = world;
    let Some(offset) = world.offset.checked_add(1) else {
        return world; // u64::MAX events folded: nothing further can be counted.
    };
    world.offset = offset;
    match event {
        WorldEvent::EntityObserved {
            key,
            entity_type,
            attrs,
        } => world.observe_entity(key, entity_type, attrs),
        WorldEvent::RelationshipObserved { from, to, kind } => {
            world.observe_relationship(from, to, kind);
        }
        WorldEvent::EntitiesMerged { survivor, absorbed } => world.merge(survivor, absorbed),
        WorldEvent::MergeRevoked { survivor, absorbed } => world.revoke(survivor, absorbed),
    }
    world
}

/// Folds a sequence of events into the world, one at a time.
#[must_use]
pub fn fold<'a>(world: World, events: impl IntoIterator<Item = &'a WorldEvent>) -> World {
    events.into_iter().fold(world, fold_one)
}

/// The id of the entity at `index` in `World::entities`.
pub(crate) fn id_at(index: usize) -> EntityId {
    // usize is at most 64 bits on every supported target.
    EntityId::new(u64::try_from(index).unwrap_or(u64::MAX))
}

/// The index of `id` in `World::entities`, if it fits a usize.
fn index_of(id: EntityId) -> Option<usize> {
    usize::try_from(id.get()).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::EntityState;
    use crate::attr_map::AttrMap;
    use crate::event::AttrValue;

    fn arb_value() -> impl Strategy<Value = AttrValue> {
        prop_oneof![
            (0..3i64).prop_map(AttrValue::Int),
            any::<bool>().prop_map(AttrValue::Bool),
            prop::sample::select(&["", "a", "b"][..]).prop_map(|s| AttrValue::Str(s.to_owned())),
        ]
    }

    fn arb_attrs() -> impl Strategy<Value = BTreeMap<String, AttrValue>> {
        prop::collection::btree_map(
            prop::sample::select(&["a", "b", "c", "d"][..]).prop_map(str::to_owned),
            arb_value(),
            0..4,
        )
    }

    fn arb_type() -> impl Strategy<Value = String> {
        prop::sample::select(&["", "user", "page"][..]).prop_map(str::to_owned)
    }

    proptest! {
        /// The fold skips an observation exactly when writing it would change nothing, so the
        /// skip changes when a shared state is copied, never the folded value (s2w#271).
        #[test]
        fn an_observation_is_skipped_exactly_when_it_would_change_nothing(
            state_type in arb_type(),
            state_attrs in arb_attrs(),
            entity_type in arb_type(),
            attrs in arb_attrs(),
        ) {
            let mut stored = AttrMap::default();
            stored.extend_from_map(&state_attrs);
            let state = EntityState { entity_type: state_type, attrs: stored, hub_refs: None };
            let mut written = state.clone();
            written.observe(&entity_type, &attrs);
            prop_assert_eq!(state.observation_changes(&entity_type, &attrs), written != state);
        }
    }
}
