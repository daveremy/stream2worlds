//! Heap bytes per entity for the fold, allocator-counted with dhat (s2w#32).
//!
//! An integration test is its own binary, so it can own the global allocator. Ignored by
//! default; `cargo xtask check` runs it with `-- --ignored --exact --nocapture` and reads the
//! last stdout line that parses as JSON:
//! `{"bytes_per_entity":N,"bytes_per_relationship":M,"entities":E,"relationships":R}`.
//!
//! Two stages, one profiler (issue #32 plan, §1.4): (a) fold entity-observed events only and
//! divide the heap growth by the entity count (the gated number); (b) fold relationship-observed
//! events between those entities into the same world and divide that growth by the relationship
//! count (informational). Every event is built before the first heap snapshot, so only the
//! world's own growth is counted.
//!
//! A second ignored test does the same over the recorded fixture (s2w#174): the committed
//! mapping's claims, entity-observed ones first and relationship-observed ones second (each in
//! emission order), with the loading and mapping done before the first snapshot. `cargo xtask
//! check` judges it against `[memory.recorded]`.

#[path = "support/scale_generator.rs"]
#[expect(
    dead_code,
    reason = "the shared generator has items only the benchmarks use"
)]
mod scale_generator;

#[path = "support/recorded.rs"]
mod recorded;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[cfg(test)]
mod tests {
    use s2w_core::{World, fold};
    use s2w_model::WorldEvent;

    use super::recorded;

    use super::scale_generator::{
        MEM_ENTITIES, MEM_RELATIONSHIPS, SEED, entity_events, relationship_events,
    };

    /// Live heap bytes right now, as dhat counts them.
    fn live_bytes() -> u64 {
        u64::try_from(dhat::HeapStats::get().curr_bytes).unwrap_or(u64::MAX)
    }

    /// Heap growth from `before` to `after`, divided by `count`; 0 when nothing was counted.
    fn per_item(before: u64, after: u64, count: usize) -> u64 {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        after.saturating_sub(before).checked_div(count).unwrap_or(0)
    }

    #[test]
    #[ignore = "a measurement, run by `cargo xtask check`; prints one JSON line"]
    fn bytes_per_entity_and_relationship() {
        let _profiler = dhat::Profiler::builder().testing().build();
        let entities = entity_events(MEM_ENTITIES, SEED);
        let relationships = relationship_events(MEM_RELATIONSHIPS, MEM_ENTITIES, SEED);

        let before = live_bytes();
        let world = fold(World::default(), &entities);
        let after_entities = live_bytes();
        assert_eq!(world.entity_count(), MEM_ENTITIES, "stage (a) entity count");

        let world = fold(world, &relationships);
        let after_relationships = live_bytes();
        assert_eq!(
            world.entity_count(),
            MEM_ENTITIES,
            "stage (b) must not mint entities"
        );
        let relationship_count = world.relationships().len();
        assert!(relationship_count > 0, "stage (b) made no relationships");

        let bytes_per_entity = per_item(before, after_entities, MEM_ENTITIES);
        let bytes_per_relationship =
            per_item(after_entities, after_relationships, relationship_count);
        assert!(bytes_per_entity > 0, "no heap growth measured for entities");
        println!(
            "{}",
            serde_json::json!({
                "bytes_per_entity": bytes_per_entity,
                "bytes_per_relationship": bytes_per_relationship,
                "entities": MEM_ENTITIES,
                "relationships": relationship_count,
            })
        );
        drop(world);
    }

    #[test]
    #[ignore = "a measurement, run by `cargo xtask check`; prints one JSON line"]
    fn bytes_per_entity_and_relationship_recorded() -> recorded::Fallible<()> {
        let _profiler = dhat::Profiler::builder().testing().build();
        let mapped = recorded::claims(recorded::load()?, recorded::mapping()?)?;
        let (entities, relationships): (Vec<WorldEvent>, Vec<WorldEvent>) = mapped
            .claims
            .into_iter()
            .partition(|c| matches!(c, WorldEvent::EntityObserved { .. }));

        let before = live_bytes();
        let world = fold(World::default(), &entities);
        let after_entities = live_bytes();
        let entity_count = world.entity_count();

        let world = fold(world, &relationships);
        let after_relationships = live_bytes();
        assert_eq!(
            world.entity_count(),
            entity_count,
            "stage (b) must not mint entities"
        );
        let relationship_count = world.relationships().len();

        let bytes_per_entity = per_item(before, after_entities, entity_count);
        assert!(bytes_per_entity > 0, "no heap growth measured for entities");
        println!(
            "{}",
            serde_json::json!({
                "bytes_per_entity": bytes_per_entity,
                "bytes_per_relationship": per_item(after_entities, after_relationships, relationship_count),
                "entities": entity_count,
                "relationships": relationship_count,
            })
        );
        drop(world);
        Ok(())
    }
}
