# 0007: Kafka client — rskafka by partition assignment

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #7 · Research [0003 §1](../../research/0003-rust-substrate.md#1-kafka-clients)

## Decision

The Kafka source uses `rskafka` 0.6.0 (pinned `=0.6.0`, default features: gzip, lz4, snappy and
zstd decompression). `s2w` reads a topic by explicit partition assignment: it reads the topic's
partitions from cluster metadata once, resolves one start offset per partition, and runs one
fetch loop per partition. It never joins a consumer group and never commits an offset. `rskafka`
has no consumer-group or offset-commit API, so this holds by construction rather than by
configuration.

- **Cursors.** Each partition is its own log source, `kafka.<topic>.p<partition>`, whose cursor
  is the last stored offset. A restart resumes at the next offset. Kafka's topic charset is the
  same as `SourceId`'s; a name longer than 128 bytes is a loud error.
- **Payload.** Each record is stored as a byte-deterministic JSON envelope with fixed key order:
  `key`, `offset`, `partition`, `timestamp_ms`, `topic`, `value`. `key` and `value` are the raw
  bytes as a JSON string when they are valid UTF-8, `{"hex": …}` otherwise, and `null` when
  absent; the value is never parsed and re-serialised. The offset is inside the payload because
  the log dedupes on `(source, payload hash)`: raw values legitimately repeat (heartbeats), and a
  real redelivery of the same offset must still collapse. Headers are not stored yet.
- **Start.** `--since` (RFC 3339 or epoch milliseconds) uses `OffsetAt::Timestamp`
  (`offsetsForTimes`, inclusive: the first record at or after the time). With neither a stored
  cursor nor `--since`, a partition starts at its high watermark.
- **Never skip.** A fetch error is reported and retried from the same offset with a 250 ms to
  30 s backoff. `rskafka`'s own retry loop gets a 60 s deadline so a dead broker surfaces as an
  error rather than as a quiet partition. A resume offset deleted by retention
  (`CursorPruned`) or past the partition's end (`CursorAhead`, a recreated topic) stops the
  source with a loud error; it never jumps.

## Measured (Apache Kafka 4.1.0, KRaft single node, hub, 2026-09-27)

The ignored test `kafka::tests::reads_by_assignment_against_a_real_broker`
(`S2W_KAFKA_BROKER=localhost:19092 cargo test -p s2w-sources -- --include-ignored`) measured:

- `OffsetAt::Timestamp(t)` returned the first offset whose record timestamp is ≥ `t` (inclusive)
  in each of three partitions.
- A time after the last record returns **-1**; the source maps it to the high watermark.
- A zstd-compressed batch decoded with the default features.
- After the run, `kafka-consumer-groups.sh --list` showed no group. The same command listed a
  group after a positive-control `kafka-console-consumer --group` run, so the instrument can see
  one.

Not measured: Redpanda (research 0003's open item). The worker had no Docker access; the
Redpanda check stays open, and `--since` against Redpanda is untrusted until it is run.

## Why

Research 0003 §1: `rskafka` is the only candidate that meets "never join a group, never commit"
by construction, has the timestamp lookup, has PLAIN/SCRAM/OAUTHBEARER in the released version,
and uses rustls. `rdkafka` requires a throwaway `group.id` even for `assign()` (librdkafka
#3261, open since 2021) and brings about 130K lines of C and an open static-musl build issue.
`kafka` (sync, OpenSSL, "at your own risk") and `samsa` (one maintainer, non-SPDX licence field)
were rejected.

## Costs, accepted

- 0.6.0 is 18 months old with unreleased work on main, and InfluxDB no longer uses it.
- `s2w` owns the fetch loop, backoff and partition discovery. Partitions are discovered once; a
  partition added while `s2w` runs is read after a restart, from its high watermark.
- The default decompression features compile C (`zstd-sys`, `lz4-sys`). A producer picks the
  compression, so dropping a codec would make some topics unreadable. All licences pass
  `deny.toml`.
- `rskafka` pins `thiserror` 1, a duplicate of the workspace's 2 (`multiple-versions = "warn"`).
- TLS and SASL are not wired yet; the URL form is plaintext `kafka://`.

## Revisit when

`rskafka` publishes a release from main (TLS provider features change), the Redpanda timestamp
check fails, or the crate goes unmaintained.
