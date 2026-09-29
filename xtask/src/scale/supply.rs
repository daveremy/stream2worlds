//! The two event supplies a scale measurement folds, and which baseline table each is judged
//! against (s2w#174).
use super::Baseline;

/// The event supply a measurement folds: the seeded generator, or the recorded fixture (s2w#174).
/// Two implementations of one seam; both are gated and neither replaces the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Supply {
    Synthetic,
    Recorded,
}

impl Supply {
    pub(crate) const ALL: [Self; 2] = [Self::Synthetic, Self::Recorded];

    pub(crate) fn ir(self) -> &'static str {
        match self {
            Self::Synthetic => "[ir]",
            Self::Recorded => "[ir.recorded]",
        }
    }

    pub(crate) fn memory(self) -> &'static str {
        match self {
            Self::Synthetic => "[memory]",
            Self::Recorded => "[memory.recorded]",
        }
    }

    /// Suffix on every report line, so the two supplies' numbers are never confused.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Synthetic => "",
            Self::Recorded => " (recorded)",
        }
    }

    pub(crate) fn changed(self) -> &'static str {
        match self {
            Self::Synthetic => "the generator changed",
            Self::Recorded => "the fixture, its mapping or the fold changed",
        }
    }
}

/// One supply's `[memory]` figures.
pub(crate) struct MemGate {
    pub(crate) bytes_per_entity: u64,
    pub(crate) entities: u64,
    /// Pinned for the recorded supply only; the generator's count is reported.
    pub(crate) relationships: Option<u64>,
    pub(crate) reported: u64,
}

impl Baseline {
    /// `(fold_ir_per_event, events)` for `supply`.
    pub(crate) fn ir_gate(&self, supply: Supply) -> (u64, u64) {
        match supply {
            Supply::Synthetic => (self.ir.fold_ir_per_event, self.ir.events),
            Supply::Recorded => (self.ir.recorded.fold_ir_per_event, self.ir.recorded.events),
        }
    }

    pub(crate) fn memory_gate(&self, supply: Supply) -> MemGate {
        let (m, r) = (&self.memory, &self.memory.recorded);
        match supply {
            Supply::Synthetic => MemGate {
                bytes_per_entity: m.bytes_per_entity,
                entities: m.entities,
                relationships: None,
                reported: m.bytes_per_relationship_reported,
            },
            Supply::Recorded => MemGate {
                bytes_per_entity: r.bytes_per_entity,
                entities: r.entities,
                relationships: Some(r.relationships),
                reported: r.bytes_per_relationship_reported,
            },
        }
    }
}
