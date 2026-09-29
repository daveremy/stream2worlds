# 0003: Hand-roll the Wikipedia SSE loop on reqwest

Date: 2026-09-27 · Status: accepted · Gate 2

## Decision

The Wikipedia source uses a small in-crate SSE parser and reconnect loop on `reqwest` 0.13 and
`tokio`. It implements `Stream` directly by receiving from a bounded channel fed by an owned
task; dropping the stream aborts that task even during HTTP I/O or backoff.

The loop retains `since=` across connection attempts until it has parsed a complete frame with a
valid Wikimedia `Last-Event-ID`. From then on it sends that exact ID as a header and omits
`since=`. The ID is also decoded into per-topic positions, choosing an offset over a timestamp
when both are present. Complete canary and `examplewiki` frames advance the cursor before being
filtered. Incomplete frames and frames with malformed IDs never advance it.

## Why

Wikimedia deliberately terminates connections after about 15 minutes, and its resume ID is a
JSON array of topic/partition positions rather than an opaque application cursor. The source
must arbitrate `since=` against `Last-Event-ID`, persist the structured position, and implement
poison-frame recovery regardless of which generic SSE client parses the wire format. Keeping
that protocol-specific behavior in one short loop avoids a dependency without hiding any of the
hard logic.

`reqwest` uses its rustls feature with default features disabled, avoiding an OpenSSL build. The
parser accepts LF, CRLF, and CR line endings and accumulates byte chunks before interpreting
lines. A bounded channel gives backpressure; capped exponential backoff handles response,
transport, and clean-disconnect retries.

## Alternatives considered

- `eventsource-client`: actively maintained and reconnect-aware, but adds a generic transport
  layer while the Wikimedia-specific cursor loop remains necessary.
- `sse-reqwest-client`: functionally close, but very new and single-maintainer at the time of
  research; it still does not remove cursor arbitration.
- `reqwest-eventsource`: pinned to reqwest 0.12 and appeared unmaintained, which would duplicate
  the HTTP stack.
- `eventsource-stream`: parser-only and dormant since 2022; the parser needed here is small and
  Wikimedia-specific recovery still has to be implemented locally.

## Research 0003 open-item disposition

- Static-musl size measurements for rskafka, axum, rmcp, redb, and parquet, plus Kafka timestamp
  checks and MCP dependency comparisons: N/A to this reqwest/tokio SSE choice.
- Re-verifying DBSP's licence carve-out, transitives, and runtime-free features on bumps: N/A to
  this choice.
- Watching rskafka, rmcp, fjall, and Differential Dataflow release/commit activity: N/A to this
  choice.
- Missing independent append-only large-value benchmarks and literal `world_id`-as-DBSP-key
  prior art: N/A to this choice.

## Open item introduced here

How should protocol fixtures be recorded without turning tests into live network tests?
Resolved: preserve a byte-for-byte live capture, including IDs, with its UTC timestamp and exact
`curl` command in comments at the top. Keep hand-written poison, partial, canary, and
`examplewiki` frames in a separately named synthetic fixture.

Two fixtures live under `crates/s2w-sources/testdata/`: `wikipedia-page-change.raw.sse` (the
live capture, 4.6MB / 1615 frames, no canary/`examplewiki` frames) and
`wikipedia-malformed.synthetic.sse` (hand-written: canary, `examplewiki`, partial, and
malformed-ID frames). Every frame with a malformed `Last-Event-ID` immediately surfaces
`WikipediaSourceError::InvalidLastEventId`; after `MALFORMED_ID_LIMIT = 3` such frames in a row,
the source also forces a fresh connection from the last good cursor rather than continuing to
read from a stream that keeps producing bad IDs.

2026-09-29 (s2w#174): a third live capture, `crates/s2w-app/tests/fixtures/recorded-10min.raw.sse`
(34MB / 11,667 frames, same stream, same recording rule), is the scale gates' recorded input.
Its provenance and licence are in that directory's `README.md`.

## Redelivery/dedup on `--since` resume is deferred to the log, not this source

The real capture's cursors are timestamp-based for the `eqiad` partition (`codfw` carries
`offset: -1`), and 12 timestamps in the capture are shared by two events. A Kafka
timestamp-seek on resume can therefore redeliver the last-seen event (inclusive seek) or skip a
same-millisecond sibling (exclusive seek) — this source cannot fix that on its own, because a
timestamp alone does not identify an event.

The issue's "no gap, no duplicate" acceptance criterion is a system-level guarantee (source +
log), not a per-source one: the append-only log (#8) is SQLite-backed and can dedupe on a
primary key derived from the event's identity, which is the layer that actually has enough
information to do so. This source's job is to never silently drop an event across a reconnect;
a rare duplicate on `--since` resume is the log's job to collapse. Tracked as a named follow-up:
[#25](https://github.com/daveremy/stream2worlds/issues/25).

## Revisit when

A second SSE source can share enough parsing and reconnection policy to justify a generic client,
or reqwest is removed from the source-adapter allowlist.

## Amendment, 2026-09-27: superseded by the generic transport + dialect split (#49)

That second source arrived: `sse://`/`https://`/`http://` targets and future presets. The
transport, connection, and frame-parsing code described above (`sse/{mod,connect,frame}.rs`) is
now generic and shared; what is left as Wikimedia-specific is the `Wikimedia` [`SseDialect`]
(`presets/wikimedia.rs`; `sse/dialect.rs` holds the generic `Opaque` dialect): the resume-ID
arbitration, the per-topic position decoding, and the canary /
`examplewiki` filter this decision describes. See decision
[0008](0008-generic-sse-adapter.md) for the dialect split and the default `Opaque` dialect's
no-`id:` behavior. Everything else in this decision (fixtures, `MALFORMED_ID_LIMIT`, the
redelivery/dedup deferral to the log) still holds unchanged.
