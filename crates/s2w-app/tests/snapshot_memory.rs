//! Memory and time of `serve`'s snapshot paths at 10^5 and 10^6 events (#33 PR 1b, #179), measured
//! as resident memory from `/proc/self/status` (Linux only). Ignored: it folds a million events. Run with
//! `cargo test --release -p s2w-app --test snapshot_memory -- --ignored --nocapture`.
//!
//! The workload is synthetic and wiki-shaped: each raw event observes a page (two attributes)
//! and an `edited` relationship from its user; pages repeat every `n / 2` events and users
//! every `n / 20`. Numbers are for decision 0024 and the demo box's `MemoryMax=1G`.

// `allow-unwrap-in-tests` applies inside `#[cfg(test)]` items only.
#[cfg(test)]
mod memory {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use s2w_app::query::{BaseTime, Timeline};
    use s2w_app::snapshot::{SNAPSHOT_FORMAT, SnapshotRefV1, codec, fold_hash, store};
    use s2w_core::{AttrValue, NaturalKey, World, WorldEvent, fold};

    const CAP: u64 = s2w_app::DEFAULT_HUB_IN_DEGREE_CAP;

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

    fn peak_since(before: usize) -> usize {
        status("VmHWM:").saturating_sub(before)
    }

    fn mib(bytes: usize) -> String {
        #[expect(clippy::cast_precision_loss, reason = "display only")]
        let mib = bytes as f64 / 1_048_576.0;
        format!("{mib:.1} MiB")
    }

    /// Raw events `lo..hi` of an `n`-event stream, as world events.
    fn events(n: usize, lo: usize, hi: usize) -> Vec<WorldEvent> {
        let (pages, users) = ((n / 2).max(1), (n / 20).max(1));
        let mut out = Vec::with_capacity(2 * (hi - lo));
        for i in lo..hi {
            let page = format!("enwiki:page:{}", i % pages);
            out.push(WorldEvent::EntityObserved {
                key: NaturalKey::new(page.clone()),
                entity_type: "page".to_owned(),
                attrs: BTreeMap::from([
                    (
                        "title".to_owned(),
                        AttrValue::Str(format!("Some page title {i}")),
                    ),
                    ("rev".to_owned(), AttrValue::Int(i64::try_from(i).unwrap())),
                ]),
            });
            out.push(WorldEvent::RelationshipObserved {
                from: NaturalKey::new(format!("enwiki:user:{}", i % users)),
                to: NaturalKey::new(page),
                kind: "edited".to_owned(),
            });
        }
        out
    }

    /// Folds in chunks so the event vectors never hold more than a sliver of resident memory.
    fn world(n: usize) -> World {
        const CHUNK: usize = 10_000;
        let mut world = World::with_hub_cap(CAP);
        for lo in (0..n).step_by(CHUNK) {
            world = fold(world, &events(n, lo, (lo + CHUNK).min(n)));
        }
        world
    }

    /// The snapshot file for `world`, encoded from the borrowed world as `serve` does (#179).
    fn encode(world: &World) -> Vec<u8> {
        codec::encode_ref(&SnapshotRefV1 {
            format: SNAPSHOT_FORMAT,
            fold_hash: fold_hash(CAP),
            feed_hash: 0,
            hub_cap: CAP,
            offset: world.offset(),
            position: 1,
            position_event_hash: 0,
            cursors: &[],
            time: BaseTime {
                first_ts: Some(0),
                last_ts: Some(1),
                clamped: 0,
            },
            world,
        })
        .unwrap()
    }

    const RESTORE_FILE: &str = "S2W_SNAPSHOT_MEMORY_RESTORE";

    fn measure(n: usize) {
        let start = reset_peak();
        let world = world(n);
        let world_bytes = status("VmRSS:").saturating_sub(start);

        // Periodic / final write as serve does it (#179): encode from the borrowed head world on
        // the bridge thread, then write, fsync and rename the bytes.
        let dir =
            std::env::temp_dir().join(format!("s2w-snapshot-memory-{n}-{}", std::process::id()));
        let before = reset_peak();
        let started = Instant::now();
        let bytes = encode(&world);
        let encode_ms = started.elapsed().as_millis();
        let path = store::write_bytes(&dir, 0, world.offset(), &bytes).unwrap();
        let write_ms = started.elapsed().as_millis();
        drop(bytes);
        let write_peak = peak_since(before);
        let file_bytes = usize::try_from(std::fs::metadata(&path).unwrap().len()).unwrap();

        // What the bridge thread spent per capture before #179: a clone of the head world.
        // Measured after the write so its freed pages cannot hide the write's peak.
        let started = Instant::now();
        let copy = world.clone();
        let clone_ms = started.elapsed().as_millis();
        drop(copy);
        drop(world);

        // Restore in a fresh process, as a restarted serve would: nothing freed to reuse.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "memory::restore_child",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env(RESTORE_FILE, &path)
            .output()
            .unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let child = String::from_utf8_lossy(&output.stdout);
        let restore = child
            .lines()
            .find(|l| l.starts_with("restore:"))
            .unwrap_or("restore: child printed nothing");

        println!(
            "n={n} raw events ({} world events): world {} resident; \
             write: peak +{} (encode {encode_ms} ms on the bridge thread, {write_ms} ms incl. \
             fsync, file {}; a head clone takes {clone_ms} ms); {restore}",
            2 * n,
            mib(world_bytes),
            mib(write_peak),
            mib(file_bytes),
        );
    }

    /// Run by [`measure`] in a child process; a no-op when run directly.
    #[test]
    #[ignore = "a child of snapshot_memory_and_time"]
    fn restore_child() {
        let Some(path) = std::env::var_os(RESTORE_FILE) else {
            return;
        };
        let before = reset_peak();
        let started = Instant::now();
        let bytes = std::fs::read(path).unwrap();
        let decoded = codec::decode(&bytes).unwrap();
        drop(bytes);
        let timeline = Timeline::from_snapshot(decoded.world, decoded.time);
        let restore_ms = started.elapsed().as_millis();
        let retained = status("VmRSS:").saturating_sub(before);
        assert_eq!(timeline.head(), timeline.base());
        println!(
            "restore: peak +{}, retained +{} (base and head share one world) ({restore_ms} ms)",
            mib(peak_since(before)),
            mib(retained)
        );
    }

    #[test]
    #[ignore = "folds a million events; run with --ignored --nocapture for the numbers"]
    fn snapshot_memory_and_time() {
        if std::env::var_os(RESTORE_FILE).is_some() {
            return;
        }
        measure(100_000);
        measure(1_000_000);
    }
}
