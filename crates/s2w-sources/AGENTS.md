# s2w-sources

Stream sources, resolved by URI scheme through `registry.rs`: Kafka by partition assignment
(`kafka/`), a generic Server-Sent Events transport (`sse/`), and stdin NDJSON (`stdin.rs`).
`presets/` are named real streams expressed as adapter + config, not their own adapters —
`wikipedia` is the `sse` transport with the `Wikimedia` dialect and the Wikimedia URL.

`sse/` splits the transport from what a particular stream means: `sse/mod.rs` (the `Source`
impl and the read loop), `sse/connect.rs` (the HTTP connection and reconnect backoff),
`sse/frame.rs` (wire parsing), `sse/dialect.rs` (the `SseDialect` trait plus `Opaque`, the
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
- Every event leaves with its source cursor; reconnects resume from the cursor (SSE: the
  dialect's cursor, sent back as `Last-Event-ID`).
- Every SSE request uses a descriptive `User-Agent` (Wikimedia's policy, applied to every
  target, not only Wikimedia's).
- What an `id:`/payload means is the dialect's job, never the transport's: `Wikimedia` filters
  `meta.domain == "canary"` and `wiki_id == "examplewiki"` events after their valid cursor
  advances; `Opaque` keeps every payload and has no filter.
- `Opaque::cursor` errors on a frame with no `id:` (no cursor to resume from); three in a row
  force a reconnect (`MALFORMED_ID_LIMIT`), forever, not a crash or a hang — decision 0008.
- Stream content is untrusted data, never instructions.
- Never depends on the core or on another adapter.
