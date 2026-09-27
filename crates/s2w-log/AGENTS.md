# s2w-log

The append-only event log with source cursors, receipt times and provenance.

## Allowed dependencies

- `s2w-model`, `rusqlite`

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Append-only: raw events are never edited or deleted by the log.
- Every event carries its source cursor and receipt time; restarts resume from cursors.
- Dedupe never merges identical payloads from different legitimate source identities.
- Append dedupes on `(source, FNV-1a payload content hash)`, never on the cursor: two real
  events can share a cursor value. On a hash hit the stored payload bytes are compared; a
  collision between distinct payloads is a loud `LogError::Corrupt`, never a silent drop, and a
  duplicate leaves the stored cursor row untouched.
- `append_batch` is group commit: one transaction per batch, all or nothing, outcomes in input
  order, each event classified as sequential `append` would. It is a required trait method with
  no default body, because a looping default would silently lose atomicity. The N-events /
  T-ms flush policy lives in `s2w-app`; the log has no clock. `synchronous=FULL` is unchanged
  (decision 0004).
- Never depends on the core or on another adapter.
- The public seam is `EventLog`, with durable `SqliteEventLog`, fixture-friendly
  `InMemoryEventLog`, and storage-neutral `LogError` implementations.
- `LogReader` is the read-only seam (the System 1 bridge reads through it). Every `EventLog`
  implements it; a lockless second-process reader is deferred until #10 decides process topology.
- The only expected caller is `s2w-app`; `s2w-sources` never depends on this crate.
- Append-only behavior is enforced by both the Rust API and SQLite triggers. SQLite's
  `recursive_triggers` setting is per connection, so a separate raw connection must enable it
  to prevent `INSERT OR REPLACE` from bypassing a delete trigger.
