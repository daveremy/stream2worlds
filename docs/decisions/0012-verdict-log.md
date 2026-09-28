# 0012: System 1 verdict log — verdicts are stored before they are served, and replayed

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #63 · Amends [0011](0011-system1-bridge.md)

## Decision

Every System 1 verdict is persisted, and a bridge start serves stored verdicts instead of
re-running engines. This makes the `s2w-system1` invariant "every verdict is persisted; replay
never re-runs an engine to reconstruct a past verdict" true, and retires 0011's rule that
engines must be pure functions of their payload.

**Record.** One row per (log position, engine name, engine version): the event's
`content_hash` (`event_hash`, `i64`), the verdict as `serde_json` bytes (the encoding the
golden file `crates/s2w-system1/testdata/page-change-sample.verdict.json` pins), a nullable
`provenance` blob, and a write-order `seq`. Rows are unique on the key, append-only (update and
delete refused by trigger), and opaque bytes to `s2w-log`; encoding stays in `s2w-app::bridge`.

**Provenance** is a reserved JSON object. Reserved keys: `model_hash`, `temperature_milli`
(an integer; no floats, per 0011), `calibration_id`. Both shipped engines write `NULL`. Adding
a key is not a schema change.

**Where.** `<log dir>/verdicts.sqlite3`, a sibling of the event log with its own writer lock
(`VERDICTS_LOCK`) and its own `user_version`, not a table in the events database. The bridge
reads the event log through the read-only `LogReader` seam, and the ingest/serve process split
is #10's decision; a sibling file lets the verdict writer and the event writer be different
processes. Same durability as 0002/0004: WAL, `synchronous=FULL`, one transaction per batch.

**Why 0002's single-store argument does not apply.** 0002 needed event and cursor atomicity.
Here the only cross-store fact is "a verdict for an event", and every torn state resolves one
way: no verdict means a first evaluation; a verdict the log does not match is
`LogError::Corrupt`. The bridge checks every replayed row's `event_hash` against the log
(catching a replaced log of equal or greater length) and, when a poll reaches the end of the
log, that the store's cursor is not past it. A mismatch is loud and never a silent
re-evaluation.

**Write site and crash semantics: verdict before claims.** Per poll batch the bridge judges
every event, commits the batch's new verdicts and the cursor in one transaction, and only then
appends the batch's claims to the timeline. No claim is ever served whose verdict is not
durable. A crash before the commit leaves nothing stored and nothing served, so a restart
evaluates those events for the first time. A failed commit ends the poll with an error, serves
nothing and does not advance; the next poll retries the batch (re-running engines only for
verdicts that were never stored, and so never served).

**Replay selection.** For each event and each routed engine name: if a stored verdict exists
for (position, engine name), it is served **whatever its version**, and if several versions
exist, the first written (lowest `seq`). The engine is not called. Otherwise the registered
engine evaluates, and the verdict is stored and served. So a `version()` bump never re-runs
history: already-evaluated positions keep serving the old version's verdict, and only new
positions (or positions a newly registered engine never saw) get the new version.

This departs from #63's scope line ("only positions with no stored verdict for (engine,
version) are evaluated"), which would re-evaluate every position on a bump and contradict the
issue's done-when. Re-evaluating history under a new version is a separate, explicit operation
(a counterfactual branch), not built here; the version in the key keeps it possible.

`Abstain(Panicked)` is persisted like any verdict. Recovering from an engine bug is a version
bump plus that later re-verdict, never a silent re-run. Stored verdicts of an engine no longer
registered are not served: the registry decides what feeds the world.

**Cursor.** The store holds a monotonic `bridge_cursor` (the highest log position some bridge
consumed), written in the batch transaction as `max(cursor, through)`; a trigger refuses
lowering it. It is not "every engine evaluated through here": a newly registered engine stores
rows below it. Today it serves the start-up sanity check; #33's resume needs it once a world
snapshot records its bridge position. A start still folds from position 0 into an empty
timeline, through stored verdicts.

**Registry.** `EngineRegistry::register` returns `Result` and refuses an engine whose name is
already registered at a different version (the store key assumes one version per name).
`engines_for` returns each name once, first registration first, so overlapping routes are
legitimate configuration and never produce two rows with one key.

**Observability.** `BridgeStats` counts `replayed` (served from the store), `evaluated` (engine
calls) and `replayed_stale_version` (served version differs from the registered one, which
makes a bump visible).

## Consequences

- Local embeddings (#64) and any engine with an input the log does not capture can ship; #64
  records its model file hash in `provenance`.
- Retention: verdicts live as long as their events. Neither store is truncated in slice 1;
  compaction is #33's and must drop events and verdicts together.
- Size, by arithmetic (to be measured by #32): Abstain rows ~40-80 B, Propose rows ~200-500 B.
  At 10^7 events and one engine, ~3 GB plus ~0.5 GB of index, 15-30% on top of the event log.
  Replay reads ~300 B of JSON per verdict instead of parsing a 1-2 KB payload, so 0004's
  "10^7 in 5 minutes" is not threatened. If #32 measures more than 2x off, compact encoding or
  not storing `Abstain(NotMine)` is a dated amendment.

verify: `cargo test -p s2w-log verdicts && cargo test -p s2w-app --test bridge_verdicts` passes.
