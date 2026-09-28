# s2w-log

The durable append-only stores: the event log with source cursors, receipt times and provenance,
and the System 1 verdict store beside it (s2w#63).

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
- `StoredEvent.content_hash` is the event's FNV-1a payload hash as stored (`i64`); both log
  implementations compute it with the same function.

## Verdict store

- `VerdictStore` is the seam; `SqliteVerdictStore::open(dir)` keeps `verdicts.sqlite3` beside
  `events.sqlite3` with its own writer lock (`VERDICTS_LOCK`) and its own `user_version`, so the
  verdict writer (the bridge) and the event writer (ingest) can be different processes (#10).
  `InMemoryVerdictStore` meets the same contract.
- Rows are opaque bytes keyed `UNIQUE(position, engine, version)`, with `seq` (AUTOINCREMENT
  rowid) as write order and `event_hash` binding each row to the exact event it judged. This
  crate never decodes a verdict or its provenance; it does not depend on `s2w-system1`.
- Append-only: SQLite triggers refuse update and delete of verdict rows.
- `commit_batch` is one transaction: every row plus the bridge cursor, all or nothing, WAL with
  `synchronous=FULL`. A duplicate key or a row after `through` is `LogError::Corrupt` and
  stores nothing. An empty batch at or below the cursor is a no-op with no fsync.
- The bridge cursor is monotonic: `commit_batch` writes `max(cursor, through)` and a trigger
  refuses lowering or deleting it. It means "positions some bridge consumed", not "every
  engine evaluated through here": a newly registered engine stores rows below it.
- There is no foreign key to the events table; the bridge checks `cursor <= log head` and each
  replayed row's `event_hash` against the event, and a mismatch is loud.
