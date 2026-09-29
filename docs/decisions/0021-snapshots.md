# 0021: World snapshots and the timeline base

Date: 2026-09-28 · Status: accepted (part 1a landed; 1b pending) · Gate 2 · Issue #33 · Research [0006 §3(d)](../../research/0006-scaling.md) · Amends [0006](0006-world-query-api.md) (a new error code and `/time.base`) · Resolves open items in [0005](0005-pure-fold.md) and [0012](0012-verdict-log.md)

"Snapshot" in this record means the world snapshot file below. It is unrelated to the golden
fixture `golden-fold-v1.snapshot.json`, which is the fold's expected output (decision 0005), and
to the MCP replay's "read-only snapshot of committed verdicts" (decision 0009).

## Decision

**The seam is log | fold | snapshot.** The log stays the only source of truth. A snapshot is a
derived, self-contained file that saves a restart from refolding the whole log; deleting every
snapshot only costs time.

**File.** One file per snapshot, `<log_dir>/snapshots/snapshot-<offset, 20 digits>.s2w`:

| Bytes | Content |
|---|---|
| 8 | magic `S2WSNAP1` |
| 4 | payload length, `u32` little-endian |
| n | payload: `SnapshotV1`, postcard-encoded |
| 8 | FNV-1a 64 of the payload, little-endian |

`SnapshotV1` holds, in wire order: `format` (1, first so a reader refuses another format before
decoding the rest), `fold_hash`, `feed_hash`, `hub_cap`, `offset` (= `world.offset()`),
`position` (the last log position the bridge consumed), `position_event_hash` (the stored
`content_hash` of the event at `position`), `cursors` (every source cursor at write time,
recorded for portability and never consulted at load), `time` (`first_ts`, `last_ts`,
`clamped` of the folded events) and `world`. There are no paths or host details, so the bytes
can move to object storage later; `codec::encode`/`decode` are pure and a future backend reuses
them. `postcard` is chosen because it is compact, has no `deserialize_any`, and is deterministic
for a world made only of ordered maps and sets with no floats. That determinism is load-bearing,
so **no `World` field may ever gain `#[serde(default)]`, `skip` or `flatten`** (s2w-core
AGENTS.md).

**Validity: every rule must hold, or the file is ignored.** An ignored file is reported and
never deleted; the loader tries the next older file, and with none valid the process replays
from offset 0 exactly as before this decision.

1. The magic, length and checksum are intact, and the payload decodes completely.
2. `format` is the format this build reads.
3. `fold_hash` equals the running fold's: FNV-1a over `FOLD_VERSION`, the hub cap, the format
   and `s2w_core::FOLD_FIXTURE_HASH`. That constant is FNV-1a of the human-owned golden
   fixtures' bytes, pinned by a core test. A human-approved fold change edits the golden
   snapshot, the pin test then demands a new constant, and old world snapshots invalidate even
   if nobody bumped `FOLD_VERSION`.
4. `feed_hash` equals `EngineRegistry::feed_fingerprint()`: every route and engine name in
   registration order. Engine versions are excluded, because a version bump never re-runs
   history (decision 0012): a replay after a bump serves the stored verdicts and folds the same
   world, so including the version would force pointless full replays. Order is included
   because the bridge runs matching engines in registration order, so reordering can reorder
   claims.
5. The log still holds, at `position`, an event whose `content_hash` is `position_event_hash`
   (read with `read_after` semantics: the first event after the previous position must be
   exactly `position`), and the verdict store's `bridge_cursor` is at or past `position`. This
   catches a replaced, truncated or rolled-back log, and a verdict store behind the snapshot.

**The timeline gets a base.** `Timeline` holds a base world (empty, or the restored snapshot's
world at offset `b`), a live head world folded once per append, and only the events after `b`.
The world at the head is a clone, not a refold, which removes the per-request refold at head
that decision 0006 left to this issue. A world at `o >= b` folds from the base. Offsets below
`b` no longer exist in the process:

- every route that takes an offset (`/world?at=`, `/diff?from=`/`to=`, `/events?from=` and
  `Last-Event-ID`, `/entity/{id}/history?to=`) answers **410 `offset_before_base`**;
- `/time?ts=` with `ts` before the base's last event answers the same code, because the offset
  it asks for lies inside the snapshot, whose per-event times are not kept; returning `b` would
  be a lie;
- `/time` gains `base` (additive; 0 without a snapshot), and entity history covers only offsets
  after `b`.

The first append after a restore clamps against the base's `last_ts`, so a restored timeline
reports the same `head`, `last_ts` and `clamped` as the full one.

**Stored snapshots.** Writes are atomic (a temporary file in the same directory, `fsync`,
rename, directory `fsync`); a same-offset write replaces the older file. The store keeps the
newest three by offset and only ever reads or removes files matching its name pattern.

## Why a base world, not a snapshot of the event list

The event list grows exactly as the log does. A snapshot must be bounded by the world's size,
and log truncation (part 2) makes pre-base history unavailable anyway. The accepted cost: after
a snapshot restart, scrub and history stop at the snapshot offset. The view's scrubber should
start at `/time.base`; that view change is a follow-up issue, and the MCP tools inherit the
contract through the shared query API today.

## Delivery

- **Part 1a (landed):** the query contract above, `Timeline::from_snapshot`, the file format
  and codec, `world_hash`, `fold_hash`, `FOLD_FIXTURE_HASH`, `feed_fingerprint`, the pure
  validity rules, the store, and the golden equivalence test: folding from 0 and restoring a
  snapshot at 0, 1, mid, head-1 and head then appending the tail give equal `world_hash`, over
  the golden log and generated streams.
- **Part 1b (next):** the writer thread and its trigger (every 1,000,000 raw events by default,
  justified by research 0006 §3(d)'s 30-second restart budget), `serve` loading the newest
  valid snapshot and resuming the bridge from its position (`Bridge::resume`, a published
  bridge mark, `LogReader::cursors`, `LogPosition::from_u64`, `QueryState::replace_timeline`),
  a final snapshot on SIGINT/SIGTERM, and `serve --snapshot-every` / `--no-snapshot`.
- **Later:** `mcp --log-dir` loading snapshots (issue), log and verdict compaction (part 2),
  sampled re-derivation that halts on a mismatch (part 3), the view's scrubber floor.

## What validity does not catch

- A fold change on a path the golden fixture does not exercise, with a forgotten
  `FOLD_VERSION` bump.
- A verdict store replaced by another whose cursor is at or past `position`.

Part 3's sampled re-derivation is the control for both. A fixture edit that does not change
behaviour still forces one full replay; that is accepted.

## Alternatives considered

- **JSON for the payload.** Rejected: larger and slower to parse for a world of 10^6
  entities, and it cannot key a map by a struct without an adapter. `postcard` is compact,
  serde-native and stable at 1.x.
- **Hashing engine versions into `feed_hash`.** Rejected above: it would invalidate snapshots
  whose replay is provably identical.
- **Answering `/time?ts=` below the base with `b`.** Rejected: it would claim events that were
  never checked against `ts`.

## Revisit when

Part 1b lands (update the status line), a measurement shows the tail after a 10^6-event
snapshot interval replaying in more than 30 seconds, or truncation (part 2) needs the snapshot
to carry the verdict-store position too.

verify: `cargo test -p s2w-app --test snapshot_golden --test snapshot_base && cargo test -p s2w-app --lib snapshot && cargo test -p s2w-core --test fixture_hash` passes.
