//! Typed deltas: what one folded event did to the world. Exactly one per offset.

use s2w_core::{EntityId, NaturalKey, Relationship, World, WorldEvent};
use serde::Serialize;

/// What one event did. The SSE stream sends exactly one per offset, so `Last-Event-ID`
/// resumes unambiguously. Entity ids serialize as raw numbers; d3 node ids are `e:<id>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    /// An entity was observed; its type and attributes landed on `resolved`.
    Entity {
        /// The id the key was minted as.
        entity: EntityId,
        /// The entity the observation landed on (the survivor, if `entity` is merged away).
        resolved: EntityId,
        /// Whether this observation minted the id.
        minted: bool,
    },
    /// A relationship was materialized as an edge (ids resolved at observation time).
    Link {
        /// The source entity.
        source: EntityId,
        /// The target entity.
        target: EntityId,
        /// The relationship kind.
        kind: String,
        /// The edge's observation count after this event.
        weight: u64,
    },
    /// A relationship to a hub was kept as the source's `hub_ref`, not an edge.
    HubRef {
        /// The source entity.
        source: EntityId,
        /// The hub entity.
        hub: EntityId,
        /// The relationship kind.
        kind: String,
        /// Whether this observation took the hub past the cap.
        tripped: bool,
        /// The hub's in-degree after this event.
        in_degree: u64,
    },
    /// A merge aliased `absorbed` under `survivor`.
    Merge {
        /// The survivor's id.
        survivor: EntityId,
        /// The absorbed id.
        absorbed: EntityId,
    },
    /// A revoked merge split `absorbed` back out of `survivor`.
    Split {
        /// The survivor's id.
        survivor: EntityId,
        /// The absorbed id, now its own entity again.
        absorbed: EntityId,
    },
    /// The fold treated the event as a no-op (decision 0005); the offset still advanced.
    Noop {
        /// The event variant that did nothing, e.g. `EntitiesMerged`.
        event: &'static str,
    },
}

impl Delta {
    /// The type name used as the SSE `event:` field.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Entity { .. } => "entity",
            Self::Link { .. } => "link",
            Self::HubRef { .. } => "hub_ref",
            Self::Merge { .. } => "merge",
            Self::Split { .. } => "split",
            Self::Noop { .. } => "noop",
        }
    }

    /// Every entity id the delta names.
    pub(crate) fn entities(&self) -> Vec<EntityId> {
        match self {
            Self::Entity {
                entity, resolved, ..
            } => vec![*entity, *resolved],
            Self::Link { source, target, .. } => vec![*source, *target],
            Self::HubRef { source, hub, .. } => vec![*source, *hub],
            Self::Merge { survivor, absorbed } | Self::Split { survivor, absorbed } => {
                vec![*survivor, *absorbed]
            }
            Self::Noop { .. } => Vec::new(),
        }
    }
}

fn in_degree(world: &World, id: EntityId) -> u64 {
    world
        .hub_counters()
        .get(&id)
        .map_or(0, |c| u64::try_from(c.in_degree()).unwrap_or(u64::MAX))
}

/// Folds one event and reports what it did. Reads only what it needs from the world before
/// the fold, so a replay never clones the world.
#[must_use]
pub fn fold_with_delta(prev: World, event: &WorldEvent) -> (World, Delta) {
    match event {
        WorldEvent::EntityObserved { key, .. } => fold_entity(prev, event, key),
        WorldEvent::RelationshipObserved { from, to, kind } => {
            fold_relationship(prev, event, from, to, kind)
        }
        WorldEvent::EntitiesMerged { survivor, absorbed } => {
            fold_merge(prev, event, survivor, absorbed)
        }
        WorldEvent::MergeRevoked { survivor, absorbed } => {
            fold_revoke(prev, event, survivor, absorbed)
        }
    }
}

fn fold_entity(prev: World, event: &WorldEvent, key: &NaturalKey) -> (World, Delta) {
    let minted = prev.id_of(key).is_none();
    let next = s2w_core::fold_one(prev, event);
    let delta = match next.id_of(key) {
        Some(id) => Delta::Entity {
            entity: id,
            resolved: next.resolve(id),
            minted,
        },
        None => Delta::Noop {
            event: "EntityObserved",
        },
    };
    (next, delta)
}

fn fold_relationship(
    prev: World,
    event: &WorldEvent,
    from: &NaturalKey,
    to: &NaturalKey,
    kind: &str,
) -> (World, Delta) {
    let before = prev
        .id_of(to)
        .map_or(0, |t| in_degree(&prev, prev.resolve(t)));
    let next = s2w_core::fold_one(prev, event);
    let (Some(f), Some(t)) = (next.id_of(from), next.id_of(to)) else {
        return (
            next,
            Delta::Noop {
                event: "RelationshipObserved",
            },
        );
    };
    let (source, target) = (next.resolve(f), next.resolve(t));
    let after = in_degree(&next, target);
    let delta = if after > next.hub_in_degree_cap() {
        Delta::HubRef {
            source,
            hub: target,
            kind: kind.to_owned(),
            tripped: before <= next.hub_in_degree_cap(),
            in_degree: after,
        }
    } else {
        let edge = Relationship {
            from: source,
            to: target,
            kind: kind.to_owned(),
        };
        let weight = next.relationships().get(&edge).copied().unwrap_or(0);
        Delta::Link {
            source,
            target,
            kind: kind.to_owned(),
            weight,
        }
    };
    (next, delta)
}

fn fold_merge(
    prev: World,
    event: &WorldEvent,
    survivor: &NaturalKey,
    absorbed: &NaturalKey,
) -> (World, Delta) {
    let ids = prev.id_of(survivor).zip(prev.id_of(absorbed));
    let had = ids.is_some_and(|(s, a)| prev.merges().get(&a) == Some(&s));
    let next = s2w_core::fold_one(prev, event);
    let delta = match ids {
        Some((s, a)) if !had && next.merges().get(&a) == Some(&s) => Delta::Merge {
            survivor: s,
            absorbed: a,
        },
        _ => Delta::Noop {
            event: "EntitiesMerged",
        },
    };
    (next, delta)
}

fn fold_revoke(
    prev: World,
    event: &WorldEvent,
    survivor: &NaturalKey,
    absorbed: &NaturalKey,
) -> (World, Delta) {
    let ids = prev.id_of(survivor).zip(prev.id_of(absorbed));
    let had = ids.is_some_and(|(s, a)| prev.merges().get(&a) == Some(&s));
    let next = s2w_core::fold_one(prev, event);
    let delta = match ids {
        Some((s, a)) if had && next.merges().get(&a) != Some(&s) => Delta::Split {
            survivor: s,
            absorbed: a,
        },
        _ => Delta::Noop {
            event: "MergeRevoked",
        },
    };
    (next, delta)
}
