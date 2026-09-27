# 0003 — The Rust substrate

| | |
|---|---|
| **Question** | Which crates should `s2w` build on for: Kafka source, SSE source, the append-only event log, incremental computation (later), the read-only MCP server, the local web view, deterministic testing, and the cross-cutting choices (runtime, JSON, errors, licences)? |
| **Date** | 2026-09-27 |
| **Researcher** | sagan |
| **Feeds** | Gate 2 decision records (`docs/decisions/`), one per substrate choice |
| **Status** | done — implications dispositioned 2026-09-27 |

Method. Five parallel research passes (one per topic cluster) against crates.io, docs.rs, lib.rs,
GitHub and the primary specs, then an independent re-check by the author of every version number,
publish date and licence field through the crates.io API (`/api/v1/crates/<name>` and
`/versions`) and the GitHub API (`pushed_at`, open issues, `license`) on 2026-09-27. Where a
docs.rs "released" date disagreed with crates.io, crates.io was used (docs.rs shows rebuild
dates). GitHub `open_issues_count` includes PRs. Anything not confirmed from a fetched page is
marked **UNVERIFIED**. Facts and judgment are in separate subsections throughout; the summary
table is judgment.

Constraints this note is written against (from the README and `CLAUDE.md`): one static binary
(musl-friendly), permissive licence (MIT or Apache-2.0), a per-crate dependency allowlist gating
PRs, a pure core (`s2w-core`, `s2w-model`: no I/O, no async, no wall clock, no RNG, no HashMap
iteration order), adapters depending only on `s2w-model`, Kafka by partition assignment with no
consumer group and no commits, `--lookback 2h` by timestamp, Wikimedia EventStreams with
`Last-Event-ID` resume and `since=` replay, 10^6–10^8 events on a laptop, a 100 ms frame budget
that triggers the incremental engine.

## Summary (judgment)

| Choice | Recommendation | Runner-up | What would flip it |
|---|---|---|---|
| 1. Kafka client | **rskafka 0.6.0** (pure Rust, no group.id concept, `OffsetAt::Timestamp`, PLAIN/SCRAM/OAUTHBEARER, rustls) | rdkafka 0.39.0 (librdkafka) | Need for librdkafka's proven OIDC/OAUTHBEARER flows against a managed cloud; rskafka's timestamp lookup misbehaving on target brokers; deciding to ship glibc-dynamic instead of musl-static |
| 2. SSE client | **Hand-rolled loop on reqwest 0.13 + a ~one-page SSE parser** (or `sse-core` as the parser) | eventsource-client 0.18.0 (LaunchDarkly) | If reqwest is excluded from the allowlist and hyper is used raw; a second, non-Wikimedia SSE source where generic behaviour suffices |
| 3. Event log storage | **Own segment-file format** for the hot log (length-prefixed, CRC32C/XXH3 per record, sealed segments, fdatasync on commit) + **redb 4.3.0** for cursors/snapshots | SQLite via rusqlite 0.40.2 (`bundled`) for both | Provenance/cursor queries turning relational; not wanting to own crash-recovery code |
| 4. Incremental engine (on trigger) | **differential-dataflow 0.25.1** (7 direct deps, no runtime, branch as a timestamp coordinate) | dbsp 0.356.0 (Feldera) | Feldera shipping a `dbsp` build without tokio/storage; workloads dominated by rolling aggregates; DD upstream going quiet for ~6 months |
| 5. MCP SDK | **rmcp 3.4.1** (official, spec 2026-07-28, stdio + Streamable HTTP, tower-mountable, `read_only_hint`) | rust-mcp-sdk 2.0.0 | Another breaking rmcp major inside the build window; rmcp's dependency tail failing the allowlist where rust-mcp-sdk's passes |
| 6. Local web view | **axum 0.8.9 + memory-serve 2.4.0 + axum's built-in SSE** | rust-embed 8.12.0 (`axum-ex`, `compression`) with axum | Browser needs to send commands (add axum `ws`); allowlist rejects hyper/tower (nothing here survives) |
| 7. Deterministic testing | **proptest + proptest-state-machine + insta** for the pure fold; **tokio `start_paused` + turmoil** for adapters; `cargo-fuzz`/`bolero` over the log decoder only | madsim end-to-end | Needing disk-fault injection through `tokio::fs` or Kafka-in-the-loop simulation (madsim-rdkafka is the only Rust option short of Antithesis) |
| 8a. Async runtime | **tokio 1.53.x** (LTS to Sept 2027) | — | None found; async-std is discontinued, io_uring runtimes are Linux-only |
| 8b. JSON | **serde_json** with `raw_value` + borrowing; no `preserve_order`; canonical hashing via a typed serializer or `serde_json_canonicalizer` | sonic-rs on the adapter side only | A profile showing parse > 30% of wall time |
| 8c. Errors | **thiserror 2 in every lib crate, anyhow 1 in `s2w` only** | miette for CLI diagnostics if a query language arrives | — |
| 8d. Licences | Everything above is MIT and/or Apache-2.0; enforce with **cargo-deny** (`[licenses] allow`, `[bans] allow`) | — | — |

## 1. Kafka clients

### Facts

Candidates and status (crates.io API, GitHub API, 2026-09-27):

| Crate | Latest, published | Licence | MSRV | GitHub | Notes |
|---|---|---|---|---|---|
| `rdkafka` | 0.39.0, 2026-01-25 (prev 0.38.0, 2025-07-05) — https://crates.io/crates/rdkafka | MIT | 1.74 | 2,001★, pushed 2026-07-15, 163 open (116 issues / 47 PRs), last commit 2026-06-14 — https://github.com/fede1024/rust-rdkafka | librdkafka FFI; `rdkafka-sys` 4.10.0 bundles librdkafka 2.12.1 — https://docs.rs/rdkafka-sys/latest/rdkafka_sys/ |
| `rskafka` | 0.6.0, 2025-03-20 (prev 0.5.0, 2023-08-11) — https://crates.io/crates/rskafka | MIT OR Apache-2.0 | 1.85 (crates.io), 1.88 on main | 342★, pushed 2026-09-14 (dependabot), 17 open — https://github.com/influxdata/rskafka | Pure Rust. README: "originally used by InfluxDB 3.0 but no longer is" — https://raw.githubusercontent.com/influxdata/rskafka/main/README.md. Main is still 0.6.0: ~18 months of unreleased work |
| `kafka` (kafka-rust) | 0.10.0, 2023-09-24 — https://crates.io/crates/kafka | MIT OR Apache-2.0 | — | 1,454★, pushed 2026-07-02, 54 open — https://github.com/kafka-rust/kafka-rust | Sync API. README: "Use it in production at your own risk" — https://raw.githubusercontent.com/kafka-rust/kafka-rust/master/README.md |
| `samsa` | 0.1.8, 2025-09-24 — https://crates.io/crates/samsa | Apache-2.0 via `license-file` (crates.io shows "non-standard") — https://raw.githubusercontent.com/CallistoLabsNYC/samsa/main/Cargo.toml | — | 228★, pushed 2025-09-24, 21 open — https://github.com/CallistoLabsNYC/samsa | Pure Rust; ~375 downloads/month — https://lib.rs/crates/samsa |
| `kafka-protocol` | 0.18.0, 2026-08-20 — https://crates.io/crates/kafka-protocol | MIT OR Apache-2.0 | — | pushed 2026-09-23 — https://github.com/kafka-protocol-rs/kafka-protocol-rs | Wire encode/decode only (Kafka 4.1.0 schema); no client |
| `franz`, `fluvio` | — | — | — | — | Not Kafka clients (a broker alternative; Fluvio's own protocol) — https://crates.io/crates/franz, https://crates.io/crates/fluvio |

**Assign without a group.**
- `rdkafka`: `Consumer::assign(&TopicPartitionList)` — "If used, automatic consumer rebalance won't be activated" — https://docs.rs/rdkafka/latest/rdkafka/consumer/trait.Consumer.html. But librdkafka **requires `group.id` even for `assign`**: issue "Assign operation of consumer requires a group id", open since 2021-02-12, label `status:planned` — https://github.com/confluentinc/librdkafka/issues/3261 (verified by the author); the 2016 request for group-less consumers was closed wontfix — https://github.com/confluentinc/librdkafka/issues/593. Practical shape: a throwaway `group.id`, `enable.auto.commit=false`, never call `commit`. Whether librdkafka contacts the group coordinator at all under assign-only usage: **UNVERIFIED**.
- `rskafka`: no consumer groups exist — "No support for offset tracking, consumer groups, transactions, etc…" — README above. `Client::partition_client(topic, partition, …) -> PartitionClient` with `fetch_records`/`get_offset` — https://docs.rs/rskafka/latest/rskafka/client/struct.Client.html. The requirement is met by construction.
- `kafka`: `Builder::with_group("")` — "the resulting consumer will be group-less" — https://docs.rs/kafka/latest/kafka/consumer/struct.Builder.html.
- `samsa`: `ConsumerBuilder` takes an explicit `TopicPartitions` assignment; groups are a separate builder — https://docs.rs/samsa/latest/samsa/prelude/index.html.

**Offsets by timestamp.**
- `rdkafka`: `Consumer::offsets_for_times(timestamps: TopicPartitionList, timeout) -> KafkaResult<TopicPartitionList>` — "Looks up the offsets for the specified partitions by timestamp" — https://docs.rs/rdkafka/latest/rdkafka/consumer/trait.Consumer.html. Also `seek`, `fetch_watermarks`.
- `rskafka`: `OffsetAt::{Earliest, Latest, Timestamp(DateTime<Utc>)}`; the doc warns the Kafka semantics are "semi-defined, unintuitive (even within Apache Kafka) and inconsistent between Apache Kafka and Redpanda … millisecond precision" — https://docs.rs/rskafka/latest/rskafka/client/partition/enum.OffsetAt.html. Landed in 0.6.0 (#248) — https://raw.githubusercontent.com/influxdata/rskafka/main/CHANGELOG.md.
- `kafka`: `FetchOffset::ByTime(i64)` — "all messages before a certain time (ms)" — https://docs.rs/kafka/latest/kafka/client/enum.FetchOffset.html. That is the old ListOffsets v0 wording ("before"), not `offsetsForTimes` ("first offset ≥ time"); which protocol version it sends is **UNVERIFIED**.
- `samsa`: low-level `list_offsets(…, timestamp)` with the same "before a certain time" wording — https://docs.rs/samsa/latest/samsa/prelude/fn.list_offsets.html; semantics **UNVERIFIED**.

**TLS / SASL.**
- `rdkafka`: TLS via `ssl` (dynamic OpenSSL) or `ssl-vendored` (static OpenSSL built from source). PLAIN, SCRAM and OAUTHBEARER are built into librdkafka; GSSAPI needs `gssapi`/`gssapi-vendored` (Cyrus libsasl2) — https://docs.rs/rdkafka-sys/latest/rdkafka_sys/, https://docs.rs/crate/rdkafka/latest/features. OAUTHBEARER refresh hook: `ClientContext::generate_oauth_token` — https://docs.rs/rdkafka/latest/rdkafka/client/trait.ClientContext.html. OIDC-mode OAUTHBEARER needs libcurl (`curl`/`curl-static`).
- `rskafka`: `transport-tls` = rustls 0.23 + tokio-rustls; crypto provider selectable on main (`transport-tls-ring` default, `transport-tls-aws-lc-rs`) but the **published 0.6.0 exposes only `transport-tls`/`transport-socks5`** — https://docs.rs/crate/rskafka/latest/features, https://raw.githubusercontent.com/influxdata/rskafka/main/Cargo.toml. SASL: `SaslConfig::{Plain, ScramSha256, ScramSha512, Oauthbearer}` in 0.6.0 — https://docs.rs/rskafka/latest/rskafka/client/enum.SaslConfig.html (PLAIN since 0.5.0, SCRAM + OAUTHBEARER in 0.6.0 via `rsasl` — https://github.com/influxdata/rskafka/issues/247). The folk memory that "rskafka has no SASL" is stale.
- `kafka` 0.10.0: OpenSSL only (`security` feature); rustls default only on unreleased master — https://docs.rs/crate/kafka/latest/features. No SASL documented.
- `samsa`: rustls TLS; SASL PLAIN, SCRAM-SHA-256/512 via `rsasl`; no OAUTHBEARER listed — https://github.com/CallistoLabsNYC/samsa.

**Static linking (musl) and size.**
- `rdkafka`: default build compiles the librdkafka submodule and links it statically; `cmake-build`, `dynamic-linking`, `libz-static`, `curl-static`, `zstd`, `ssl-vendored` features — https://docs.rs/rdkafka-sys/latest/rdkafka_sys/. musl history: #170 (2019, closed) — https://github.com/fede1024/rust-rdkafka/issues/170; #772 (2025-05, open, CMake cannot find `x86_64-linux-musl-g++`) — https://github.com/fede1024/rust-rdkafka/issues/772; **#827 (2026-02-15, open)**: needed a librdkafka patch, `libz-static` gave `__snprintf_chk` link errors, CMake could not find ZLIB/ZSTD; the reporter patched build.rs and used `cross` with `DEP_Z_ROOT`/`DEP_ZSTD_ROOT` — https://github.com/fede1024/rust-rdkafka/issues/827. ~129K SLoC of C in the crate tree — https://lib.rs/crates/rdkafka. Binary-size impact: **UNVERIFIED** (no measured figure found; Alpine's dynamic librdkafka 2.15.0 package is 3.3 MiB installed — https://pkgs.alpinelinux.org/package/edge/community/x86_64/librdkafka — a static OpenSSL adds more).
- `rskafka`: no C from the crate itself; default-on compression features pull `zstd`/`lz4`/`flate2`, which are C-backed (`zstd` builds zstd-sys) — disable or accept knowingly. No musl issues found (absence, not proof).
- `samsa`: pure Rust including compression (`lz4_flex`, `ruzstd`, `snap`, `flate2`).

**Concurrency across partitions.** `rdkafka`: `StreamConsumer::split_partition_queue` gives per-partition streams; "You must periodically await `StreamConsumer::recv`, even if no messages are expected, to serve events" — https://docs.rs/rdkafka/latest/rdkafka/consumer/stream_consumer/struct.StreamConsumer.html. `rskafka`: one `PartitionClient` per partition, one task each; you write the fetch loop and handle leader changes.

### Judgment

**Recommend rskafka.** It is the only candidate that satisfies "never join a group, never commit" by construction, has the timestamp lookup, has PLAIN/SCRAM/OAUTHBEARER in the released version, uses rustls, and adds no C beyond optional compression. Its costs are real and should be recorded in the decision: 0.6.0 is 18 months old with unreleased work on main; InfluxDB no longer uses it (maintenance is dependabot plus community); it gives no fetch-loop or metadata-refresh helpers (s2w writes the loop, which it wants to own anyway for deterministic merge order); and `OffsetAt::Timestamp` must be tested against both Apache Kafka and Redpanda before `--lookback` is trusted.

**Runner-up rdkafka.** Feature-complete and the most battle-tested, with the fastest maintenance cadence of the four. It loses on the hard constraints: a dummy `group.id` is mandatory (open since 2021), and the static-musl story is an open issue as of Feb 2026 requiring patched sources. It also brings ~130K SLoC of C and a vendored OpenSSL into a project whose pitch is one audited static binary.

**Do not pick:** `kafka` (sync, OpenSSL-only release, self-described at-your-own-risk, ByTime semantics unverified), `samsa` (one maintainer, one-year release gaps, ~375 downloads/month, non-SPDX licence field that `cargo-deny` will flag).

## 2. SSE clients

### Facts

| Crate | Latest, published | Licence | Activity | Behaviour |
|---|---|---|---|---|
| `reqwest-eventsource` | 0.6.0, 2024-03-29 — https://crates.io/crates/reqwest-eventsource | MIT OR Apache-2.0 | 71★, last commit 2024-03-29; open issue "Project abandoned?" (2026-04-23, unanswered); "Support for reqwest v0.13" open — https://github.com/jpopesculian/reqwest-eventsource/issues | Pinned to reqwest ^0.12 while reqwest is at 0.13.5 (2026-09-08, https://crates.io/crates/reqwest), so it forces a second HTTP stack. Auto `Last-Event-ID`, honours `retry:`, exponential backoff, custom `Retry` trait — https://docs.rs/reqwest-eventsource |
| `eventsource-client` (LaunchDarkly) | 0.18.0, 2026-08-10 — https://crates.io/crates/eventsource-client | Apache-2.0 | 113★, pushed 2026-08-24, 1 open — https://github.com/launchdarkly/rust-eventsource-client | hyper v1 via `launchdarkly-sdk-transport` 0.1.x (created 2026-02); `ClientBuilder::for_url`, `.last_event_id()`, `.reconnect(ReconnectOptions)` with backoff; "automatically tracks and re-sends Last-Event-ID headers upon reconnection" — https://docs.rs/eventsource-client. Honouring server `retry:` **UNVERIFIED**. MSRV 1.95/1.96 (very fresh). TLS via rustls or native-tls features |
| `eventsource-stream` | 0.2.3, 2022-02-17 — https://crates.io/crates/eventsource-stream | MIT OR Apache-2.0 | 38★, last commit 2022 | Parser only over `Stream<Item=Bytes>`; no HTTP, no reconnect — https://docs.rs/eventsource-stream |
| `sse-reqwest-client` | 0.4.0, 2026-07-31 (crate created 2026-05-01) — https://crates.io/crates/sse-reqwest-client | MIT OR Apache-2.0 | 20★, one author — https://github.com/PizzasBear/sse-rs | reqwest ^0.13; `Last-Event-ID` on reconnect, jittered exponential backoff, honours `retry`; parser in `sse-core` (no_std, zero I/O) — https://crates.io/crates/sse-core |
| Hand-rolled | — | — | — | Parser is `event:`/`data:`/`id:`/`retry:` lines, blank-line dispatch, `\r\n|\n|\r`; reconnect = loop { GET with `Last-Event-ID`; on EOF/error sleep(backoff) } |

**Wikimedia EventStreams protocol.**
- `since` query parameter: "either an integer UTC milliseconds unix epoch timestamp, or a string timestamp parseable by `Date.parse()`"; disregarded if no offsets exist for it; **superseded when `Last-Event-ID` carries offsets or timestamps**; history is "likely only one or a few weeks" — https://stream.wikimedia.org/?spec. Wikitech: "between 7 and 31 days of history available" — https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams_HTTP_Service.
- `Last-Event-ID` is a JSON array of `{topic, partition, offset}` or `{topic, partition, timestamp}`; offset takes precedence over timestamp; each SSE `id:` field is that array — same wikitech page and https://github.com/wikimedia/KafkaSSE.
- **Connections are cut every 15 minutes**: "WMF's HTTP connection termination layer enforces a connection timeout of 15 minutes. A good SSE / EventSource client should be able to automatically reconnect and begin consuming at the right location using the Last-Event-ID header" — EventStreams_HTTP_Service above. Capacity ~450 concurrent connections total with per-IP concurrency limiting — https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams/Administration. Set a descriptive `User-Agent`; discard `meta.domain === 'canary'` events — https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams.

### Judgment

**Recommend a hand-rolled loop on reqwest 0.13 with an inline (or `sse-core`) parser.** The reasons are protocol-specific: reconnects are guaranteed every 15 minutes; the resume key is a JSON array s2w wants to parse, persist as the source cursor, and reason about (offset vs timestamp precedence); and `since=` must be dropped once a `Last-Event-ID` is held or every reconnect re-reads history. That arbitration lives in s2w's loop whichever crate is used, so a generic client adds a dependency without removing code. The allowlist stays at reqwest + tokio.

**Runner-up eventsource-client.** The only mainstream SSE crate that is actively maintained, permissive, tokio-native, with automatic `Last-Event-ID`. Costs: a separate hyper stack plus a 7-month-old transport crate, an MSRV of 1.96, and unverified `retry:` handling. `sse-reqwest-client` ticks every functional box but is five months old with one author — allowlist risk rather than a feature gap.

**Do not pick:** `reqwest-eventsource` (unmaintained since 2024-03, reqwest 0.12 pin), `eventsource-stream` (2022 parser; `sse-core` is the newer no_std equivalent if a parser dependency is wanted).

## 3. Embedded storage for the event log

### Facts

**Existing WAL / segment-log crates.** None is simultaneously maintained, permissive and widely used:

| Crate | Latest, published | Licence | Status |
|---|---|---|---|
| `okaywal` (khonsulabs) | 0.3.1, 2023-11-26 — https://crates.io/crates/okaywal | MIT OR Apache-2.0 | Last commit 2023-11-26; README: "Please do not use in any production projects… The file format is currently considered unstable" — https://github.com/khonsulabs/okaywal |
| `commitlog` | 0.2.0, 2021-11-06 — https://lib.rs/crates/commitlog | MIT | Kafka-style segments, crc32c + memmap2; dep bumps only to 2024-03 — https://github.com/zowens/commitlog |
| `simple_wal` | 0.3.0, 2020-10-25 — https://lib.rs/crates/simple_wal | MIT | Dormant; obsolete deps flagged by lib.rs |
| `wal` | 0.1.0, 2026-03-09 — https://crates.io/api/v1/crates/wal | MIT | 12 lines of code, no repository: a placeholder |
| `sharded-log` | 0.0.1, 2022 — https://lib.rs/crates/sharded-log | **GPL-3.0** | Licence-incompatible |
| `chunked-wal` / `raft-log` (drmingdrmer) | 0.2.3, 2026-08-21 / 0.4.6, 2026-09-01 — https://crates.io/crates/chunked-wal, https://lib.rs/crates/raft-log | MIT OR Apache-2.0 | Active, single author, 1–4★, bare README; raft-log's API is Raft-specific |
| `open-wal` | 0.2.1, 2026-07-03 (first release 2026-06-17) — https://crates.io/crates/open-wal | **BSD-3-Clause** | 1★, 61 downloads; Linux-only; design matches s2w's need exactly: CRC32C per record, `commit()` = one `fdatasync` + directory fsync, sealed segments, "torn tails… detected, truncated, and durably invalidated", "Mid-log corruption is a loud fatal error, never a silent truncation"; tested with a SIGKILL matrix, LazyFS, dm-flakey — https://github.com/guyo13/open-wal |
| `walrus` | — | — | A WebAssembly transformation library, not a WAL — https://crates.io/crates/walrus |

Primitives for an own format: `memmap2` 0.9.11, 2026-06-22, MIT OR Apache-2.0, ~30M downloads/month — https://lib.rs/crates/memmap2. `crc32c`, `crc32fast`, `xxhash-rust` (XXH3, chosen by both redb and fjall).

**SQLite via `rusqlite`.** 0.40.2, 2026-08-08, MIT; bundled SQLite 3.53.2 via `libsqlite3-sys` 0.38.2 — https://crates.io/crates/rusqlite, https://lib.rs/crates/rusqlite. SQLite is public domain — https://www.sqlite.org/copyright.html. Static musl works with `features = ["bundled"]` (a 2026-09-17 PR ships `x86_64-` and `aarch64-unknown-linux-musl` static binaries — https://github.com/suiflex/ForgeGuard/pull/86); the failure mode is falling back to dynamic linking without `bundled` (issue #914, open since 2021 — https://github.com/rusqlite/rusqlite/issues/914). Crash model: WAL mode; `synchronous=FULL` syncs every commit; `NORMAL` keeps consistency but "transactions… might rollback following a power failure" — https://www.sqlite.org/wal.html. Throughput: "50,000 or more INSERT statements per second… But it will only do a few dozen transactions per second" unbatched — https://www.sqlite.org/faq.html.

**redb.** 4.3.0, 2026-09-15; 4.0.0 2026-04-02; 3.0.0 2025-08-09 — https://crates.io/crates/redb. MIT OR Apache-2.0, MSRV 1.90; 4,809★, pushed 2026-09-24, 9 open — https://github.com/cberner/redb. Dependencies: `libc` only (plus optional chrono/log/uuid); docs.rs build 4 s — https://docs.rs/crate/redb/latest. Pure Rust, no C. Crash model (design doc): copy-on-write B+trees, MVCC; default commit is "1PC+C" — one fsync, XXH3-128 checksums over both roots and the transaction slot, an atomic "god byte" flip, recovery validates checksums and rolls back partial commits; optional 2PC — https://github.com/cberner/redb/blob/master/docs/design.md. `Durability::{None, Immediate}` — https://docs.rs/redb/latest/redb/enum.Durability.html. "The file format is stable, and a reasonable effort will be made to provide an upgrade path" — README; history honoured that (2.6 → 3 via `Database::upgrade()`; no format change in 4.x) — https://github.com/cberner/redb/blob/master/CHANGELOG.md.

**fjall.** 3.1.10, 2026-08-30; 3.0.0 2026-01-02; last 2.x 2025-07-21 — https://crates.io/crates/fjall. MIT OR Apache-2.0, MSRV 1.90; 2,353★, pushed 2026-09-13, 43 open — https://github.com/fjall-rs/fjall. `fjall` has 9 normal deps; `lsm-tree` is 22K SLoC — https://lib.rs/crates/lsm-tree. 100% safe Rust. **Default durability is OS buffers only**: "By default, any operation will flush to OS buffers, but **not** to disk. This matches RocksDB's default durability" — call `persist(PersistMode::{Buffer, SyncData, SyncAll})` — https://raw.githubusercontent.com/fjall-rs/fjall/main/README.md, https://docs.rs/fjall/latest/fjall/enum.PersistMode.html. Journal recovery: XXH3 per batch; an unterminated final batch is truncated; a mid-file checksum mismatch is an error — https://raw.githubusercontent.com/fjall-rs/fjall/main/src/journal/batch_reader.rs. The 3.0 release post says "for the forseeable future active development on new features will mostly wind down going into 2026" — https://fjall-rs.github.io/post/fjall-3/. KV-only.

**sled.** `max_stable_version` 0.34.7 (2021-09-12); newest 1.0.0-alpha.124 (2024-10-11); no release since — https://crates.io/api/v1/crates/sled. GitHub main: 3 commits 2026-04-04, then 2026-03-26, 2025-11-04 — https://github.com/spacejam/sled/commits/main. README: "if reliability is your primary constraint, use SQLite. sled is beta"; "the on-disk format is going to change in ways that require manual migrations before the 1.0.0 release!" — https://raw.githubusercontent.com/spacejam/sled/main/README.md. Successor engine `komora-io/marble` last commit 2025-11-04 — https://github.com/komora-io/marble. Characterisation: stalled, not formally abandoned. `[no-record: sled hiatus statement searched=github.com/spacejam/sled README + issues, crates.io, web search, as of 2026-09-27]`

**RocksDB via `rocksdb`.** 0.25.0, 2026-08-16, Apache-2.0, MSRV 1.88; `librocksdb-sys` 0.19.0+11.8.1 — https://crates.io/crates/rocksdb, https://crates.io/api/v1/crates/librocksdb-sys. C++ (~300K SLoC, figure from a stale lib.rs page — **UNVERIFIED** for 11.8.1); needs clang/bindgen; docs.rs build 6m46s — https://docs.rs/crate/rocksdb/latest. **"RocksDB does not build on musl" open since 2018-03-27** — https://github.com/rust-rocksdb/rust-rocksdb/issues/174; CI has no musl target; an open 2026-08 PR adds only i686-musl with a downloaded cross toolchain — https://github.com/rust-rocksdb/rust-rocksdb/pull/1104. RocksDB itself is dual GPL-2.0 / Apache-2.0 — https://github.com/facebook/rocksdb. Stripped static size: **UNVERIFIED** (unstripped `librocksdb.a` > 1 GB with debug symbols — https://github.com/facebook/rocksdb/issues/12636).

**Parquet/Arrow for cold segments.** `parquet` 60.0.0 and `arrow` 60.0.0, 2026-09-15, Apache-2.0, MSRV 1.88 — https://crates.io/crates/parquet. Monthly releases, a new major "at most once a quarter" — https://raw.githubusercontent.com/apache/arrow-rs/main/README.md. With `default-features = false` parquet has **9 non-optional deps** (bytes, chrono, num-bigint, num-integer, num-traits, seq-macro, hashbrown, twox-hash, half) and writes through the low-level column writer, without the arrow-* family (arrow alone is 135K SLoC) — https://raw.githubusercontent.com/apache/arrow-rs/main/parquet/Cargo.toml. Compression: pure-Rust `lz4` (lz4_flex) / `snap`, or `zstd` (C). Binary-size/compile-time delta for the minimal build: **UNVERIFIED**.

**heed (LMDB).** 0.22.1, 2026-04-07, MIT — https://crates.io/crates/heed; `lmdb-master-sys` vendors LMDB from `mdb.master` under the **OpenLDAP Public License** (~11K SLoC of C) — https://raw.githubusercontent.com/meilisearch/heed/main/lmdb-master-sys/Cargo.toml. LMDB: single mmap file, COW pages, no WAL, no page checksums, fixed maximum map size, serialized writers, long read transactions grow the file — https://raw.githubusercontent.com/LMDB/lmdb/mdb.master/libraries/liblmdb/lmdb.h. musl: **UNVERIFIED** (no CI evidence).

**The only comparison table found** (redb README, Ryzen 9950X3D + Samsung 9100 PRO; vendor-published, small-KV random workload; element count and per-engine sync settings not located — treat as relative) — https://raw.githubusercontent.com/cberner/redb/master/README.md:

| ms | redb | lmdb | rocksdb | fjall | sqlite |
|---|---|---|---|---|---|
| bulk load | 17063 | **9232** | 13969 | 18619 | 15341 |
| individual (fsynced) writes | **920** | 1598 | 2432 | 3488 | 7040 |
| batch writes | 1595 | 942 | 451 | **353** | 2625 |
| random reads | 1138 | **637** | 2911 | 2177 | 4283 |
| compacted size | 1.69 GiB | 1.26 GiB | **455 MiB** | 1001 MiB | 557 MiB |
| uncompacted size | 4.00 GiB | 2.61 GiB | 893 MiB | 1001 MiB | 1.09 GiB |

Fjall's own 3.0 post publishes graphs, not tables — https://fjall-rs.github.io/post/fjall-3/. Two harnesses exist without published numbers: https://github.com/marvin-j97/rust-storage-bench, https://github.com/surrealdb/crud-bench. `[no-record: independent 2025–26 benchmark of these engines on an append-only, 0.5–5 KB value, sequential-replay workload searched=web ×3, HN, lobste.rs, r/rust, as of 2026-09-27]`

### Judgment

**Hot log: own segment-file format. Side store (cursors, snapshots, provenance index): redb.**

Why an own format for the log:
1. The requirement is literally a segmented, append-only, checksummed, replayable byte log. Every KV engine puts a B-tree or LSM between s2w and the bytes and then makes replay a pointer-chase through an iterator; a sealed segment streams at disk speed and can be `memmap2`-ed for zero-copy replay. Their costs show in the table: redb's 2.4× space amplification uncompacted, fjall's compaction rewrites and OS-buffer default durability, LMDB's fixed map size.
2. There is no maintained, permissive, popular WAL crate to adopt (table above). Adding one is adding an unmaintained dependency for roughly 300–500 lines s2w would rather own and test with the same instruments `open-wal` used (SIGKILL matrix, dm-flakey, truncation fuzzing — see §7).
3. Format sketch the evidence supports: `[u32 len][u64 seq][payload][u32 crc32c]` (or XXH3-64), segment rollover at a fixed size, `fdatasync` on commit and directory fsync on rollover, sealed segments immutable, a sparse `seq → byte offset` index per segment rebuildable from the segment. Recovery: valid-prefix scan, truncate after the last checksum-valid record at the tail, refuse to start on a mid-file checksum failure ("loud fatal error, never a silent truncation"). Per-source cursors and provenance are records in the same log (they are events about the log) plus a fast copy in redb.
4. Scale check: 10^8 events × 0.5–5 KB is 50–500 GB, beyond a laptop at the top end for any engine; at 10^6–10^7 (0.5–50 GB) a flat file streams at NVMe speed and JSON parsing, not storage, is the bottleneck.

Why redb for the side store: one dependency, 4 s build, pure Rust so musl is trivial, checksummed single-fsync commits (fastest fsynced writes in the only table available), MVCC readers, a stable file format whose upgrade promise has actually been kept, monthly releases with commits this week. Side-store data is small, so its space amplification is irrelevant.

**Runner-up: SQLite via `rusqlite` (`bundled`) for both.** One well-understood public-domain file, proven static musl builds, WAL mode, batched inserts at 50K+/s, and ad-hoc SQL over events and provenance for free. It loses on fsynced write latency (slowest in the table), replay through a B-tree rather than a stream, and C in the binary (C, not C++ — acceptable). If the team values "one dependency, queryable, boring" over replay speed, this flips to first.

**Do not pick:** sled (alpha since 2023, format will change, author says use SQLite), RocksDB (C++, musl issue open since 2018, minutes-long builds, huge static library), sharded-log (GPL). heed/LMDB is a defensible side-store alternative (fastest reads, mature C) but adds a third licence text, a fixed map size, and no page checksums; redb's checksummed commits fit the torn-tail requirement better.

**Cold Parquet segments later:** feasible without Arrow — 9 extra crates plus one compressor, written through the column writer API; pin and bump deliberately because of the quarterly major-version cadence. Measure size with `cargo bloat` before committing.

## 4. Differential Dataflow vs DBSP

### Facts: differential-dataflow

- 0.25.1, published 2026-07-15 (0.24.0 2026-05-29; 0.23.0 2026-04-13) — https://crates.io/crates/differential-dataflow. MIT, MSRV 1.86 — https://raw.githubusercontent.com/TimelyDataflow/differential-dataflow/master/Cargo.toml. `timely` 0.31.0, 2026-07-14, MIT — https://crates.io/crates/timely.
- **Direct dependencies: 7** — `timely`, `columnar`, `columnation`, `fnv`, `paste`, `serde`, `smallvec`; no async runtime, no `abomonation` (verified by the author from the sub-crate Cargo.toml — https://raw.githubusercontent.com/TimelyDataflow/differential-dataflow/master/differential-dataflow/Cargo.toml). timely adds columnar, columnation, bincode, byteorder, itertools, serde, smallvec and its own sub-crates — https://raw.githubusercontent.com/TimelyDataflow/timely-dataflow/master/timely/Cargo.toml.
- GitHub: 3,013★, pushed 2026-09-27, 131 open (issues + PRs); 15+ commits in July 2026, nearly all by frankmcsherry — https://github.com/TimelyDataflow/differential-dataflow/commits/master. Single-maintainer risk is visible.
- Single-threaded, no networking: `timely::execute_directly` "Executes a single-threaded timely dataflow computation" on the calling thread — https://docs.rs/timely/latest/timely/execute/fn.execute_directly.html.
- Model: a `Collection` is a stream of `(data, time, diff)` updates with multiset semantics; arrangements are sorted by key then value, not hash-ordered — https://docs.rs/differential-dataflow/latest/differential_dataflow/collection/struct.Collection.html, https://docs.rs/differential-dataflow/latest/differential_dataflow/trace/implementations/index.html. Output order within a batch is not a documented guarantee (**UNVERIFIED** beyond the docs read): consolidate before comparing.
- Materialize consumes the crates.io releases (`differential-dataflow = "0.25.0"`, `timely = "0.31.0"`, no `[patch]`) — https://raw.githubusercontent.com/MaterializeInc/materialize/main/Cargo.toml.
- Ergonomics critique remains Jamie Brandon's 2021 post (type-variable soup, unclear which operators are stateful, hard to pull results, hard to mix with tokio) — https://www.scattered-thoughts.net/writing/why-isnt-differential-dataflow-more-popular/. No substantive 2025–26 HN/Reddit discussion found — https://hn.algolia.com/api/v1/search?query=%22differential%20dataflow%22&tags=story&numericFilters=created_at_i%3E1735689600.

### Facts: dbsp (Feldera)

- **Licence: `MIT OR Apache-2.0`** on crates.io and in the workspace `Cargo.toml` — https://crates.io/crates/dbsp, https://raw.githubusercontent.com/feldera/feldera/main/Cargo.toml. The repo `LICENSE` is MIT text plus an Enterprise carve-out: code "gated behind the 'feldera-enterprise' feature flag or documented as Enterprise-only features… is not licensed under the MIT License above" — https://raw.githubusercontent.com/feldera/feldera/main/LICENSE. That flag lives in `crates/pipeline-manager/Cargo.toml`, not in `crates/dbsp/Cargo.toml`, whose only feature is `backend-mode` (verified by the author) — https://raw.githubusercontent.com/feldera/feldera/main/crates/dbsp/Cargo.toml, https://raw.githubusercontent.com/feldera/feldera/main/crates/pipeline-manager/Cargo.toml. So the `dbsp` crate carries no enterprise-gated code today; the "documented as Enterprise-only" clause is non-mechanical and should be re-audited on every bump, including the transitive `feldera-*` crates. No BSL/ELv2 relicensing found.
- 0.356.0, published 2026-09-26; a release every 1–3 days (0.354.0 09-22, 0.352.0 09-19, 0.350.0 09-17…) — https://crates.io/api/v1/crates/dbsp/versions. Main is already 0.357.0; some git tags (v0.351.0, v0.353.0, v0.355.0) have no crates.io release — policy or publish failures **UNVERIFIED**. **MSRV 1.96.1** (the toolchain released this month). GitHub 2,106★, pushed 2026-09-27, 576 open — https://github.com/feldera/feldera.
- **Direct dependencies: 64 on crates.io for 0.356.0, 67 on main** (author's count), including `tokio` (`rt-multi-thread`), `futures`, `rkyv`, `petgraph`, `zstd`, `lz4_flex`, `snap`, `roaring`, `mimalloc-rust-sys`, `nix`, `libc`, `core_affinity`, `clap`, `rand`/`rand_chacha`, `time`, `tempfile`, and seven `feldera-*` crates (storage, ir, buffer-cache, types, macros, modular-bloom, samply). **No feature drops storage or tokio** — https://crates.io/api/v1/crates/dbsp/0.356.0/dependencies.
- docs.rs **failed to build 0.356.0** (3.53 GB in the sandbox); the last successful docs build is 0.324.0 — https://docs.rs/crate/dbsp/latest/builds. No published compile-time figure for the crate alone (**UNVERIFIED**; Feldera's compile-time post is about SQL-generated code — https://www.feldera.com/blog/cutting-down-rust-compile-times-from-30-to-2-minutes-with-one-thousand-crates).
- Embedding without the pipeline manager or SQL compiler: yes. The tutorial builds circuits in Rust via `Runtime::init_circuit`; `RootCircuit::build` creates "a circuit that executes in the calling thread" — https://docs.rs/dbsp/0.324.0/dbsp/tutorial/index.html, https://docs.rs/dbsp/0.324.0/dbsp/circuit/index.html. `Runtime` uses its own OS worker threads, not tokio — https://docs.rs/dbsp/0.324.0/dbsp/circuit/struct.Runtime.html.
- Determinism: `Runtime` requires identical circuits built in the same order across workers; core types are sorted `OrdZSet`/`OrdIndexedZSet`; multi-worker output-order guarantees **UNVERIFIED**; the single-thread `RootCircuit` path avoids the question — https://docs.rs/dbsp/0.324.0/dbsp/index.html. Formal semantics: https://arxiv.org/abs/2203.16684.
- Documentation-quality complaints on record: issue #4769 (2025-09-17) — https://github.com/feldera/feldera/issues/4769.

### Facts: others

`dfir_rs` (Hydro) 0.17.0-alpha.5, 2026-09-21, Apache-2.0 — a distributed dataflow IR, not a Z-set/IVM engine — https://crates.io/crates/dfir_rs. `salsa` 0.28.5, 2026-09-24 — pull-based compiler memoization, wrong shape for streams — https://crates.io/crates/salsa. `timely` alone — progress tracking without collections/retractions. `readyset` (noria's successor) — **BSL 1.1**, a proxy, not a library — https://github.com/readysettech/readyset. DDlog (built on DD) archived 2026-07-13 — https://github.com/vmware/differential-datalog. `d2ts` shows DD's model porting cleanly to a small embedded runtime — https://github.com/electric-sql/d2ts. None fits branching state better than DD/DBSP.

### Prior art: worlds/branches as a column

- **McSherry, "World Enough, and Timely Dataflow" (2018-02-19)** — product timestamps `(system_time, event_time)` in DD; frames the coordinates as `(version, history)` and describes "forking your timeline and playing it forward again". The direct precedent for a world/branch coordinate in the timestamp lattice rather than the key — https://github.com/frankmcsherry/blog/blob/master/posts/2018-02-19.md.
- Differential dataflow (CIDR 2013): differences indexed by partially ordered versions — https://www.cidrdb.org/cidr2013/Papers/CIDR13_Paper111.pdf (content **UNVERIFIED** by fetch).
- Graphsurge (2020): many graph views computed as one differential computation, ordered to minimise adjacent deltas — https://arxiv.org/abs/2004.05297.
- Towards Multiverse Databases (HotOS 2019, Noria): per-user "universes" as dataflow views — https://pdos.csail.mit.edu/papers/multiversedb:hotos19.pdf.
- Hypothetical queries: Griffin & Hull (SIGMOD 1997) — https://dl.acm.org/doi/10.1145/253262.253304; Heraclitus, deltas as first-class values (TODS 1996) — https://dl.acm.org/doi/abs/10.1145/232753.232801.
- Dataset branching: Decibel (VLDB 2016) — https://www.vldb.org/pvldb/vol9/p624-maddox.pdf; OrpheusDB — https://arxiv.org/abs/1703.02475; TARDiS — https://www.cs.cornell.edu/lorenzo/papers/Crooks16Tardis.pdf; **BranchBench (2026-04)**: across Neon, DoltgreSQL, Tiger Data, Xata, "5–4000× slower reads as branches deepen" and "no current system supports the representative workloads at scale" — https://arxiv.org/abs/2604.17180.
- Git-for-data: Dolt prolly trees — https://www.dolthub.com/blog/2025-07-16-announcing-fast-merge/; Noms (archived) — https://github.com/attic-labs/noms; TerminusDB — https://github.com/terminusdb/terminusdb.
- Temporal: Datomic `with`/`as-of` and the "you can't branch the past" limitation — https://docs.datomic.com/reference/filters.html, https://blog.danieljanus.pl/datomic-forking-the-past/; XTDB bitemporal — https://docs.xtdb.com/about/time-in-xtdb.html.
- IVM survey with delta relations (±1 multiplicities) — https://arxiv.org/pdf/2404.17679.

Nothing found that literally uses a `world_id` column inside DBSP; the pattern is implied by indexed Z-sets but not documented as a use case (**UNVERIFIED** as prior art).

### Judgment

**Target differential-dataflow; DBSP is the runner-up.**
1. **Allowlist and static-binary cost dominate.** DD is 7 direct crates and no runtime; `execute_directly` runs on the calling thread. DBSP is 64–67 direct crates including tokio, three compressors, mimalloc, nix/libc, RNG and wall-clock crates and Feldera's storage layer, with no feature to shed them. For this project that is the adoption cost, before any API question.
2. **Purity fit.** DD's core is data plus logical time. DBSP would need an audit that storage, RNG and clock never execute on the single-thread path.
3. **Branching has a native home in DD.** Partially ordered timestamps let a branch be a timestamp coordinate (McSherry's `(version, history)`), so a fork shares every pre-fork arrangement and stores only diverging deltas — the "git branches over events" shape. In DBSP the clock is totally ordered, so a world must be a key column: it works but shares nothing structurally between worlds.
4. **Stability.** MIT with MSRV 1.86 and roughly monthly releases, versus MSRV 1.96.1, a release every two days with skipped tags, a docs.rs build that currently fails, and a LICENSE carve-out to re-audit on each bump.

DBSP stays a real runner-up: its Z-set algebra is the cleanest match to a delta-in/delta-out fold, the tutorial is better than anything DD has, rolling aggregates are fully incremental, and the core has formal semantics. The two predecessors (worldcraft on DD, timely_worlds on Timely) also mean DD experience already exists on the team.

**Flip to DBSP if:** Feldera ships a minimal `dbsp` without tokio/storage (watch `crates/dbsp/Cargo.toml` `[features]`); world computations turn out to be dominated by windowed/rolling aggregates rather than joins and graph-like state; DD goes quiet for ~6 months while Materialize still pins crates.io; or worlds are thousands of shallow, mostly independent forks where per-key partitioning beats lattice sharing.

**Flip away from both if** the v1 fold stays under the 100 ms budget at target forks × world size. Both engines cost more than they return below that line.

**Shaping v1's fold so either is a drop-in (judgment):**
1. State is multisets of flat tuples, never nested structs: each relation a `BTreeMap<Tuple, i64>` (weight) over `Ord` types. That is a Z-set / DD collection verbatim, and `BTreeMap` satisfies the no-HashMap-order rule.
2. Fold signature is delta-in, delta-out: `step(state, input: ZSet<Event>) -> ZSet<Row>` per relation, with `±weight` retractions; an update is a −1 and a +1, never in-place mutation.
3. `world_id` is a key column on every tuple from day one, opaque and totally ordered (u64). Forking in v1 = emitting +1 copies of the parent's consolidated state under the new id — later a DBSP indexed-Z-set key or a DD product-timestamp coordinate.
4. Explicit logical time on every update: `(row, world_id, logical_time, weight)`, `logical_time` = log offset, never wall clock. That is DD's `(data, time, diff)` plus the coordinate DD needs for `Product<world, offset>`.
5. Only six operators inside the fold: `map`, `filter`, equi-`join`, `reduce`/aggregate, `distinct`, `concat`/`negate`. Iteration (transitive closure) is a fixed point over these six, mapping to `iterate`/DBSP recursion.
6. Order-independent aggregates only: commutative monoids or full re-reduce of a group; "first/last/latest" must break ties by an explicit column.
7. Consolidate at every step boundary (merge equal tuples, drop zero weights) and define world equality as equality of consolidated multisets — the only equality either engine offers.
8. Golden tests are consolidated multisets: `fold(snapshot, events) == fold_from_scratch(events)`. The v1 fold becomes the oracle for the engine backend later.
9. One trait, `WorldEngine { ingest(delta), fork(from, at) -> WorldId, read(world) }`, with the fold as the first implementation. DD/DBSP types never enter `s2w-core`; they live in an adapter crate the allowlist can gate.

## 5. Rust MCP SDKs

### Facts

| Crate | Latest, published | Licence | Spec revision | Transports | GitHub |
|---|---|---|---|---|---|
| `rmcp` (official) | 3.4.1, 2026-09-23; releases every 1–2 weeks (3.4.0 09-15, 3.3.0 09-10, 3.2.0 08-31); 1.0.0 was 2026-03-03 — https://crates.io/crates/rmcp | Apache-2.0 on crates.io and in the workspace Cargo.toml; the repo LICENSE describes an MIT→Apache-2.0 transition with some older contributions still MIT (verified by the author) — https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/LICENSE | Implements **2026-07-28**, compatible with 2025-11-25 and earlier — https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md | `transport-io` (stdio), `transport-async-rw`, `transport-streamable-http-server`, client transports; no legacy SSE server in 3.4.1 — https://docs.rs/rmcp/latest/rmcp/transport/index.html | 3,957★, pushed 2026-09-27, 50 open; MSRV 1.88 — https://github.com/modelcontextprotocol/rust-sdk |
| `rust-mcp-sdk` | 2.0.0, 2026-08-27 — https://crates.io/crates/rust-mcp-sdk | MIT | 2.x = 2026-07-28 stateless; claims 110/110 server conformance tests — https://github.com/rust-mcp-stack/rust-mcp-sdk | `stdio`, `sse` (compat), `streamable-http`; axum and actix integrations | 195★, pushed 2026-09-19, 1 open |
| `tower-mcp` | 0.23.0, 2026-09-26 (created 2026-01-28) — https://crates.io/crates/tower-mcp | MIT OR Apache-2.0 | 2025-11-25 baseline, `protocol-2026-07-28` feature | stdio, Streamable HTTP, WebSocket, Unix socket | 11★, one maintainer — https://github.com/joshrotenberg/tower-mcp |
| `poem-mcpserver` | 0.3.1, 2025-10-13 — https://crates.io/crates/poem-mcpserver | MIT OR Apache-2.0 | **UNVERIFIED**; predates 2025-11-25 by date | stdio, streamable HTTP | in the poem monorepo; no release in ~11 months |
| `mcp-core`, `mcpr`, `mcp-sdk`, `mcp-rs` | 2025-05-01 / 2025-03-16 / 2025-01-20 / 2024-11-29 | Apache/MIT | 2024-11-05 era | stdio + legacy SSE | dormant; `mcpr` repo archived 2026-02-08 — https://github.com/conikeec/mcpr |

**rmcp server surface** (docs.rs, 3.4.1):
- `ServerHandler` trait: `list_tools`, `call_tool`, `list_resources(Option<PaginatedRequestParams>, …) -> Result<ListResourcesResult, McpError>`, `read_resource`, `list_resource_templates`, `list_prompts`, `get_prompt`, `complete`, `get_info`, `on_initialized` — https://docs.rs/rmcp/latest/rmcp/handler/server/trait.ServerHandler.html.
- Read-only annotation: `ToolAnnotations { read_only_hint: Option<bool>, destructive_hint, idempotent_hint, open_world_hint, title }` — https://docs.rs/rmcp/latest/rmcp/model/struct.ToolAnnotations.html; macro form `#[tool(name = "…", description = "…", annotations(read_only_hint = true))]` — https://docs.rs/rmcp-macros/latest/rmcp_macros/attr.tool.html; `#[tool_router]`/`#[tool_handler]` — https://docs.rs/rmcp/latest/rmcp/attr.tool_router.html.
- Push: `subscribe`/`unsubscribe` are deprecated (legacy); 2026-07-28 uses `accepted_subscription_filter` + `listen(SubscriptionContext)` and `SubscriptionSink::notify_resource_updated(uri)` — https://docs.rs/rmcp/latest/rmcp/service/struct.SubscriptionSink.html.
- Runtime: **tokio is a non-optional dependency**; `hyper` and `tower-service` are optional (HTTP transports); axum is a dev-dependency only. `StreamableHttpService` implements `tower_service::Service`, so it mounts with `Router::nest_service("/mcp", …)`; `StreamableHttpServerConfig` has `allowed_hosts` (defaults to loopback) and `allowed_origins` — https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/tower/struct.StreamableHttpService.html. Other deps pulled by `server` + `macros`: schemars, uuid, pastey, serde, serde_json, futures, thiserror, tracing, tokio-util, indexmap, chrono — https://crates.io/api/v1/crates/rmcp/3.4.1/dependencies.
- Official stdio example logs to stderr — https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/counter_stdio.rs.

**Spec facts (2026-07-28 is current — https://modelcontextprotocol.io/specification/versioning):**
- stdio: "The server **MUST NOT** write anything to its `stdout` that is not a valid MCP message"; it "**MAY** write UTF-8 strings to `stderr` for any logging purposes" — https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio. The protocol-level Logging feature is deprecated in favour of stderr/OpenTelemetry — https://modelcontextprotocol.io/specification/2026-07-28/changelog.
- Streamable HTTP: 2026-07-28 **removed protocol-level sessions and the GET stream endpoint**; servers SHOULD answer GET/DELETE with 405 and ignore `Mcp-Session-Id` and `Last-Event-ID`; every POST carries `MCP-Protocol-Version`, `Mcp-Method` and (for tools/call, resources/read, prompts/get) `Mcp-Name`; servers MUST validate `Origin`, SHOULD bind 127.0.0.1 locally, SHOULD send `X-Accel-Buffering: no` — https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http. `server/discover` is mandatory; list/read results carry `ttlMs`/`cacheScope`; tools/list SHOULD be deterministic in order. Legacy HTTP+SSE: "New implementations **SHOULD NOT** adopt it."
- 2026 comparisons: ChatForest recommends rmcp but flags "the pace of major bumps since 1.0" — https://chatforest.com/reviews/mcp-server-frameworks-sdks/; a benchmark of 15 implementations puts rmcp fastest at 10.9 MB RSS — https://www.tmdevlab.com/mcp-server-performance-benchmark-v2.html. `[no-record: r/rust thread comparing Rust MCP SDKs searched=WebSearch ×3 incl. site:reddit.com, as of 2026-09-27]`

### Judgment

**Recommend rmcp**, pinned to `=3.4.x`. It is the only option that is official, on 2026-07-28 with back-compat, tokio-native, tower-mountable into the same axum listener the web view needs, and has `read_only_hint` in struct and macro form. `listen` + `notify_resource_updated` is the right primitive for "world changed". Costs: a fast-moving 3.x line (read every changelog), partial docs coverage, and a dependency tail (schemars, uuid, pastey, sse-stream, rand, http-body…) the allowlist must accept. Keep `transport-streamable-http-server` behind a cargo feature so stdio-only builds stay small; log to stderr only; deterministic `tools/list` order falls out of a `BTreeMap` router.

**Runner-up rust-mcp-sdk 2.0** — MIT, 2026-07-28 with a published conformance score, nicer `#[mcp_resource]` macros, but 195 stars and its own schema type layer.

**Do not pick:** tower-mcp (11 stars, one maintainer, 0.x churn), poem-mcpserver (stale, revision unknown, drags in poem), the 2024–25 crates (dormant or archived).

## 6. Local web view

### Facts

Servers (crates.io / GitHub, 2026-09-27):

| Crate | Latest, published | Licence | GitHub | Notes |
|---|---|---|---|---|
| `axum` | 0.8.9, 2026-04-14 — https://crates.io/crates/axum | MIT | 27,266★, pushed 2026-09-25 — https://github.com/tokio-rs/axum | hyper 1, tower 0.5, tokio; MSRV 1.80; 489M downloads (121.8M recent vs actix's 11M — the hardest convergence datum). SSE in core, no feature flag; `ws` feature pulls `tokio-tungstenite` 0.29 — https://raw.githubusercontent.com/tokio-rs/axum/axum-v0.8.9/axum/Cargo.toml |
| `actix-web` | 4.15.0, 2026-08-21 | MIT OR Apache-2.0 | 24,846★ | Own runtime layer over tokio |
| `salvo` | 1.0.0, 2026-09-24 | Apache-2.0 | 4,437★ | `sse`, `websocket`, `serve-static` |
| `poem` | 3.1.12, 2025-07-28 | MIT OR Apache-2.0 | 4,443★ | 14 months since a release |
| `warp` | 0.4.3, 2026-05-04 | MIT | 10,373★, 233 open | Author: axum is "the better choice for standard server requirements" — https://seanmonstar.com/blog/warp-v04/ |
| `rocket` | 0.5.1, 2024-05-23 | MIT OR Apache-2.0 | 25,783★ | No release in 28 months |
| `tiny_http` / `astra` | 0.12.0, 2022-10-06 / 0.4.0, 2024-11-07 | permissive | — | Sync, no tokio |

Asset embedding:

| Crate | Latest, published | Licence | Behaviour |
|---|---|---|---|
| `memory-serve` | 2.4.0, 2026-09-17 — https://crates.io/crates/memory-serve | Apache-2.0 OR MIT | Returns an axum 0.8 `Router`; compile-time brotli for text, gzip/brotli by `Accept-Encoding`, ETag, hashed cache-busting routes; **serves from disk in debug builds**, `force-embed` overrides; 45★ — https://docs.rs/memory-serve |
| `rust-embed` | 8.12.0, 2026-07-08 — https://crates.io/crates/rust-embed | MIT | Embeds in release, reads disk in dev; `debug-embed`, `compression` (include-flate), `axum-ex` integration; 55M downloads. **Repository moved off GitHub** to https://pyrossh.dev/repos/rust-embed — activity metrics **UNVERIFIED** |
| `include_dir` | 0.7.4, 2024-06-17 — https://crates.io/crates/include_dir | MIT | Pure compile-time `include_dir!()`; no compression, no dev reload; 395★, pushed 2024-07-05 |
| `tower-serve-static` | 0.1.2, 2026-05-08 | MIT | `ServeDir` over an `include_dir::Dir` |
| `axum-embed` | 0.1.0, 2023-12-17 | MIT | axum 0.7 era; 0.8 compatibility **UNVERIFIED** |

Live updates:
- axum SSE: `axum::response::sse::{Sse, Event, KeepAlive}`; `Event` has `data`, `json_data`, `id`, `event`, `retry` — https://docs.rs/axum/latest/axum/response/sse/index.html. WebSocket: `axum::extract::ws` behind `ws` — https://docs.rs/axum/latest/axum/extract/ws/index.html.
- Browser `EventSource` reconnects automatically, honours `retry:`, resends `Last-Event-ID`; one-way; 6 connections per host on HTTP/1.1 — https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events, https://developer.mozilla.org/en-US/docs/Web/API/EventSource. WebSocket requires manual reconnection and supports binary/bidirectional — https://websocket.org/comparisons/sse/.
- Binary size (third-party, dated): an axum musl microservice ~5.9 MB → ~1.5 MB stripped (2023) — https://apatisandor.hu/blog/rust-microservice/; axum+tokio+serde hello world "a 1 MB release binary" with fat LTO, `codegen-units=1`, `strip` — https://github.com/AlfonsoCampodonico/axum-hello-world. No 2026 figure — **UNVERIFIED** for 0.8.9; measure in-repo.

### Judgment

**Recommend axum 0.8 + memory-serve + axum SSE.** rmcp already requires tokio and speaks tower `Service`, so axum adds one thin layer and the MCP endpoint and the web view share one listener and one runtime. memory-serve gives compile-time brotli, ETags, and disk reads in debug with the fewest moving parts. SSE beats WebSocket for a read-only server→browser view: zero extra dependencies, automatic browser reconnect, plain HTTP; the 6-connection cap is irrelevant for one local page. Send `KeepAlive` and `X-Accel-Buffering: no` regardless.

**Runner-up rust-embed** (`axum-ex`, `compression`) — larger community and framework-agnostic, but s2w writes the handler and the project has left GitHub (note for the allowlist audit). `include_dir` if zero build.rs matters more than compression or dev reload.

**No sync server.** tiny_http's last release is 2022 and the MCP SDK forces tokio in-process anyway, so a sync server adds a second concurrency model without removing the runtime.

## 7. Deterministic testing

### Facts

Property-based / fuzz: `proptest` 1.11.0, 2026-03-24, MIT OR Apache-2.0, org-maintained, four releases in 12 months — https://crates.io/crates/proptest; `proptest-state-machine` 0.8.0 (lockstep) — reference model + `StateMachineTest`, shrinks by deleting transitions from the back; **sequential only** — https://proptest-rs.github.io/proptest/proptest/state-machine.html. `quickcheck` 1.1.0, 2026-02-10 after a five-year gap; its README points users wanting shrinking to proptest — https://github.com/BurntSushi/quickcheck. `cargo-fuzz` 0.13.2, 2026-06-09, nightly + x86-64/aarch64 Unix only — https://github.com/rust-fuzz/cargo-fuzz; `bolero` 0.13.4, 2025-07-03, MIT — one `check!` front-end whose default engine replays corpora under plain `cargo test` — https://docs.rs/bolero.

Snapshot / golden: `insta` 1.48.0, 2026-06-11, Apache-2.0 — `assert_json_snapshot!`, `redactions` feature (`sorted_redaction`, `rounded_redaction`), `INSTA_UPDATE=auto` refuses to write in CI, `cargo insta review` — https://docs.rs/insta, https://insta.rs/docs/redactions/. `expect-test` 1.5.1 (2024-12), `goldenfile` 1.11.0 (2026-02), `trycmd` 1.2.1 (2026-07) / `snapbox` 1.2.2 for CLI golden tests — https://github.com/assert-rs/snapbox.

Deterministic simulation:
- `turmoil` 0.7.2, 2026-04-24, MIT, tokio-rs — single-thread simulation of hosts, time and network with seeded faults ("latency, drops, partitions, crashes, torn writes"); filesystem shim behind `unstable-fs`; it enables tokio `test-util` itself — https://raw.githubusercontent.com/tokio-rs/turmoil/main/crates/turmoil/Cargo.toml, https://docs.rs/turmoil. Announced as experimental — https://tokio.rs/blog/2023-01-03-announcing-turmoil. Does not control syscalls, foreign threads, or HashMap order.
- `madsim` 0.2.34, 2025-10-11, Apache-2.0 — replaces tokio under `--cfg madsim` with wrapper crates including **madsim-rdkafka**; used by RisingWave (simulation ~4–5× faster than real runs) — https://github.com/madsim-rs/madsim, https://risingwave.com/blog/applying-deterministic-simulation-the-risingwave-story-part-2-of-2/. GitHub pushed 2026-02-16.
- `shuttle` 0.9.4, 2026-09-22, Apache-2.0, AWS — randomized/PCT schedulers over `shuttle::sync`/`shuttle::thread`; its `future` module is its own executor, not tokio — https://docs.rs/shuttle. `loom` 0.7.2, 2024-04-23 — exhaustive model checking of atomics; small scope — https://github.com/tokio-rs/loom.
- tokio `time::pause`/`start_paused`: `test-util` + `current_thread` only; auto-advances idle timers — https://docs.rs/tokio/latest/tokio/time/fn.pause.html.
- Antithesis: commercial deterministic hypervisor; `antithesis_sdk` 0.3.0, 2026-08-28, MIT, no-op outside the platform; no public pricing — https://antithesis.com/docs/.

Prior art: FoundationDB simulation — https://apple.github.io/foundationdb/testing.html; TigerBeetle VOPR ("the seed and Git commit hash can be used to replay back the exact simulation"; replicas byte-for-byte identical) — https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/vopr.md; sled's simulation guide — https://sled.rs/simulation.html; Turso simulator — https://github.com/tursodatabase/turso/blob/main/testing/simulator/README.md; S2.dev (2025-04): nondeterminism sources hit were HashMap randomization, tokio scheduler RNG, wall clock leaking into headers, third-party threads; validated by diffing TRACE logs across two runs of one seed — https://s2.dev/blog/dst; Polar Signals "theater of state machines" (2025-07) and FOSDEM 2026 talk — https://www.polarsignals.com/blog/posts/2025/07/08/dst-rust; curated list — https://github.com/ivanyu/awesome-deterministic-simulation-testing.

### Judgment

**Pure fold:** proptest + proptest-state-machine + insta. Sequential-only is no limitation for a pure fold; the state-machine crate is exactly "reference model vs fold". insta's JSON snapshots with redactions are the golden-replay files, which `CLAUDE.md` already declares human-owned (`INSTA_UPDATE=no` in CI enforces that). Fuzz (`cargo-fuzz`, or `bolero` to keep targets runnable under `cargo test`) only over the log decoder — the one surface that sees untrusted bytes.

**Adapters and log:** tokio `current_thread` + `start_paused` for time, turmoil for the network layer. Do not adopt madsim by default: it is a whole-runtime swap gated on `--cfg madsim` and git patches, last released Oct 2025, trading tokio's LTS story for one maintainer's cadence. shuttle and loom are for hand-written `std::sync` state s2w should not have.

**Three seedable tests for interleavings and restarts:**
1. *Merge is a pure function.* `merge(Vec<(partition, offset, ts, bytes)>) -> Vec<Event>` in `s2w-core`, total order `(ts, partition, offset)` spelled out, `BTreeMap`/sorted `Vec` only. proptest generates events across N partitions, then `prop_shuffle()`s arrival order; assert `merge(shuffled) == merge(sorted)` and identical fold hash. Shrinking yields the minimal misordering.
2. *Crash = truncate.* Write the log, truncate at a proptest-chosen byte offset, recover, assert `recovered == prefix_of_committed_records`, then re-append the tail and assert the same final world hash. turmoil `unstable-fs` can inject torn writes later; this test needs no framework.
3. *Replay determinism.* Run the pipeline twice on one seed (tokio paused clock, turmoil seed, RNG seed); compare the final world hash and the TRACE log byte-for-byte (S2's "meta test"). Any diff is a nondeterminism leak.
Invariant for the state-machine test: per-partition offsets strictly increasing in the log, so restart-safety is "resume from max committed offset per partition".

**Runner-up madsim end-to-end.** Flips if turmoil's fs shim stays unstable and disk faults through `tokio::fs` are needed, or if Kafka semantics must be simulated (madsim-rdkafka is the only Rust option short of Antithesis).

## 8. Cross-cutting

### 8a. Async runtime

Facts: tokio 1.53.1, 2026-07-20, MIT — https://crates.io/crates/tokio; LTS table: "1.51.x — LTS release until March 2027", "1.53.x — LTS release until September 2027", MSRV 1.71 for LTS lines — https://github.com/tokio-rs/tokio/blob/master/README.md. async-std is **discontinued**: crates.io description "Deprecated in favor of `smol`", RUSTSEC-2025-0052 — https://rustsec.org/advisories/RUSTSEC-2025-0052.html, https://github.com/async-rs/async-std. smol 2.0.2 (2024-09). io_uring runtimes (`compio` 0.19.2, `monoio`, `glommio`) are Linux-only and thread-per-core.

Judgment: tokio, pinned to the 1.53.x LTS. rskafka, reqwest, axum and rmcp are all tokio-shaped. Determinism is unaffected because the fold is pure; use `current_thread` in tests.

### 8b. JSON

Facts: `serde_json` 1.0.151, 2026-07-20, MIT OR Apache-2.0 — https://crates.io/crates/serde_json; README ballpark "500 to 1000 megabytes per second deserialization" — https://raw.githubusercontent.com/serde-rs/json/master/README.md; features `raw_value` (borrowed `&'a RawValue` defers or skips parsing — https://docs.rs/serde_json/latest/serde_json/value/struct.RawValue.html), `preserve_order` (switches `Map` to indexmap insertion order), `float_roundtrip`. `simd-json` 0.18.1, 2026-08-23, Apache-2.0 OR MIT — requires a **mutable** input buffer, runtime SIMD detection, "uses **a lot** of unsafe code" — https://github.com/simd-lite/simd-json. `sonic-rs` 0.5.10, 2026-09-11, Apache-2.0 — stable Rust, x86_64/aarch64 SIMD, but the README asks for **`-C target-cpu=native`** (no runtime dispatch), README numbers: twitter struct 796 µs vs 1,061 µs simd-json vs 2,266 µs serde_json — https://github.com/cloudwego/sonic-rs. No independent 2025–26 benchmark covering all three was found; the one dated reproducible page (serde_json_borrow 0.8, Rust 1.87) shows serde_json at 309–684 MB/s on its datasets — https://flexineering.com/posts/serde-json-borrow-08/. Canonical JSON: RFC 8785 JCS — https://www.rfc-editor.org/rfc/rfc8785; `serde_json_canonicalizer` 0.3.2, 2026-02-03, MIT — https://crates.io/crates/serde_json_canonicalizer; `serde_jcs` 0.2.0, 2026-03-25.

Judgment: serde_json with `raw_value` and borrowing is enough. At 10^8 × 1 KB (100 GB) and 500 MB/s, parse CPU is ~200 s — not the bottleneck next to fetch and the fold. sonic-rs's `target-cpu=native` conflicts with one portable binary; simd-json's mutable buffer forces a copy of every Kafka payload and adds a large unsafe surface to the allowlist. Determinism: keep the default `serde_json::Map` (sorted keys) — never enable `preserve_order`; enable `float_roundtrip` if floats reach the world, or better, have `s2w-model` refuse floats in favour of integers/decimal strings; hash worlds through a typed canonical serializer in `s2w-core` (a typed struct with sorted fields has one serialization), reaching for JCS only when hashing untyped JSON. Revisit only if a profile shows parse > 30% of wall time, and then on the adapter side only.

### 8c. Error handling

Facts: `thiserror` 2.0.21, 2026-09-23, MIT OR Apache-2.0, `#![no_std]` source with a default `std` feature — https://crates.io/crates/thiserror, https://raw.githubusercontent.com/dtolnay/thiserror/master/Cargo.toml; `anyhow` 1.0.104, 2026-07-18 — README: "Use Anyhow if you don't care what error type your functions return… common in application code. Use thiserror if you are a library that wants to design your own dedicated error type(s)" — https://github.com/dtolnay/anyhow. `core::error::Error` stabilised in Rust 1.81 (2024-09-05) — https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/. `miette` 7.6.0 (2025-04), `eyre` 0.6.14, `snafu` 0.9.2 exist.

Judgment: thiserror in every `s2w-*` lib crate (already `s2w-model`'s declared dependency), anyhow only in the `s2w` binary. Skip eyre/snafu (a second vocabulary for no gain); miette only if a query/rules language needs span diagnostics. `core::error::Error` lets `s2w-core` be no_std-clean, a cheap mechanical proof of "no I/O".

### 8d. Licences

Every recommended crate is MIT and/or Apache-2.0 (crates.io `license` fields verified 2026-09-27): rskafka, rusqlite (SQLite public domain), redb, fjall, differential-dataflow (MIT), dbsp (MIT OR Apache-2.0, see §4 carve-out), rmcp (Apache-2.0; repo mid-transition from MIT), rust-mcp-sdk (MIT), axum, memory-serve, rust-embed, tokio, serde_json, thiserror, anyhow, proptest, insta (Apache-2.0), turmoil, madsim (Apache-2.0), shuttle (Apache-2.0), cargo-deny. Permissive-but-not-MIT/Apache texts the allowlist must name explicitly: librdkafka BSD-2-Clause (if rdkafka), RocksDB BSD-3 components (if RocksDB), `ring` "Apache-2.0 AND ISC", `aws-lc-rs`/`aws-lc-sys` (ISC, BSD-3-Clause, MIT-0 mix; rustls 0.23's default provider — https://docs.rs/rustls/latest/rustls/), LMDB's OpenLDAP Public License (if heed), quickcheck's Unlicense, samsa's non-SPDX field, open-wal's BSD-3. Copyleft to exclude: `sharded-log` (GPL-3.0), `readyset` (BSL 1.1), RocksDB's GPL-2.0 alternative (choose Apache-2.0).

Enforcement: `cargo-deny` 0.20.2, 2026-07-09, MIT OR Apache-2.0 — https://crates.io/crates/cargo-deny. `[licenses] allow` is deny-by-default ("Licenses not in this list are denied by default") — https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html; `[bans] allow` with one or more entries denies every crate not listed ("use with care"), plus `allow-workspace`, `multiple-versions = "deny"`, `wildcards = "deny"` — https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html. That is the per-crate allowlist as a CI gate; its `advisories` check makes `cargo-audit` redundant. `cargo-vet` 0.10.2 (Mozilla) adds source audits if wanted — https://mozilla.github.io/cargo-vet/. Judgment: `deny.toml` with `[licenses] allow = ["MIT","Apache-2.0","BSD-2-Clause","BSD-3-Clause","ISC","Unicode-3.0","Zlib"]`, `[bans] allow-workspace = true, multiple-versions = "deny", wildcards = "deny"` plus the explicit crate list from this note, `[sources] unknown-registry = "deny", unknown-git = "deny"`; pick one rustls crypto provider (`ring`, or aws-lc-rs knowingly with its cmake build) and never let both into the tree.

## Open items for the decision records

- Measure, do not assume: static-musl binary size with rskafka + axum + rmcp + redb; `cargo tree -e no-dev` for rmcp vs rust-mcp-sdk; the minimal `parquet` build's size delta; rskafka `OffsetAt::Timestamp` against Apache Kafka and Redpanda.
- Re-verify on every `dbsp` bump: the LICENSE carve-out clause and the `feldera-*` transitive crates; the crate's `[features]` for a runtime-free build (the flip condition in §4).
- Watch: rskafka releasing the 18 months of work on main (it changes the TLS provider features); rmcp 4.0; fjall's stated 2026 wind-down; DD's commit cadence past ~6 months quiet.
- Two things this note could not find and says so: an independent append-only large-value storage benchmark, and any literal prior art for `world_id` as a DBSP key column.

## Sources

Per-question source lists are inline above. Aggregate verification done by the author on 2026-09-27: crates.io `GET /api/v1/crates/<name>` and `/versions?per_page=1` for 35 crates (version, publish date, licence, MSRV); GitHub `GET /repos/<owner>/<repo>` for 22 repositories (`pushed_at`, stars, open issues, archived flag, licence); raw Cargo.toml and LICENSE files for feldera/feldera (root, `crates/dbsp`), TimelyDataflow (differential-dataflow, timely), modelcontextprotocol/rust-sdk (root, `crates/rmcp`), CallistoLabsNYC/samsa; librdkafka issue #3261; rskafka `OffsetAt` on docs.rs.

## Design implications

Each choice becomes a decision record written by the issue that first uses it, after measuring
the open items above that apply.

1. Kafka client rskafka. → adopted: [decision 0006](../docs/decisions/0006-kafka-client.md) (stream2worlds#7; Redpanda timestamp check still open)
2. SSE: hand-rolled loop on reqwest. → deferred: stream2worlds#6
3. Log: own segment files + redb. → deferred: stream2worlds#8
4. Incremental engine: differential-dataflow first, DBSP runner-up, still on trigger.
   → adopted: README technical-architecture row (2026-09-27)
5. MCP rmcp; 6. web view axum + memory-serve. → deferred: stream2worlds#10
7. Testing: proptest + insta for the fold, paused clock + turmoil for adapters. → adopted: docs/decisions/0005-pure-fold.md for the fold, proptest + insta (2026-09-27); paused clock + turmoil apply as each adapter gets tests
8. tokio, serde_json, thiserror/anyhow. → deferred: stream2worlds#6 (first crate that needs them)
9. Licences enforced with cargo-deny. → deferred: stream2worlds#16
