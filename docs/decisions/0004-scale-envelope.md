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
