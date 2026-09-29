//! A mapping accepted, revoked or re-proposed while `serve` runs (s2w#184, decision 0023
//! "Rebuild"): the served world is always the cold fold of the log under the routes the
//! proposal store resolves to, and its epoch is their feed fingerprint.

use super::*;

/// The feed fingerprint `serve` resolves for [`MAPPED`] routed to `mapping` by proposal `id`.
fn fingerprint(mapping: StreamMapping, id: &str) -> u64 {
    routed_to(engine(mapping, id)).feed_fingerprint()
}

/// The world a fresh directory serves after folding raw events `range` under `mapping`.
async fn cold(
    name: &str,
    mapping: StreamMapping,
    id: &str,
    range: std::ops::Range<usize>,
) -> serde_json::Value {
    let dir = TestDirectory::new(name);
    let feed = mapped(range, Some(routed_to(engine(mapping, id))));
    serve_until(&dir, NO_SNAPSHOT, feed)
        .await
        .expect("sockets allowed once")
        .world
}

/// A human reject of proposal `id`, as a reviewer revoking an accepted mapping would.
fn reject(dir: &std::path::Path, id: &str) {
    use s2w_log::{Decider, NewDecision, Outcome, ProposalStore, SqliteProposalStore};
    SqliteProposalStore::open(dir)
        .expect("proposal store")
        .append_decision(&NewDecision {
            proposal_id: id.to_owned(),
            decider: Decider::Human,
            outcome: Outcome::Reject,
            basis: "revoked".to_owned(),
            decided_at_ms: 0,
        })
        .expect("decision");
}

fn step(act: impl FnOnce(&std::path::Path) + 'static, until: Until) -> Step {
    (Box::new(act), until)
}

#[test]
fn accepting_a_mapping_while_serving_rebuilds_the_cold_fold_and_a_restart_serves_it() {
    run(false, async {
        let dir = TestDirectory::new("serve-rebuild-accept");
        accept_mapping(dir.path(), "p-a", mapping_a());
        let (fp_a, fp_b) = (
            fingerprint(mapping_a(), "p-a"),
            fingerprint(mapping_b(), "p-b"),
        );
        let mut feed = mapped(0..10, None);
        feed.then = vec![step(
            |dir| accept_mapping(dir, "p-b", mapping_b()),
            Until::Rebuilt {
                feed: fp_b,
                consumed: 10,
            },
        )];
        let Some(live) = serve_until(&dir, NO_SNAPSHOT, feed).await else {
            return;
        };
        let expected = cold("serve-rebuild-accept-cold", mapping_b(), "p-b", 0..10).await;
        assert_eq!(live.world["epoch"], format!("{fp_b:016x}"));
        assert_eq!(live.world, expected, "rebuilt world == cold fold under B");
        assert!(
            noted(&live, &format!("rebuild: feed {fp_a:016x} -> {fp_b:016x}"))
                && noted(&live, "rebuild: replaying the log from the start")
                && noted(&live, "rebuild complete: 10 events")
                && noted(&live, "(replayed 0, evaluated 10,"),
            "{:?}",
            live.notes
        );

        // B's verdicts were stored by the rebuild: a restart replays them into the same world.
        let mut restart = mapped(0..0, None);
        restart.until = Until::Consumed(10);
        let restarted = serve_until(&dir, NO_SNAPSHOT, restart)
            .await
            .expect("sockets allowed once");
        assert_eq!(
            restarted.world, expected,
            "restart under B == cold fold under B"
        );
        assert!(!noted(&restarted, "rebuild"), "{:?}", restarted.notes);
    });
}

#[test]
fn revoking_the_accepted_mapping_rebuilds_the_previous_epoch_from_its_stored_verdicts() {
    run(false, async {
        let dir = TestDirectory::new("serve-rebuild-revoke");
        accept_mapping(dir.path(), "p-a", mapping_a());
        let (fp_a, fp_b) = (
            fingerprint(mapping_a(), "p-a"),
            fingerprint(mapping_b(), "p-b"),
        );
        let mut feed = mapped(0..10, None);
        let rebuilt = |feed| Until::Rebuilt { feed, consumed: 10 };
        feed.then = vec![
            step(|dir| accept_mapping(dir, "p-b", mapping_b()), rebuilt(fp_b)),
            step(|dir| reject(dir, "p-b"), rebuilt(fp_a)),
        ];
        let Some(live) = serve_until(&dir, NO_SNAPSHOT, feed).await else {
            return;
        };
        assert_eq!(
            live.world["epoch"],
            format!("{fp_a:016x}"),
            "A's epoch is back"
        );
        let expected = cold("serve-rebuild-revoke-cold", mapping_a(), "p-a", 0..10).await;
        assert_eq!(live.world, expected, "world == cold fold under A");
        assert!(
            noted(&live, &format!("rebuild: feed {fp_b:016x} -> {fp_a:016x}"))
                && noted(&live, "(replayed 10, evaluated 0,"),
            "A's stored verdicts replay, nothing is evaluated again: {:?}",
            live.notes
        );
    });
}

#[test]
fn re_proposing_the_same_mapping_bytes_does_not_rebuild() {
    run(false, async {
        let dir = TestDirectory::new("serve-rebuild-same-bytes");
        accept_mapping(dir.path(), "p-a", mapping_a());
        let fp_a = fingerprint(mapping_a(), "p-a");
        let mut feed = mapped(0..10, None);
        feed.then = vec![step(
            |dir| accept_mapping(dir, "p-a2", mapping_a()),
            Until::Noted("routes: unchanged after proposal store change; no rebuild"),
        )];
        let Some(live) = serve_until(&dir, NO_SNAPSHOT, feed).await else {
            return;
        };
        assert_eq!(live.world["epoch"], format!("{fp_a:016x}"));
        assert!(!noted(&live, "rebuild:"), "{:?}", live.notes);
    });
}

#[test]
fn a_stop_as_soon_as_a_rebuild_starts_restarts_to_the_cold_fold() {
    run(false, async {
        let dir = TestDirectory::new("serve-rebuild-crash");
        accept_mapping(dir.path(), "p-a", mapping_a());
        let fp_b = fingerprint(mapping_b(), "p-b");
        let mut feed = mapped(0..10, None);
        // Stop the moment B's epoch is served, whether or not its backfill has run: the final
        // snapshot is written under B's fingerprint at whatever B has folded so far.
        feed.then = vec![step(
            |dir| accept_mapping(dir, "p-b", mapping_b()),
            Until::Epoch(fp_b),
        )];
        let Some(_stopped) = serve_until(&dir, SNAPSHOT_ON_STOP, feed).await else {
            return;
        };
        // Whatever the stop left, the restart converges on the cold fold (or times out).
        let expected = cold("serve-rebuild-crash-cold", mapping_b(), "p-b", 0..20).await;
        let mut restart = mapped(10..20, None);
        restart.until = Until::World(expected.clone());
        // Reaching `expected` within the harness timeout is the check; no rebuild on restart.
        let restarted = serve_until(&dir, SNAPSHOT_ON_STOP, restart)
            .await
            .expect("sockets allowed once");
        assert!(!noted(&restarted, "rebuild:"), "{:?}", restarted.notes);
    });
}

/// Mapping B with its first entity rule's type label changed again: a third identity.
fn mapping_c() -> StreamMapping {
    let mut mapping = mapping_b();
    let rule = mapping.entities.first_mut().expect("an entity rule");
    rule.type_label = format!("{}-c", rule.type_label);
    mapping
}

#[test]
fn two_accepts_before_the_next_check_rebuild_once_under_the_last() {
    run(false, async {
        let dir = TestDirectory::new("serve-rebuild-coalesce");
        accept_mapping(dir.path(), "p-a", mapping_a());
        let (fp_a, fp_b, fp_c) = (
            fingerprint(mapping_a(), "p-a"),
            fingerprint(mapping_b(), "p-b"),
            fingerprint(mapping_c(), "p-c"),
        );
        let mut feed = mapped(0..10, None);
        // Both land in one synchronous step, so no check can run between them.
        feed.then = vec![step(
            |dir| {
                accept_mapping(dir, "p-b", mapping_b());
                accept_mapping(dir, "p-c", mapping_c());
            },
            Until::Rebuilt {
                feed: fp_c,
                consumed: 10,
            },
        )];
        let Some(live) = serve_until(&dir, NO_SNAPSHOT, feed).await else {
            return;
        };
        let expected = cold("serve-rebuild-coalesce-cold", mapping_c(), "p-c", 0..10).await;
        assert_eq!(live.world, expected, "world == cold fold under C");
        let swaps: Vec<_> = live
            .notes
            .iter()
            .filter(|note| note.starts_with("rebuild: feed "))
            .collect();
        assert_eq!(swaps.len(), 1, "{swaps:?}");
        assert!(
            swaps[0].contains(&format!("{fp_a:016x} -> {fp_c:016x}"))
                && !swaps[0].contains(&format!("{fp_b:016x}")),
            "{swaps:?}"
        );
    });
}

/// `serve`'s bridge loop by hand, one event per poll: a change can land mid-backfill, which
/// the served loop cannot be made to hold open (its backfill of the fixture is one batch).
struct Driver {
    bridge: Bridge<SqliteEventLog, SqliteVerdictStore>,
    rebuild: Rebuild,
    state: QueryState,
    notes: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Driver {
    /// Resolves routes from `dir`'s proposal store and starts from the log's start, as a
    /// `serve` with snapshots off does.
    fn open(dir: &std::path::Path) -> Self {
        let mut reporter = TestReporter::default();
        let (registry, watcher) =
            RouteWatcher::start(dir.to_path_buf(), &mut reporter, |_, _| false).expect("routes");
        let (log, verdicts) = (
            SqliteEventLog::open(dir).expect("log opens"),
            SqliteVerdictStore::open(dir).expect("verdicts open"),
        );
        let state = state();
        let (resume, _) = snapshots::prepare(
            &state,
            (&log, &verdicts),
            dir,
            (NO_SNAPSHOT, &registry),
            &mut reporter,
        )
        .expect("prepare");
        assert!(resume.is_none(), "snapshots are off");
        let config = BridgeConfig {
            batch: 1,
            ..BridgeConfig::default()
        };
        let bridge =
            Bridge::new(log, verdicts, registry, state.clone(), config).expect("bridge starts");
        let rebuild = Rebuild::new(
            watcher,
            state.clone(),
            NO_SNAPSHOT,
            None,
            reporter.note_sink(),
        );
        Self {
            bridge,
            rebuild,
            state,
            notes: reporter.sunk,
        }
    }

    /// One poll, then the rebuild check `local_bridge` runs after it; the events consumed.
    fn poll(self) -> (Self, u64) {
        let Self {
            mut bridge,
            mut rebuild,
            state,
            notes,
        } = self;
        let report = bridge.poll_once().expect("poll");
        rebuild.check_on_next_poll();
        let bridge = rebuild.after_poll(bridge, &report).expect("after poll");
        let consumed = report.stats.consumed;
        (
            Self {
                bridge,
                rebuild,
                state,
                notes,
            },
            consumed,
        )
    }

    /// Polls until a poll consumes nothing: the backfill, if any, has completed.
    fn drain(mut self) -> Self {
        loop {
            let (next, consumed) = self.poll();
            self = next;
            if consumed == 0 {
                return self;
            }
        }
    }

    /// The served epoch's feed fingerprint and how many sources are rebuilding.
    fn epoch(&self) -> (u64, usize) {
        let (epoch, rebuilding) = self.state.epoch_and_rebuilding();
        (epoch.0, rebuilding)
    }

    fn noted(&self, needle: &str) -> Vec<String> {
        let notes = self.notes.lock().expect("notes");
        notes
            .iter()
            .filter(|n| n.contains(needle))
            .cloned()
            .collect()
    }

    async fn world(&self) -> serde_json::Value {
        let app = router(self.state.clone()).layer(middleware::from_fn(host_allowlist));
        let (status, _, bytes) = web_response(&app, "/worlds/default/world").await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_slice(&bytes).expect("world")
    }
}

/// A served log of raw events `0..10` under mapping A, its verdicts stored.
async fn seeded_under_a(name: &str) -> Option<TestDirectory> {
    let dir = TestDirectory::new(name);
    accept_mapping(dir.path(), "p-a", mapping_a());
    let mut feed = mapped(0..10, None);
    feed.until = Until::Consumed(10);
    serve_until(&dir, NO_SNAPSHOT, feed).await?;
    Some(dir)
}

#[test]
fn an_accept_during_a_running_backfill_supersedes_it_and_folds_under_the_last() {
    run(false, async {
        let Some(dir) = seeded_under_a("serve-rebuild-supersede").await else {
            return;
        };
        let (fp_b, fp_c) = (
            fingerprint(mapping_b(), "p-b"),
            fingerprint(mapping_c(), "p-c"),
        );
        let driver = Driver::open(dir.path()).drain();

        accept_mapping(dir.path(), "p-b", mapping_b());
        let (mut driver, _) = driver.poll();
        assert_eq!(driver.noted("rebuild: feed ").len(), 1, "B swapped in");
        for _ in 0..2 {
            let (next, consumed) = driver.poll();
            assert_eq!(consumed, 1, "B's backfill is running");
            driver = next;
        }

        accept_mapping(dir.path(), "p-c", mapping_c());
        let (driver, consumed) = driver.poll();
        assert_eq!(consumed, 1, "the change lands on a poll that consumed");
        let superseded = driver.noted("rebuild: superseded at position ");
        let position: u64 = superseded
            .first()
            .and_then(|note| note.rsplit(' ').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or_else(|| panic!("one supersede note: {superseded:?}"));
        assert!((1..10).contains(&position), "mid-backfill: {position}");
        assert!(
            driver.noted("rebuild complete").is_empty(),
            "B never completed"
        );
        assert_eq!(
            driver
                .noted(&format!("rebuild: feed {fp_b:016x} -> {fp_c:016x}"))
                .len(),
            1
        );
        assert_eq!(driver.epoch(), (fp_c, 1));

        let driver = driver.drain();
        assert_eq!(driver.noted("rebuild complete: 10 events").len(), 1);
        assert_eq!(driver.epoch().1, 0);
        let expected = cold("serve-rebuild-supersede-cold", mapping_c(), "p-c", 0..10).await;
        assert_eq!(driver.world().await, expected, "world == cold fold under C");
    });
}

#[test]
fn a_crash_mid_backfill_replays_the_stored_verdicts_and_converges() {
    run(false, async {
        let Some(dir) = seeded_under_a("serve-rebuild-crash-mid").await else {
            return;
        };
        let fp_b = fingerprint(mapping_b(), "p-b");
        let driver = Driver::open(dir.path()).drain();
        accept_mapping(dir.path(), "p-b", mapping_b());
        let (mut driver, _) = driver.poll();
        assert_eq!(driver.epoch(), (fp_b, 1));
        for _ in 0..3 {
            driver = driver.poll().0;
        }
        assert_eq!(driver.bridge.stats().consumed, 3, "3 of 10 folded under B");
        // A crash: no final snapshot, no completion; only what each poll stored survives.
        drop(driver);

        let restarted = Driver::open(dir.path()).drain();
        let stats = restarted.bridge.stats();
        assert_eq!(
            (stats.consumed, stats.replayed, stats.evaluated),
            (10, 3, 7),
            "the 3 verdicts B stored before the crash replay; the rest are evaluated"
        );
        assert_eq!(restarted.epoch(), (fp_b, 0));
        let expected = cold("serve-rebuild-crash-mid-cold", mapping_b(), "p-b", 0..10).await;
        assert_eq!(
            restarted.world().await,
            expected,
            "world == cold fold under B"
        );
    });
}

#[test]
fn revoking_the_only_accepted_mapping_unroutes_the_source_and_empties_the_world() {
    run(false, async {
        let dir = TestDirectory::new("serve-rebuild-revoke-only");
        accept_mapping(dir.path(), "p-a", mapping_a());
        let fp_a = fingerprint(mapping_a(), "p-a");
        // No accepted mapping resolves to the default routes, where the source has no engine.
        let fp_none = EngineRegistry::with_defaults().feed_fingerprint();
        let mut feed = mapped(0..10, None);
        feed.until = Until::Rebuilt {
            feed: fp_a,
            consumed: 10,
        };
        feed.then = vec![step(
            |dir| reject(dir, "p-a"),
            Until::Rebuilt {
                feed: fp_none,
                consumed: 10,
            },
        )];
        let Some(live) = serve_until(&dir, NO_SNAPSHOT, feed).await else {
            return;
        };
        assert_eq!(live.world["epoch"], format!("{fp_none:016x}"));
        assert_eq!(node_count(&live), 0, "no entities without a mapping");
        let swap = live
            .notes
            .iter()
            .position(|note| note.contains(&format!("rebuild: feed {fp_a:016x} -> {fp_none:016x}")))
            .unwrap_or_else(|| panic!("{:?}", live.notes));
        assert!(
            !live.notes[swap..]
                .iter()
                .any(|note| note.starts_with("route: source ")),
            "the source is unrouted: {:?}",
            live.notes
        );
    });
}

#[test]
fn a_restart_after_a_live_rebuild_restores_the_snapshot_taken_under_the_new_mapping() {
    run(false, async {
        let dir = TestDirectory::new("serve-rebuild-snapshot-b");
        accept_mapping(dir.path(), "p-a", mapping_a());
        let fp_b = fingerprint(mapping_b(), "p-b");
        let mut feed = mapped(0..10, None);
        feed.then = vec![step(
            |dir| accept_mapping(dir, "p-b", mapping_b()),
            Until::Rebuilt {
                feed: fp_b,
                consumed: 10,
            },
        )];
        // The stop writes the final snapshot under B's fingerprint, at the log's head.
        let Some(_live) = serve_until(&dir, SNAPSHOT_ON_STOP, feed).await else {
            return;
        };

        // Routes resolve to B again; B's snapshot passes rule 5b (`check_routed`) only because
        // the live rebuild stored B's verdict at the snapshot's position.
        let mut restart = mapped(10..20, None);
        restart.until = Until::Consumed(10);
        let restarted = serve_until(&dir, SNAPSHOT_ON_STOP, restart)
            .await
            .expect("sockets allowed once");
        assert!(
            noted(&restarted, "restored from snapshot") && !noted(&restarted, "ignoring snapshot"),
            "{:?}",
            restarted.notes
        );
        let expected = cold("serve-rebuild-snapshot-b-cold", mapping_b(), "p-b", 0..20).await;
        assert_eq!(restarted.world, expected, "world == cold fold under B");
    });
}

#[test]
fn revoking_a_live_mapping_restores_the_previous_mappings_snapshot() {
    run(false, async {
        let dir = TestDirectory::new("serve-rebuild-snapshot-a");
        accept_mapping(dir.path(), "p-a", mapping_a());
        let (fp_a, fp_b) = (
            fingerprint(mapping_a(), "p-a"),
            fingerprint(mapping_b(), "p-b"),
        );
        // A's era ends in a snapshot under A's fingerprint at the log's head.
        let mut feed = mapped(0..10, None);
        feed.until = Until::Consumed(10);
        let Some(_first) = serve_until(&dir, SNAPSHOT_ON_STOP, feed).await else {
            return;
        };

        let mut restart = mapped(0..0, None);
        restart.until = Until::Epoch(fp_a);
        restart.then = vec![
            step(
                |dir| accept_mapping(dir, "p-b", mapping_b()),
                Until::Rebuilt {
                    feed: fp_b,
                    consumed: 10,
                },
            ),
            step(
                |dir| reject(dir, "p-b"),
                Until::Rebuilt {
                    feed: fp_a,
                    consumed: 0,
                },
            ),
        ];
        let live = serve_until(&dir, SNAPSHOT_ON_STOP, restart)
            .await
            .expect("sockets allowed once");
        let revoke = live
            .notes
            .iter()
            .position(|note| note.contains(&format!("rebuild: feed {fp_b:016x} -> {fp_a:016x}")))
            .unwrap_or_else(|| panic!("{:?}", live.notes));
        // Start-up notes come first in `notes`, then everything sunk while serving in order, so
        // the notes after the revoke's swap are the revoke's own: start-up's restore is not among them.
        let after = &live.notes[revoke..];
        assert!(
            after.iter().any(|n| n.contains("restored from snapshot"))
                && after
                    .iter()
                    .any(|n| n.contains("rebuild: resuming after log position"))
                && !after.iter().any(|n| n.contains("ignoring snapshot")),
            "the revoke restores A's snapshot: {after:?}"
        );
        let expected = cold("serve-rebuild-snapshot-a-cold", mapping_a(), "p-a", 0..10).await;
        assert_eq!(live.world, expected, "world == cold fold under A");
    });
}
