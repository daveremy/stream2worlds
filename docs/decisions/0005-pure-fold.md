# 0005: The pure fold and golden replay

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #9 · Research [0003 §4, §7](../../research/0003-rust-substrate.md), [0005](../../research/0005-3d-exploration.md), [0006](../../research/0006-scaling.md)

## Decision

`s2w-core` folds `WorldEvent`s into a `World` with `fold_one(World, &WorldEvent) -> World`. The
fold is pure and **total**: it never panics and never returns an error. An event it cannot apply
is a documented no-op that still advances `World::offset` (except once `offset` is `u64::MAX`,
when events are dropped without advancing it).

`WorldEvent` has four variants: `EntityObserved`, `RelationshipObserved`, `EntitiesMerged` and
`MergeRevoked`. `EntityId`, `NaturalKey`, `AttrValue`, `WorldEvent` and `World` live in
`s2w-core`, not `s2w-model`: no adapter crate needs them yet (the `s2w-model` rule; `xtask`
is tooling and depends on the core directly). They move when `s2w-system1` emits claims.

`cargo xtask check` (check 6) is the fitness function. It folds the human-owned golden log
twice and requires the same bytes both times and the same bytes as the committed snapshot. It
also folds every prefix, round-trips that world through JSON, folds the rest, and requires the
same bytes again.

## Identity

- `keys: NaturalKey → EntityId` is **write-once**. An id is minted from a counter the first time
  any event names a key: `EntityObserved` for its key, `RelationshipObserved` for either
  endpoint. Merges and revokes never mint.
- `merges: absorbed → survivor` stores each merge **raw**, as the event named it, never a
  pre-resolved target. `World::resolve` does all chain-walking, capped at `merges.len()` steps
  so a malformed deserialized world still terminates.
- A merge is a no-op when either key is unknown, when the absorbed id already has an outstanding
  merge (revoke it first), or when both keys already resolve to the same entity. The last case
  covers self-merges, repeated merges and every merge that would close a cycle, so `merges` is
  always a forest.
- `MergeRevoked` removes an edge only when the stored raw edge is exactly the pair it names. If
  `a→b` and then `b→c` were merged, revoking `a→c` does nothing; revoking `a→b` splits `a` out.
- Attributes and types from `EntityObserved` land on the entity the key resolves to at that
  moment, and a later revoke does not move them back. Relationships and hub counters store the
  ids resolved at observation time and are never rewritten. This is the core invariant's "two
  histories": the log holds what was known then; a repair changes how later events are read.
- The property test compares `resolve` with an **independent reference model**: a naive
  union-find the test rebuilds from its own list of live merges after every event. It never
  reads `World::merges` or calls `World::resolve`, so it cannot share a bug with the fold. A
  loosened revoke (matching any edge from the absorbed id) fails it.
- **Id exhaustion is a no-op.** If minting would overflow the counter, the event does nothing.
  Panicking breaks totality, wrapping reuses ids, and an error return would thread a `Result`
  through every fold for a case that needs 2^64 entities. `u64::MAX` itself is never assigned.

## World state and wire format

- `world_id` is always `WorldId::ACTUAL`. Branches are a later gate; the field exists now so
  they need no wire-format migration (research 0003 §4, worlds as a column).
- `offset` counts `WorldEvent`s folded, no-ops included. It is **not** a log position: the core
  cannot see `s2w-log`, and once System 1 exists one raw event may yield several `WorldEvent`s.
  Mapping it to a log position belongs with snapshots (#33). *(2026-09-28: resolved by decision 0021; a world
  snapshot records its fold offset and the log position it was folded through.)*
- `fold_version` is stamped from `FOLD_VERSION` (1) and `hub_in_degree_cap` defaults to 10,000.
  Both are serialized fields, so a reader of a serialized world knows which fold, under which
  cap, produced it. A replay under another cap is a different world.
- `relationships` is a `BTreeMap<Relationship, u64>` in memory: an edge's presence plus its
  observation count, the weighted-collection shape of research 0003 §4. JSON cannot key a map
  by a struct, so on the wire it is a list of `[relationship, weight]` pairs. The adapter
  collects the map's iterator, so the list is sorted for free and deserializing collects it
  back: one shape in memory, one on disk, no sort step.
- Every collection is a `BTreeMap` or `BTreeSet`, so serialization order is the key order.
- `AttrValue` has no float variant: float equality would break byte-identical replay.

## Hub cap (research 0006, item 7)

`hub_counters` holds, per relationship target, the distinct source ids, a per-kind observation
count and the offset of the latest observation. They update on every `RelationshipObserved`.
The in-degree is the number of **distinct sources**, so one source repeating a relationship
counts once. When a target's in-degree exceeds `hub_in_degree_cap`, the relationship is not
materialized: the source entity's `hub_refs[kind]` is set to the hub's id instead (keyed by
kind, so the latest hub wins if one source points at two hubs with the same kind). `hub_refs`
is its own field, so a source attribute can never collide with it. The test is on the count
after the observation, so once a target has tripped, every later observation of it becomes a
`hub_ref`, including one from a source whose edge was stored before the trip. That edge stays
at its pre-trip weight; a reader unions `relationships` and `hub_refs`. Serving a hub as an
aggregate at every `lod` is query work (#10, #36).

## Golden replay

- The fixtures live in `crates/s2w-core/tests/fixtures/`, not `s2w-testkit`: `s2w-testkit`
  depends on `s2w-log`, which `s2w-core` must not reach. `xtask` depends on `s2w-core` to fold
  them.
- The golden log uses every `WorldEvent` variant and makes one entity a hub. The check fails if
  it stops doing either, so the fixture cannot be hollowed out and still pass.
- The golden fold starts from `World::with_hub_cap(3)`, so the hub fits in a log a person can
  read. It tests fold behaviour at the cap, not the production default.
- The snapshot is pretty JSON plus a trailing newline. It is human-owned: the check never
  rewrites it. An intentional fold change bumps `FOLD_VERSION` where the same log now folds
  differently, and a human reviews and edits the snapshot in the same PR.
- An `insta` JSON snapshot of a small fold is a separate regression test of the output shape.
  The xtask check asks whether the bytes are deterministic; the snapshot asks whether the shape
  changed.

## Scope cuts

- **No retraction.** No event retracts a relationship; the weighted map is the shape for Z-set
  semantics, not the mechanism. Revisit with the first event that needs retraction.
- **No `proptest-state-machine`.** Plain `proptest` generates event sequences and checks the
  reference model after every step, which is the state-machine shape without the extra
  dependency. Revisit if the model grows preconditions that plain generation wastes cases on.
- **No snapshot I/O.** The world is serializable and versioned; writing and loading snapshots,
  and a fold-version hash, are #33. *(2026-09-28: resolved by decision 0021: a postcard snapshot file, and
  a fold hash over `FOLD_VERSION`, the hub cap, the format and `FOLD_FIXTURE_HASH`.)*

## Research 0003 open items

None of research 0003's open items (the `rskafka` timestamp lookup, the `rmcp` major-version
churn, DBSP's licence, Parquet) concern the fold, so this decision settles none of them.

### Amendment, 2026-09-27: WorldEvent, NaturalKey, AttrValue move to s2w-model

System 1 now constructs claims and the core folds them, so these three types have two
consumers and move to `s2w-model`. Their serde shapes and no-float rule are unchanged.
`s2w-core` re-exports them to preserve existing import paths.
`EntityId` stays in the core: only the fold may call its crate-private constructor.
`World`, relationships, the fold, its version and hub cap also stay in the core.
Engines address entities by natural key and never see a world.
See [0011: System 1 bridge](0011-system1-bridge.md) for the engine contract.

verify: `cargo xtask check` check 6 passes with the golden fixtures untouched.
