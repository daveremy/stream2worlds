//! Pins the recorded scale fixture (s2w#174): its bytes, how many events it holds, and what the
//! committed mapping and the fold make of them. The scale gates assert the same counts, so a
//! parse or fold that silently drops work cannot measure cheaper; this test fails first, on
//! every `cargo test`, and names what changed.
//!
//! The fixture is human-owned: when this fails, fix the code or record a new fixture on
//! purpose (and re-pin here). Never edit the fixture to make it pass.

#[path = "support/recorded.rs"]
mod recorded;

#[path = "support/recorded_links.rs"]
mod recorded_links;

use s2w_core::{World, fold};
use s2w_model::WorldEvent;
use s2w_system1::{Engine, MappingEngine};

use recorded::{
    ENTITIES, FIXTURE_HASH, Fallible, RELATIONSHIPS, bytes, claims, hash, load, mapping,
};

/// Events (SSE frames with both `data` and `id`) in the fixture.
const EVENTS: usize = 11_667;
/// Claims the mapping proposes over all events.
const CLAIMS: usize = 58_335;
/// Entity-observed claims among them.
const ENTITY_CLAIMS: usize = 35_001;
/// Relationship-observed claims among them.
const RELATIONSHIP_CLAIMS: usize = 23_334;
/// Events the engine abstains on.
const ABSTAINED: usize = 0;

#[test]
fn the_fixture_bytes_are_pinned() -> Fallible<()> {
    let actual = hash(&bytes()?);
    assert_eq!(
        actual, FIXTURE_HASH,
        "the recorded fixture changed: its FNV-1a 64 is now {actual:#x}. The fixture is \
         human-owned; restore it, or pin a deliberate re-recording in \
         tests/support/recorded.rs and update tests/fixtures/README.md"
    );
    Ok(())
}

#[test]
fn the_fixture_parses_maps_and_folds_to_the_pinned_counts() -> Fallible<()> {
    let events = load()?;
    assert_eq!(events.len(), EVENTS, "events");

    let mapped = claims(events, mapping()?)?;
    let entity_claims = mapped
        .claims
        .iter()
        .filter(|c| matches!(c, WorldEvent::EntityObserved { .. }))
        .count();
    let relationship_claims = mapped
        .claims
        .iter()
        .filter(|c| matches!(c, WorldEvent::RelationshipObserved { .. }))
        .count();
    assert_eq!(mapped.abstained, ABSTAINED, "abstentions");
    assert_eq!(mapped.claims.len(), CLAIMS, "claims");
    assert_eq!(entity_claims, ENTITY_CLAIMS, "entity-observed claims");
    assert_eq!(
        relationship_claims, RELATIONSHIP_CLAIMS,
        "relationship-observed claims"
    );

    let world = fold(World::default(), &mapped.claims);
    assert_eq!(world.entity_count(), ENTITIES, "entities after the fold");
    assert_eq!(
        world.relationships().len(),
        RELATIONSHIPS,
        "relationships after the fold"
    );
    Ok(())
}

#[test]
fn replay_is_deterministic() -> Fallible<()> {
    let events = load()?;
    let first = claims(events, mapping()?)?.claims;
    let second = claims(events, mapping()?)?.claims;
    assert!(
        first == second,
        "two runs of the mapping over the same events proposed different claims"
    );
    Ok(())
}

#[test]
fn the_linked_mapping_makes_the_pinned_counts() -> Fallible<()> {
    let engine = MappingEngine::new(recorded_links::linked_mapping()?)?;
    let verdicts: Vec<_> = load()?.iter().map(|e| engine.evaluate(e)).collect();
    assert_eq!(
        recorded_links::counts(&verdicts),
        (
            recorded_links::CLAIMS,
            recorded_links::MERGES,
            recorded_links::ABSTAINED
        ),
        "(claims, merges, abstentions) of the linked mapping over the recorded fixture; the \
         parse benchmark's teardown asserts the same pins in tests/support/recorded_links.rs"
    );
    Ok(())
}
