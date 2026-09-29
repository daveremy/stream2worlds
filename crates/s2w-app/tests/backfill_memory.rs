//! Resident memory of a full backfill, split into world state, timeline history and transient
//! peaks (s2w#216). Ignored: it folds 1.5x10^5 raw events several times. Run with
//! `cargo test --release -p s2w-app --test backfill_memory -- --ignored --nocapture`.
//!
//! The demo box was OOM-killed at `MemoryMax=1G` about 6 s into a backfill of 151,331 logged
//! events (476,795 world events). s2w#208 measured only the folded world (`discover_volume.rs`).
//! This test measures what a serve process holds on the same load, each part in a fresh child
//! process so no measurement reuses pages another freed:
//!
//! - `world`: the raw events through the discovered mapping, folded into one `World`.
//! - `timeline`: the same claims appended to a `QueryState` one at a time, as the bridge does.
//!   Its resident delta minus `world` is the history the timeline keeps.
//! - `bridge`: the real `Bridge` over a durable SQLite log holding the raw events, with the
//!   mapping engine routed, polled to the end of the log. Its peak minus `timeline` is the
//!   backfill's transient (payload batches, verdict rows, claims in flight). It asserts the
//!   process's peak resident stays under [`SERVE_PEAK_LIMIT`] (600 MiB): the fitness function
//!   for decision 0026's history cap, with no viewer connected.
//! - `queries`: after the `timeline` fold, a `/world` read at the head split into its parts,
//!   each its own peak window: the projection alone (`view_at`, which borrows the head) and the
//!   JSON serialization alone; then the streamed `/world` through the real router (#216 PR 2b,
//!   no owned view and no whole body), and one `/diff` from the head to itself. Before #216
//!   PR 2a each read also copied the head world, the size `world` reports. (Not measured here:
//!   a freed copy's pages stay resident and pad whichever window comes next.) The streamed read
//!   also prints the server's split of its read-guard hold (below). Last, `World::clone` of the
//!   head and each of its collections alone (entities, keys, merges, relationships, hub
//!   counters), timed, each copy kept so none reuses another's pages (s2w#243). Resident
//!   deltas can still read low; `S2W_BACKFILL_MEMORY_VARIANTS=queries` on the
//!   `backfill_memory_heap` target adds each copy's exact heap bytes.
//! - `viewer`: the `bridge` backfill with a reader thread attached before the first poll,
//!   issuing one `/world` and one `/diff?from=<head>&to=<head>` through the real router every
//!   tick and draining each body, as a page does. The `/world` carries the last `ETag` in
//!   `If-None-Match`, so an unchanged head answers 304 before any projection, as the page's
//!   refresh does. The tick is [`VIEWER_TICK_MS`] milliseconds: 5000 by default (the page's
//!   `WORLD_REFRESH_MS`), 1000 for the page's old rate, a worst case. It asserts the whole-process peak stays under [`VIEWER_PEAK_LIMIT`], and
//!   reports the slowest `/world` (an upper bound on how long one read held the fold's lock:
//!   the guard is held until the last chunk is queued, and the reader drains as it goes) and
//!   the slowest `poll_once`, which is where the fold waits for that lock. The server side
//!   splits each `/world` body's hold (`QueryState::with_read_timings`, s2w#243): `wait` for
//!   the guard, `build` under it (`HeadView::new`: `Graph::new` and the sorts), `write` with it
//!   still held (serialization, which still reads each node out of the world). `build share`
//!   is `build / (build + write)`: the part of the hold a handoff that releases the guard after
//!   the projection keeps.
//!
//! `S2W_BACKFILL_MEMORY_VARIANTS=bridge,viewer` runs only the named variants;
//! `S2W_BACKFILL_MEMORY_VIEWER_TICK_MS=1000` sets the `viewer` tick;
//! `S2W_BACKFILL_MEMORY_VIEWERS=4` runs four readers, reader `i` starting `i * tick / 4` after
//! the first, each with its own `ETag`. More than one reader reports and does not assert.
//!
//! Measurement variants (s2w#220, where the bridge's ~170 MiB over the head world goes). They
//! run only when named in `S2W_BACKFILL_MEMORY_VARIANTS`, and never assert the 600 MiB limit:
//!
//! - `bridge-run`: the `bridge` child with every `poll_once` on a tokio blocking-pool thread, as
//!   `Bridge::run` does in serve (a per-thread glibc arena, not the main one).
//! - `<base>+<allocator>`, base `bridge` or `bridge-run`: the parent sets one glibc tuning on
//!   that child only, never on itself (it populates the log). [`ALLOCATORS`] lists them;
//!   `arena2` only means something under `bridge-run` (the main-thread child has one arena).
//! - `S2W_BACKFILL_MEMORY_HISTORY_CAP=<n>` and `S2W_BACKFILL_MEMORY_BATCH=<n>`, read by the
//!   bridge child: the timeline's history cap and `BridgeConfig::batch`.
//! - The default sweep (no `VARIANTS`) refuses to run with any knob (`VIEWERS` too) or any
//!   allocator variable set (any `MALLOC_*`, `GLIBC_TUNABLES`, `LD_PRELOAD`), so a green
//!   default run always asserted. A named `bridge` or `viewer` child under an allocator variable reports
//!   and does not assert.
//! - The `backfill_memory_heap` target includes this file with dhat as the global allocator
//!   and runs `bridge` by default, printing dhat's `max_bytes` (Rust heap peak) beside `VmHWM`;
//!   a named `queries` child there prints each clone's exact heap bytes (s2w#243).
//!   Compare its `max_bytes` with this target's `bridge` `VmHWM`: the gap is non-heap resident
//!   memory (SQLite's C heap, stacks, the binary) plus allocator overhead and fragmentation.
//!   (dhat's own bookkeeping makes that target's `VmHWM` meaningless.)
//! - The `backfill_memory_mimalloc` target includes this file with mimalloc as the global
//!   allocator: the same variants with every Rust allocation off glibc malloc (SQLite's C heap
//!   stays on it). Its children never assert, and it refuses `+<allocator>` glibc tunings.
//!
//! Every child prints one `result {json}` line for aggregation. Run one variant per invocation:
//! the bridge child stores verdicts in the shared log directory, so a second bridge-type child
//! in the same invocation replays them and measures a different workload. The sweep:
//! `S2W_BACKFILL_MEMORY_VARIANTS=bridge-run+arena2 timeout -s KILL 600 cargo test --release -p
//! s2w-app --test backfill_memory -- --ignored --nocapture 2>&1 | grep '^result'`.
//!
//! The fixture (11,667 events) is cycled to 1.5x10^5 with every string leaf suffixed by the
//! cycle number (`fresh`, as in `discover_volume.rs`), so every cycle observes new entities:
//! the upper bound, and the only variant the log accepts (it dedupes identical payloads).

#[path = "support/recorded.rs"]
#[expect(
    dead_code,
    reason = "this test uses the loader, not the committed mapping"
)]
mod recorded;

// `allow-unwrap-in-tests` applies inside `#[cfg(test)]` items only.
#[cfg(test)]
mod backfill {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use axum::body::Body;
    use axum::http::header::{ETAG, IF_NONE_MATCH};
    use axum::http::{HeaderValue, Request, StatusCode};
    use tokio_stream::StreamExt;
    use tower::ServiceExt;

    use s2w_app::bridge::{Bridge, BridgeConfig, EngineRegistry, Route};
    use s2w_app::discover::DISCOVER_WINDOW;
    use s2w_app::query::{
        DEFAULT_HISTORY_CAP, QueryState, ReadTimingsSnapshot, Timeline, ViewParams, router,
    };
    use s2w_core::{World, fold};
    use s2w_discover::{Config, Discovery, discover};
    use s2w_log::{EventLog, SqliteEventLog, SqliteVerdictStore};
    use s2w_model::{RawEvent, StreamMapping, Timestamp};
    use s2w_system1::{Engine, MappingEngine, Verdict};

    use super::recorded::load;

    const EVENTS: usize = 150_000;
    const CAP: u64 = s2w_app::DEFAULT_HUB_IN_DEGREE_CAP;
    const VARIANT: &str = "S2W_BACKFILL_MEMORY_VARIANT";
    const VARIANTS: &str = "S2W_BACKFILL_MEMORY_VARIANTS";
    const LOG_DIR: &str = "S2W_BACKFILL_MEMORY_LOG";
    const MAPPING_FILE: &str = "mapping.json";
    const SOURCE_FILE: &str = "source";
    /// The most a serve process may hold at its peak during this backfill, whole process,
    /// no viewer connected (decision 0026): room under the demo box's `MemoryMax=1G` for the
    /// viewer path and the allocator.
    const SERVE_PEAK_LIMIT: usize = 600 * 1024 * 1024;
    /// The demo box's `MemoryMax`: the most a serve process may hold at its peak with a viewer
    /// connected (s2w#216's finish line).
    const VIEWER_PEAK_LIMIT: usize = 1024 * 1024 * 1024;
    /// How often, in milliseconds, the `viewer` reader asks for the world. Unset: 5000, the
    /// page's `WORLD_REFRESH_MS` (the real client); 1000 is the page's old rate, a worst case.
    const VIEWER_TICK_MS: &str = "S2W_BACKFILL_MEMORY_VIEWER_TICK_MS";
    const VIEWER_TICK_DEFAULT: Duration = Duration::from_millis(5000);

    /// The `viewer` tick from [`VIEWER_TICK_MS`].
    fn viewer_tick() -> Duration {
        std::env::var(VIEWER_TICK_MS).map_or(VIEWER_TICK_DEFAULT, |ms| {
            Duration::from_millis(ms.parse().expect("S2W_BACKFILL_MEMORY_VIEWER_TICK_MS: ms"))
        })
    }
    /// The variants the default sweep (no `VARIANTS`) runs.
    const DEFAULT_VARIANTS: [&str; 5] = ["world", "timeline", "queries", "bridge", "viewer"];
    /// The bridge child's timeline history cap, when set (s2w#220).
    const HISTORY_CAP: &str = "S2W_BACKFILL_MEMORY_HISTORY_CAP";
    /// The bridge child's `BridgeConfig::batch`, when set (s2w#220).
    const BATCH: &str = "S2W_BACKFILL_MEMORY_BATCH";
    /// How many `viewer` readers the `viewer` child runs, when set (s2w#243); unset, one.
    const VIEWERS: &str = "S2W_BACKFILL_MEMORY_VIEWERS";
    /// The glibc tunings a `<base>+<allocator>` variant sets on its child (s2w#220).
    const ALLOCATORS: [(&str, &[(&str, &str)]); 4] = [
        ("arena2", &[("MALLOC_ARENA_MAX", "2")]),
        // A fixed threshold: every buffer of 64 KiB or more is mmapped and unmapped on free.
        ("mmap64k", &[("MALLOC_MMAP_THRESHOLD_", "65536")]),
        (
            "trim",
            &[
                ("MALLOC_TRIM_THRESHOLD_", "131072"),
                ("MALLOC_TOP_PAD_", "0"),
            ],
        ),
        (
            // Replaces any GLIBC_TUNABLES the caller set.
            "notcache",
            &[("GLIBC_TUNABLES", "glibc.malloc.tcache_count=0")],
        ),
    ];

    /// A `/proc/self/status` field in bytes (`VmRSS`: resident now, `VmHWM`: peak resident).
    fn status(field: &str) -> usize {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let line = status.lines().find(|l| l.starts_with(field)).unwrap();
        let kib: usize = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        kib * 1024
    }

    /// Starts a new peak window: resets `VmHWM` to the current RSS (Linux 4.0+).
    fn reset_peak() -> usize {
        std::fs::write("/proc/self/clear_refs", "5").unwrap();
        status("VmRSS:")
    }

    /// A measurement knob: `None` when unset; a value that is not a count panics.
    fn knob(name: &str) -> Option<usize> {
        let value = std::env::var(name).ok()?;
        Some(
            value
                .parse()
                .unwrap_or_else(|_| panic!("{name}={value} is not a count")),
        )
    }

    /// True in the `backfill_memory_heap` target, which includes this file with dhat as the
    /// global allocator.
    fn heap_target() -> bool {
        env!("CARGO_CRATE_NAME") == "backfill_memory_heap"
    }

    /// True in the `backfill_memory_mimalloc` target, which includes this file with mimalloc as
    /// the global allocator.
    fn mimalloc_target() -> bool {
        env!("CARGO_CRATE_NAME") == "backfill_memory_mimalloc"
    }

    fn mib(bytes: usize) -> String {
        #[expect(clippy::cast_precision_loss, reason = "display only")]
        let mib = bytes as f64 / 1_048_576.0;
        format!("{mib:.1} MiB")
    }

    fn mapping(events: &[RawEvent]) -> StreamMapping {
        let payloads: Vec<&[u8]> = events[..DISCOVER_WINDOW]
            .iter()
            .map(|e| e.payload.as_slice())
            .collect();
        match discover(&payloads, &Config::default()).1 {
            Discovery::Mapping(mapping) => mapping,
            Discovery::Abstain(reason) => panic!("the profiler abstained: {reason}"),
        }
    }

    /// Every string leaf suffixed with `~cycle`, inside the envelope's `data` text as well.
    fn fresh(payload: &[u8], cycle: usize) -> Vec<u8> {
        fn walk(value: serde_json::Value, cycle: usize) -> serde_json::Value {
            match value {
                serde_json::Value::Object(fields) => fields
                    .into_iter()
                    .map(|(k, v)| (k, walk(v, cycle)))
                    .collect(),
                serde_json::Value::Array(items) => {
                    items.into_iter().map(|v| walk(v, cycle)).collect()
                }
                serde_json::Value::String(text) => {
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(
                            inner @ (serde_json::Value::Object(_) | serde_json::Value::Array(_)),
                        ) => serde_json::Value::String(walk(inner, cycle).to_string()),
                        _ => serde_json::Value::String(format!("{text}~{cycle}")),
                    }
                }
                other => other,
            }
        }
        let value: serde_json::Value = serde_json::from_slice(payload).unwrap();
        serde_json::to_vec(&walk(value, cycle)).unwrap()
    }

    /// Raw event `i` of the cycled stream: fresh strings, a distinct cursor, and a receive time
    /// that keeps rising across cycles.
    fn cycled(events: &[RawEvent], i: usize) -> RawEvent {
        let cycle = i / events.len();
        let mut event = events[i % events.len()].clone();
        if cycle > 0 {
            event.payload = fresh(&event.payload, cycle);
            let mut cursor = event.cursor.as_bytes().to_vec();
            cursor.extend_from_slice(format!("~{cycle}").as_bytes());
            event.cursor = s2w_model::Cursor::new(cursor).unwrap();
            let span = events[events.len() - 1].received_at.as_millis()
                - events[0].received_at.as_millis()
                + 1;
            let shift = span * i64::try_from(cycle).unwrap();
            event.received_at = Timestamp::from_millis(event.received_at.as_millis() + shift);
        }
        event
    }

    fn report(variant: &str, before: usize, started: Instant, extra: &str) {
        let resident = status("VmRSS:").saturating_sub(before);
        let peak = status("VmHWM:").saturating_sub(before);
        eprintln!(
            "{variant}: resident {}, peak {}, {:.1} s{extra}",
            mib(resident),
            mib(peak),
            started.elapsed().as_secs_f64()
        );
    }

    /// The claims of every cycled event under `engine`, one event at a time, to `sink`.
    fn claims(
        events: &[RawEvent],
        engine: &MappingEngine,
        mut sink: impl FnMut(Timestamp, Vec<s2w_core::WorldEvent>),
    ) {
        for i in 0..EVENTS {
            let event = cycled(events, i);
            if let Verdict::Propose { claims, .. } = engine.evaluate(&event) {
                sink(event.received_at, claims);
            }
        }
    }

    fn world(events: &[RawEvent], engine: &MappingEngine) {
        let before = reset_peak();
        let started = Instant::now();
        let mut world = World::with_hub_cap(CAP);
        let mut batch = Vec::new();
        let mut encoded = 0_usize;
        claims(events, engine, |_, claims| {
            encoded += claims
                .iter()
                .map(|c| postcard::to_allocvec(c).unwrap().len())
                .sum::<usize>();
            batch.extend(claims);
            if batch.len() >= 20_000 {
                world = fold(std::mem::take(&mut world), &batch);
                batch.clear();
            }
        });
        world = fold(world, &batch);
        drop(batch);
        report(
            "world",
            before,
            started,
            &format!(
                ", {} world events, {} entities",
                world.offset(),
                world.entity_count()
            ),
        );
        let t = Instant::now();
        let bytes = postcard::to_allocvec(&world).unwrap();
        let encode_ms = t.elapsed().as_millis();
        let t = Instant::now();
        let back: World = postcard::from_bytes(&bytes).unwrap();
        let decode_ms = t.elapsed().as_millis();
        drop(back);
        eprintln!(
            "encoded (postcard): world {} ({encode_ms} ms encode, {decode_ms} ms decode), world events {}",
            mib(bytes.len()),
            mib(encoded)
        );
    }

    fn timeline(events: &[RawEvent], engine: &MappingEngine, queries: bool) {
        let before = reset_peak();
        let started = Instant::now();
        let mut state = QueryState::new(Timeline::new(CAP));
        if queries {
            state = state.with_read_timings();
        }
        claims(events, engine, |at, claims| {
            for claim in claims {
                state.append(at, claim).unwrap();
            }
        });
        let (_, head, _) = state.bounds().unwrap();
        report(
            "timeline",
            before,
            started,
            &format!(", {head} world events"),
        );
        if queries {
            let before = reset_peak();
            let started = Instant::now();
            let view = state
                .view_at(Some(head), None, &ViewParams::default())
                .unwrap();
            report(
                "query /world part: projection",
                before,
                started,
                &format!(", {} nodes, {} links", view.nodes.len(), view.links.len()),
            );
            let before = reset_peak();
            let started = Instant::now();
            let json = serde_json::to_vec(&view).unwrap();
            report(
                "query /world part: serialization",
                before,
                started,
                &format!(", body {}", mib(json.len())),
            );
            drop((json, view));
            let before = reset_peak();
            let started = Instant::now();
            let len = drain_world(&state);
            report(
                "query /world streamed",
                before,
                started,
                &format!(", body {}", mib(len)),
            );
            report_timings(&state.read_timings().unwrap());
            let before = reset_peak();
            let started = Instant::now();
            // Past the history cap, `/diff` serves the head only (decision 0026).
            let diff = state.diff(head, Some(head), None).unwrap();
            drop(diff);
            report("query /diff head..head", before, started, "");
            let timings = state.read_timings();
            state
                .with_head(|world, _| clone_breakdown(world, timings.as_ref()))
                .unwrap();
        }
    }

    /// Times `World::clone` of the head, then each of its collections alone (s2w#243: what a
    /// snapshot that copies the maps would cost). Every copy is kept until the end, so no window
    /// reuses another's freed pages; pages freed by the windows before this one can still be
    /// reused, so a resident delta can read low. The heap target's `heap` figure (dhat's live
    /// bytes) is the exact size. `timings` is the streamed read's hold split, for the `result`.
    fn clone_breakdown(world: &World, timings: Option<&ReadTimingsSnapshot>) {
        let mut rows = Vec::new();
        let whole = timed_clone(&mut rows, "world", || world.clone());
        let entities = timed_clone(&mut rows, "entities", || {
            world.entities().map(|(_, s)| s.clone()).collect::<Vec<_>>()
        });
        let keys = timed_clone(&mut rows, "keys", || world.keys().clone());
        let merges = timed_clone(&mut rows, "merges", || world.merges().clone());
        let relationships =
            timed_clone(&mut rows, "relationships", || world.relationships().clone());
        let hubs = timed_clone(&mut rows, "hub_counters", || world.hub_counters().clone());
        eprintln!(
            "result {}",
            serde_json::json!({
                "target": env!("CARGO_CRATE_NAME"),
                "variant": "queries",
                "entities": world.entity_count(),
                "relationships": world.relationships().len(),
                "keys": world.keys().len(),
                "read_timings": timings.map(timings_json),
                "clone": rows,
            })
        );
        drop((whole, entities, keys, merges, relationships, hubs));
    }

    /// Runs `clone` in its own peak window, prints and records its time and size, and returns
    /// the copy so the caller keeps it alive.
    fn timed_clone<T>(
        rows: &mut Vec<serde_json::Value>,
        part: &str,
        clone: impl FnOnce() -> T,
    ) -> T {
        let heap = || heap_target().then(|| dhat::HeapStats::get().curr_bytes);
        let (before, heap_before) = (reset_peak(), heap());
        let started = Instant::now();
        let copy = clone();
        let elapsed = started.elapsed();
        let resident = status("VmRSS:").saturating_sub(before);
        let heap_bytes = heap()
            .zip(heap_before)
            .map(|(after, b)| after.saturating_sub(b));
        eprintln!(
            "clone {part}: {} ms, resident {}{}",
            elapsed.as_millis(),
            mib(resident),
            heap_bytes.map_or_else(String::new, |b| format!(", heap {}", mib(b)))
        );
        rows.push(serde_json::json!({
            "part": part,
            "ms": ms(elapsed),
            "resident_bytes": resident,
            "heap_bytes": heap_bytes,
        }));
        copy
    }

    /// One `/world` through the real router, its body drained chunk by chunk and counted, never
    /// held whole.
    fn drain_world(state: &QueryState) -> usize {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let res = router(state.clone())
                .oneshot(
                    Request::get("/worlds/default/world")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "/world");
            drained_len(res.into_body()).await
        })
    }

    /// A response body's length, read chunk by chunk as a streaming client reads: the reader
    /// never holds the whole body, so the peak is the server's.
    async fn drained_len(body: Body) -> usize {
        let mut body = body.into_data_stream();
        let mut len = 0;
        while let Some(chunk) = body.next().await {
            len += chunk.unwrap().len();
        }
        len
    }

    fn populate(events: &[RawEvent], directory: &PathBuf) {
        let mut log = SqliteEventLog::open(directory).unwrap();
        let mut batch = Vec::with_capacity(1000);
        for i in 0..EVENTS {
            batch.push(cycled(events, i));
            if batch.len() == 1000 {
                log.append_batch(std::mem::take(&mut batch)).unwrap();
            }
        }
        log.append_batch(batch).unwrap();
    }

    /// What one `viewer` reader saw.
    #[derive(Default)]
    struct Viewed {
        worlds: usize,
        /// `/world` reads answered 304: the head had not moved since the last one.
        unchanged: usize,
        diffs: usize,
        refused: usize,
        largest_body: usize,
        slowest_world: Duration,
        /// The tick the reader ran at.
        tick: Duration,
    }

    impl Viewed {
        fn json(&self) -> serde_json::Value {
            serde_json::json!({
                "worlds": self.worlds,
                "unchanged": self.unchanged,
                "diffs": self.diffs,
                "refused": self.refused,
                "largest_body_bytes": self.largest_body,
                "slowest_world_ms": ms(self.slowest_world),
            })
        }
    }

    /// `count` [`viewer`] readers on their own threads, reader `i` starting `i * tick / count`
    /// after the first so their reads interleave the way independent pages would.
    fn spawn_viewers(
        state: &QueryState,
        stop: &Arc<AtomicBool>,
        count: usize,
        tick: Duration,
    ) -> Vec<JoinHandle<Viewed>> {
        (0..count)
            .map(|reader| {
                let (state, stop) = (state.clone(), Arc::clone(stop));
                let offset = tick / u32::try_from(count).unwrap() * u32::try_from(reader).unwrap();
                std::thread::spawn(move || {
                    let started = Instant::now();
                    while started.elapsed() < offset && !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    viewer(&state, &stop, tick)
                })
            })
            .collect()
    }

    /// Milliseconds with the fraction kept, for a `result` field.
    fn ms(duration: Duration) -> f64 {
        duration.as_secs_f64() * 1000.0
    }

    /// The server-side `/world` hold split as a `result` field, in milliseconds.
    fn timings_json(timings: &ReadTimingsSnapshot) -> serde_json::Value {
        serde_json::json!({
            "bodies": timings.bodies,
            "wait_ms_sum": ms(timings.wait.total),
            "wait_ms_max": ms(timings.wait.max),
            "build_ms_sum": ms(timings.build.total),
            "build_ms_max": ms(timings.build.max),
            "write_ms_sum": ms(timings.write.total),
            "write_ms_max": ms(timings.write.max),
            "build_share": timings.build_share(),
            "build_share_of_max": timings.build_share_of_max(),
        })
    }

    /// Prints the server-side split of the `/world` hold (s2w#243): `build` is what a handoff
    /// that releases the guard after the projection would keep under it.
    fn report_timings(timings: &ReadTimingsSnapshot) {
        let share = |s: Option<f64>| s.map_or_else(|| "-".to_owned(), |s| format!("{s:.2}"));
        eprintln!(
            "/world hold over {} bodies: wait total {} ms (max {}), build total {} ms (max {}), \
             write total {} ms (max {}); build share {} (of maxima {})",
            timings.bodies,
            timings.wait.total.as_millis(),
            timings.wait.max.as_millis(),
            timings.build.total.as_millis(),
            timings.build.max.as_millis(),
            timings.write.total.as_millis(),
            timings.write.max.as_millis(),
            share(timings.build_share()),
            share(timings.build_share_of_max()),
        );
    }

    /// A page's reads until `stop`: every `tick`, one `/world` (with the last `ETag` in
    /// `If-None-Match`, so an unmoved head is a 304) and one `/diff` of the head with itself
    /// through the real router, each body drained so the JSON is counted.
    fn viewer(state: &QueryState, stop: &AtomicBool, tick_every: Duration) -> Viewed {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let app = router(state.clone());
        let mut seen = Viewed {
            tick: tick_every,
            ..Viewed::default()
        };
        let get = |uri: String, etag: Option<&HeaderValue>| {
            let app = app.clone();
            let mut req = Request::get(uri);
            if let Some(etag) = etag {
                req = req.header(IF_NONE_MATCH, etag);
            }
            runtime.block_on(async move {
                let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
                let status = res.status();
                let etag = res.headers().get(ETAG).cloned();
                (status, drained_len(res.into_body()).await, etag)
            })
        };
        let mut last_etag: Option<HeaderValue> = None;
        while !stop.load(Ordering::Relaxed) {
            let tick = Instant::now();
            let (status, len, etag) = get("/worlds/default/world".to_owned(), last_etag.as_ref());
            seen.slowest_world = seen.slowest_world.max(tick.elapsed());
            match status {
                StatusCode::OK => {
                    assert!(len > 0, "/world answered 200 with an empty body");
                    last_etag = Some(etag.expect("/world sends an ETag"));
                }
                StatusCode::NOT_MODIFIED => seen.unchanged += 1,
                other => panic!("/world answered {other}"),
            }
            seen.worlds += 1;
            seen.largest_body = seen.largest_body.max(len);
            let (_, head, _) = state.bounds().unwrap();
            // The head can move between the two reads; below the base that is a 410, fine.
            let (status, len, _) = get(format!("/worlds/default/diff?from={head}&to={head}"), None);
            match status {
                StatusCode::OK => seen.diffs += 1,
                StatusCode::GONE => seen.refused += 1,
                other => panic!("/diff answered {other}"),
            }
            seen.largest_body = seen.largest_body.max(len);
            std::thread::sleep(tick_every.saturating_sub(tick.elapsed()));
        }
        seen
    }

    /// Prints what each `viewer` reader saw and the server's `/world` hold split and, unless
    /// `measuring`, asserts the whole-process `peak`.
    fn report_viewer(
        viewed: &[Viewed],
        timings: Option<&ReadTimingsSnapshot>,
        peak: usize,
        measuring: bool,
    ) {
        for (reader, seen) in viewed.iter().enumerate() {
            eprintln!(
                "viewer {}/{} (tick {} ms): {} /world ({} unchanged, 304), \
                 {} /diff ({} refused), largest body {}, slowest /world {} ms",
                reader + 1,
                viewed.len(),
                seen.tick.as_millis(),
                seen.worlds,
                seen.unchanged,
                seen.diffs,
                seen.refused,
                mib(seen.largest_body),
                seen.slowest_world.as_millis()
            );
        }
        // A reader staggered past the end of a short backfill may never read; one must.
        assert!(
            viewed.iter().any(|v| v.worlds > 0),
            "the viewer never read the world"
        );
        report_timings(timings.expect("the viewer child records the /world hold"));
        if measuring {
            eprintln!("viewer: assert skipped (measurement variant)");
            return;
        }
        assert!(
            peak < VIEWER_PEAK_LIMIT,
            "serve peak with a viewer {} is over {}",
            mib(peak),
            mib(VIEWER_PEAK_LIMIT)
        );
    }

    /// Polls `bridge` to the end of its log: raw events consumed, and the slowest poll (where
    /// the fold waits for a reader's lock).
    fn poll_to_end(bridge: &mut Bridge<SqliteEventLog, SqliteVerdictStore>) -> (u64, Duration) {
        let mut consumed = 0;
        let mut slowest = Duration::ZERO;
        loop {
            let poll = Instant::now();
            let report = bridge.poll_once().unwrap();
            slowest = slowest.max(poll.elapsed());
            if report.stats.consumed == 0 {
                return (consumed, slowest);
            }
            consumed += report.stats.consumed;
        }
    }

    /// [`poll_to_end`] with every `poll_once` on a tokio blocking-pool thread, as `Bridge::run`
    /// does in serve (s2w#220): glibc gives that thread its own arena.
    fn poll_to_end_blocking(
        runtime: &tokio::runtime::Runtime,
        mut bridge: Bridge<SqliteEventLog, SqliteVerdictStore>,
    ) -> (u64, Duration) {
        runtime.block_on(async move {
            let mut consumed = 0;
            let mut slowest = Duration::ZERO;
            loop {
                let poll = Instant::now();
                let (returned, report) = tokio::task::spawn_blocking(move || {
                    let report = bridge.poll_once();
                    (bridge, report)
                })
                .await
                .unwrap();
                bridge = returned;
                slowest = slowest.max(poll.elapsed());
                let report = report.unwrap();
                if report.stats.consumed == 0 {
                    return (consumed, slowest);
                }
                consumed += report.stats.consumed;
            }
        })
    }

    /// The log, verdict store and registry a serve process would open on `directory`: the
    /// mapping and source the parent wrote, never the fixture or the profiler.
    fn bridge_inputs(directory: &PathBuf) -> (SqliteEventLog, SqliteVerdictStore, EngineRegistry) {
        let mapping: StreamMapping =
            serde_json::from_slice(&std::fs::read(directory.join(MAPPING_FILE)).unwrap()).unwrap();
        let source = std::fs::read_to_string(directory.join(SOURCE_FILE)).unwrap();
        let engine = MappingEngine::new(mapping).unwrap();
        let log = SqliteEventLog::open(directory).unwrap();
        let verdicts = SqliteVerdictStore::open(directory).unwrap();
        let mut registry = EngineRegistry::new();
        registry
            .register(Route::Exact(source), Box::new(engine))
            .unwrap();
        (log, verdicts, registry)
    }

    /// The `bridge` child starts like a serve process: it reads the mapping and the source
    /// the parent wrote, never loading the fixture or running the profiler, so its whole-process
    /// peak is what serve would hold. `variant` is `bridge`, `viewer` (a [`viewer`] thread reads
    /// throughout), or a measurement variant (module doc), which reports and never asserts.
    #[expect(
        clippy::too_many_lines,
        reason = "one child's measurement reads top to bottom: configure, peak window, report"
    )]
    fn bridge(directory: &PathBuf, variant: &str) {
        let (base, _) = split(variant);
        let with_viewer = base == "viewer";
        let blocking = base == "bridge-run";
        let history_cap = knob(HISTORY_CAP);
        let batch_knob = knob(BATCH);
        let viewers = knob(VIEWERS);
        assert!(
            viewers != Some(0),
            "{VIEWERS}=0: the viewer child needs a reader"
        );
        // Only the default configuration asserts: a knob, an allocator tuning (named or
        // inherited), another thread topology, more than one reader, dhat or mimalloc measures
        // something else.
        let measuring = (variant != "bridge" && variant != "viewer")
            || history_cap.is_some()
            || batch_knob.is_some()
            || viewers.is_some_and(|n| n != 1)
            || allocator_env().is_some()
            || heap_target()
            || mimalloc_target();
        let (log, verdicts, registry) = bridge_inputs(directory);
        let mut timeline = Timeline::new(CAP);
        if let Some(cap) = history_cap {
            timeline = timeline.with_history_cap(cap);
        }
        let mut state = QueryState::new(timeline);
        if with_viewer {
            state = state.with_read_timings();
        }
        let config = BridgeConfig {
            batch: batch_knob.unwrap_or(BridgeConfig::default().batch),
            ..BridgeConfig::default()
        };
        let batch = config.batch;
        let runtime = blocking.then(|| {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
        });
        let before = reset_peak();
        // dhat's max_bytes counts from the child's start, VmHWM from here: the heap baseline
        // puts both on one footing.
        let heap_before = heap_target().then(|| dhat::HeapStats::get().curr_bytes);
        let started = Instant::now();
        let mut bridge = Bridge::new(log, verdicts, registry, state.clone(), config).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let readers = if with_viewer {
            spawn_viewers(&state, &stop, viewers.unwrap_or(1), viewer_tick())
        } else {
            Vec::new()
        };
        let (consumed, slowest_poll) = match &runtime {
            Some(runtime) => poll_to_end_blocking(runtime, bridge),
            None => {
                let polled = poll_to_end(&mut bridge);
                drop(bridge);
                polled
            }
        };
        stop.store(true, Ordering::Relaxed);
        let viewed: Vec<Viewed> = readers.into_iter().map(|r| r.join().unwrap()).collect();
        let timings = state.read_timings();
        let (_, head, _) = state.bounds().unwrap();
        let name = if with_viewer { "viewer" } else { variant };
        let seconds = started.elapsed().as_secs_f64();
        report(
            name,
            before,
            started,
            &format!(", {consumed} raw events, {head} world events"),
        );
        let peak = status("VmHWM:");
        let heap_max = heap_target().then(|| dhat::HeapStats::get().max_bytes);
        // What the process still holds once the backfill's state is gone: the demo box kept
        // 906 MiB resident with no world loaded (s2w#220), which the peak cannot show.
        drop(state);
        drop(runtime);
        std::thread::sleep(Duration::from_secs(2));
        let rss_after_drop = status("VmRSS:");
        eprintln!(
            "{name}: whole-process peak {} (baseline {} before the bridge), slowest poll_once {} ms",
            mib(peak),
            mib(before),
            slowest_poll.as_millis()
        );
        eprintln!(
            "result {}",
            serde_json::json!({
                "target": env!("CARGO_CRATE_NAME"),
                "variant": variant,
                "history_cap": history_cap.unwrap_or(DEFAULT_HISTORY_CAP),
                "batch": batch,
                "peak_bytes": peak,
                "rss_after_drop_bytes": rss_after_drop,
                "baseline_bytes": before,
                "heap_max_bytes": heap_max,
                "heap_baseline_bytes": heap_before,
                "seconds": seconds,
                "slowest_poll_ms": slowest_poll.as_millis(),
                "raw_events": consumed,
                "world_events": head,
                "asserted": !measuring,
                "viewer_tick_ms": with_viewer.then(|| viewer_tick().as_millis()),
                "readers": viewed.iter().map(Viewed::json).collect::<Vec<_>>(),
                "read_timings": timings.as_ref().map(timings_json),
            })
        );
        if !with_viewer {
            if measuring {
                eprintln!("{name}: assert skipped (measurement variant)");
                return;
            }
            assert!(
                peak < SERVE_PEAK_LIMIT,
                "serve peak {} is over {}",
                mib(peak),
                mib(SERVE_PEAK_LIMIT)
            );
            return;
        }
        report_viewer(&viewed, timings.as_ref(), peak, measuring);
    }

    /// The variants to run: the named ones, else the default sweep, which refuses a
    /// measurement knob so that a green default run always asserted.
    fn variants(only: Option<&str>) -> Vec<&str> {
        match only {
            Some(only) => only.split(',').collect(),
            None if heap_target() => vec!["bridge"],
            None => {
                // The knobs and any allocator variable: the children inherit the caller's
                // environment.
                let knobs = [HISTORY_CAP, BATCH, VIEWERS]
                    .into_iter()
                    .find(|name| std::env::var_os(name).is_some())
                    .map(str::to_owned);
                if let Some(name) = knobs.or_else(allocator_env) {
                    panic!(
                        "{name} is set: name the variants to measure; the default sweep asserts"
                    );
                }
                DEFAULT_VARIANTS.to_vec()
            }
        }
    }

    /// An allocator variable in the environment, if any: glibc's `MALLOC_*` and
    /// `GLIBC_TUNABLES`, or an `LD_PRELOAD` that may replace the allocator. Any of them makes
    /// the peak a different measurement.
    fn allocator_env() -> Option<String> {
        std::env::vars_os()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .find(|name| {
                name.starts_with("MALLOC_") || name == "GLIBC_TUNABLES" || name == "LD_PRELOAD"
            })
    }

    /// A variant's base and its allocator tuning: `bridge-run+arena2` is
    /// `("bridge-run", Some("arena2"))`.
    fn split(variant: &str) -> (&str, Option<&str>) {
        variant
            .split_once('+')
            .map_or((variant, None), |(base, allocator)| (base, Some(allocator)))
    }

    /// The glibc tuning `variant` (`<base>+<allocator>`) sets on its child, if any.
    fn tuning(variant: &str) -> &'static [(&'static str, &'static str)] {
        let (base, Some(allocator)) = split(variant) else {
            return &[];
        };
        assert!(
            base == "bridge" || base == "bridge-run",
            "{variant}: an allocator tuning needs base bridge or bridge-run"
        );
        ALLOCATORS
            .iter()
            .find(|(name, _)| *name == allocator)
            .unwrap_or_else(|| panic!("{variant}: unknown allocator tuning {allocator}"))
            .1
    }

    /// The mimalloc target measures mimalloc's defaults: mimalloc reads `MIMALLOC_*` options
    /// from the environment, which the children inherit.
    fn refuse_mimalloc_options() {
        assert!(
            !mimalloc_target()
                || !std::env::vars_os().any(|(k, _)| k.to_string_lossy().starts_with("MIMALLOC_")),
            "a MIMALLOC_* variable is set: the mimalloc target measures mimalloc's defaults"
        );
    }

    #[test]
    #[ignore = "folds 1.5x10^5 events in five children; run by hand with --release"]
    fn backfill_memory_breakdown() {
        let events = load().unwrap();
        let discovered = mapping(events);
        let id = discovered.identity().unwrap();
        eprintln!("mapping {id} (window {DISCOVER_WINDOW}), {EVENTS} raw events, fresh cycles");
        let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("{}-sqlite-log", env!("CARGO_CRATE_NAME")).replace('_', "-"));
        let _ignored = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let started = Instant::now();
        populate(events, &directory);
        std::fs::write(
            directory.join(MAPPING_FILE),
            serde_json::to_vec(&discovered).unwrap(),
        )
        .unwrap();
        std::fs::write(directory.join(SOURCE_FILE), events[0].source.as_str()).unwrap();
        eprintln!("log populated in {:.1} s", started.elapsed().as_secs_f64());
        refuse_mimalloc_options();
        let only = std::env::var(VARIANTS).ok();
        let variants = variants(only.as_deref());
        // A bridge-type child stores verdicts in the shared log directory; a second one in the
        // same invocation replays them. The default sweep's `viewer` after `bridge` predates this.
        let measurement = variants.iter().any(|v| !DEFAULT_VARIANTS.contains(v));
        let bridges = variants
            .iter()
            .filter(|v| matches!(split(v).0, "bridge" | "bridge-run" | "viewer"))
            .count();
        assert!(
            !measurement || bridges <= 1,
            "run one bridge-type variant per invocation when measuring: {variants:?}"
        );
        let filter = format!(
            "{}::child",
            module_path!()
                .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
                .unwrap()
        );
        for variant in variants {
            let tuning = tuning(variant);
            assert!(
                !heap_target() || matches!(variant, "bridge" | "queries"),
                "{variant}: the heap target measures bridge and queries only"
            );
            assert!(
                !mimalloc_target() || tuning.is_empty(),
                "{variant}: a glibc tuning means nothing to Rust allocations under mimalloc"
            );
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([filter.as_str(), "--exact", "--ignored", "--nocapture"])
                .env(VARIANT, variant)
                .env(LOG_DIR, &directory)
                .envs(tuning.iter().copied())
                .output()
                .unwrap();
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(output.status.success(), "{variant} child failed");
            // A filter that matched no test also exits 0: prove the child ran.
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("test result: ok. 1 passed;"),
                "{variant} child ran no test (filter {filter})"
            );
        }
        let _ignored = std::fs::remove_dir_all(&directory);
    }

    /// One measurement, in a child process started by the test above; does nothing when run
    /// directly.
    #[test]
    #[ignore = "a child of backfill_memory_breakdown"]
    fn child() {
        let Ok(variant) = std::env::var(VARIANT) else {
            return;
        };
        // In the heap target, count every allocation the child makes (s2w#220).
        let _profiler = heap_target().then(|| {
            dhat::Profiler::builder()
                .testing()
                .trim_backtraces(Some(1))
                .build()
        });
        if matches!(split(&variant).0, "bridge" | "bridge-run" | "viewer") {
            bridge(&PathBuf::from(std::env::var(LOG_DIR).unwrap()), &variant);
            return;
        }
        let events = load().unwrap();
        let engine = MappingEngine::new(mapping(events)).unwrap();
        match variant.as_str() {
            "world" => world(events, &engine),
            "timeline" => timeline(events, &engine, false),
            "queries" => timeline(events, &engine, true),
            other => panic!("unknown variant {other}"),
        }
    }
}
