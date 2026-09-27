# s2w-core

The pure fold: events in, world out, replayable to any offset.

## Allowed dependencies

- `s2w-model` only

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- No I/O, no async, no clock, no randomness, no hash-order iteration (clippy enforces).
- Time and randomness are arguments. The same log always folds to the same world.
- Repairs are events; nothing raw is ever edited. Two histories: as known then, reinterpreted now.
- Golden replay files are human-owned: never regenerate one to make a test pass.
