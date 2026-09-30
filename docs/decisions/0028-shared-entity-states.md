# 0028: Shared entity states, owned projections, single-flight `/world`

Date: 2026-09-29 · Status: accepted; part A landed (#271, PR 3 of #235), parts B and C pending · Gate 2 · Issue #235 · Builds on [0026](0026-bounded-timeline-history.md) (one resident world, no world clone on a request path)

## Context

A `/world?lod=entity` body is built and written while the reader holds the timeline guard, so
the fold waits for every client (#235). s2w#243 measured the recorded backfill (base main
156903a, 3 runs per row, busy host):

| Row | wall s | peak MiB | build share of the hold |
|---|---|---|---|
| no viewer | 80 | 548 | - |
| 1 viewer x 5 s | 115 | 875 | 0.37 |
| 4 viewers x 5 s | 419 | 1861 | 0.39 |
| 1 viewer x 1 s (s2w#267) | 592 | 1087 | 0.37 |

- The build is 0.37 to 0.43 of the hold; serialization is the rest. Releasing the guard after
  the build shrinks the hold 2.3x to 2.7x.
- Cloning the world per generation is out: `World::clone` costs 221.5 MiB and ~0.62 s, of
  which `entities` is 168.8 MiB (~1.2 KiB per entity, 138k entities).
- Memory fails before time: four concurrent projections reach 1.86 GiB against a 1 GiB box.

## Decision

Three mechanisms, each removing one measured cost.

**A. Entity states are shared and copied on write** (#271). `World::entities` is
`Vec<Arc<EntityState>>`. A cloned world shares every state; the fold writes through
`Arc::make_mut`, so a write copies only the entity it changes, and only when another handle
still holds it. `World::entity_arc` hands a reader the shared handle.

- **A write that changes nothing copies nothing.** An entity observation whose type is the
  entity's type and whose every attribute the entity already holds with that value
  (`AttrMap::changes`) is skipped before `entity_mut`. So is an over-cap relationship whose
  hub ref the source already holds. The skip is exact: it fires only when the write would leave
  the state `==` to what it was (proptest in `world.rs`), so it changes when memory is copied,
  never the folded value.
- **Bytes do not change.** `wire::entities` serializes each `&EntityState` and deserializes into
  `Arc::new`; postcard, JSON, `world_hash`, `FOLD_FIXTURE_HASH` and the golden files are
  untouched. serde's `rc` feature is not used.
- `World::clone` is now shallow for entity states. No request path clones a world.

**B. Owned projection, guard released before sorting and writing** (pending, PR 4). A
projection captures the header scalars, owned keys, interned relationship kinds and the kept
nodes' `Arc`s under the guard, then sorts and serializes after releasing it. The `Arc`s are
immutable, so the body still describes one (epoch, offset).

**C. Single-flight generations** (pending, PR 2). At most one `lod=entity` projection is in
flight; requests with equal ETag inputs share one build and one serialization through a
fan-out writer. `If-None-Match` is answered under the guard before any build.

*Amended 2026-09-30 (s2w#297):* full `lod=type` views join the same queue, so at most one
full projection of either level is in flight. Only the type summary (`lod=type&links=none`,
s2w#296) is served outside the queue, so first paint never waits behind a full build.

## Consequences

- **Memory, part A (measured, hub, rustc 1.98.1, dhat):** `[memory]` 360 to 369 B per entity
  (+2.5%), `[memory.recorded]` 346 to 349 B (+0.9%). Each state moves into its own allocation
  with a 16 B reference-count header, and the `Vec` slot shrinks from 56 B to 8 B.
- **Divergence** is the one new memory term once B lands: states the fold replaces while a
  projection holds the old ones. It lives as long as one generation and is bounded by the whole
  entity set (~169 MiB on the recorded backfill). A stream that rewrites every entity every
  batch reaches that bound; the no-op skip keeps an unchanged re-observation from counting.
  PR 4 measures it (`diverged`).
- Once C lands: a stalled subscriber holds the fan-out writer for up to `STALL` (5 s) per
  chunk before it is cut; no guard is held, so the fold is unaffected, but the other
  subscribers wait. With many distinct keys (per-focus views) the FIFO can push a waiter past
  the 30 s limit, and it gets 503.
- The no-op check costs one type compare and one binary search per incoming attribute before a
  write. A local `cargo xtask scale` on hub measured 5770 Ir per event against the 5764
  baseline (+0.1%) and 15340 against 15285 recorded (+0.4%); CI job `scale` owns the number.
