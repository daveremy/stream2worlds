# 0006 — Where s2w breaks as streams get bigger, and the cheapest way past each wall

| | |
|---|---|
| **Question** | Dave (2026-09-27): *"what scaling issues will we hit? how might we surmount them?"* Where does `s2w` break first as streams grow from Wikipedia (tens of events/s) toward ordinary Kafka topics (10^3–10^6 events/s), along ten dimensions; for each, the likely limit estimated from first principles and cited benchmarks, the earliest observable symptom, a ladder of remedies from cheapest to most complex with prior art, and the measurement that says when to climb a rung. Also: a target envelope for the first slice, with explicit non-goals, and a scale fitness function for CI. |
| **Date** | 2026-09-27 |
| **Researcher** | sagan |
| **Feeds** | The README Technical architecture rows marked *on trigger* (incremental engine, event log); [issue #19](https://github.com/daveremy/stream2worlds/issues/19) (segment files / batch append); the fold ([#9](https://github.com/daveremy/stream2worlds/issues/9)); the query API ([#10](https://github.com/daveremy/stream2worlds/issues/10)); gate-3 H ([#4](https://github.com/daveremy/stream2worlds/issues/4)); a future decision record "the slice-1 scale envelope" and an xtask fitness function |
| **Status** | done — implications dispositioned 2026-09-27 |

Method. Five measurements were taken on the hub on 2026-09-27 (machine: AMD Ryzen 7 7730U, 8 cores / 16 threads, 30 GB RAM, Kingston OM8TAP41024K1 1 TB NVMe, ext4 on LVM, Linux 7.0.0-30-generic; the SQLite measurements ran through Python 3's `sqlite3` module with `journal_mode=WAL`, `synchronous=FULL`, a ~1 KB payload and a cursor row updated in the same transaction, matching `crates/s2w-log`). Every other number is cited to the page it came from, fetched the same day; anything not confirmed from a fetched page is marked **UNVERIFIED**. Facts and judgment are in separate subsections. ⚠️ One measurement was thrown away: the first SQLite run reported 40,000 fsynced transactions per second and an fdatasync p50 of 0.00 ms, because the scratchpad directory is tmpfs (`df -T /tmp` → `tmpfs`). Every number below is from the NVMe-backed filesystem. The fitness function in §12 has to run on real storage for the same reason.

Constraints this note is written against (README, `CLAUDE.md`, decisions 0001 and 0002, research 0003 and 0005): one static Rust binary; a pure single-threaded fold over an append-only log; slice 1 stores the log in SQLite (WAL, `synchronous=FULL`, one transaction per append, no batch API); System 1 per event in µs–ms; System 2 reads snapshots asynchronously on a budget; possible worlds sampled by rolling forward from the current snapshot; a forecast ledger graded against the stream; a web view and later a 3D explorer that read the world through the MCP query interface with level of detail as an API parameter; Kafka read by partition assignment, no consumer group, no commits; the incremental engine switches in only when *forks × world size* misses a 100 ms frame budget (0003 §4).

## Summary (judgment)

The walls arrive in this order for a Kafka user, and each has a one-rung remedy that is cheaper than the next wall:

| Order | Wall | Where it lands | First symptom | Cheapest rung |
|---|---|---|---|---|
| 1 | **Fsync per append** | ~1,000 events/s on a consumer NVMe (measured: 871–1,204 txn/s) | Source cursor falls behind the source head; lag grows linearly | Group commit: one transaction per N events or T ms (measured 69k/s at N=100) — #19 |
| 2 | **Disk fill** | 1.3 KB × rate: Wikipedia 4.5 GB/day; 10^4/s → 1.1 TB/day | Free space, not CPU | A retention window on the log, with cold segments compressed; snapshots so replay does not need the whole log |
| 3 | **World memory** | 300 B/entity × entities: Wikipedia ~0.7 GB/day worst case; an orders topic at 10^3/s ~26 GB/day | RSS climbs linearly with events; fold p99 rises with cache misses | Age entities out on log time (a tombstone is an event); then tier cold entities to disk |
| 4 | **Replay from 0** | 10^8 events at a 50k/s fold ≈ 33 min | Cold start and scrub latency | Snapshots (state + log position + fold hash), sampled golden re-derivation in the background |
| 5 | **Single-threaded fold** | 10–100k events/s depending on rule cost; embeddings ~8k/s | Fold thread saturated while parse threads idle | Pipeline parse off the fold thread; per-key monoid pre-folds; then DD with workers |
| 6 | **Possible worlds** | Full clone: 100 MB world × 20 forks = 2 GB and seconds | Fork latency, frame drops | Overlay worlds (parent + delta); persistent maps; DD branch coordinate |
| 7 | **Hub entities** | One entity with degree 10^5–10^7 (a wiki) | Any per-edge op on the hub costs O(degree) per event | Cap materialized in-degree; hubs become attributes plus counters |
| 8 | **Ledger and System 2** | Grow with issuance rate and schema width, not with the world | Ledger outgrows the event table; LLM spend | Index pending by deadline; issue per registered question; drift-triggered System 2 with a cached prefix |

Distribution (dimension 10) is not on the list: the target user points `s2w` at one topic on one machine, and at every rate where one machine is not enough, the disk fills before the CPU does. The right response is partition subsets and key sampling, not a cluster.

## 0. The stream that sizes everything

### Facts

Live sample by the author, 2026-09-27, 30 s of `https://stream.wikimedia.org/v2/stream/recentchange`, all wikis: **1,197 events (39.9/s), median 1,299 B, mean 1,339 B, max 3,399 B per event**. Research 0005 §0 measured 27.7/s over 62 s the same morning and ~1,900 new entity ids per minute (≈100k/hour). Python's `json.loads` took 7 µs per event (181 MB/s) on the hub. English Wikipedia alone: 7,245,398 articles, **66,321,117 total pages, 54,698,258 registered accounts, 267,685 active in the last 30 days** — https://en.wikipedia.org/wiki/Special:Statistics. Kafka's own published ceilings: 1 million 1 KB messages/s (605 MB/s) across a three-broker cluster of i3en.2xlarge (8 vCPU, 64 GB, 2 × 2.5 TB NVMe), p99 5 ms at 200 MB/s — https://www.confluent.io/blog/kafka-fastest-messaging-system/ (Nikhil and Chandar, 2020-08-21); 821,557 100-byte records/s from one producer thread to three 2014-era machines — https://engineering.linkedin.com/kafka/benchmarking-apache-kafka-2-million-writes-second-three-cheap-machines (Kreps, 2014-04-27).

### Judgment

Derived rates used throughout: Wikipedia is **~40 events/s, ~52 KB/s, ~3.5 M events and 4.5 GB per day**, ~100 M events and ~135 GB per month. A Kafka topic at 10^4/s of similar events is 13 MB/s and 1.1 TB/day. Dave's "10^3–10^6 events/s" spans a single Kafka cluster's whole published throughput at the top; most single topics sit near the bottom of that range, and the benchmark numbers say a 10^6/s *topic* is an exceptional installation. The id universe of a stream is large (10^8 pages and accounts on enwiki alone) but the active set is not (267k users in a month), so distinct-entity growth is sublinear (Heaps' law, 0002 §8) on human streams; it is linear on machine streams where every order or shipment is a new entity.

## 1. Ingest rate

### Facts

**SQLite append, measured on the hub** (WAL, `synchronous=FULL`, ~1 KB payload, cursor row in the same transaction, Python-driven):

| Transaction size | Events/s | Transactions/s |
|---|---|---|
| 1 event (as `s2w-log` does today) | **871–1,204** | 871–1,204 |
| 10 events | 11,333 | 1,133 |
| 100 events | 68,984 | 690 |
| 1,000 events | 158,109 | 158 |
| 1 event, `synchronous=NORMAL` | 18,480 | — |

Raw `fdatasync` after a 1 KB append on the same disk: **p50 1.02 ms, p99 2.42 ms, max 2.65 ms** (300 samples). So one fsynced transaction ≈ one fdatasync, and the single-transaction ceiling is the disk's sync latency, not SQLite. The batched figures are Python-bound upper limits on overhead: SQLite's own FAQ says "50,000 or more INSERT statements per second on an average desktop computer. But it will only do a few dozen transactions per second… Transaction speed is limited by the rotational speed of your disk drive", with a 2024-11-19 update that "SQLite will do far more than 50K inserts/second now… But the gist of the answer above remains correct: Putting multiple operations inside a single transaction can improve performance dramatically" — https://www.sqlite.org/faq.html#q19. `synchronous=NORMAL` in WAL mode keeps the database consistent but "transactions… might rollback following a power failure" — https://www.sqlite.org/wal.html (cited in 0003 §3). Decision 0002 fixed "one transaction per append; no batch API in slice 1" and names #19 as the reopening issue.

**Group commit prior art.** PostgreSQL `commit_delay` "adds a time delay before a WAL flush is initiated. This can improve group commit throughput by allowing a larger number of transactions to commit via a single WAL flush", and `synchronous_commit = off` "does not create any risk of database inconsistency: an operating system or database crash might result in some recent allegedly-committed transactions being lost" — https://www.postgresql.org/docs/current/runtime-config-wal.html. Kafka itself: "Kafka does not require that crashed nodes recover with all their data intact" — it relies on replication rather than per-message fsync — https://github.com/apache/kafka/blob/trunk/docs/design/design.md. Flink's exactly-once recovery likewise assumes "a persistent (or durable) data source that can replay records for a certain amount of time. Examples for such sources are persistent messages queues (e.g., Apache Kafka…)" — https://nightlies.apache.org/flink/flink-docs-stable/docs/dev/datastream/fault-tolerance/checkpointing/.

**JSON.** serde_json's README ballpark is 500–1,000 MB/s; a dated reproducible page shows 309–684 MB/s (0003 §8b). At 1.3 KB per event that is roughly **250k–750k events/s per core**. `raw_value` lets the log store bytes and defer parsing of fields System 1 does not read (0003 §8b).

**System 1 budget by arithmetic.** Single-threaded, the per-event budget is 1/rate: 1 ms at 10^3/s, 100 µs at 10^4/s, 10 µs at 10^5/s, 1 µs at 10^6/s. The local name-embedding model runs at about 8,000 strings/s single-threaded (model2vec, 0002 §7).

### Judgment

**(a) Limit.** Wikipedia (40/s) uses 3–5% of the single-transaction ceiling and nothing else is close; slice 1 has no ingest problem. The first wall for a Kafka user is exactly at 10^3/s: the disk's fdatasync latency. Batching by 100 moves it to ~7 × 10^4/s; JSON parsing on one core is the next wall at a few 10^5/s; System 1 rules at 10 µs each saturate one core at 10^5/s; embeddings per event cap at ~8 × 10^3/s and must be sampled or made asynchronous above that.

**(b) Earliest symptom.** The source cursor's lag behind the source head (Kafka: high-water mark minus assigned offset, per partition; Wikipedia: `meta.dt` minus receipt time) grows without bound. This is also the statistic to print on `s2w watch`'s status line from the first slice, because a lag that is merely large looks the same as one that is growing unless it is plotted against time.

**(c) Ladder.**
1. *Nothing.* Below ~500/s the one-transaction-per-append design is correct and the cheapest to reason about (decision 0002).
2. *Group commit* (#19): the log takes a `Vec<RawEvent>` and commits one transaction per N events or T ms, whichever first, advancing cursors in the same transaction. Same atomicity story as today; the visible change is that a crash loses up to N events that are re-fetched from the cursor. Measured 11k/s at N=10 and 69k/s at N=100 on this disk. Prior art: SQLite FAQ 19, PostgreSQL `commit_delay`, Kafka producer batching.
3. *`synchronous=NORMAL` for replayable sources only.* 18k/s single-transaction, and a power loss can roll back the last transactions without corruption. Correct for Kafka (the source is durable and the cursor rolls back with the event, so the events are simply re-fetched) and for Wikipedia within its 7–31 days of history (0003 §2); **wrong for stdin**, which has no replay. Make it a per-source property, never a global flag.
4. *Parse off the fold thread.* Decoding is per-event and order-independent; run it on a pool that hands `(position, parsed)` to the single fold thread in log order. Determinism is untouched because the fold still consumes the log's order.
5. *Deferred parsing* with `raw_value`: System 1 reads the paths the profile says matter; everything else stays bytes until a query asks.
6. *Own segment format* (0003 §3, #19): length-prefixed records, `fdatasync` per batch, `memmap2` replay. Needed only when batched SQLite (measured 1.6 × 10^5/s, Python-bound) is the bottleneck — that is a 10^5+/s topic, where the disk fills in hours (§2, §10) before this matters.
7. *Partition-parallel fold* — dimension 4.

**(d) Trigger.** Climb from rung 1 to 2 when the measured append rate is below 2× the source's sustained rate, or when cursor lag grows for five consecutive minutes. Climb to 6 when a `cargo bench` on the fitness fixture shows the batched append below 2× the target envelope's rate on real storage.

## 2. World size over time

### Facts

Research 0005 §0: ~100k new entity ids per hour on the full stream; enwiki's id universe is 66 M pages and 55 M accounts but 268k accounts were active in 30 days (§0 above). Flink's in-memory backend: "state size is limited by available memory within the cluster"; its RocksDB backend: "the amount of state that you can keep is only limited by the amount of disk space available", at the cost that "all reads/writes from/to this backend have to go through de-/serialization" and "the maximum throughput that can be achieved will be lower" — https://nightlies.apache.org/flink/flink-docs-stable/docs/ops/state/state_backends/. Flink state TTL: "If a TTL is configured and a state value has expired, the stored value will be cleaned up on a best effort basis"; expired values are "explicitly removed on read" and can be garbage-collected by an incremental lazy iterator or, on RocksDB, "a Flink specific compaction filter" during compaction — https://nightlies.apache.org/flink/flink-docs-stable/docs/dev/datastream/fault-tolerance/state/. Kafka log compaction deletes by a marker: "Such a record is sometimes referred to as a *tombstone*", visible to consumers that reach the head within `delete.retention.ms` "(the default is 24 hours)" — https://github.com/apache/kafka/blob/trunk/docs/design/design.md. Kafka Streams' state is local RocksDB per task with a changelog topic for restoration, and "standby replicas" to shorten restore time — https://github.com/apache/kafka/blob/trunk/docs/streams/architecture.md. Research 0003 §4 shapes the world as `BTreeMap<Tuple, i64>` multisets of flat tuples with `world_id` and logical time on every row.

**Memory per entity, first principles (judgment, not measured):** a stable id (8 B), a name (24 B `String` header plus ~30 B on the heap), a type tag and timestamps (~24 B), B-tree overhead at ~1.1× the key/value bytes, plus each relationship stored twice (~40 B per direction). An entity with 2–3 relationships lands around **200–500 B**; 300 B is the planning number, to be replaced by the fitness function's measured figure (§12).

### Judgment

**(a) Limit.** Linear worst case on Wikipedia: 2.4 M new ids/day × 300 B ≈ **0.7 GB/day**, so a 16 GB laptop holds ~10 days of the full stream if nothing repeats and ~3 weeks on enwiki alone; Heaps'-law saturation stretches that, and no one has measured by how much. On a machine stream where every event is a fresh entity (orders at 10^3/s: 86 M/day), it is **26 GB/day** — one day. The world's memory is therefore the second wall for a Kafka user and the first for anyone running Wikipedia for a month.

**(b) Earliest symptom.** RSS grows linearly with events processed rather than flattening; fold p99 creeps up as the B-tree stops fitting in cache; then swap. The world's entity count and bytes-per-entity belong on the status line beside lag.

**(c) Ladder.**
1. *Measure.* Entities, relationships, bytes per entity, and the distinct-count growth curve (0002 §6 stage 2 already keeps it) — the curve says whether the stream is Heaps-like or linear, which decides how urgent the rest is.
2. *Bounded retention on log time.* `--retain 24h`: an entity with no event in the window is tombstoned by an event the fold emits (like a repair: it has a source and can be revoked), driven by **event time or log offset, never wall clock**, so replay reproduces the same tombstones and determinism holds. Prior art: Kafka tombstones and `delete.retention.ms`; Flink TTL's cleanup-on-read plus background sweep. The forecast ledger keeps its own references (an issuance names an entity id, which is never reused — 0005 implication 2), so grading a forecast about an aged-out entity still works.
3. *Tiered state.* Hot entities (active in the window) in memory; cold ones serialized to the side store keyed by id (SQLite, or redb per 0003 §3) and rehydrated on their next event — a cache miss costs one disk read. This is Flink's RocksDB backend in miniature and it costs what Flink says it costs: serialization on every cold access and lower throughput. Because the fold is pure, the tier lives in the shell: the fold takes the hot set plus the specific cold entities the batch touches.
4. *Compact representation.* Intern strings, `u32` entity ids inside the fold, struct-of-arrays per type, tuples not structs (0003 §4 #1 already says flat tuples). Roughly halves bytes per entity; do it after measuring, not before.
5. *What must stay resident, and what may not.* The forecaster needs the focus entity's ego-network and per-type statistics; the explorer at `lod=type|cluster` needs aggregates and at `lod=entity` needs a focus and a few hops (0005 §7). Neither needs every entity resident, which is why rungs 2 and 3 do not change what either sees. The per-type profile (0002 §6) is O(paths), not O(entities), and stays resident forever.

**(d) Trigger.** Climb to rung 2 when projected RSS at 30 days of the current rate exceeds half of RAM (bytes/entity × distinct-count slope × 30 days); climb to 3 when the retention window a user wants is longer than memory allows. The fitness function (§12) gates bytes per entity so rung 4 is never an emergency.

## 3. Replay cost

### Facts

Measured on the hub: paged `SELECT` replay from SQLite at **1.42 M rows/s** (256-row pages, as `s2w-log` does, Python-driven) — storage is not the replay bottleneck; parsing and the fold are. Fowler on event sourcing: "A system in use during a working day could be started at the beginning of the day from an overnight snapshot… Should it crash it replays the events from the overnight store", and "we can discard the application state completely and rebuild it by re-running the events from the event log on an empty application" — https://martinfowler.com/eaaDev/EventSourcing.html. Flink: "Checkpoints allow Flink to recover state and positions in the streams to give the application the same semantics as a failure-free execution" — https://nightlies.apache.org/flink/flink-docs-stable/docs/dev/datastream/fault-tolerance/checkpointing/. Kafka Streams restores local state from its changelog topic and keeps standby replicas to shorten that (architecture.md above). Serialization crates for a snapshot, crates.io 2026-09-27: `rkyv` 0.8.18 (2026-08-05, MIT, zero-copy — https://crates.io/crates/rkyv), `postcard` 1.1.3 (2025-07-24, MIT OR Apache-2.0 — https://crates.io/crates/postcard), `bincode` 3.0.0 (2025-12-16, MIT — https://crates.io/crates/bincode). Decision 0001 already requires "golden replays across partition interleavings, thread counts and restarts"; 0003 §7 names the S2/TigerBeetle "run twice on one seed, diff the trace" test.

### Judgment

**(a) Limit.** Replay from offset 0 costs parse plus fold per event. At a combined 50k events/s (a 10 µs fold plus parse): 10^7 events in ~3 min, **10^8 in ~33 min, 10^9 in 5.5 h**. On Wikipedia that is a month's log in half an hour, on a 10^4/s topic a day's log in half an hour. Storage for the same log (135 GB/month on Wikipedia, §0) fills a laptop disk in months, so the disk is the earlier wall on the log itself.

**(b) Earliest symptom.** Cold start time and scrub latency (a scrub to offset *o* is a replay from the nearest earlier snapshot) grow linearly with log length.

**(c) Ladder.**
1. *Snapshots.* Every N events or on shutdown, write the consolidated world (the `BTreeMap<Tuple, i64>` relations, 0003 §4) plus the log position, the source cursors, and a hash of the fold's code version. Startup loads the latest snapshot and replays the tail. Prior art: Fowler's overnight snapshot; Flink checkpoints (state + positions); Kafka Streams changelog restore. Snapshots are derived, so they can be deleted at will; the log stays the record.
2. *Snapshot format:* `postcard` is the smallest dependency and enough for slice 1; `rkyv` gives a zero-copy load (mmap the file, no deserialize pass) when snapshot size makes load time visible. Both are on the allowlist's licence set.
3. *Scrub index:* keep every k-th snapshot (say hourly) so "scrub to any moment" is at most one hour of replay. This is a time-space trade the user sets; report both.
4. *Determinism at scale, sampled.* Full golden replay is a CI test on a fixture (§12). In production, a background thread periodically re-derives the world from the log (or from an older snapshot) and compares the consolidated multiset hash with the live world's hash at the same offset — the "meta test" of 0003 §7. A mismatch is a nondeterminism leak and stops the process from writing further snapshots (it can keep serving). Sampling snapshot boundaries rather than every event costs nothing per event.
5. *Log retention with cold segments:* compress and, if the user chooses, prune the log before the oldest kept snapshot (0003 §3's Parquet idea). This changes the promise "replay to any moment" into "replay to any kept snapshot and after", so it is a user-visible setting, default off.
6. *Incremental engine:* under DD a scrub is a query at a logical time over arrangements, not a replay (0003 §4).

**(d) Trigger.** Snapshot when the estimated replay time from the last snapshot exceeds 30 s, or every 10^6 events, whichever first; add the scrub index when a scrub takes longer than the 100 ms frame budget plus a second. Climb to rung 5 when log bytes exceed a user's retention setting.

## 4. Parallelism vs determinism

### Facts

Kafka: "Events with the same event key (e.g., a customer or vehicle ID) are written to the same partition, and Kafka guarantees that any consumer of a given topic-partition will always read that partition's events in exactly the same order as they were written" — https://kafka.apache.org/intro. Order is per partition only. Kafka Streams: "the maximum parallelism at which your application may run is bounded by the maximum number of stream tasks, which itself is determined by maximum number of partitions of the input topic(s)", each task "assigned a list of partitions" — https://github.com/apache/kafka/blob/trunk/docs/streams/architecture.md. Flink keys state by `keyBy`; "the value you get from the state depends on the key of the input element" — the Flink state page above. Research 0003 §7 fixes the merge as a pure function with the total order `(ts, partition, offset)` and a proptest that shuffles arrival order. Research 0003 §4: differential dataflow runs single-threaded on the calling thread via `execute_directly`, and multi-worker runs produce identical consolidated outputs by construction of its progress tracking (order within a batch is not documented; consolidate before comparing). DBSP requires identical circuits across workers; its multi-worker output-order guarantee is **UNVERIFIED** (0003 §4).

### Judgment

**(a) Limit.** One fold thread at 10 µs per event is 10^5/s; at 100 µs (a rule that touches a neighbourhood) it is 10^4/s. That sits inside the "typical Kafka topic" range, so this wall is real for the product and irrelevant for slice 1.

**(b) Earliest symptom.** The fold thread is at 100% while parse threads and the disk are idle; lag grows even though append throughput is fine. Distinguishable from wall 1 by which thread is busy — print per-stage time in the status line.

**(c) Ladder.**
1. *Pipeline, not partition.* Parse and System 1 feature extraction on a pool, fold on one thread in log order. No determinism cost at all. This alone buys the parse budget back and is enough to 10^4–10^5/s.
2. *Asynchronous System 1 engines as events.* Embeddings and decision-model verdicts run off-thread and are appended to the log as events with a position; the fold consumes them in log order, so a replay sees the same verdicts at the same positions. Latency-insensitive judgments (types, repairs) already work this way for System 2.
3. *Per-key monoid pre-folds.* Anything that is a commutative monoid per entity key (counts, last-seen by explicit tiebreak column, distinct sketches, profiles) can be folded per partition and merged deterministically, because 0003 §4 #6 already restricts aggregates to order-independent ones. Cross-entity relationships are the part that cannot: an edit that links a page to a user needs both keys' state.
4. *Keyed shards with deterministic exchange.* Shard the fold by entity key; a cross-entity event becomes two messages, one per key, delivered in a fixed round per log batch; each shard processes its inbox in `(position, key)` order. This is exactly what timely's progress tracking formalizes, so the honest version of this rung is:
5. *Differential dataflow with N workers.* The engine already chosen on trigger for possible worlds (0003 §4) also gives multi-worker parallelism with deterministic consolidated output. Since it costs the same integration either way, the two triggers (branch cost, fold throughput) should be tracked together and the engine adopted once for both.

Determinism invariants to keep on every rung: the log's position is the one total order (Kafka's partition order is respected by the merge rule, not by the fold); every threaded stage emits results keyed by position and the fold consumes in position order; golden replays run at thread counts 1 and N (decision 0001 already says so). Wall-clock never enters the fold.

**(d) Trigger.** Rung 1 when fold-thread utilisation exceeds 70% at the target rate; rung 3 when it exceeds 70% after rung 1; rung 5 when either that or the §5 trigger fires.

## 5. Possible worlds: forks × world size

### Facts

Research 0003 §4 #3: forking in v1 is "emitting +1 copies of the parent's consolidated state under the new id", and the DD-native shape is a branch as a timestamp coordinate that "shares every pre-fork arrangement and stores only diverging deltas". BranchBench (2026-04) measured "5–4000× slower reads as branches deepen" across Neon, DoltgreSQL, Tiger Data and Xata (0003 §4). Persistent structures in Rust, crates.io and GitHub 2026-09-27: `rpds` 1.2.1 (2026-05-15, **MIT**, 1,769★, pushed 2026-07-19 — https://crates.io/crates/rpds, https://github.com/orium/rpds): `HashTrieMap` insert/remove/get Θ(1) average, Θ(n) worst, **clone Θ(1)** — https://docs.rs/rpds/latest/rpds/map/hash_trie_map/struct.HashTrieMap.html; `RedBlackTreeMap` is its ordered map. `imbl` 7.0.2 (2026-09-09, **MPL-2.0+**, 192★, pushed 2026-09-10 — https://crates.io/crates/imbl, https://github.com/jneem/imbl) is the maintained fork of `im` (15.1.0, 2022-04-29, unmaintained — https://crates.io/crates/im); its ordered map: "Most operations on this type of map are O(log n)", clone O(1), "This is a copy-on-write operation, so that the parts of the map's structure which are shared with other maps will be safely copied before mutating" — https://docs.rs/imbl/latest/imbl/ordmap/struct.GenericOrdMap.html. Rollout size on the gate-4 question: 1,854 eligible enwiki edits in 30 min (research 0004), so a 30-minute horizon touches on the order of 2 × 10^3 entities per fork, whatever the world's size.

### Judgment

**(a) Limit.** With a full clone per fork, cost is forks × world bytes: a 10^6-tuple world at ~100 B per tuple is 100 MB, so 20 forks are 2 GB of copies and seconds of allocation — the 100 ms budget is gone by roughly **10^5 tuples × 20 forks**. That is the trigger 0003 already names, and on Wikipedia it arrives within the first hour of running.

**(b) Earliest symptom.** Fork latency and RSS spikes proportional to world size; frame drops in the possible-worlds view.

**(c) Ladder.**
1. *Full clone* (v1, correct, fine below ~10^4 tuples per world).
2. *Overlay worlds.* A fork is `(parent: WorldRef, delta: BTreeMap<Tuple, i64>)`; a read consults the delta then the parent; consolidation at the end of a rollout folds the delta. Cost per fork becomes **forks × events per horizon**, independent of world size — on the gate-4 question ~2 × 10^3 touched tuples per fork instead of 10^6. About fifty lines on top of the v1 fold, and the `world_id` column (0003 §4 #3) already gives every tuple its branch. This is "sample only the affected neighbourhood" done by construction: nothing outside the rollout's touch set is ever copied.
3. *Persistent maps for the state itself* when overlays nest (branches of branches) or reads through long parent chains show up in profiles. `rpds` first: MIT, already permitted by the cargo-deny licence set (0003 §8d); use `RedBlackTreeMap` inside the core, because the core forbids hash-order iteration and a HAMT iterates in hash order even with a fixed hasher. `imbl` is MPL-2.0, which the current `deny.toml` allow list does not include; adopting it is a licence decision first (MPL-2.0 is file-scoped copyleft, compatible with a permissive binary in practice, but it is a new licence text in the tree).
4. *Differential dataflow with the branch as a timestamp coordinate* (0003 §4): sharing of pre-fork arrangements is structural and reads do not degrade with branch depth, which is BranchBench's failure mode for database-style branching.

**(d) Trigger.** Unchanged from 0003: fork + first rollout step p99 > 100 ms at the envelope's forks × world size. Add the measurement to the fitness function so the trigger is watched rather than remembered.

## 6. Forecast ledger volume

### Facts

Gate 4 asks one question per eligible human enwiki edit: 1,854 in 30 min (research 0004) ≈ **1/s**. Each issuance is compared with matched baselines (base rate, simple-features model, Wikimedia's revert-risk model — README Evaluation), so each question yields several predictor rows. Outcomes arrive within the horizon or are censored at the deadline (contract). Kafka's `delete.retention.ms` default of 24 h is the analogous "how long a marker stays" knob (design.md above).

### Judgment

**(a) Limit.** At 1/s with ~300 B per issuance and four predictor rows: **~100 MB/day, 3 GB/month** — a nuisance, not a wall. Pending forecasts awaiting grading = rate × horizon = 1/s × 1,800 s = 1,800 rows, trivial if indexed by deadline and by entity. The wall appears when forecasts are issued per event on a fast topic: 10^4/s × 1 h horizon = 36 M pending and ~10 GB/day of ledger — larger than the event log itself.

**(b) Earliest symptom.** The ledger's bytes per day exceed the event log's; or grading time per outcome event grows, which means pending forecasts are being scanned rather than looked up.

**(c) Ladder.**
1. *Index pending by deadline and by entity id* (`BTreeMap<deadline, Vec<forecast_id>>`; the entity index is what grading an outcome event consults). O(log n) per event, and deadlines expire in order.
2. *Issue per registered question, not per event.* The contract already registers questions with a cutoff and horizon; a stream without a registered question gets a sampled calibration set (reservoir per predictor and horizon, e.g. 10^4 open forecasts) rather than one forecast per event.
3. *Derive, do not store, the trivial baselines* (the base rate is a number per window; store the window id, compute on grading), and keep probabilities as `f32`.
4. *Incremental scorecards.* Skill, coverage and calibration are computable from bins (10–20 per predictor and horizon) updated on grading, so the raw ledger can move to a separate SQLite file or cold Parquet without the scorecard reading it.
5. *Ledger retention* with the same discipline as the log: graded rows are immutable and may be archived; never pruned from the score (README: "pruning a branch from the view never removes it from the score").

**(d) Trigger.** Rung 2 when pending forecasts exceed 10^6 or ledger bytes/day exceed log bytes/day; rung 4 when a scorecard query takes longer than the frame budget.

## 7. System 2 cost as the world grows

### Facts

API prices on 2026-09-27 (https://claude.com/pricing): Sonnet 5 $2 / $10 per MTok in / out; Opus 5.5 $4 / $20; Haiku 4.5 $1 / $5; Fable 5.1 $10 / $50; batch processing 50% off; prompt-cache reads $0.20/MTok on Sonnet 5 (writes $2.50). Graphiti calls an LLM on every episode (research 0001, README Related work). Research 0002 §6 defines the profile System 2 needs: one column per JSONPath with counts, presence, type mix, distinct set or sketch, top values, growth curve, role scores, and identifier relations — a structure whose size is O(paths × event types), not O(entities). Schema-drift prior art: Debezium emits schema change events on DDL (0002 §4 cites its topic routing; the schema-change topic itself was not fetched — **UNVERIFIED**); JSONoid's per-path monoids merge, so drift is a comparison of two merged profiles (0002 §2).

### Judgment

**(a) Limit.** Arithmetic first. A snapshot for System 2 = the profile (~50 paths × ~100 tokens), a fixed sample of k entities per type with their recent events (20 × ~300 tokens), the current type map and open questions: **~15–25k input tokens, ~2k output**. On Sonnet 5 that is ~$0.06 per call; hourly, ~$1.50/day; with the profile prefix cached, the input side drops to ~$0.005. Per event instead (Wikipedia, 40/s) would be ~$200k/day, which is the arithmetic behind "the LLM never touches an event". Because the snapshot is O(paths) and k is fixed, **System 2 cost does not grow with the world**; it grows with schema width (a 500-path stream is ~$0.50 per call) and with how often it re-runs.

**(b) Earliest symptom.** Spend per day against the budget cap, and a rising share of System 2 calls whose proposals are identical to the previous call's (no drift, wasted call).

**(c) Ladder.**
1. *Fixed cadence with a daily budget cap* (already the design: "on a fixed budget").
2. *Drift-triggered re-runs.* Hash the discrete part of the profile — the set of paths, each path's role class, the alias classes, the identifier relations — and re-run only when the hash changes or a continuous drift score (presence-rate or growth-regime change beyond a threshold) fires, with a floor cadence (daily) so a silent stream still gets looked at. New paths appearing (a log subtype seen for the first time) are the common trigger on Wikipedia.
3. *Prompt caching of the stable prefix* (profile and type map first, sample last), which is a 10× cut on the input side at current prices; batch mode for non-urgent re-runs (50%).
4. *Cheaper model for the check, dearer for the proposal:* Haiku decides "anything new here?", Sonnet or better writes types, repairs and forecasters.
5. *MCP sampling through the user's own agent, or a local model* (README System 2 row): shifts cost to the user's allowance or to zero, at different latency and privacy.

**(d) Trigger.** Rung 2 when more than half of calls in a day produce no new proposal; rung 4 when spend exceeds the cap on two days in a week.

## 8. Structure discovery at scale

### Facts

Research 0002 §2 judged exact 64-bit hash sets right for gate 3 ("tens of megabytes" at 10^5–10^6 events) with HyperMinHash or Bloom "past a per-field cap, for the product's unbounded case", and warned that HLL inclusion–exclusion is a poor containment estimator (Ertl). `cardinality-estimator` 1.0.3 (Cloudflare, 2026-02-11, Apache-2.0): three representations — "8 bytes of stack memory and 0 bytes of heap memory" for 0–2 elements, an array up to ~4 KB, then HyperLogLog++ at ~4 KB, mean relative error 0.64% across tested ranges — https://github.com/cloudflare/cardinality-estimator, https://crates.io/crates/cardinality-estimator. A Bloom filter at 1% false positives needs about 9.6 bits per element (standard formula m = −n ln p / (ln 2)²; https://en.wikipedia.org/wiki/Bloom_filter, not fetched this session — **UNVERIFIED**). Entity resolution at scale: Splink's guide — "a dataset of 1 million input records would generate around 500 billion pairwise record comparisons" without blocking; "It's usually better to use a longer list of strict blocking rules, than a short list of loose blocking rules" — https://moj-analytical-services.github.io/splink/topic_guides/blocking/blocking_rules.html; Papadakis et al.'s survey organises blocking, filtering and hybrid techniques and notes semi-structured big data "pose challenges not only to the scalability of efficiency techniques, but also to their core assumptions" — https://arxiv.org/abs/1905.06167. Hub nodes: PowerGraph (OSDI 2012) — "natural graphs commonly found in the real-world have highly skewed power-law degree distributions, which challenge the assumptions made by these abstractions, limiting performance and scalability"; its vertex-cut placement "exploits the structure of power-law graphs" for "order of magnitude gains" — https://www.usenix.org/conference/osdi12/technical-sessions/presentation/gonzalez. Measured hubs on our stream: `commonswiki` alone carried 739 of 2,299 events in 62 s (0005 §0), and every page event references its wiki.

### Judgment

**(a) Limits.** *Sketches:* an exact set of 10^6 `u64` values is ~16–24 MB in a hash set; twenty identifier-like paths at that size are ~400 MB, and a linear-growth path (event ids) never stops. At ~4 KB per HLL++ the same twenty paths are 80 KB. *Identity resolution:* `s2w`'s alias and relationship discovery is per-event co-occurrence and set containment (0002 §4), which is O(events × identifier paths²) with ~20 paths — not pairwise record matching, so there is no O(n²) wall unless fuzzy matching is added. *Hubs:* an entity linked to everything has degree 10^5–10^7; any per-edge operation on it (recomputing an aggregate over its neighbours, drawing it, copying it into a fork) is O(degree) per event, and it is also the skew that breaks keyed sharding (§4 rung 4).

**(b) Earliest symptoms.** Profile memory growing linearly with events (the exact sets); one entity's per-event update cost dominating the fold p99; the explorer asking for a hub's ego-network and receiving 10^6 edges.

**(c) Ladder.**
1. *Tiered per-path representation, decided by the profiler:* exact set up to a cap (10^5–10^6 values), then HLL++ for counts and a Bloom filter for membership — the same small/array/HLL progression `cardinality-estimator` uses internally. The switch belongs **inside the profiler, per path**, not in the fold or in a global mode, because paths cross the cap at different times and the growth curve already says which will.
2. *Containment past the cap* by Bloom membership of the dependent side against the key side plus verification on a reservoir sample (MANY and Faida, 0002 §2), never by HLL differences.
3. *Identity resolution stays exact and structural* (co-occurrence, containment, carry-over chains). If fuzzy matching is ever added (normalized titles across wikis), block by `(type, normalized-key prefix)` and match only entities active in the retention window — Splink's rule: many strict blocks.
4. *Hub cap.* When a target's in-degree passes a cap (10^4), stop materializing edges to it: the relationship becomes an attribute on the source (`page.wiki = enwiki`) plus per-hub counters (degree, per-type counts, last-seen), and "edges of the hub" becomes a query by attribute. The hub keeps its entity id and shows in the explorer as an aggregate at any level of detail, which is what 0005's `lod=type` needs anyway. This is PowerGraph's insight without the cluster: split the hub's state from its edges.
5. *Under keyed sharding,* hub counters are per-shard monoids merged on read (the vertex-cut).

**(d) Trigger.** Rung 1 when any path's exact set passes the cap or profile RSS exceeds 10% of the world's; rung 4 when max in-degree passes 10^4 or a single entity's update cost exceeds 1 ms.

## 9. Visualization

Covered by research 0005 (three.js directly, level of detail as an API parameter, log offset as the time axis). Interactions with the above: (1) retention tombstones (§2) arrive at the view as `entity.remove` deltas, which 0005 §7 already defines; (2) `lod=type|cluster` aggregates must be maintained incrementally over the hot tier, not recomputed over all entities per frame — clusters are recomputed on a timer, counts on every event; (3) a scrub is a snapshot lookup plus tail replay (§3), so scrub latency is the snapshot spacing; (4) a possible world drawn as ghosts is an overlay's delta (§5 rung 2), which is exactly the `/diff` endpoint of 0005 §7 — the delta is the diff, computed once; (5) hubs (§8 rung 4) are never drawn with their edges.

## 10. Single binary vs distributed

### Facts

Tigani (MotherDuck, 2023): "the vast majority of customers had less than a terabyte of data in total data storage"; "90% of queries processed less than 100 MB of data"; "by the time data gets to be a week old, it is probably 20 times less likely to be queried than from the most recent day"; a standard cloud instance now offers "64 cores and 256 GB of RAM" — https://motherduck.com/blog/big-data-is-dead/. Kafka's own ceiling for a three-broker cluster is 10^6 1 KB messages/s (§0). Kafka partitions are the unit of parallelism and each key's events stay in one partition (§4). The hub: 8 cores, 30 GB, one NVMe.

### Judgment

**(a) Limit.** One laptop-class machine, with the rungs above climbed: batched append ~10^5/s, parse ~3 × 10^5/s per core, fold 10^4–10^5/s per thread, world memory ~10^7 entities in 3 GB at 300 B. The binding wall at 10^5/s is not compute; it is **130 MB/s of log, 11 TB/day** — a 1 TB disk fills in two hours. So at every rate where one machine's CPU is not enough, the disk was not enough first, and the user has already had to choose a retention window. A topic at 10^6/s is a whole Kafka cluster's benchmark throughput and not the target user.

**(b) Earliest symptom.** Disk fill projection under a day; then, after retention, fold saturation (§4).

**(c) Ladder.**
1. *Partition subsets:* `s2w watch kafka://…/orders --partitions 0,3` reads a deterministic key-sample, because Kafka fixes each key to a partition. Relationships across keys in other partitions are simply unseen, which the evidence view must say.
2. *Key sampling on any source:* `--sample 1/16 by <key>` hashes the entity key; same semantics for SSE and stdin. Structure discovery (per-path profiles, INDs) works on a sample; forecasts about sampled entities remain gradeable.
3. *Retention window* (§2, §3).
4. *Several independent `s2w` processes*, one per partition group, each with its own log and world and no shared state — a user choice, no code.
5. *A distributed world* (shared state across machines) is Flink-, Kafka-Streams- or Materialize-class engineering and out of scope for this project's promise ("small enough to drop into someone else's network"). Say so in the README, and name those systems as where to go.

**(d) Trigger.** Sustained source rate above 10^5/s or a required retention that exceeds local disk moves the user to rungs 1–4; rung 5 is a decision Dave would take as a new project, not a trigger.

## 11. Proposed slice-1 target envelope

Judgment; numbers chosen at 10× Wikipedia's measured load where a cheap rung gives 10× headroom, and at the measured ceiling where it does not.

| Dimension | Promise at launch | Why this number |
|---|---|---|
| Sustained ingest | **1,000 events/s** | The measured single-transaction SQLite ceiling on a consumer NVMe (871–1,204/s); 25× Wikipedia's rate. Group commit (#19) is the first rung past it and is not needed to keep the promise. |
| Bursts | 5,000/s for 60 s | Absorbed by a bounded in-memory queue ahead of the log; lag reported. |
| Event size | ≤ 8 MiB hard cap (existing), ≤ 4 KB typical | Wikipedia's max in the sample was 3.4 KB. |
| World | **10^6 entities, 3 × 10^6 relationships resident, ≤ 1 GB RSS** | ~10 hours of the full stream at 100k ids/hour, or ~4 days of enwiki; a 300 B/entity budget. |
| Log | **10^7 events (~13 GB), replay from 0 in ≤ 5 min** | Three days of the full stream; requires parse + fold ≥ 35k events/s, well inside the estimates. |
| Possible worlds | 20 forks × a 10^4-event horizon, fork + first rollout step p99 < 100 ms | 0003 §4's frame budget, made measurable; needs §5 rung 2 (overlays), which is small. |
| Ledger | 10 issuances/s, 10^5 pending, grading O(log n) | 10× gate 4's rate. |
| System 2 | ≤ $5/day at the default cadence on Sonnet-class pricing | §7 arithmetic with headroom. |
| Machine | 4 cores, 16 GB, NVMe SSD, Linux or macOS laptop | The user's, not ours. |

**Explicit non-goals for slice 1:** a parallel fold (one thread, by design); a distributed world; more than 10^8 retained events; unbounded retention (the default is no window, but "we keep everything forever" is not promised beyond the log size above); an LLM call per event; fuzzy entity matching; hub entities drawn with their edges; the segment-file log (#19 stays a trigger, not a plan).

## 12. A scale fitness function for CI

### Facts

Criterion's FAQ on CI: "The virtualization used by Cloud-CI providers like Travis-CI and Github Actions introduces a great deal of noise into the benchmarking process… You probably shouldn't (or, if you do, don't rely on the results)", recommending instruction-counting under Valgrind instead — https://bheisler.github.io/criterion.rs/book/faq.html (criterion 0.8.2, 2026-02-04 — https://crates.io/crates/criterion). `iai-callgrind` (0.16.1, 2025-07-30 — https://crates.io/crates/iai-callgrind) has continued as **`gungraun`** 0.20.0 (2026-09-26, Apache-2.0 OR MIT — https://crates.io/crates/gungraun): Callgrind, Cachegrind, DHAT and Linux perf; "take accurate measurements with Valgrind even in virtualized CI environments and make them comparable between different systems completely negating the noise of the environment"; Linux only; "Valgrind cycles estimation is primarily designed to be a relative metric" — https://github.com/gungraun/gungraun. Its README says it detects regressions; the exact CLI/attribute for failing a run on a threshold was not confirmed from the fetched page — **UNVERIFIED**, check the docs before wiring it. `divan` 0.1.21 (2025-04-10) is a wall-clock alternative to criterion — https://crates.io/crates/divan. The workspace already has `xtask` as its fitness-function home (decision 0001), and CI runs `cargo test --workspace`.

### Judgment

**Fixture.** A recorded 10-minute Wikipedia `recentchange` sample (~24k events, ~32 MB raw, a few MB zstd-compressed — the compression ratio is an estimate, **UNVERIFIED**) committed under `s2w-testkit` (or fetched by content hash if the repo size matters), plus a seeded synthetic generator for 10^6-event local runs. The recorded sample is also the golden-replay input.

**Three gated numbers, all deterministic, all in `cargo xtask check` (or a `cargo test` that reads a committed baseline):**

1. **Instructions per event** for parse + fold over the fixture, measured with gungraun (Callgrind), compared with a committed baseline; fail on > +5%. Instruction counts are the only stable per-PR metric on a shared runner; wall-clock is reported, never gated.
2. **Bytes per entity and per relationship** after folding the fixture, measured with a counting global allocator in the test binary (or DHAT under gungraun); fail above the envelope's 300 B/entity budget, and on > +5% against the baseline.
3. **Fork cost:** instructions for fork + one rollout step at the fixture's world size × 20 forks; fail on > +5%.

**Two more, informational until a rung is climbed:** replay events/s on the fixture (wall clock, reported); append events/s at transaction sizes 1 and 100 on the runner's real disk — and the harness must assert the temp directory is not tmpfs (`statfs` magic ≠ `TMPFS_MAGIC`) before trusting a number, because the tmpfs figure looks like a 40× success.

**Baselines live in one file** (`xtask/scale-baseline.toml`: instructions, bytes, fork instructions, with the commit that set them), human-owned like the golden replays: a PR that regresses updates the baseline explicitly in the same PR and says why, the way `CLAUDE.md` treats golden files. A gate that only ratchets down is the mistake decision 0001's amendment already made once; a baseline with a stated tolerance is the fix.

## Open items

- Measure, do not assume: bytes per entity in the real fold (the 300 B figure is arithmetic); fold µs per event on the fixture; the distinct-count growth curve over 24 hours of the full stream (Heaps exponent decides §2's urgency); batched SQLite append from Rust rather than Python (the 1.6 × 10^5/s figure is Python-bound).
- Verify before wiring: gungraun's regression-limit mechanism; the Debezium schema-change topic as drift prior art; the Bloom sizing citation.
- Decide before slice 2: whether `synchronous=NORMAL` per replayable source is acceptable (it is a durability trade, so Dave's call); whether `imbl`'s MPL-2.0 may enter `deny.toml` if `rpds` proves too slow.
- Watch: `rpds` (one maintainer, MIT); `gungraun`'s rename settling; DD's cadence (0003 §4).

## Sources

Measurements by the author on the hub, 2026-09-27: SQLite append at transaction sizes 1/10/100/1,000 and `synchronous=NORMAL`, paged replay, raw `fdatasync` latency (ext4 on NVMe, after discarding the tmpfs run); 30 s of the live `recentchange` stream. Fetched pages: SQLite FAQ; PostgreSQL WAL configuration; Kafka intro, `docs/design/design.md` and `docs/streams/architecture.md` in `apache/kafka` (the site's HTML is JS-assembled and returned only navigation); Confluent and LinkedIn Kafka benchmarks; Flink state, state backends and checkpointing pages; Fowler's Event Sourcing; enwiki `Special:Statistics`; PowerGraph's USENIX abstract page; Papadakis et al. on arXiv; Splink's blocking guide; cardinality-estimator, gungraun, rpds and imbl READMEs and docs.rs pages; criterion's FAQ; MotherDuck's "Big Data is Dead"; claude.com/pricing; crates.io API for im, imbl, rpds, criterion, iai-callgrind, gungraun, divan, cardinality-estimator, fastbloom, rkyv, postcard, bincode; GitHub API for imbl, rpds, and the `apache/kafka` docs tree. Prior research notes 0002, 0003, 0004, 0005 and decisions 0001, 0002 as cited inline.

## Design implications

1. **Slice-1 scale envelope** (§11) becomes a decision record, with the non-goals listed, and the README's Technical architecture table gains a row for it. → adopted: docs/decisions/0004-scale-envelope.md (Dave approved, 2026-09-27, #37); README Technical architecture Scale row
2. **Status line from the first slice**: source lag (per partition), append rate, fold-thread utilisation, entities and bytes per entity, log bytes and days-to-disk-full. Every trigger in this note reads one of these; without them the walls are discovered by a crash. → deferred: stream2worlds#32
3. **#19 reframed**: group commit (a batch append API, N events or T ms, cursors in the same transaction) is the first rung and stays SQLite; segment files are the sixth. `synchronous=NORMAL` is a per-source property gated on replayability, decided by Dave. → adopted: #19 reframed, group commit moved into #7 (2026-09-27); synchronous=NORMAL → stream2worlds#31
4. **Retention on log time** (`--retain`): tombstones as fold-emitted events driven by event time or offset, never wall clock; then tiered cold state in the shell. Feeds #9 and the `s2w-model` entity-id invariant (ids never reused, so ledger references survive aging). → deferred: stream2worlds#33
5. **Snapshots** (state + log position + cursors + fold version hash; `postcard` first, `rkyv` when load time shows), a scrub index, and a background sampled re-derivation that halts snapshot writing on a hash mismatch. Feeds #9. → deferred: stream2worlds#33
6. **Overlay worlds** (parent + delta) as the v1 fork, replacing the +1-copy fork in 0003 §4 #3; `rpds` as the persistent-map step if needed; the DD trigger now measured by the fitness function rather than remembered. → deferred: gate-4 epic stream2worlds#14
7. **Hub cap** in the fold: past an in-degree of 10^4 a relationship to that entity is an attribute plus counters, and the entity is served as an aggregate at every `lod`. Feeds #9 and #10. → deferred: stream2worlds#10, #36 (the cap itself, a relationship past the in-degree cap becoming a source attribute plus counters, is built: docs/decisions/0005-pure-fold.md, 2026-09-27; serving the hub as an aggregate at every `lod` landed in docs/decisions/0006-world-query-api.md, #36, 2026-09-27)
8. **Profiler tiering** per path (exact set → HLL++ + Bloom past a cap) inside H, with containment past the cap by Bloom membership plus sampled verification. → decided (in scope for H): [decision 0010](../docs/decisions/0010-gate3-h-arm.md) (2026-09-27); H-min build and measurement, where this tiering is actually implemented: stream2worlds#56
9. **System 2 re-runs are drift-triggered** (profile hash + drift score + floor cadence + budget cap), with the stable prefix ordered first for prompt caching. Feeds the gate-3 System 2 design. → deferred: gate-3 epic stream2worlds#13
10. **Forecast ledger**: indexed by deadline and entity, issuance per registered question with a reservoir for calibration, incremental scorecard bins. Feeds gate 4 (#14). → deferred: gate-4 epic stream2worlds#14
11. **Distribution is a stated non-goal**; the README's Kafka section gains `--partitions` and `--sample 1/N by key` as the scale levers and names Flink / Kafka Streams / Materialize for shared-state scale-out. → adopted: README Planned interface (`--partitions`, `--sample 1/N`) and docs/decisions/0004-scale-envelope.md (2026-09-27); naming Flink / Kafka Streams / Materialize waits for the launch README
12. **Scale fitness function** (§12): fixture + gungraun instruction counts, allocator-counted bytes per entity, fork cost, a human-owned baseline file with a 5% tolerance, and a not-tmpfs assertion for any storage number. Feeds `xtask`. → deferred: stream2worlds#32
