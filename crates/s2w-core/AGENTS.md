# s2w-core

The pure fold: events in, world out, replayable to any offset.

## Allowed dependencies

- `s2w-model`, `serde` (dev: `serde_json`, `proptest`, `insta`)

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- No I/O, no async, no clock, no randomness, no hash-order iteration (clippy enforces).
- Time and randomness are arguments. The same log always folds to the same world.
- Repairs are events; nothing raw is ever edited. Two histories: as known then, reinterpreted now.
- Golden replay files are human-owned: never regenerate one to make a test pass.
  `tests/fixtures/golden-fold-v1.*` is checked by `cargo xtask check` (decision 0005).
- `FOLD_FIXTURE_HASH` is FNV-1a of those golden files, pinned by `tests/fixture_hash.rs`. When a
  human-approved fold change edits a golden file, update the constant (never the other way
  round): it invalidates stored world snapshots (decision 0024).
- `World` serializes with postcard for world snapshots and `world_hash` (decision 0024): no
  `World` field (or type inside it) may gain `#[serde(default)]`, `skip`, `flatten` or
  `untagged`, and no floats or hash-ordered maps.
- An entity id is assigned once and never reused, and the fold enforces it: `keys` is
  write-once. A merge aliases ids under the survivor; revoking a repair splits them back apart;
  neither operation changes an id. Merge edges are stored raw, as named, and resolved on read.
- Entity ids are dense: minted in order from 0, never deleted, and an id is its index into
  `World::entities` (a `Vec`, s2w#190). Deserialize rejects a world whose ids are not exactly
  `0..len`; a world whose id counter is not its entity count mints nothing, and a snapshot
  carrying one fails `check_fold`. `keys`, `merges` and `hub_counters` in a deserialized world
  are not bounds-checked against `entities`: a write to an entity past its end is a no-op.
- `fold_one` is total: an event it cannot apply is a documented no-op, never a panic or error.
- No domain knowledge in this crate; see decision 0018.
