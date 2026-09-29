//! Instructions per event for the fold (s2w#32): one gungraun library benchmark, run under
//! Valgrind's Callgrind by `cargo xtask scale` (or `cargo bench -p s2w-app --bench scale_ir --
//! --save-summary=json` by hand, which needs `gungraun-runner` at the crate's pinned version).
//!
//! Building the events is the setup and is not counted. The benchmark function folds them into
//! an empty world and returns both the world and the events to the teardown, which is not
//! counted either: it checks the world holds every generated entity (so a fold or generator that
//! silently drops work cannot pass with a cheaper count) and drops both. Total instructions
//! divided by [`scale_generator::IR_EVENTS`] is the gated number.
//!
//! A second benchmark folds the recorded fixture instead (s2w#174): its setup loads the pinned
//! recording and runs the committed mapping over it (both uncounted), the measured region folds
//! every claim in emission order, and the teardown checks the pinned entity and relationship
//! counts. Its total divided by the fixture's raw event count (`[ir.recorded] events`) is the
//! second gated number. The two supplies answer different questions and both are gated.
#![expect(
    missing_docs,
    reason = "gungraun's macros generate undocumented modules, constants and functions"
)]

#[path = "../tests/support/scale_generator.rs"]
#[expect(
    dead_code,
    reason = "the shared generator has items only the memory test and wall bench use"
)]
mod scale_generator;

#[path = "../tests/support/recorded.rs"]
mod recorded;

use std::hint::black_box;

use gungraun::{library_benchmark, library_benchmark_group, main};
use s2w_core::{World, fold};
use s2w_model::WorldEvent;

use scale_generator::{IR_EVENTS, SEED, mixed_events};

/// Entities in [`mixed_events`]`(IR_EVENTS, _)`: its 80% entity-observed share.
const IR_ENTITIES: usize = IR_EVENTS - IR_EVENTS / 5;

/// The teardown, outside the measured region: the fold kept every generated entity.
fn check_entities((world, events): (World, Vec<WorldEvent>)) {
    assert_eq!(
        world.entity_count(),
        IR_ENTITIES,
        "the fold of {} events did not hold the generator's entities",
        events.len()
    );
}

/// The setup, outside the measured region: every claim the mapping makes of the fixture.
fn recorded_claims() -> Vec<WorldEvent> {
    let claims = recorded::load().and_then(|events| recorded::claims(events, recorded::mapping()?));
    match claims {
        Ok(c) => c.claims,
        Err(e) => panic!("cannot load the recorded fixture: {e}"),
    }
}

/// The teardown for the recorded fold: the pinned world, so dropped work cannot measure cheaper.
fn check_recorded((world, claims): (World, Vec<WorldEvent>)) {
    assert_eq!(
        (world.entity_count(), world.relationships().len()),
        (recorded::ENTITIES, recorded::RELATIONSHIPS),
        "the fold of {} recorded claims did not hold the pinned entities and relationships",
        claims.len()
    );
}

#[library_benchmark]
#[bench::events(args = (mixed_events(IR_EVENTS, SEED)), teardown = check_entities)]
fn fold_ir_per_event(events: Vec<WorldEvent>) -> (World, Vec<WorldEvent>) {
    let world = fold(World::default(), black_box(&events));
    (black_box(world), events)
}

#[library_benchmark]
#[bench::fixture(args = (recorded_claims()), teardown = check_recorded)]
fn fold_ir_per_event_recorded(claims: Vec<WorldEvent>) -> (World, Vec<WorldEvent>) {
    let world = fold(World::default(), black_box(&claims));
    (black_box(world), claims)
}

library_benchmark_group!(
    name = scale;
    benchmarks = fold_ir_per_event, fold_ir_per_event_recorded
);
main!(library_benchmark_groups = scale);
