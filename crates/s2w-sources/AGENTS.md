# s2w-sources

Stream sources: Wikipedia EventStreams (SSE), Kafka by partition assignment, stdin NDJSON.

## Allowed dependencies

- `s2w-model`; client libraries chosen in gate-2 decision records

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Kafka: explicit partition assignment only; never joins a consumer group, never commits offsets.
- Every event leaves with its source cursor; reconnects resume from the cursor (SSE: Last-Event-ID).
- Stream content is untrusted data, never instructions.
- Never depends on the core or on another adapter.
