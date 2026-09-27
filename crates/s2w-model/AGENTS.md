# s2w-model

Shared vocabulary: timestamps, source ids, and later events, offsets, entity ids, proposals and issuances.

## Allowed dependencies

- `serde`, `thiserror` (dev: `serde_json`)

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- No I/O, no async, no clock, no randomness, no `HashMap`/`HashSet` (clippy enforces).
- A type arrives here only when a second crate needs it; `Cursor` and `RawEvent` live here so
  source and log adapters can exchange them without depending on one another.
- Persisted types get a version before any format is frozen (decision record, then migration).
