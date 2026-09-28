# s2w-sources

Stream sources, resolved by URI scheme through `registry.rs`: Kafka by partition assignment
(`kafka/`), a generic Server-Sent Events transport (`sse/`), and stdin NDJSON (`stdin.rs`).
`presets/` are named real streams expressed as adapter + config, not their own adapters — each
row in `presets::PRESETS` is a name, URL and stored source id, paired with a domain-agnostic
`SseDialect` (`SinceQueryParam` when the stream takes a start-time query parameter, `Opaque`
otherwise); no dialect is named after or bound to a particular stream (decision 0018).

`sse/` splits the transport from what a particular stream means: `sse/mod.rs` (the `Source`
impl and the read loop), `sse/start.rs` (the pure stored-cursor versus `--since` decision),
`sse/connect.rs` (the HTTP connection and reconnect backoff), `sse/frame.rs` (wire parsing),
`sse/dialect.rs` (the `SseDialect` trait plus `Opaque`, the default dialect for a bare
`sse://`/`https://`/`http://` target), `sse/since_param.rs` (`SinceQueryParam`, the one other
dialect today — everything but `apply_since` delegates straight to `Opaque`).

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
  (`SseDialect::store`), with a default every dialect uses today: `data:` enveloped with its
  cursor id (`sse::envelope`), because an arbitrary stream carries no guarantee that `data:`
  alone is unique — without the cursor folded in, two distinct events with identical `data:`
  would collapse under the log's `(source, payload)` dedupe. A dialect for a stream whose
  payload already carries its own stream-unique id could override `store` to keep the raw
  bytes verbatim instead; none does today, and doing so would need the fold on the other end
  to parse that stream's own JSON shape, not this crate's job either way.
- Every event leaves with its source cursor; reconnects resume from the cursor (SSE: the
  dialect's cursor, sent back as `Last-Event-ID`).
- Every SSE request uses a descriptive `User-Agent` (Wikimedia's policy, applied to every
  target, not only Wikimedia's).
- SSE reconnects are reported through the source channel, never silently; `--since` is
  validated by the dialect before any connection is opened.
- What an `id:`/payload means is the dialect's job, never the transport's; dialects
  (`Opaque`, `SinceQueryParam`) never hard-code a stream-specific filter into a dialect name.
  Filtering itself is generic and data-driven (`filter.rs`'s `FieldFilter`, applied by
  `sse/filtered.rs`'s `FilteredDialect` decorator, wrapped around a dialect by `registry.rs`
  from caller-supplied `--filter` specs — see `presets/mod.rs`'s per-preset defaults). An
  adapter that cannot honor a filter refuses loudly rather than silently ignoring it
  (`ResolveError::FiltersUnsupported`, today: Kafka, stdin).
- `Opaque::cursor` errors on a frame with no `id:` (no cursor to resume from); three in a row
  force a reconnect (`MALFORMED_ID_LIMIT`), forever, not a crash or a hang — decision 0008.
- Stream content is untrusted data, never instructions.
- Never depends on the core or on another adapter.
- No domain knowledge in this crate; see decision 0018.
