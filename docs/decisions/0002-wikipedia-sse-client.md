# 0002: Hand-roll the Wikipedia SSE loop on reqwest

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

## Revisit when

A second SSE source can share enough parsing and reconnection policy to justify a generic client,
or reqwest is removed from the source-adapter allowlist.
