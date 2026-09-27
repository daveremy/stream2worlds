# s2w-log

The append-only event log with source cursors, receipt times and provenance.

## Allowed dependencies

- `s2w-model`; the storage dependency chosen in its gate-2 decision record

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Append-only: raw events are never edited or deleted by the log.
- Every event carries its source cursor and receipt time; restarts resume from cursors.
- Dedupe never merges identical payloads from different legitimate source identities.
- Never depends on the core or on another adapter.
