# 0026: A bounded timeline history

Date: 2026-09-29 · Status: accepted (PR 1 of #216) · Gate 2 · Issue #216 · Amends [0024](0024-snapshots.md) (what a timeline keeps and what it serves after a restore)

## Context

The demo box was OOM-killed at `MemoryMax=1G` about 6 s into a backfill (#216). The timeline
kept every world event since offset 0 so any offset could be refolded, and on the recorded
fixture that history, not the world, was the memory. `tests/backfill_memory.rs` measures the
fixture cycled to 1.5×10^5 raw events (fresh strings every cycle) under the v2 profiler mapping:
75 claims per raw event, 11,266,766 world events, release build, one fresh child per part.

| Part (before this decision) | Resident |
|---|---|
| Head world alone | 403 MiB |
| World plus every world event (the old timeline) | 6,093 MiB, ~530 B per world event |
| Real `Bridge` over a SQLite log | 6,131 MiB |
| World events, postcard-encoded | 1,801 MiB (160 B each) |

Compacting the events (1.8 GiB) cannot meet the budget; the history has to be capped. The head
world alone is 403 MiB, so the budget cannot hold a second world either: not a base world, and
not one per SSE follower (each `/events` subscriber used to fold its own).

## Decision

**The timeline holds one world, the head, and at most `history_cap` recent world events**
(`DEFAULT_HISTORY_CAP = 50,000`; 20,000 until #220 PR B), each stored with the `Delta` its fold produced.
`Timeline::new` and `Timeline::from_snapshot` apply the default, so every serve, rebuild and
MCP construction site is bounded without a flag.

- **Dropping.** When more than `history_cap` events are retained, the oldest are dropped down to
  `history_cap / 2` (amortized O(1) per append). The dropped events' first and last timestamps
  fold into the timeline's base time, so `/time`'s `first_ts`, `last_ts` and `clamped` still
  span every event.
- **Full history.** While every event since offset 0 is retained (a new timeline, or a restore
  of an offset-0 snapshot, `world.offset() == 0`), nothing changes: any offset is refolded from
  the empty world, so the timeline never holds a base world. After the first drop, or after a
  restore above offset 0, it no longer has full history.
- **World queries without full history** serve the head only: `/world?at`, `/diff`, and
  `/time?ts` answer 410 `offset_before_base` for anything older, with `base` = the head.
  `/entity/{id}/history` needs every event since offset 0, so it is 410 at any `to`
  (checked after the bounds check, before the unknown-entity check), never a partial list.
  `/time?ts` maps a time at or after the newest event to the head, anything earlier to
  `offset_before_base`; it never names an offset the world queries would refuse.
- **`/events` replays the retained window from the stored deltas**, with no world per
  follower. `/time` gains `replay_base`, the offset before the oldest retained event; `from`
  (or `Last-Event-ID`) below it is 410 `offset_before_base`, and a live follower overtaken by
  a drop gets the existing final `event: error` frame. The window a follower can resume from is
  at least `history_cap / 2` events. `events_start` checks, in order: the epoch, `at >= from`,
  `from` against `replay_base..=head`, then `at` against the head.
- **The web page** treats `offset_before_base` on its live stream like `stale_epoch`: it
  restarts from a fresh `/world` read instead of reconnecting from an offset that is gone. Its
  500-event evidence seed starts empty when those events are gone.
- **`TimedEvent` gains `delta`.** It is never serialized or persisted (snapshots store the world
  and its `BaseTime`), so there is no compatibility concern.

**The trade, plainly.** 50,000 world events is about 670 raw events at 75 claims per event: on
the demo, time travel and SSE resume reach back seconds to minutes, not the whole stream, and
after a restart from a snapshot, world queries serve the head only. Serving older history from
snapshots plus the log is a separate issue ([#218](https://github.com/daveremy/stream2worlds/issues/218)).

## Measured (2026-09-29, `cargo test --release -p s2w-app --test backfill_memory -- --ignored --nocapture`)

| Part (after) | Resident / peak |
|---|---|
| World plus the capped timeline | 410 MiB |
| Real `Bridge` backfill, whole process, no viewer | **588 MiB peak** (baseline 6 MiB) |
| Same, history cap 100,000 | 631 MiB peak |
| Same, history cap 2 | 576 MiB peak |

The bridge child asserts the whole-process peak stays under 600 MiB. The cap was first planned
at 100,000 (#216's ruling: "about 1,300 raw events"); at 631 MiB that misses the budget, so it
is 20,000. The history is no longer the cost: with a cap of 2 the process still peaks at
576 MiB, about 170 MiB over the head world, in the bridge's own transients and allocator
fragmentation. That is the next lever (#220): shrinking it is what lets the cap rise again. A
second, smaller one: once the window has dropped events, no reader uses a retained event's
`WorldEvent`, only its `Delta`, so dropping the event after the first trim would shrink the
window further.

*(Superseded by the PR 2a entry below; the figures stay as its "before" evidence.)*
**Not covered: a connected viewer.** One `/world` read at the head clones the head world and
projects it: +1.1 GiB resident, +1.4 GiB peak on this load; one `/diff` +1.5 GiB (+2.3 peak).
The 600 MiB figure is for a backfill with no viewer. Serving `/world` and `/diff` at the head
without cloning the world is PR 2 of #216, whose finish line is `demo: PASS` on the box.

**2026-09-29, #216 PR 2a (no world copy).** `/world` and `/diff` now read the head where it lies,
under the read lock, and `/diff` with `from == to` projects nothing. The `viewer` variant runs the
same backfill with a reader asking for `/world` and `/diff` of the head every second through the
real router (same host, same run order; "Before" is this branch's `backfill_memory.rs` run
against the `s2w-app` source of `main` at 3fc07c0):

| Part | Before | After |
|---|---|---|
| One `/world` at the head: projection | 1,406 MiB peak (with the copy) | 920 MiB peak |
| Its JSON body | 194 MiB | 194 MiB |
| One `/diff` head..head | 2,261 MiB peak | 0 |
| Backfill with a viewer, whole process | 3,983 MiB peak | **1,700-1,711 MiB peak (3 runs)** |
| Slowest `/world` (bounds one read-lock hold) | 2.9 s, lock not held | 3.4-3.8 s, lock held |
| Backfill wall time with a viewer (no viewer: 43-48 s) | 54 s | 69-77 s |

Still over the 1 GiB finish line: the owned view (138,462 nodes, 1,225,116 links) and its body
are the rest, which the streamed `/world` of PR 2b removes. Holding the lock through the
projection slows the fold by about half under a 1 s viewer; the page now refetches at most
every 5 s. The projection still runs on the HTTP runtime's thread, as it did before, but now
with the lock held: once the bridge is waiting to append, every other read waits behind it too.
PR 2b moves the projection to `spawn_blocking`.

**2026-09-29, #216 PR 2b (streamed `/world`, on 2b-i's one write lock per poll batch).**
`/world` now streams from the head with no owned view, on `spawn_blocking`, and a quiet head
answers 304 from its `ETag` before any projection. The `viewer` variant's tick is now
`S2W_BACKFILL_MEMORY_VIEWER_TICK_MS` (default 5000, the page's refresh cadence) and it sends
`If-None-Match` as the page does. Three runs on a loaded host (load average 14-17; the same-run
no-viewer `bridge` took 54.8-115.7 s against its quiet 43-48 s):

| Part | 5 s tick (the page) | 1 s tick (worst case) |
|---|---|---|
| Backfill with a viewer, whole process | **876-902 MiB peak (2 runs)** | **923 MiB peak** |
| Backfill wall time with a viewer (no viewer: 43-48 s) | 60-65 s | 207 s |
| Slowest `/world` (bounds one read-lock hold) | 2.2-2.5 s | 4.1 s |
| Slowest `poll_once` | 2.4-2.7 s | 5.1 s |
| `/world` answered 304 | 0 of 12-13 | 0 of 115 |

Both cadences are under the 1 GiB finish line, and the `viewer` variant now asserts it. The
lock is held through projection and serialization, and a poll batch waits out one whole `/world`,
so the fold's slowdown scales with read frequency: one page at 5 s costs about 1.3x, a 1 s
reader about 4.5x. No 304s during a backfill, because the head moves every batch. Many
concurrent viewers act like a faster tick; a shorter guard (a snapshot `Arc<World>` handoff) is
s2w#235.

**2026-09-29, #220 PR B (poll batch 250, rows freed before the fold, cap 50,000).**
PR A's knobs (`S2W_BACKFILL_MEMORY_HISTORY_CAP`, `_BATCH`, the `bridge-run` variant) measured
where the bridge's ~170 MiB went: a Rust heap peak of 470 MiB (dhat) against 589 MiB resident,
and batch size as the largest lever
([results](https://github.com/daveremy/stream2worlds/issues/220#issuecomment-5897477984)).
`BridgeConfig::default().batch` is now 250 (it was 1000), and `poll_once` frees a batch's events
and verdict rows once they are committed, before the fold grows the world. `mcp/replay.rs`'s
catch-up uses the same default, so it now reads the log in batches of 250. `bridge` polls on the
test's main thread, as before; `bridge-run` polls on the tokio blocking pool, as `Bridge::run`
does in serve. Release build, runs interleaved on a shared host:

| Whole-process peak | Before (batch 1000, 3 runs) | `bridge`, after (3 runs) | `bridge-run`, after (5 runs) |
|---|---|---|---|
| History cap 2 | 577.2 MiB | 520.3-520.4 MiB | 537.0-538.5 MiB |
| History cap 20,000 | 589.0 MiB (`bridge-run`: 589 or ~646) | 530.9-531.0 MiB | 531.2-547.8 MiB |
| History cap 100,000 | 631.7 MiB | 578.1 MiB | 588.6-591.6 MiB |
| **History cap 50,000 (the new default)** | | **548.3-548.5 MiB** | **548.5-563.0 MiB** |
| Backfill with a viewer (5 s tick), cap 50,000 | 876-902 MiB (cap 20,000) | 871.6-882.9 MiB (2 runs) | |

The early drop saved nothing measurable (531.0 MiB against PR A's 530.9 at batch 250); the batch
size is the whole saving. `bridge-run`'s second mode, about 60 MiB higher at batch 1000, shrank to about
15 MiB. Wall times on the shared host were too noisy to compare; PR A's quieter sweep measured
73.2 s at batch 1000 and 73.4 s at 250. Each retained world event costs about 0.53 KiB
(`bridge-run`) to 0.59 KiB (`bridge`). The cap is the largest multiple of 10,000 whose predicted
peak, from the worst cap-2 run of either topology (538.5 MiB) plus 0.59 KiB per event (the larger
slope), stays under 570 MiB (a 30 MiB margin under the asserted 600 MiB). `bridge` still asserts 600 MiB;
`bridge-run` is reported and not asserted, because a bimodal value would flake. Neither variant
includes serve's snapshot encode, HTTP server or SSE, so the demo box holding at 1 GiB is inferred
from these runs rather than measured there. The allocator swap is #220's PR C.

## Alternatives considered

- **Keep a base world and advance it at each drop.** Rejected: a second 403 MiB world does not
  fit, and re-encoding it at each drop costs ~0.35 s encode plus ~0.75 s decode per drop.
- **Compact the event list.** Rejected alone: 1.8 GiB encoded is still three times the budget.
- **A bigger box.** Not chosen: Dave's call, and it costs money.
- **A byte cap or a CLI flag.** Deferred: a count cap is enough to meet the budget measured here.

## Revisit when

The serve-side peak on the recorded load crosses 600 MiB again, older history from disk lands,
or a stream's world events are much larger than the fixture's (~560 B with their delta).

verify: `cargo test -p s2w-app --lib query::timeline && cargo test -p s2w-app --test snapshot_golden --test snapshot_base` passes, and `cargo test --release -p s2w-app --test backfill_memory -- --ignored` passes.
