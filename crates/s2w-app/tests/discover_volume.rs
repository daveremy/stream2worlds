//! Claim volume and world memory under a DISCOVERED mapping (s2w#197 PR 4b, plan §4; decisions
//! 0022 and 0025). Ignored: it folds 10^5 events twice. Run with
//! `cargo test --release -p s2w-app --test discover_volume -- --ignored --nocapture`.
//!
//! Two questions, both on the recorded fixture (11,667 events of a real stream):
//! 1. Window: profile the first 1k, 5k and 10k events and the whole fixture with the production
//!    `Config`, and report whether each mapping identity equals the whole fixture's. (The plan
//!    asked for 20k; the fixture holds fewer, so its full length is the largest window here.)
//! 2. Volume: fold 10^5 events under the 10k-window mapping (`DISCOVER_WINDOW`) and report
//!    claims per event, entities, relationships, resident world bytes and events per second.
//!    The fixture is cycled to 10^5 two ways, which bracket a real stream: `repeat` replays it
//!    as-is, so later cycles re-observe the same entities (a lower bound on growth); `fresh`
//!    suffixes every string leaf with the cycle number, so every cycle observes new entities
//!    (an upper bound). Decision 0025's deploy rule reads the upper bound against ~350 MiB.
//!
//! The fold uses serve's own profiler settings (`DiscoverConfig::default()`: links off, decision
//! 0027), so the volume is what auto-apply files today; `S2W_DISCOVER_VOLUME_LINKS=on` folds a
//! linked (version 2) mapping instead, by hand only. `S2W_DISCOVER_VOLUME_VARIANTS=fresh` runs
//! only the named variants (comma-separated); the default runs both.
//!
//! Each child prints one JSON line last on stdout: `{"heap_bytes":H,"rss_bytes":R,"entities":E,
//! "relationships":L,"links":K,"events":100000,"window":10000,"profiler_version":"9",
//! "variant":"fresh"}`. `rss_bytes` is the `VmRSS` delta over the fold, the figure decisions 0022
//! and 0025 quote. `heap_bytes` is 0 here; the `discover_volume_heap` target includes this file
//! with dhat as the global allocator and fills it with dhat's live heap bytes after the fold
//! minus before it (s2w#392). That count is exact and reproduces to the byte, because the fold
//! allocates the same sequence every run; its `rss_bytes` is inflated by dhat's bookkeeping.
//! `cargo xtask discover-volume` runs the heap target's `fresh` child and gates `heap_bytes`
//! against `[discover_volume]` in `xtask/scale-baseline.toml`.

#[path = "support/recorded.rs"]
#[expect(
    dead_code,
    reason = "this test uses the loader, not the committed mapping"
)]
mod recorded;

// `allow-unwrap-in-tests` applies inside `#[cfg(test)]` items only.
#[cfg(test)]
mod volume {
    use std::time::Instant;

    use s2w_app::discover::{DISCOVER_WINDOW, DiscoverConfig};
    use s2w_core::{World, fold};
    use s2w_discover::{Config, Discovery, PROFILER_VERSION, discover};
    use s2w_model::{RawEvent, StreamMapping, WorldEvent};
    use s2w_system1::{Engine, MappingEngine, Verdict};

    use super::recorded::load;

    const EVENTS: usize = 100_000;
    const VARIANT: &str = "S2W_DISCOVER_VOLUME_VARIANT";
    const VARIANTS: &str = "S2W_DISCOVER_VOLUME_VARIANTS";
    const LINKS: &str = "S2W_DISCOVER_VOLUME_LINKS";

    /// True in the `discover_volume_heap` target, which includes this file with dhat as the
    /// global allocator (s2w#392).
    fn heap_target() -> bool {
        env!("CARGO_CRATE_NAME") == "discover_volume_heap"
    }

    /// dhat's live heap bytes; `None` outside the heap target, which has no profiler to ask.
    fn live_heap() -> Option<usize> {
        heap_target().then(|| dhat::HeapStats::get().curr_bytes)
    }

    fn status(field: &str) -> usize {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let line = status.lines().find(|l| l.starts_with(field)).unwrap();
        let kib: usize = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        kib * 1024
    }

    /// The heap figure for the human-readable line, when there is one.
    fn heap_note(heap_bytes: Option<usize>) -> String {
        heap_bytes.map_or_else(String::new, |b| format!(", world heap {b} B ({})", mib(b)))
    }

    fn mib(bytes: usize) -> String {
        #[expect(clippy::cast_precision_loss, reason = "display only")]
        let mib = bytes as f64 / 1_048_576.0;
        format!("{mib:.1} MiB")
    }

    fn profile(events: &[RawEvent], n: usize, config: &Config) -> (StreamMapping, u128) {
        let payloads: Vec<&[u8]> = events[..n].iter().map(|e| e.payload.as_slice()).collect();
        let started = Instant::now();
        let discovery = discover(&payloads, config).1;
        let ms = started.elapsed().as_millis();
        match discovery {
            Discovery::Mapping(mapping) => (mapping, ms),
            Discovery::Abstain(reason) => panic!("the profiler abstained at {n}: {reason}"),
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

    fn fold_cycled(events: &[RawEvent], mapping: &StreamMapping, variant: &str) {
        let suffix = variant == "fresh";
        let engine = MappingEngine::new(mapping.clone()).unwrap();
        let (before, heap_before) = (status("VmRSS:"), live_heap());
        let started = Instant::now();
        let mut world = World::with_hub_cap(s2w_app::DEFAULT_HUB_IN_DEGREE_CAP);
        let (mut claims, mut abstained, mut batch) = (0_usize, 0_usize, Vec::new());
        let mut merge_claims = 0_usize;
        for i in 0..EVENTS {
            let cycle = i / events.len();
            let mut event = events[i % events.len()].clone();
            if suffix && cycle > 0 {
                event.payload = fresh(&event.payload, cycle);
            }
            match engine.evaluate(&event) {
                Verdict::Propose { claims: out, .. } => {
                    claims += out.len();
                    merge_claims += out
                        .iter()
                        .filter(|c| matches!(c, WorldEvent::EntitiesMerged { .. }))
                        .count();
                    batch.extend(out);
                }
                Verdict::Abstain { .. } => abstained += 1,
            }
            if batch.len() >= 20_000 {
                world = fold(world, &batch);
                batch.clear();
            }
        }
        world = fold(world, &batch);
        drop(batch);
        let secs = started.elapsed().as_secs_f64();
        // Live heap now minus before the world existed: the world's own bytes, since the engine
        // and the loaded fixture were live on both sides and every event clone has been freed.
        let heap_bytes = live_heap()
            .zip(heap_before)
            .map(|(after, before)| after.saturating_sub(before));
        let world_bytes = status("VmRSS:").saturating_sub(before);
        #[expect(clippy::cast_precision_loss, reason = "display only")]
        let (per_event, rate) = (claims as f64 / EVENTS as f64, EVENTS as f64 / secs);
        let (entities, relationships) = (world.entity_count(), world.relationships().len());
        eprintln!(
            "{variant}: {EVENTS} events, {abstained} abstained, {claims} claims ({per_event:.2}/event, {merge_claims} merges), {entities} entities, {relationships} relationships, {} merges, world resident {}{}, {rate:.0} events/s",
            world.merges().len(),
            mib(world_bytes),
            heap_note(heap_bytes),
        );
        drop(world);
        println!(
            "{}",
            serde_json::json!({
                "heap_bytes": heap_bytes.unwrap_or(0),
                "rss_bytes": world_bytes,
                "entities": entities,
                "relationships": relationships,
                "links": mapping.links.len(),
                "events": EVENTS,
                "window": DISCOVER_WINDOW,
                "profiler_version": PROFILER_VERSION,
                "variant": variant,
            })
        );
    }

    /// Serve's profiler settings (links off), or links on when `S2W_DISCOVER_VOLUME_LINKS=on`.
    fn fold_config() -> Config {
        let serve = DiscoverConfig::default();
        assert_eq!(serve.window, DISCOVER_WINDOW, "serve's window moved");
        let links = std::env::var(LINKS).is_ok_and(|v| v == "on");
        Config {
            links: links || serve.profiler.links,
            ..serve.profiler
        }
    }

    #[test]
    #[ignore = "folds 10^5 events twice; run by hand with --release for decision 0022's numbers"]
    fn claim_volume_and_world_memory_under_a_discovered_mapping() {
        let events = load().unwrap();
        let config = Config::default();
        let (full, full_ms) = profile(events, events.len(), &config);
        let full_id = full.identity().unwrap();
        eprintln!(
            "window {}: identity {full_id}, {} entity rules, {} relationship rules, {} links, {full_ms} ms",
            events.len(),
            full.entities.len(),
            full.relationships.len(),
            full.links.len()
        );
        for n in [1_000, 5_000, DISCOVER_WINDOW] {
            let (mapping, ms) = profile(events, n, &config);
            let id = mapping.identity().unwrap();
            eprintln!(
                "window {n}: identity {id}, {} entity rules, {} relationship rules, {} links, {ms} ms, equals window {}: {}",
                mapping.entities.len(),
                mapping.relationships.len(),
                mapping.links.len(),
                events.len(),
                id == full_id
            );
        }
        let only = std::env::var(VARIANTS).ok();
        let variants: Vec<&str> = only
            .as_deref()
            .map_or_else(|| vec!["fresh", "repeat"], |v| v.split(',').collect());
        assert!(
            variants.iter().all(|v| matches!(*v, "fresh" | "repeat")),
            "{VARIANTS} names fresh and/or repeat: {variants:?}"
        );
        let filter = format!(
            "{}::fold_child",
            module_path!()
                .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
                .unwrap()
        );
        // Each fold runs in a fresh process, so neither resident delta reuses pages the other
        // freed.
        for variant in variants {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([filter.as_str(), "--exact", "--ignored", "--nocapture"])
                .env(VARIANT, variant)
                .output()
                .unwrap();
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(output.status.success(), "{variant} child failed");
            let stdout = String::from_utf8_lossy(&output.stdout);
            // A filter that matched no test also exits 0: prove the child ran.
            assert!(
                stdout.contains("test result: ok. 1 passed;"),
                "{variant} child ran no test (filter {filter})"
            );
            stdout
                .lines()
                .filter(|l| l.starts_with('{'))
                .for_each(|l| println!("{l}"));
        }
    }

    /// One fold, in a child process started by the test above or by `cargo xtask
    /// discover-volume`; does nothing when run without `S2W_DISCOVER_VOLUME_VARIANT`.
    #[test]
    #[ignore = "a child of claim_volume_and_world_memory_under_a_discovered_mapping"]
    fn fold_child() {
        let Ok(variant) = std::env::var(VARIANT) else {
            return;
        };
        assert!(
            matches!(variant.as_str(), "fresh" | "repeat"),
            "{VARIANT}={variant}: fresh or repeat"
        );
        // In the heap target, count every allocation the child makes (s2w#392).
        let _profiler = heap_target().then(|| {
            dhat::Profiler::builder()
                .testing()
                .trim_backtraces(Some(1))
                .build()
        });
        let events = load().unwrap();
        let (mapping, _) = profile(events, DISCOVER_WINDOW, &fold_config());
        fold_cycled(events, &mapping, &variant);
    }
}
