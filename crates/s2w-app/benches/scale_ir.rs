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
//!
//! A third benchmark measures the parse half (s2w#166): RawEvent to claims through System 1's
//! `MappingEngine`, the engine every real stream runs. Its setup loads the pinned recording and
//! builds the engine from the committed linked mapping (both uncounted); the measured region is
//! one `evaluate` per raw event, in file order; the teardown checks the pinned claim, merge and
//! abstention counts. Its total divided by `[parse] events` is the third gated number.
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

#[path = "../tests/support/recorded_links.rs"]
mod recorded_links;

use std::hint::black_box;

use gungraun::{library_benchmark, library_benchmark_group, main};
use s2w_core::{World, fold};
use s2w_model::{RawEvent, WorldEvent};
use s2w_system1::{Engine, MappingEngine, Verdict};

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

/// The setup for the parse, outside the measured region: the pinned recording and an engine
/// built from the committed linked mapping.
fn parse_input() -> (MappingEngine, &'static [RawEvent]) {
    let input = recorded_links::linked_mapping().and_then(|mapping| {
        let engine = MappingEngine::new(mapping)?;
        Ok((engine, recorded::load()?))
    });
    match input {
        Ok(i) => i,
        Err(e) => panic!("cannot load the recorded fixture or its linked mapping: {e}"),
    }
}

/// The teardown for the parse: the pinned counts, so an engine that silently drops work cannot
/// measure cheaper.
fn check_parse((_engine, verdicts): (MappingEngine, Vec<Verdict>)) {
    assert_eq!(
        recorded_links::counts(&verdicts),
        (
            recorded_links::CLAIMS,
            recorded_links::MERGES,
            recorded_links::ABSTAINED
        ),
        "the parse of {} recorded events did not make the pinned (claims, merges, abstentions)",
        verdicts.len()
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

#[library_benchmark]
#[bench::fixture(args = (parse_input()), teardown = check_parse)]
fn parse_ir_per_event(
    (engine, events): (MappingEngine, &'static [RawEvent]),
) -> (MappingEngine, Vec<Verdict>) {
    // A plain loop, never a closure: a closure here is its own symbol under this function's
    // name, and gungraun's collection toggle fires on entering it, so everything the closure
    // calls would go uncounted (measured: 100 Ir for the whole parse).
    let mut verdicts = Vec::with_capacity(events.len());
    for event in events {
        verdicts.push(engine.evaluate(black_box(event)));
    }
    (engine, black_box(verdicts))
}

library_benchmark_group!(
    name = scale;
    benchmarks = fold_ir_per_event, fold_ir_per_event_recorded, parse_ir_per_event
);
main!(library_benchmark_groups = scale);
