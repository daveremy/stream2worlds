# 0004: The slice-1 scale envelope

Date: 2026-09-27 · Status: accepted (Dave, 2026-09-27) · Gate 2 · Issue #31 · Research [0006](../../research/0006-scaling.md)

## Decision

At launch, one `s2w` process on a 4-core, 16 GB laptop targets:

| Dimension | Target |
|---|---|
| Ingest | 1,000 events/s sustained |
| World | 10^6 live entities in 1 GB |
| Replay | a 10^7-event log replayed in 5 minutes |
| Possible worlds | 20 forks in under 100 ms |

These are **targets, not promises, until measured.** The ingest target rests on a measurement
(fsync per append tops out at 871–1,204 transactions/s on the hub's NVMe; group commit measured
69k/s at 100 events per commit). The memory target rests on arithmetic (about 300 bytes per
entity). The scale fitness function (#32) measures all four. If a measured number differs from
its target by more than 2×, this record gets a dated amendment.

**Non-goals for slice 1:** a parallel fold, a distributed world, logs larger than 10^8 events, and
an LLM call per event. A bigger topic is handled by `--partitions` (read a subset of partitions)
and `--sample 1/N by key` (keep every Nth entity, whole). Shared-state scale-out is a job for
Flink, Kafka Streams or Materialize. A later hosted version scales across users by giving each
user a machine of this size (#35), not by distributing one world.

**Durability stays `synchronous=FULL` for every source.** Throughput comes from group commit (a
batch append that commits every N events or T ms, with cursors in the same transaction; built
with the Kafka source, #7), which meets the ingest target about 69× over. `synchronous=NORMAL`
for replayable sources was considered and declined: it would add a per-source setting, a
power-loss failure mode to test, and a replayability rule, for speed the envelope does not need.
Reopen it only if #32 shows a Kafka workload that group commit cannot carry. This amends
[decision 0002](0002-event-log-storage.md)'s "no batch API in slice 1" once #7 adds one.

## Consequences

- At about 100k new entities an hour, Wikipedia reaches 10^6 live entities in roughly 10 hours,
  so entity aging on log time (#33) is required for gate 4's 7–21-day test windows.
- Every trigger in research 0006 reads a status-line metric (#32); walls are found by
  measurement, not by a crash.

## Measurement, 2026-09-28 (s2w#32)

The scale fitness function now measures two proxies for the targets above. Heap bytes per
entity (`cargo xtask check`, allocator-counted with `dhat`) is gated against
`xtask/scale-baseline.toml` now. Fold instructions per event (`cargo xtask scale`, CI job
`scale`, Valgrind) is gated against the same file; its baseline, 9093 Ir/event, was set from
the first CI run of the `scale` job (run 36527045232, 2026-09-28) and belongs to that CI image.
Append events/s at one event per transaction is reported, not gated. Parse cost, fork cost and
per-partition lag are not measured yet.

The first bytes-per-entity measurement, measured on the synthetic generator, is **830 B**
(dhat live heap after folding 100,000 entities from a seeded generator with Zipf-distributed
attribute counts; test profile; hub, rustc 1.98.1): **2.77×** the 300 B planning figure above,
beyond this record's 2× line. The generator's distribution is chosen, not observed, and the
figure leaves out relationships (234 B each, reported separately) and allocator overhead, while
the 1 GB target is RSS; so 830 B × 10^6 ≈ 0.83 GB does not show that 10^6 entities fit in 1 GB.
By this record's own rule it needs a dated amendment. Ruling, 2026-09-28 (karpathy, on s2w#32):
accept an interim hard ceiling of 900 B per entity (`budget_bytes_per_entity`), so the gate
ships and stops regressions now. The 300 B target does not move; the baseline file carries it
for the printed ratio. The overshoot is tracked in s2w#172: measure where the bytes go, then cut
toward 300 B.

Amendment, 2026-09-29 (s2w#172): a dhat breakdown put 544 B of the 830 B in one `BTreeMap` leaf
node per entity for its attributes. Storing them as a sorted `Vec` at exact capacity (same wire
bytes, `world_hash` pinned) measures **438 B** per entity (1.46×), inside this record's 2× line,
so the interim ceiling is retired and `budget_bytes_per_entity` is 600 B, the 2× line itself.
Relationships are unchanged at 234 B. The remaining gap to 300 B is node overhead in the
`entities` and `keys` maps and repeated type and attribute-name text: s2w#190 (entities by
dense id) and s2w#191 (interning, a snapshot format change).

Amendment, 2026-09-29 (s2w#190): entities are now a `Vec` indexed by their dense id, and
`EntityState` is 56 B (hub refs boxed, absent when empty). Same wire bytes; snapshot bytes and
`world_hash` pinned. Measured **360 B** per entity (1.20×). The `Vec` doubles, so the figure is
lumpy at powers of two: at 100,000 entities it holds 131,072 slots, 73 B per entity.
Relationships are unchanged at 234 B. s2w#191 (interning) is the remaining cut toward 300 B.

Amendment, 2026-09-29 (s2w#202): #198 (entities as a `Vec` by dense id) cut fold cost from 9093 to
5763 Ir/event (-36.6%), measured by CI run 36563251251 (job `scale`) on main. `fold_ir_per_event` is
now 5764 so the gate keeps the gain.

Amendment, 2026-09-29 (s2w#174): the scale gates now also replay a recorded stream, alongside
the synthetic generator, not instead of it. The generator scales to 10^6 events and catches
regressions on a large world, but its distributions are chosen; the recording's are observed.
The recording is 10 minutes of Wikimedia EventStreams `mediawiki.page_change.v1`
(`crates/s2w-app/tests/fixtures/recorded-10min.raw.sse`: 11,667 events, 34.2 MB raw, committed
uncompressed without LFS; the plan estimated about 15 MB, but live traffic ran at about 19.4
events/s). Its mapping yields 58,335 claims, which fold to 11,462 entities and 19,512
relationships. Both supplies, side by side in `xtask/scale-baseline.toml`:

| | Fold Ir per event (CI job `scale`) | Heap bytes per entity (`cargo xtask check`) |
|---|---|---|
| Synthetic generator | 5,764 (the gate; run 36563251251 measured 5,763) | 360 B (1.20×) |
| Recorded fixture | **15,285** per raw event (run 36569430562) | **346 B** (1.15×), 409 B per relationship |

The two Ir figures are not the same unit of work: a synthetic event is one fold input, and a
recorded raw event maps to about 5 claims (58,335 / 11,667), so the recorded fold costs about
3,060 Ir per claim. Both byte figures are inside this record's 2× line, so neither needs a
finding. The recorded gate shares `[memory]`'s 300 B target and 600 B budget. Parse cost
(#166) and fork cost (#167) are still not measured. The fixture is pinned by its FNV-1a 64:
CI job `scale` checks the baseline's `[recorded] fixture_fnv1a64`, while check 13 relies on the
same pin compiled into `crates/s2w-app/tests/support/recorded.rs` (`FIXTURE_HASH`, checked by
`load()`), not on the baseline key.

Amendment, 2026-09-29 (s2w#168): per-partition source lag is now reported, not measured as a
gate. The `Source` seam's `Started` carries a `Watermarks` signal: Kafka tracks the high
watermark each fetch reply already returns, per partition, and the app's position on its stream;
SSE and stdin state that they report none. `watch`'s and `serve`'s human status line prints
`lag p0 12, p1 0` (a `?` until a position is known; `lag not reported` for SSE and stdin, never
0). The `sources` route, MCP and the view gain it in s2w#284. The "per-partition lag are not
measured yet" sentence above predates this.

**Amendment 2026-09-29 (s2w#166): parse cost is measured.** `[parse] parse_ir_per_event` in
`xtask/scale-baseline.toml`, gated at the same 5% by `cargo xtask scale`: every raw event of the
recorded fixture through System 1's `MappingEngine` with the committed linked mapping (the fold
supply's mapping plus a second site entity and a link merging it into the first, so link merges
are inside the measured region), divided by the 11,667 raw events. First figure 215,237 Ir per raw
event, about 14 times the recorded fold's 15,285; the engine's per-event `serde_json` parse of a
~2.9 KB envelope dominates. `JsonClaimsEngine` is not measured: it only deserializes
already-formed events on the stdin bridge, not a raw stream. Fork cost (#167) is still not
measured.
