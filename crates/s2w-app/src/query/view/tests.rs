use s2w_core::{NaturalKey, World, WorldEvent, fold};

use super::{Lod, ViewParams, graph_view, type_view};

const GOLDEN: &str = include_str!("../../../../s2w-core/tests/fixtures/golden-fold-v1.json");

fn observe(key: &str, entity_type: &str) -> WorldEvent {
    WorldEvent::EntityObserved {
        key: NaturalKey::new(key),
        entity_type: entity_type.to_owned(),
        attrs: std::collections::BTreeMap::new(),
    }
}

fn relate(from: &str, to: &str, kind: &str) -> WorldEvent {
    WorldEvent::RelationshipObserved {
        from: NaturalKey::new(from),
        to: NaturalKey::new(to),
        kind: kind.to_owned(),
    }
}

fn merge(survivor: &str, absorbed: &str) -> WorldEvent {
    WorldEvent::EntitiesMerged {
        survivor: NaturalKey::new(survivor),
        absorbed: NaturalKey::new(absorbed),
    }
}

/// Asserts the cheap full type view equals the `Graph` one, as values and as bytes.
fn assert_same(world: &World, at: usize) {
    let params = ViewParams {
        lod: Lod::Type,
        ..ViewParams::default()
    };
    let (cheap, full) = (type_view(world), graph_view(world, &params).unwrap());
    assert_eq!(cheap, full, "type view differs at {at}");
    assert_eq!(
        serde_json::to_string(&cheap).unwrap(),
        serde_json::to_string(&full).unwrap(),
        "type view bytes differ at {at}"
    );
}

#[test]
fn the_cheap_type_view_equals_the_graph_one_at_every_golden_offset() {
    let log: Vec<WorldEvent> = serde_json::from_str(GOLDEN).unwrap();
    for cap in 1..=3 {
        for at in 0..=log.len() {
            assert_same(&fold(World::with_hub_cap(cap), &log[..at]), at);
        }
    }
}

/// Hubs as link sources and targets, a hub relating to a hub, untyped entities, merges that
/// collapse link endpoints, a merge that absorbs a hub (its refs re-resolve), a relationship
/// observed before its target trips the cap, and a revoked merge, at every prefix.
#[test]
fn the_cheap_type_view_equals_the_graph_one_through_merges_and_hubs() {
    let log = [
        observe("p1", "user"),
        observe("p2", "user"),
        observe("p4", ""),
        observe("a", "page"),
        observe("a2", "page"),
        observe("c", "page"),
        relate("p1", "a", "on"),
        relate("p1", "c", "on"),
        relate("p2", "a", "on"),
        relate("p1", "b", "on"),
        relate("p2", "b", "at"),
        relate("a", "b", "on"),
        relate("a", "p1", "by"),
        relate("p3", "p1", "on"),
        relate("p4", "c", "on"),
        relate("p4", "b", "on"),
        relate("p4", "a", "on"),
        merge("a", "a2"),
        relate("a2", "c", "on"),
        merge("p1", "p2"),
        relate("p2", "c", "on"),
        merge("c", "b"),
        relate("p3", "a2", "on"),
        WorldEvent::MergeRevoked {
            survivor: NaturalKey::new("p1"),
            absorbed: NaturalKey::new("p2"),
        },
        relate("p2", "a", "at"),
    ];
    for cap in 1..=3 {
        for at in 0..=log.len() {
            assert_same(&fold(World::with_hub_cap(cap), &log[..at]), at);
        }
    }
}
