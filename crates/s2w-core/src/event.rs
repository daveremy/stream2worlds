//! The fold's input vocabulary: identities, attribute values, and [`WorldEvent`].

use serde::{Deserialize, Serialize};

/// An entity's identity inside one world. Opaque, minted only by the fold, never reused.
///
/// A merge aliases an id under a survivor and a revoked merge splits it back out; neither
/// changes the id itself.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(u64);

impl EntityId {
    /// Wraps a raw id. Only the fold mints ids.
    #[must_use]
    pub(crate) const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

pub use s2w_model::{AttrValue, NaturalKey, WorldEvent};
