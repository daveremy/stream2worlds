//! The fold's input vocabulary: identities, attribute values, and [`WorldEvent`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An entity's identity inside one world. Opaque, minted only by the fold, never reused.
///
/// A merge aliases an id under a survivor and a revoked merge splits it back out; neither
/// changes the id itself.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(u64);

impl EntityId {
    /// Wraps a raw id. Only the fold mints ids that mean anything; this exists for the fold
    /// itself and for tools that read a serialized world.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A source-defined identity (a page title, a user name) that the fold maps to an [`EntityId`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NaturalKey(String);

impl NaturalKey {
    /// Wraps a source-defined key.
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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
