//! The world state and the fold over [`WorldEvent`]s.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::event::{AttrValue, EntityId, NaturalKey, WorldEvent};

/// The version of the fold's semantics, stamped into every [`World`] it creates.
///
/// Bump it when a change to [`fold_one`] would fold the same log to a different world.
pub const FOLD_VERSION: u32 = 1;

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
    pub attrs: BTreeMap<String, AttrValue>,
    /// Relationships this entity has to hub entities, by kind, kept as attributes because the
    /// target is past the in-degree cap. The value is the hub's id.
    pub hub_refs: BTreeMap<String, EntityId>,
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
/// serializes, so a fold can resume from a serialized prefix (decision 0004).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    world_id: WorldId,
    offset: u64,
    fold_version: u32,
    hub_in_degree_cap: u64,
    next_entity_id: u64,
    keys: BTreeMap<NaturalKey, EntityId>,
    merges: BTreeMap<EntityId, EntityId>,
    entities: BTreeMap<EntityId, EntityState>,
    #[serde(with = "relationships_wire")]
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
            entities: BTreeMap::new(),
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

    /// Every entity ever minted. Merges never delete one.
    #[must_use]
    pub const fn entities(&self) -> &BTreeMap<EntityId, EntityState> {
        &self.entities
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

    /// The key's id, minting one on first mention. `None` only if the id space is exhausted.
    fn mint(&mut self, key: &NaturalKey) -> Option<EntityId> {
        if let Some(&id) = self.keys.get(key) {
            return Some(id);
        }
        let id = EntityId::new(self.next_entity_id);
        self.next_entity_id = self.next_entity_id.checked_add(1)?;
        self.keys.insert(key.clone(), id);
        self.entities.insert(id, EntityState::default());
        Some(id)
    }

    /// Whether `n` more ids can be minted. The id `u64::MAX` is never assigned.
    fn can_mint(&self, n: u64) -> bool {
        self.next_entity_id.checked_add(n).is_some()
    }

    fn unknown(&self, keys: &[&NaturalKey]) -> u64 {
        let mut distinct: BTreeSet<&NaturalKey> = BTreeSet::new();
        for k in keys {
            if !self.keys.contains_key(*k) {
                distinct.insert(k);
            }
        }
        u64::try_from(distinct.len()).unwrap_or(u64::MAX)
    }

    fn observe_entity(
        &mut self,
        key: &NaturalKey,
        entity_type: &str,
        attrs: &BTreeMap<String, AttrValue>,
    ) {
        if !self.can_mint(self.unknown(&[key])) {
            return;
        }
        let Some(id) = self.mint(key) else {
            return;
        };
        let target = self.resolve(id);
        let state = self.entities.entry(target).or_default();
        entity_type.clone_into(&mut state.entity_type);
        for (k, v) in attrs {
            state.attrs.insert(k.clone(), v.clone());
        }
    }

    fn observe_relationship(&mut self, from: &NaturalKey, to: &NaturalKey, kind: &str) {
        if !self.can_mint(self.unknown(&[from, to])) {
            return;
        }
        // Capacity for both endpoints was checked above, so neither mint can fail alone.
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
            self.entities
                .entry(from_r)
                .or_default()
                .hub_refs
                .insert(kind.to_owned(), to_r);
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
/// towards [`World::offset`].
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

/// `relationships` is a struct-keyed map in memory, which JSON cannot key by. On the wire it is
/// a list of `[relationship, weight]` pairs, sorted because the map iterates in key order.
mod relationships_wire {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::Relationship;

    pub(super) fn serialize<S: Serializer>(
        map: &BTreeMap<Relationship, u64>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<Relationship, u64>, D::Error> {
        Ok(Vec::<(Relationship, u64)>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}
