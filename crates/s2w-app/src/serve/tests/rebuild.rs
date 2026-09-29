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
