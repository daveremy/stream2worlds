//! The two event supplies a scale measurement folds, and which `[memory]` table each is judged
//! against (s2w#174). Their `[ir]` tables are in `ir_bench.rs`.
use super::{Baseline, MemMeasurement};

/// The event supply a measurement folds: the seeded generator, or the recorded fixture (s2w#174).
/// Two implementations of one seam; both are gated and neither replaces the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Supply {
    Synthetic,
    Recorded,
}

impl Supply {
    pub(crate) const ALL: [Self; 2] = [Self::Synthetic, Self::Recorded];

    pub(crate) fn memory(self) -> &'static str {
        match self {
            Self::Synthetic => "[memory]",
            Self::Recorded => "[memory.recorded]",
        }
    }

    /// `metric` as every report line names it, so the two supplies' numbers are never confused.
    pub(crate) fn name(self, metric: &str) -> String {
        match self {
            Self::Synthetic => metric.to_owned(),
            Self::Recorded => format!("{metric} (recorded)"),
        }
    }

    /// The `scale_mem` test that measures this supply; run with `--exact`, so a rename matches
    /// nothing and fails.
    pub(crate) fn mem_test(self) -> &'static str {
        match self {
            Self::Synthetic => "tests::bytes_per_entity_and_relationship",
            Self::Recorded => "tests::bytes_per_entity_and_relationship_recorded",
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

/// `--tighten-baseline` for one supply's `[memory]` table: lowers `bytes_per_entity` and
/// `bytes_per_relationship_reported` to the measurement. `None` when nothing is lower.
pub(crate) fn tighten_text(text: &str, supply: Supply, m: &MemMeasurement) -> Option<String> {
    super::tighten::lower_in(
        text,
        supply.memory(),
        &[
            ("bytes_per_entity", m.bytes_per_entity),
            ("bytes_per_relationship_reported", m.bytes_per_relationship),
        ],
    )
}
