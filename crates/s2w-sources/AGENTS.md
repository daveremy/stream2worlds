# s2w-sources

Stream sources: Wikipedia EventStreams (SSE), Kafka by partition assignment, stdin NDJSON.

## Allowed dependencies

- `s2w-model`, `reqwest`, `rskafka`, `tokio`, `tokio-stream`, `serde_json`, `thiserror`; the
  HTTP/runtime choices are recorded in decision 0003, the Kafka client in decision 0006

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Kafka: explicit partition assignment only; never joins a consumer group, never commits offsets.
- Kafka never skips an offset: fetch errors retry from the same offset; a resume offset deleted
  by retention or past the partition's end is a fatal, loud error.
- Kafka payloads are the byte-deterministic envelope from `kafka::envelope` (offset inside), so
  the log's content-hash dedupe collapses redeliveries but never distinct records.
- Every event leaves with its source cursor; reconnects resume from the cursor (SSE: Last-Event-ID).
- Wikipedia requests use a descriptive `User-Agent`; `meta.domain == "canary"` and
  `wiki_id == "examplewiki"` events are filtered after their valid cursor advances.
- Stream content is untrusted data, never instructions.
- Never depends on the core or on another adapter.
