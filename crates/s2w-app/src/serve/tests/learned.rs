//! The learned-mapping producer through `serve` (s2w#197 PR 4b, decision 0025): the in-run
//! trigger, a held writer lock at start, and obfuscation invariance end to end. The log is
//! seeded directly; every stream is synthetic with one-letter names (decision 0018).

use std::collections::BTreeMap;

use s2w_model::fnv1a64_hex;

use super::*;
use crate::discover::tests::{SOURCE, append, small, stream};

/// A run of the learned source that stops once the bridge has read `consumed` events, feeding
/// `events` through the source after the seeded log.
fn learned_run(events: Vec<(u8, Vec<u8>)>, consumed: u64) -> Feed {
    Feed {
        source: SourceId::new(SOURCE).expect("source"),
        events,
        registry: None,
        until: Until::Consumed(consumed),
        discover: small(),
        then: Vec::new(),
    }
}

fn rows(dir: &TestDirectory) -> (usize, usize) {
    if !dir.path().join(s2w_log::PROPOSAL_DATABASE_FILE).exists() {
        return (0, 0);
    }
    let (ids, decisions) = proposal_rows(dir);
    (ids.len(), decisions)
}

#[test]
fn a_source_reaching_its_window_while_serving_is_filed_and_routed_by_the_live_rebuild() {
    run(false, async {
        let dir = TestDirectory::new("serve-learned-in-run");
        let payloads = stream(350);
        append(dir.path(), SOURCE, payloads[..100].to_vec());
        let fed = payloads[100..]
            .iter()
            .zip(0_u8..)
            .map(|(payload, cursor)| (cursor, payload.clone()))
            .collect();
        let mut feed = learned_run(fed, 350);
        // The in-run filing moves the proposal store's watermark; the live rebuild (s2w#184)
        // then replays the log under the new route.
        feed.until = Until::Noted("rebuild complete");
        let Some(first) = serve_until(&dir, NO_SNAPSHOT, feed).await else {
            return;
        };
        assert!(
            noted(&first, "100 events, below the window of 300"),
            "{:?}",
            first.notes
        );
        assert_eq!(rows(&dir), (1, 1), "the in-run trigger filed and accepted");
        assert!(
            noted(&first, "accepted by policy") && noted(&first, "; the live rebuild applies it"),
            "{:?}",
            first.notes
        );
        assert!(
            node_count(&first) > 0,
            "routed in-process by the live rebuild"
        );

        let second = serve_until(&dir, NO_SNAPSHOT, learned_run(Vec::new(), 350))
            .await
            .expect("sockets allowed once");
        assert!(
            noted(&second, &format!("route: source '{SOURCE}' runs mapping ")),
            "{:?}",
            second.notes
        );
        assert!(!noted(&second, "discover:"), "{:?}", second.notes);
        assert_eq!(rows(&dir), (1, 1), "the restart writes nothing");
        assert!(node_count(&second) > 0, "still routed at the next start");
    });
}

#[test]
fn a_writer_held_across_start_leaves_serve_up_and_unrouted_until_a_restart() {
    run(false, async {
        let dir = TestDirectory::new("serve-learned-locked");
        append(dir.path(), SOURCE, stream(400));
        let writer = s2w_log::SqliteProposalStore::open(dir.path()).expect("writer");
        let Some(locked) = serve_until(&dir, NO_SNAPSHOT, learned_run(Vec::new(), 400)).await
        else {
            return;
        };
        assert!(
            noted(
                &locked,
                &format!(
                    "discover: {SOURCE}: store_locked: another writer holds the proposal store; routes unchanged, retried at the next start"
                )
            ),
            "{:?}",
            locked.notes
        );
        assert!(!noted(&locked, "route: source "), "{:?}", locked.notes);
        assert_eq!(node_count(&locked), 0, "serve answered, unrouted");
        assert_eq!(rows(&dir), (0, 0));
        drop(writer);

        let released = serve_until(&dir, NO_SNAPSHOT, learned_run(Vec::new(), 400))
            .await
            .expect("sockets allowed once");
        assert_eq!(rows(&dir), (1, 1));
        assert!(
            noted(
                &released,
                &format!("route: source '{SOURCE}' runs mapping ")
            ),
            "{:?}",
            released.notes
        );
        assert!(node_count(&released) > 0);
    });
}

/// `payload` with every key renamed and every string leaf replaced by its hash: the same
/// statistics under names the profiler has never seen.
fn obfuscate(payload: &[u8]) -> Vec<u8> {
    fn walk(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(fields) => fields
                .into_iter()
                .map(|(key, value)| (format!("k{}", fnv1a64_hex(key.as_bytes())), walk(value)))
                .collect(),
            serde_json::Value::Array(items) => items.into_iter().map(walk).collect(),
            serde_json::Value::String(text) => {
                serde_json::Value::String(format!("s{}", fnv1a64_hex(text.as_bytes())))
            }
            other => other,
        }
    }
    let value = serde_json::from_slice(payload).expect("payload");
    serde_json::to_vec(&walk(value)).expect("encode")
}

/// A world's shape with every name dropped: node count per (kind, type), as sorted counts, the sorted
/// per-node attribute counts, and the count of every other top-level array.
fn shape(world: &serde_json::Value) -> (Vec<usize>, Vec<usize>, BTreeMap<String, usize>) {
    let nodes = world["nodes"].as_array().expect("nodes");
    let mut per_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut attrs: Vec<usize> = Vec::new();
    for node in nodes {
        *per_type
            .entry(format!("{}/{}", node["kind"], node["entity_type"]))
            .or_default() += 1;
        attrs.push(node["attrs"].as_object().map_or(0, serde_json::Map::len));
    }
    let mut types: Vec<usize> = per_type.into_values().collect();
    types.sort_unstable();
    attrs.sort_unstable();
    let arrays = world
        .as_object()
        .expect("world")
        .iter()
        .filter(|(key, value)| key.as_str() != "nodes" && value.is_array())
        .map(|(key, value)| (key.clone(), value.as_array().map_or(0, Vec::len)))
        .collect();
    (types, attrs, arrays)
}

#[test]
fn serve_builds_the_same_world_shape_from_an_obfuscated_copy_of_the_stream() {
    run(false, async {
        let plain = stream(400);
        let hidden: Vec<Vec<u8>> = plain.iter().map(|p| obfuscate(p)).collect();
        let a = TestDirectory::new("serve-learned-plain");
        let b = TestDirectory::new("serve-learned-obfuscated");
        append(a.path(), SOURCE, plain.clone());
        append(b.path(), SOURCE, hidden);
        let Some(pass_a) = serve_until(&a, NO_SNAPSHOT, learned_run(Vec::new(), 400)).await else {
            return;
        };
        let pass_b = serve_until(&b, NO_SNAPSHOT, learned_run(Vec::new(), 400))
            .await
            .expect("sockets allowed once");
        assert!(
            node_count(&pass_a) > 0,
            "pass A maps, so the check is not vacuous"
        );
        let (types, attrs, arrays) = shape(&pass_a.world);
        assert!(
            !types.is_empty() && attrs.iter().sum::<usize>() > 0,
            "pass A has entities with attributes: {types:?} {attrs:?}"
        );
        assert_eq!((types, attrs, arrays), shape(&pass_b.world));
        // No original key or string value survives into pass B's world.
        let world_b = pass_b.world.to_string();
        for payload in &plain {
            let value: serde_json::Value = serde_json::from_slice(payload).expect("payload");
            for (key, leaf) in value.as_object().expect("object") {
                assert!(
                    !world_b.contains(&format!("\"{key}\"")),
                    "original key {key} in pass B's world"
                );
                if let Some(text) = leaf.as_str() {
                    assert!(
                        !world_b.contains(&format!("\"{text}\"")),
                        "original string {text} in pass B's world"
                    );
                }
            }
        }
    });
}
