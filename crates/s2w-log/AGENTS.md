# s2w-log

The durable append-only stores: the event log with source cursors, receipt times and provenance,
and the System 1 verdict store beside it (s2w#63).

## Allowed dependencies

- `s2w-model`, `rusqlite`, `serde`, `serde_json`

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
  implements it; serve shares one event-log handle in one process (decision 0014), and
  `ReadOnlySqliteEventLog` opens the same WAL database without taking the writer lock.
- The only expected caller is `s2w-app`; `s2w-sources` never depends on this crate.
- Append-only behavior is enforced by both the Rust API and SQLite triggers. SQLite's
  `recursive_triggers` setting is per connection, so a separate raw connection must enable it
  to prevent `INSERT OR REPLACE` from bypassing a delete trigger.
- `StoredEvent.content_hash` is the event's FNV-1a payload hash as stored (`i64`); both log
  implementations compute it with the same function.

## Verdict store

- `VerdictStore` is the seam; `SqliteVerdictStore::open(dir)` keeps `verdicts.sqlite3` beside
  `events.sqlite3` with its own writer lock (`VERDICTS_LOCK`) and its own `user_version`, so the
  stores have independent ownership; serve holds both locks in one process (decision 0014).
  `InMemoryVerdictStore` meets the same contract.
- `ReadOnlySqliteVerdictStore` is the matching lockless reader for `verdicts.sqlite3`; it and
  `ReadOnlySqliteEventLog` can coexist with active writers holding the two writer locks.
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
- No domain knowledge in this crate; see decision 0018.

## Presentation

- `WorldPresentation` (title, tagline, description, palettes, typefaces, stylesheet) is operator-authored
  viewer styling, stored in `world_presentation` alongside events and membership: append-only
  (latest row per world wins, unlike `WorldManifest`'s create-once identity), with an `origin`
  column (`'operator'` today; `'discovered'` reserved for a follow-up issue) so a proposed
  record can later be told apart from an accepted one without another schema bump.
- `stylesheet` is operator CSS text: max 16 KiB, no `@import`/`@font-face`/`url()`/`src()`/
  `image-set()` (nothing that loads a resource), no `<` `>` or backslash escapes, no remote
  addresses. The viewer injects it only on that world's `/w/<world>/` page, never the home page.
- Two types by design: `WorldPresentationInput` (`#[serde(deny_unknown_fields)]`) is the CLI
  write-path shape — a typo'd key is a loud error. `WorldPresentation` (`#[serde(default)]` on
  every field) is the load-path shape — an older or partial row must still deserialize. They
  are not the same struct because the two attributes cannot both apply to one type.
- `validate()` runs only on `set` (the write path); `load` never re-validates, and a corrupt
  (undecodable) row is a loud `LogError::Corrupt`, never a silent `None` — matches the
  "a cursor that cannot be decoded is a loud error" invariant elsewhere in this crate.
- `set` refuses a world with no `WorldManifest` row: presentation cannot exist for a world that
  does not.
- Schema version 4 (bumped from 3): `migrate_v3_to_v4` adds the table via idempotent
  `CREATE TABLE IF NOT EXISTS`, mirroring `migrate_v2_to_v3`. `open_sqlite_store`'s dispatch
  chains a v2 database straight through v3 to v4 in one `open()` call — it never stops at the
  intermediate version.
