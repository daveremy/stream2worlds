# 0008: Generic SSE adapter — `Opaque` dialect, reconnect-forever on untagged frames

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #49

## Decision

Decision 0003's Wikipedia-specific SSE loop is now split into a generic transport
(`crates/s2w-sources/src/sse/{mod,connect,frame}.rs`) and a per-stream [`SseDialect`] trait
(`sse/dialect.rs`): how an `id:` becomes a cursor, how to ask for a start time, and which frames
to keep. `Wikimedia` implements the existing behavior; `Opaque` is the default dialect for a bare
`sse://`, `https://` or `http://` target with no preset — the `id:` verbatim as the cursor, no
`--since` support (a generic stream's resume point is whatever cursor the log already stored),
and every payload kept.

A frame's `id:` is required for `Opaque` to have a cursor. A frame with no `id:` (or an empty
one) cannot be resumed from, so `Opaque::cursor` returns an error, which the loop reports as
`SourceError::Skipped` and counts toward `MALFORMED_ID_LIMIT` (3, unchanged from 0003) — after
three in a row the loop forces a fresh connection, same as three malformed Wikimedia
`Last-Event-ID`s do today.

`SseDialect` also decides what bytes get stored (`store`, added round 3, codex astra BLOCK):
`Opaque` envelopes each accepted frame as `{"data":…,"id":…}` (`sse::envelope`) because an
arbitrary target carries no guarantee that `data:` alone is unique per event — without the
cursor folded in, two distinct events with identical `data:` collapse under the log's
`(source, payload)` content-hash dedupe, exactly the failure mode Kafka's own envelope
(`kafka::envelope`) exists to prevent. `Wikimedia` overrides `store` to keep its default: the
raw `data:` bytes verbatim, since its payload already carries a stream-unique `meta.id` and
changing the stored bytes would both break dedupe against logs already written by the
pre-envelope build and break the fold, which parses this payload as Wikimedia's own JSON
shape. The choice is per-dialect, not per-transport, so a future preset picks whichever fits
its own payload's identity guarantees.

## Why

The generic transport cannot know, for an arbitrary target, whether a given stream is expected
to carry `id:` fields at all — that is stream-specific knowledge decision 0003 already assigned
to the dialect, not the transport. A stream that never sends `id:` therefore reconnects every
`MALFORMED_ID_LIMIT` frames forever. This is accepted behavior for `Opaque`, not a bug: the
alternative (a stream-agnostic "no id ever means fatal" or "no id ever means never skip") both
require guessing a per-stream property the transport doesn't have.

The forced reconnect is silent progress, not a hang or a crash — each connection attempt still
resends the last cursor seen (`None`, before any `id:` is ever parsed successfully) and the
stream keeps delivering `Skipped` events an operator can see in the log. A stream that never
carries ids is a genuinely bad fit for this adapter's cursor/resume model; the operator's signal
is the recurring `Skipped` reason, not a special-cased error path.

## Revisit when

A generic target that needs resume-without-ids shows up (e.g. an SSE proxy that never sets
`id:` but is otherwise well-behaved) — at that point `Opaque` would need either a
sequence-number-as-cursor fallback or a documented "this source cannot resume" mode, rather than
reconnecting forever.
