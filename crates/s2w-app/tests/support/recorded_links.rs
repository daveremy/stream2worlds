//! The parse measurement's input (s2w#166): the committed linked mapping, and the pinned counts
//! of what it makes of the recorded fixture that `recorded.rs` loads.
//!
//! The parse benchmark (`benches/scale_ir.rs`, `parse_ir_per_event`) runs this mapping, not the
//! fold supply's, because it is a superset: the same entities, attributes and relationships
//! plus a second site entity and a link that merges it into the first, so every stage of
//! `MappingEngine::evaluate` (decode, keys and attributes, link merges, relationships) is inside
//! the measured region. The counts are pinned once here, for the benchmark's teardown and for
//! `tests/recorded_fixture.rs`, which checks them on every `cargo test`.
//!
//! Shared through `#[path]` next to `recorded.rs` (it uses that module's `Fallible`).

use std::fs;

use s2w_model::{StreamMapping, WorldEvent};
use s2w_system1::Verdict;

use super::recorded::Fallible;

/// The linked stream mapping, a symlink to `s2w-system1/testdata/sample-links.mapping.json`,
/// the one the engine's own link-merge tests replay, so the two cannot drift.
const LINKED_MAPPING: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/recorded-links.mapping.json"
);

/// Claims the linked mapping proposes over every event of the fixture.
pub(crate) const CLAIMS: usize = 81_669;
/// Link merges (`EntitiesMerged`) among them.
pub(crate) const MERGES: usize = 11_667;
/// Events it abstains on.
pub(crate) const ABSTAINED: usize = 0;

/// The committed linked mapping.
pub(crate) fn linked_mapping() -> Fallible<StreamMapping> {
    Ok(serde_json::from_str(&fs::read_to_string(LINKED_MAPPING)?)?)
}

/// `(claims, merges, abstained)` over a run's verdicts.
pub(crate) fn counts(verdicts: &[Verdict]) -> (usize, usize, usize) {
    verdicts
        .iter()
        .fold((0, 0, 0), |(claims, merges, abstained), v| match v {
            Verdict::Propose { claims: c, .. } => (
                claims + c.len(),
                merges
                    + c.iter()
                        .filter(|e| matches!(e, WorldEvent::EntitiesMerged { .. }))
                        .count(),
                abstained,
            ),
            Verdict::Abstain { .. } => (claims, merges, abstained + 1),
        })
}
