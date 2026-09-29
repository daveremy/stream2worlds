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
//!   a freed copy's pages stay resident and pad whichever window comes next.)
//! - `viewer`: the `bridge` backfill with a reader thread attached before the first poll,
//!   issuing one `/world` and one `/diff?from=<head>&to=<head>` through the real router every
//!   tick and draining each body, as a page does. The `/world` carries the last `ETag` in
//!   `If-None-Match`, so an unchanged head answers 304 before any projection, as the page's
//!   refresh does. The tick is [`VIEWER_TICK_MS`] milliseconds: 5000 by default (the page's
//!   `WORLD_REFRESH_MS`), 1000 for the page's old rate, a worst case. It asserts the whole-process peak stays under [`VIEWER_PEAK_LIMIT`], and
//!   reports the slowest `/world` (an upper bound on how long one read held the fold's lock:
//!   the guard is held until the last chunk is queued, and the reader drains as it goes) and
//!   the slowest `poll_once`, which is where the fold waits for that lock.
//!
//! `S2W_BACKFILL_MEMORY_VARIANTS=bridge,viewer` runs only the named variants;
//! `S2W_BACKFILL_MEMORY_VIEWER_TICK_MS=1000` sets the `viewer` tick.
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
//!   bridge child: the timeline's history cap and `BridgeConfig::batch`. The default sweep
//!   (no `VARIANTS`) refuses to run with either set, so a green default run always asserted.
//! - The `backfill_memory_heap` target includes this file with dhat as the global allocator
//!   and runs `bridge` only, printing dhat's `max_bytes` (Rust heap peak) beside `VmHWM`.
//!   Compare its `max_bytes` with this target's `bridge` `VmHWM`: the gap is non-heap resident
//!   memory (SQLite's C heap, stacks, the binary) plus allocator overhead and fragmentation.
//!   (dhat's own bookkeeping makes that target's `VmHWM` meaningless.)
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
    use std::time::{Duration, Instant};

    use axum::body::Body;
    use axum::http::header::{ETAG, IF_NONE_MATCH};
    use axum::http::{HeaderValue, Request, StatusCode};
    use tokio_stream::StreamExt;
    use tower::ServiceExt;

    use s2w_app::bridge::{Bridge, BridgeConfig, EngineRegistry, Route};
    use s2w_app::discover::DISCOVER_WINDOW;
    use s2w_app::query::{DEFAULT_HISTORY_CAP, QueryState, Timeline, ViewParams, router};
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
        let state = QueryState::new(Timeline::new(CAP));
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
            let before = reset_peak();
            let started = Instant::now();
            // Past the history cap, `/diff` serves the head only (decision 0026).
            let diff = state.diff(head, Some(head), None).unwrap();
            drop(diff);
            report("query /diff head..head", before, started, "");
        }
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

    /// What the `viewer` reader saw.
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

    /// Prints what the `viewer` reader saw and asserts the whole-process `peak`.
    fn report_viewer(viewed: &Viewed, peak: usize) {
        eprintln!(
            "viewer (tick {} ms): {} /world ({} unchanged, 304), {} /diff ({} refused), \
             largest body {}, slowest /world {} ms",
            viewed.tick.as_millis(),
            viewed.worlds,
            viewed.unchanged,
            viewed.diffs,
            viewed.refused,
            mib(viewed.largest_body),
            viewed.slowest_world.as_millis()
        );
        assert!(viewed.worlds > 0, "the viewer never read the world");
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
        // Only the default configuration asserts: a knob, an allocator tuning, another thread
        // topology or dhat measures something else.
        let measuring = (variant != "bridge" && variant != "viewer")
            || history_cap.is_some()
            || batch_knob.is_some()
            || heap_target();
        let (log, verdicts, registry) = bridge_inputs(directory);
        let mut timeline = Timeline::new(CAP);
        if let Some(cap) = history_cap {
            timeline = timeline.with_history_cap(cap);
        }
        let state = QueryState::new(timeline);
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
        let reader = with_viewer.then(|| {
            let (state, stop) = (state.clone(), Arc::clone(&stop));
            std::thread::spawn(move || viewer(&state, &stop, viewer_tick()))
        });
        let (consumed, slowest_poll) = match &runtime {
            Some(runtime) => poll_to_end_blocking(runtime, bridge),
            None => poll_to_end(&mut bridge),
        };
        stop.store(true, Ordering::Relaxed);
        let viewed = reader.map(|r| r.join().unwrap());
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
                "baseline_bytes": before,
                "heap_max_bytes": heap_max,
                "heap_baseline_bytes": heap_before,
                "seconds": seconds,
                "slowest_poll_ms": slowest_poll.as_millis(),
                "raw_events": consumed,
                "world_events": head,
                "asserted": with_viewer || !measuring,
            })
        );
        let Some(viewed) = viewed else {
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
        };
        report_viewer(&viewed, peak);
    }

    /// The variants to run: the named ones, else the default sweep, which refuses a
    /// measurement knob so that a green default run always asserted.
    fn variants(only: Option<&str>) -> Vec<&str> {
        match only {
            Some(only) => only.split(',').collect(),
            None if heap_target() => vec!["bridge"],
            None => {
                for name in [HISTORY_CAP, BATCH] {
                    assert!(
                        std::env::var_os(name).is_none(),
                        "{name} is set: name the variants to measure; the default sweep asserts"
                    );
                }
                DEFAULT_VARIANTS.to_vec()
            }
        }
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

    #[test]
    #[ignore = "folds 1.5x10^5 events in five children; run by hand with --release"]
    fn backfill_memory_breakdown() {
        let events = load().unwrap();
        let discovered = mapping(events);
        let id = discovered.identity().unwrap();
        eprintln!("mapping {id} (window {DISCOVER_WINDOW}), {EVENTS} raw events, fresh cycles");
        let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(if heap_target() {
            "backfill-memory-heap-sqlite-log"
        } else {
            "backfill-memory-sqlite-log"
        });
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
                !heap_target() || variant == "bridge",
                "{variant}: the heap target measures bridge only"
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
