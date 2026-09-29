//! Serde forms for [`World`](crate::World) fields whose in-memory layout is not the wire layout.
//!
//! `entities` and `hub_refs` write exactly the bytes of the `BTreeMap` each replaced (postcard and
//! JSON), so stored snapshots and `world_hash` do not depend on the in-memory layout (s2w#190).
//! Every map serializer's iterator must stay exact-size: postcard writes a map's length first and
//! refuses one it is not told.

/// `World::entities` is a `Vec` indexed by id. On the wire it stays a map `{id: state}` in id
/// order. A deserialized world is untrusted: its ids must be exactly `0..len`, or it is rejected.
pub(crate) mod entities {
    use std::collections::BTreeMap;

    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::event::EntityId;
    use crate::world::EntityState;

    pub(crate) fn serialize<S: Serializer>(
        entities: &[EntityState],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_map(
            (0_u64..)
                .zip(entities)
                .map(|(id, state)| (EntityId::new(id), state)),
        )
    }

    /// Through a `BTreeMap`, as the old field was, so unsorted or repeated keys behave as they
    /// did; then the keys must be exactly `0..len` (no gap, no offset start).
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<EntityState>, D::Error> {
        let map = BTreeMap::<EntityId, EntityState>::deserialize(deserializer)?;
        if let Some((index, id)) = (0_u64..)
            .zip(map.keys())
            .find(|&(index, id)| id.get() != index)
        {
            return Err(D::Error::custom(format_args!(
                "entity ids are not dense: id {} at position {index}",
                id.get()
            )));
        }
        Ok(map.into_values().collect())
    }
}

/// `EntityState::hub_refs` is `None` for the (nearly every) entity with no hub refs. On the wire
/// it stays a map, `None` written as an empty one; an empty map reads back as `None`, so a round
/// trip is `==`.
#[expect(
    clippy::box_collection,
    reason = "the field's type: 8 B inline, not 24 (s2w#190)"
)]
pub(crate) mod hub_refs {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serializer};

    use crate::event::EntityId;

    pub(crate) fn serialize<S: Serializer>(
        refs: &Option<Box<BTreeMap<String, EntityId>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        // Not `flat_map`: its size hint is inexact, which postcard refuses.
        match refs {
            Some(map) => serializer.collect_map(map.iter()),
            None => serializer.collect_map(std::iter::empty::<(&String, &EntityId)>()),
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Box<BTreeMap<String, EntityId>>>, D::Error> {
        let map = BTreeMap::<String, EntityId>::deserialize(deserializer)?;
        Ok((!map.is_empty()).then(|| Box::new(map)))
    }
}

/// `relationships` is a struct-keyed map in memory, which JSON cannot key by. On the wire it is
/// a list of `[relationship, weight]` pairs, sorted because the map iterates in key order.
pub(crate) mod relationships {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::world::Relationship;

    pub(crate) fn serialize<S: Serializer>(
        map: &BTreeMap<Relationship, u64>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<Relationship, u64>, D::Error> {
        Ok(Vec::<(Relationship, u64)>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}
