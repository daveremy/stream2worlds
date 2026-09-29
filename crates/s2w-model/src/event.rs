//! Shared claim vocabulary emitted by engines and consumed by the fold.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::NaturalKey;

/// An attribute value. No floats: float equality would break byte-identical replay.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AttrValue {
    /// Text.
    Str(String),
    /// A signed integer.
    Int(i64),
    /// A flag.
    Bool(bool),
}

/// One input to the fold. Every variant is total: an event the fold cannot apply is a
/// documented no-op, never an error (decision 0005).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldEvent {
    /// An entity was seen. Mints an id on the key's first mention; the type and attributes land
    /// on the entity the key currently resolves to (the survivor, if the key was merged away).
    EntityObserved {
        /// The source's identity for the entity.
        key: NaturalKey,
        /// The entity's type; the latest observation wins.
        entity_type: String,
        /// Attributes; each key's latest observation wins.
        attrs: BTreeMap<String, AttrValue>,
    },
    /// A relationship was seen. Mints ids for either endpoint on its first mention.
    RelationshipObserved {
        /// The source endpoint.
        from: NaturalKey,
        /// The target endpoint.
        to: NaturalKey,
        /// The relationship kind, e.g. `edited`.
        kind: String,
    },
    /// A repair: `absorbed` is the same entity as `survivor`. Never mints an id.
    EntitiesMerged {
        /// The key that keeps its identity.
        survivor: NaturalKey,
        /// The key aliased under the survivor.
        absorbed: NaturalKey,
    },
    /// Undoes exactly one earlier merge, matched on the raw pair it named. Never mints an id.
    MergeRevoked {
        /// The survivor named by the merge being revoked.
        survivor: NaturalKey,
        /// The absorbed key named by the merge being revoked.
        absorbed: NaturalKey,
    },
}
