# 0021: World snapshots and the timeline base

Date: 2026-09-28 · Status: accepted (parts 1a and 1b landed) · Gate 2 · Issue #33 · Research [0006 §3(d)](../../research/0006-scaling.md) · Amends [0006](0006-world-query-api.md) (a new error code and `/time.base`) · Resolves open items in [0005](0005-pure-fold.md) and [0012](0012-verdict-log.md)

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
- **Part 1b (landed):** `serve` restores the newest valid snapshot and resumes the bridge
  after its position (`Bridge::resume`, `Bridge::mark`, `LogPosition::from_u64`,
  `QueryState::replace_timeline`), writes snapshots on a dedicated thread (every 1,000,000 raw
  events by default, justified by research 0006 §3(d)'s 30-second restart budget), writes a
  final snapshot on SIGINT/SIGTERM, and takes `--snapshot-every <n>` / `--no-snapshot`. The
  lifecycle and its measurements are in the next section. `cursors` is recorded empty: `serve`
  does not enumerate source cursors yet, and the field is never consulted at load.
- **Later:** `mcp --log-dir` loading snapshots (issue), log and verdict compaction (part 2),
  sampled re-derivation that halts on a mismatch (part 3), the view's scrubber floor.

## Serve lifecycle (part 1b)

**Start.** Before the listener binds, `serve` removes temporary files a crashed write left
behind, loads the newest file that passes every validity rule (rule 5 reads the log with
`previous = position - 1`), installs it with `QueryState::replace_timeline`, and checks that
the restored base and head both equal the snapshot's offset. The bridge then starts with
`Bridge::resume(position)`, which refuses a timeline whose head is past its base. No valid
file, or an unreadable directory, means a full replay from offset 0; every file skipped and an
unreadable directory are reported, an absent directory is not.

**Capture.** After every successful `poll_once`, with no `.await` in between, the snapshotter
records a checkpoint: the bridge's `mark()` (last consumed position and its stored
`content_hash`) and the timeline head. When `every` raw events have been consumed since the
last attempt and the writer is idle, it clones the head world and queues it; a busy writer
leaves the snapshot due for the next poll. The capture refuses a head that has moved past the
checkpoint. Only the clone runs on the bridge's thread; encoding and `fsync` run on the
`s2w-snapshot` thread.

*Deviation from plan amendment A3.* The plan captured only after a poll whose report carried
no error. The implementation captures after every `Ok` poll, because a poll that reports an
engine or log error still commits a consistent prefix: `poll_once` appends only the claims of
the events it judged and moves `mark` to the last of them, so the checkpoint and the head
agree. Skipping those polls would only delay snapshots on a feed with frequent per-event
errors. A poll that returns `Err` (the bridge is stopping) captures nothing.

**Stop.** On the first SIGINT or SIGTERM, before the bridge is dropped and before the HTTP
drain, `serve` waits for any in-flight write and then writes a final snapshot if at least
100,000 raw events arrived since the last one written (`SHUTDOWN_MIN`). Below that the tail
replays in seconds, and a new snapshot would move the base to the head and cost the restarted
process its scrub history for no real saving. A failed write does not count as written, so
the stop tries again. The final write blocks the current-thread runtime, so a second signal
during it is not observed; the write is atomic, so `SIGKILL` mid-write leaves the previous
snapshot intact. A fatal error never writes a final snapshot. The signal handlers are registered once the
listener is bound, after the restore; a SIGTERM before then takes the default action, which
loses nothing because no snapshot is owed yet.

**Budget and memory (measured).** The demo unit (`s2w-wiki.service`) has `TimeoutStopSec=30`
and `MemoryMax=1G`; the HTTP drain may take 5 s, leaving 25 s for the final write. The
ignored test `tests/snapshot_memory.rs` measures a synthetic wiki-shaped world (per raw event:
one page observation with two attributes and one `edited` relationship; pages repeat every
n/2 events, users every n/20) on hub, release build:

| raw events | world resident | write: peak extra, time (incl. `fsync`), file | restore (fresh process): peak, time |
|---|---|---|---|
| 10^5 | 92 MiB | +83 MiB, 0.18 s, 4.8 MiB | +163 MiB, 0.34 s |
| 10^6 | 665 MiB | +919 MiB, 2.4 s, 50 MiB | +1,624 MiB, 5.0 s |

The final write fits the stop budget by a factor of ten at 10^6 events. Memory does not: a
restored timeline holds two worlds (base and head), and a write briefly holds a second copy
of the head, so near 10^6 events of this shape `serve` would exceed 1 GiB. Without snapshots
the same process holds the head world plus the event list since offset 0, which is larger
still, so the demo box cannot reach that size either way. Sharing the base with the head until
the first append after a restore would halve restore memory; that is #179.

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

A measurement shows the tail after a 10^6-event
snapshot interval replaying in more than 30 seconds, or truncation (part 2) needs the snapshot
to carry the verdict-store position too.

verify: `cargo test -p s2w-app --test snapshot_golden --test snapshot_base --test bridge_replay && cargo test -p s2w-app --lib snapshot && cargo test -p s2w-core --test fixture_hash` passes.
