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
  `tests/fixtures/golden-fold-v1.*` is checked by `cargo xtask check` (decision 0004).
- An entity id is assigned once and never reused, and the fold enforces it: `keys` is
  write-once. A merge aliases ids under the survivor; revoking a repair splits them back apart;
  neither operation changes an id. Merge edges are stored raw, as named, and resolved on read.
- `fold_one` is total: an event it cannot apply is a documented no-op, never a panic or error.
