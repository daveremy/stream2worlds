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
//!   each its own peak window: a copy of the head world (what `/world` paid before #216 PR 2a,
//!   kept as the reference), the projection alone (`view_at`, which now borrows the head), and
//!   the JSON serialization alone; then one `/diff` from the head to itself.
//! - `viewer`: the `bridge` backfill with a reader thread attached before the first poll,
//!   issuing one `/world` and one `/diff?from=<head>&to=<head>` through the real router every
//!   [`VIEWER_TICK`] and draining each body, as a page does (at 1 s, the page's old rate, a
//!   worst case). It reports the whole-process peak against [`VIEWER_PEAK_LIMIT`] and the
//!   slowest `/world`, an upper bound on how long one read held the fold's lock.
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
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use s2w_app::bridge::{Bridge, BridgeConfig, EngineRegistry, Route};
    use s2w_app::discover::DISCOVER_WINDOW;
    use s2w_app::query::{QueryState, Timeline, ViewParams, router};
    use s2w_core::{World, fold};
    use s2w_discover::{Config, Discovery, discover};
    use s2w_log::{EventLog, SqliteEventLog, SqliteVerdictStore};
    use s2w_model::{RawEvent, StreamMapping, Timestamp};
    use s2w_system1::{Engine, MappingEngine, Verdict};

    use super::recorded::load;

    const EVENTS: usize = 150_000;
    const CAP: u64 = s2w_app::DEFAULT_HUB_IN_DEGREE_CAP;
    const VARIANT: &str = "S2W_BACKFILL_MEMORY_VARIANT";
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
    /// How often the `viewer` reader asks for the world.
    const VIEWER_TICK: Duration = Duration::from_millis(1000);

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
            // The reference: what copying the head cost every `/world` before #216 PR 2a.
            let before = reset_peak();
            let started = Instant::now();
            let copy = state.world_at(Some(head)).unwrap();
            drop(copy);
            report(
                "query /world part: head copy (old path)",
                before,
                started,
                "",
            );
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
            // Past the history cap, `/diff` serves the head only (decision 0026).
            let diff = state.diff(head, Some(head), None).unwrap();
            drop(diff);
            report("query /diff head..head", before, started, "");
        }
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
        diffs: usize,
        refused: usize,
        largest_body: usize,
        slowest_world: Duration,
    }

    /// A page's reads until `stop`: every [`VIEWER_TICK`], one `/world` and one `/diff` of the
    /// head with itself through the real router, each body drained so the JSON is counted.
    fn viewer(state: &QueryState, stop: &AtomicBool) -> Viewed {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let app = router(state.clone());
        let mut seen = Viewed::default();
        let get = |uri: String| {
            let app = app.clone();
            runtime.block_on(async move {
                let res = app
                    .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let status = res.status();
                let body = axum::body::to_bytes(res.into_body(), usize::MAX)
                    .await
                    .unwrap();
                (status, body.len())
            })
        };
        while !stop.load(Ordering::Relaxed) {
            let tick = Instant::now();
            let started = Instant::now();
            let (status, len) = get("/worlds/default/world".to_owned());
            seen.slowest_world = seen.slowest_world.max(started.elapsed());
            assert_eq!(status, StatusCode::OK, "/world");
            seen.worlds += 1;
            seen.largest_body = seen.largest_body.max(len);
            let (_, head, _) = state.bounds().unwrap();
            // The head can move between the two reads; below the base that is a 410, fine.
            let (status, len) = get(format!("/worlds/default/diff?from={head}&to={head}"));
            if status == StatusCode::OK {
                seen.diffs += 1;
            } else {
                seen.refused += 1;
            }
            seen.largest_body = seen.largest_body.max(len);
            std::thread::sleep(VIEWER_TICK.saturating_sub(tick.elapsed()));
        }
        seen
    }

    /// Prints what the `viewer` reader saw against the whole-process `peak`.
    fn report_viewer(viewed: &Viewed, peak: usize) {
        eprintln!(
            "viewer: {} /world, {} /diff ({} refused), largest body {}, slowest /world {} ms \
         (upper bound on one read-lock hold: includes the wait for an append and the JSON \
         after release)",
            viewed.worlds,
            viewed.diffs,
            viewed.refused,
            mib(viewed.largest_body),
            viewed.slowest_world.as_millis()
        );
        assert!(viewed.worlds > 0, "the viewer never read the world");
        // Reported, not asserted: stage 1 (#216 PR 2a) removes the world copy but still builds
        // the whole view and its JSON body; the streamed `/world` (PR 2b) turns this on.
        eprintln!(
            "viewer: peak {} the {} finish line",
            if peak < VIEWER_PEAK_LIMIT {
                "under"
            } else {
                "OVER"
            },
            mib(VIEWER_PEAK_LIMIT)
        );
    }

    /// The `bridge` child starts like a serve process: it reads the mapping and the source
    /// the parent wrote, never loading the fixture or running the profiler, so its whole-process
    /// peak is what serve would hold. With `with_viewer`, a [`viewer`] thread reads throughout.
    fn bridge(directory: &PathBuf, with_viewer: bool) {
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
        let state = QueryState::new(Timeline::new(CAP));
        let before = reset_peak();
        let started = Instant::now();
        let mut bridge = Bridge::new(
            log,
            verdicts,
            registry,
            state.clone(),
            BridgeConfig::default(),
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let reader = with_viewer.then(|| {
            let (state, stop) = (state.clone(), Arc::clone(&stop));
            std::thread::spawn(move || viewer(&state, &stop))
        });
        let mut consumed = 0;
        loop {
            let report = bridge.poll_once().unwrap();
            if report.stats.consumed == 0 {
                break;
            }
            consumed += report.stats.consumed;
        }
        stop.store(true, Ordering::Relaxed);
        let viewed = reader.map(|r| r.join().unwrap());
        let (_, head, _) = state.bounds().unwrap();
        let name = if with_viewer { "viewer" } else { "bridge" };
        report(
            name,
            before,
            started,
            &format!(", {consumed} raw events, {head} world events"),
        );
        let peak = status("VmHWM:");
        eprintln!(
            "{name}: whole-process peak {} (baseline {} before the bridge)",
            mib(peak),
            mib(before)
        );
        let Some(viewed) = viewed else {
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

    #[test]
    #[ignore = "folds 1.5x10^5 events in five children; run by hand with --release"]
    fn backfill_memory_breakdown() {
        let events = load().unwrap();
        let discovered = mapping(events);
        let id = discovered.identity().unwrap();
        eprintln!("mapping {id} (window {DISCOVER_WINDOW}), {EVENTS} raw events, fresh cycles");
        let directory =
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("backfill-memory-sqlite-log");
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
        for variant in ["world", "timeline", "queries", "bridge", "viewer"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["backfill::child", "--exact", "--ignored", "--nocapture"])
                .env(VARIANT, variant)
                .env(LOG_DIR, &directory)
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
        if variant == "bridge" || variant == "viewer" {
            bridge(
                &PathBuf::from(std::env::var(LOG_DIR).unwrap()),
                variant == "viewer",
            );
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
