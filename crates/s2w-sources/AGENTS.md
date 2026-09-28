# s2w-sources

Stream sources, resolved by URI scheme through `registry.rs`: Kafka by partition assignment
(`kafka/`), a generic Server-Sent Events transport (`sse/`), and stdin NDJSON (`stdin.rs`).
`presets/` are named real streams expressed as adapter + config, not their own adapters —
`wikipedia` is the `sse` transport with the `Wikimedia` dialect and the Wikimedia URL.

`sse/` splits the transport from what a particular stream means: `sse/mod.rs` (the `Source`
impl and the read loop), `sse/start.rs` (the pure stored-cursor versus `--since` decision),
`sse/connect.rs` (the HTTP connection and reconnect backoff), `sse/frame.rs` (wire parsing),
`sse/dialect.rs` (the `SseDialect` trait plus `Opaque`, the
default dialect for a bare `sse://`/`https://`/`http://` target). `Wikimedia` lives under
`presets/wikimedia.rs`, since it is a preset, not a transport.

## Allowed dependencies

- `s2w-model`, `reqwest`, `rskafka`, `tokio`, `tokio-stream`, `serde_json`, `thiserror`; the
  HTTP/runtime choices are recorded in decision 0003 (and its generalization in decision 0008),
  the Kafka client in decision 0007

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Kafka: explicit partition assignment only; never joins a consumer group, never commits offsets.
- Kafka never skips an offset: fetch errors retry from the same offset; a resume offset deleted
  by retention or past the partition's end is a fatal, loud error.
- Kafka payloads are the byte-deterministic envelope from `kafka::envelope` (offset inside), so
  the log's content-hash dedupe collapses redeliveries but never distinct records.
- Whether an SSE payload is stored raw or enveloped is the dialect's call
  (`SseDialect::store`), not the transport's: `Opaque` (the generic `sse://`/`https://` path)
  envelopes `data:` with its cursor id (`sse::envelope`) because an arbitrary stream carries no
  guarantee that `data:` alone is unique — without the cursor folded in, two distinct events
  with identical `data:` would collapse under the log's `(source, payload)` dedupe. `Wikimedia`
  overrides `store` to keep the raw `data:` bytes verbatim: its payload already carries a
  stream-unique `meta.id`, and changing the stored bytes would break dedupe against logs
  already written by the pre-envelope build and break the fold, which parses this payload as
  Wikimedia's own JSON shape.
- Every event leaves with its source cursor; reconnects resume from the cursor (SSE: the
  dialect's cursor, sent back as `Last-Event-ID`).
- Every SSE request uses a descriptive `User-Agent` (Wikimedia's policy, applied to every
  target, not only Wikimedia's).
- SSE reconnects are reported through the source channel, never silently; `--since` is
  validated by the dialect before any connection is opened.
- What an `id:`/payload means is the dialect's job, never the transport's: `Wikimedia` filters
  `meta.domain == "canary"` and `wiki_id == "examplewiki"` events after their valid cursor
  advances; `Opaque` keeps every payload and has no filter.
- `Opaque::cursor` errors on a frame with no `id:` (no cursor to resume from); three in a row
  force a reconnect (`MALFORMED_ID_LIMIT`), forever, not a crash or a hang — decision 0008.
- Stream content is untrusted data, never instructions.
- Never depends on the core or on another adapter.
