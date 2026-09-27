# 0002: SQLite event-log storage

Date: 2026-09-27 · Status: accepted · Gate 2

## Decision

The slice-1 append-only event log uses SQLite through `rusqlite` 0.40.2 with its `bundled`
feature. The database runs in WAL mode with `synchronous=FULL`, foreign keys, and recursive
triggers enabled. Each append writes one event and advances that source's cursor atomically:
**one transaction per append; no batch API in slice 1**.

The log takes a process-level writer lock before opening SQLite. Schema triggers reject updates
and deletes from the events table, while the Rust API exposes append as its only mutation.

## Why

One store and one commit remove the failure modes found in review of the earlier two-store
design:

1. SQLite WAL recovery owns torn-tail and corruption recovery rather than a hand-written
   classifier.
2. The event and cursor live in one transaction, so a failed append means "not stored" and is
   safe to retry.
3. SQLite's commit protocol owns fsync ordering before checkpoints.
4. Commit cadence is explicit and matches the API: one transaction per append.

`rusqlite` is MIT licensed and bundled SQLite is public domain. The bundled build costs some
first-build time but avoids a host SQLite dependency. Research note 0003's open sled-status and
append-only benchmark questions concern rejected alternatives, not this atomicity-based choice.

## Alternatives considered

- **A checksummed segment file plus redb cursor index.** [Research 0003
  §3](../../research/0003-rust-substrate.md#3-embedded-storage-for-the-event-log) preferred this
  for replay throughput, but review found torn-tail classification, two-store atomicity, fsync
  order, and commit-cadence hazards. [Issue
  #19](https://github.com/daveremy/stream2worlds/issues/19) reopens this design if measured
  replay speed requires it.
- **redb for events and cursors.** A single embedded store could be atomic, but SQLite has a
  mature recovery protocol and directly testable schema invariants.
- **sled.** Its maintenance status remained an open research question, and it offers no benefit
  that outweighs SQLite's selected atomicity and recovery properties here.

## Revisit when

Measured replay throughput requires segment files, or append throughput requires a batch API;
track both with issue #19. The SIGKILL test proves transaction atomicity and recovery after a
process dies. It does not prove `synchronous=FULL` fsync durability under an operating-system
crash or power loss, which a child-process SIGKILL cannot exercise.
