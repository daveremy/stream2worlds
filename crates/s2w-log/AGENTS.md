# s2w-log

The append-only event log with source cursors, receipt times and provenance.

## Allowed dependencies

- `s2w-model`, `rusqlite`

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Append-only: raw events are never edited or deleted by the log.
- Every event carries its source cursor and receipt time; restarts resume from cursors.
- Dedupe never merges identical payloads from different legitimate source identities.
- Never depends on the core or on another adapter.
- The public seam is `EventLog`, with durable `SqliteEventLog`, fixture-friendly
  `InMemoryEventLog`, and storage-neutral `LogError` implementations.
- The only expected caller is `s2w-app`; `s2w-sources` never depends on this crate.
- Append-only behavior is enforced by both the Rust API and SQLite triggers. SQLite's
  `recursive_triggers` setting is per connection, so a separate raw connection must enable it
  to prevent `INSERT OR REPLACE` from bypassing a delete trigger.
