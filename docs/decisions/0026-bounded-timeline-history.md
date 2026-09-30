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

**2026-09-29, #220 PR C (mimalloc measurement target; the head world has grown).** Re-measured
on `main` @ f6900a3, with no product change. The recorded fixture's discovered mapping
now yields 20,040,997 world events and 224,586 entities (PR B: 11,266,766 world events). The head
world alone is **887.3 MiB** resident (PR B: 403 MiB; the change is in discovery since PR B, likely #261 or #276, not bisected), so the 600 MiB budget no longer holds
even at history cap 2. The default `bridge` assertion fails on `main`. It is `#[ignore]`d, so CI
does not run it. Batch 250, cap 2, release build, 3 runs each, interleaved on a shared host:

| Whole-process peak, cap 2 | glibc malloc | mimalloc (`backfill_memory_mimalloc`) |
|---|---|---|
| `bridge` (main thread) | 1,027.4 [1,027.2-1,027.8] MiB | **943.7 [942.8-945.7] MiB** |
| `bridge-run` (blocking pool) | 1,055.6 [1,055.4-1,055.8] MiB | **954.1 [952.7-957.1] MiB** |
| Resident 2 s after the backfill's state is dropped, `bridge-run` | 1,021.6-1,048.9 MiB | 71.0-932.6 MiB (2 of 3 runs under 83 MiB) |

mimalloc saves 84 MiB on the main thread and 102 MiB on the blocking pool, which clears the
≥40 MiB bar #220's plan set for an allocator swap. Under glibc, the memory stays resident after
the drop, as it did on the demo box (906 MiB with no world). The cap was not raised: no cap fits
600 MiB at this world size. The budget and the product allocator swap wait on a ruling in #220.

**2026-09-29, s2w#243 (PR 1 of s2w#235: where the `/world` hold goes).** The `viewer` child
now runs `S2W_BACKFILL_MEMORY_VIEWERS=N` phase-staggered readers and records each body's hold on
the server (`QueryState::with_read_timings`, opt-in): `build` (guard to `HeadView::new`) and
`write` (serialization with the guard held). On `main` @ 156903a, 3 runs per row, loaded host
(load average 8-22), median [min-max]:

| Row | Peak | Wall | Slowest `/world` | Build share of the hold |
|---|---|---|---|---|
| 1 viewer, 5 s | 875 [860-888] MiB | 115 [70-140] s | 2.0 s | 0.37 [0.37-0.38] |
| 4 viewers, 5 s | **1861 [1732-1874] MiB** | 419 [399-494] s | 8.1 s | 0.39 [0.38-0.39] |
| 1 viewer, 1 s | 1087 [1085-1088] MiB | 592 [518-697] s | 3.5 s | 0.37 [0.37-0.37] |
| One read of the full head, no fold | - | - | - | 0.43 [0.39-0.43] |

A build share under 0.4 means releasing the guard after the projection shortens the hold about
2.6x, so s2w#235 proceeds with an owned projection over `Arc<EntityState>` plus single-flight
generations. Copying the four maps instead (a `World::clone` handoff) would cost 221.5 MiB (dhat)
and about 0.62 s per generation, so that option is ruled out. Four viewers exceed 1 GiB on `main`,
and one viewer at 1 s now does too (after the batch-250 change above). Full table:
[s2w#235](https://github.com/daveremy/stream2worlds/issues/235).

**2026-09-30, s2w#282 (the budget re-derived from the post-#291 world).** s2w#282 bisected the
head world's growth from 403 to 887 MiB across #255..#280 on the recorded load. #261
(`PROFILER_VERSION` 4) added 6.11M world events and 347 MiB by promoting edit counters, a byte
size and edit-summary text to entity types: a regression, fixed by #291. #276 (version 5) added
2.67M and 136 MiB for the revision entity, which research 0009 scores (`revision` recall 0 → 1.0):
expected. Per-commit table: [s2w#282](https://github.com/daveremy/stream2worlds/issues/282).
Re-measured on `main` @ 9a672f0 (`PROFILER_VERSION` 7), history cap 50,000, batch 250, glibc,
release build, loaded host (load average 4-8, a workspace build running alongside):

| Part | #255 (the 600 MiB budget) | #280 | **9a672f0 (post-#291)** |
|---|---|---|---|
| World events | 11,266,766 | 20,040,997 | **14,323,096** (213,153 entities) |
| Head world alone | 403 MiB | 887 MiB | **598.9 MiB** (postcard 141.4 MiB) |
| World plus the capped timeline | 427 MiB | 913 MiB | **623.5 MiB** |
| `bridge`, whole-process peak | 548.5 MiB | 1,131.9 MiB | **780.8 [780.6-780.8] MiB** (3 runs) |
| `viewer` (1 viewer, 5 s tick), whole-process peak | 851 MiB | not reached | **1,280.0 [1,270.8-1,280.0] MiB** (2 runs) |
| One `/world` projection at the head (`queries`) | | | 1,248 MiB peak; body 292 MiB |

The bridge's overhead above the head world is now 182 MiB (145 MiB at #255). The budget follows
the world, so both limits in `tests/backfill_memory.rs` move to the measured peak plus a margin:
**`SERVE_PEAK_LIMIT` 810 MiB** (780.8 MiB + 29 MiB, the same margin the 600 MiB figure had) and
**`VIEWER_PEAK_LIMIT` 1,320 MiB** (1,280.0 MiB + 40 MiB; the viewer spread measured 28 MiB in
#243). The history cap stays 50,000: cap 2 was about 11 MiB under cap 50,000 at #255, so no cap buys back
a 180 MiB step in the world. The viewer limit is now above the demo box's `MemoryMax=1G`, so the
box cannot run this `main`; it stays on b207c74f until its `MemoryMax` is raised (lifeos
`deploy/demo-box`, proposed in s2w#282's PR). Neither child covers serve's snapshot encode (the
world encodes to 141 MiB), the HTTP server or SSE, and four viewers measured 2.1x one viewer's
peak in #243, so the box needs headroom above 1,320 MiB.

`tests/discovered_types.rs` now reports each discovered entity type's share of world events on
every PR and fails when the set of types changes, and `.github/workflows/nightly-memory.yml` runs
this test's default sweep nightly on the hub runner, so a doubling can no longer land unseen.
Its first run on 9a672f0 shows two types the #282 ruling calls attributes still discovered:
`revision/comment` (edit-summary text, 5.2% of world events) and `mediainfo/content_size` (a byte
size, 0.4%).

## Alternatives considered

- **Keep a base world and advance it at each drop.** Rejected: a second 403 MiB world does not
  fit, and re-encoding it at each drop costs ~0.35 s encode plus ~0.75 s decode per drop.
- **Compact the event list.** Rejected alone: 1.8 GiB encoded is still three times the budget.
- **A bigger box.** Not chosen: Dave's call, and it costs money.
- **A byte cap or a CLI flag.** Deferred: a count cap is enough to meet the budget measured here.

## Revisit when

The serve-side peak on the recorded load crosses `SERVE_PEAK_LIMIT` (810 MiB since s2w#282) again, the nightly `nightly-memory` run fails, older history from disk lands,
or a stream's world events are much larger than the fixture's (~560 B with their delta).

verify: `cargo test -p s2w-app --lib query::timeline && cargo test -p s2w-app --test snapshot_golden --test snapshot_base` passes, and `cargo test --release -p s2w-app --test backfill_memory -- --ignored` passes.
