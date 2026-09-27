//! Example-based tests of the fold's documented cases, plus the insta regression snapshot.

use std::collections::BTreeMap;

use s2w_core::{AttrValue, NaturalKey, World, WorldEvent, fold};

fn key(k: &str) -> NaturalKey {
    NaturalKey::new(k)
}

fn observe(k: &str, entity_type: &str, attrs: &[(&str, AttrValue)]) -> WorldEvent {
    WorldEvent::EntityObserved {
        key: key(k),
        entity_type: entity_type.to_owned(),
        attrs: attrs
            .iter()
            .map(|(name, v)| ((*name).to_owned(), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn relate(from: &str, to: &str, kind: &str) -> WorldEvent {
    WorldEvent::RelationshipObserved {
        from: key(from),
        to: key(to),
        kind: kind.to_owned(),
    }
}

fn merge(survivor: &str, absorbed: &str) -> WorldEvent {
    WorldEvent::EntitiesMerged {
        survivor: key(survivor),
        absorbed: key(absorbed),
    }
}

fn revoke(survivor: &str, absorbed: &str) -> WorldEvent {
    WorldEvent::MergeRevoked {
        survivor: key(survivor),
        absorbed: key(absorbed),
    }
}

#[test]
fn ids_are_minted_once_in_order_of_first_mention() {
    let w = fold(
        World::default(),
        &[
            relate("a", "b", "edited"),
            observe("c", "page", &[]),
            observe("a", "user", &[]),
        ],
    );
    let ids: Vec<u64> = ["a", "b", "c"]
        .iter()
        .map(|k| w.id_of(&key(k)).unwrap().get())
        .collect();
    assert_eq!(ids, [0, 1, 2]);
    assert_eq!(w.offset(), 3);
    assert_eq!(w.entities().len(), 3);
}

#[test]
fn merge_edge_cases_are_no_ops() {
    let base = fold(
        World::default(),
        &[observe("a", "user", &[]), observe("b", "user", &[])],
    );
    for event in [
        merge("a", "a"),       // self-merge
        merge("a", "unknown"), // unknown absorbed
        merge("unknown", "a"), // unknown survivor
        revoke("a", "b"),      // revoke of a merge that never happened
    ] {
        let after = fold(base.clone(), [&event]);
        assert!(after.merges().is_empty(), "{event:?} changed merges");
        assert_eq!(after.keys(), base.keys(), "{event:?} changed keys");
    }

    // A cycle attempt and a double merge are no-ops too.
    let merged = fold(base, &[merge("a", "b")]);
    let cycle = fold(merged.clone(), &[merge("b", "a")]);
    assert_eq!(cycle.merges(), merged.merges());
    let other = fold(
        merged.clone(),
        &[observe("c", "user", &[]), merge("c", "b")],
    );
    assert_eq!(other.merges(), merged.merges());
}

#[test]
fn revoke_matches_only_the_exact_raw_edge() {
    let events = [
        observe("a", "user", &[]),
        observe("b", "user", &[]),
        observe("c", "user", &[]),
        merge("b", "a"),
        merge("c", "b"),
    ];
    let w = fold(World::default(), &events);
    let (a, c) = (w.id_of(&key("a")).unwrap(), w.id_of(&key("c")).unwrap());
    assert_eq!(w.resolve(a), c);

    // a resolves to c, but the recorded edge is a→b: revoking a→c does nothing.
    let not_matched = fold(w.clone(), &[revoke("c", "a")]);
    assert_eq!(not_matched.merges(), w.merges());

    let split = fold(w, &[revoke("b", "a")]);
    assert_eq!(split.resolve(a), a);
}

#[test]
fn attrs_land_on_the_survivor_and_stay_after_revoke() {
    let w = fold(
        World::default(),
        &[
            observe("a", "user", &[]),
            observe("b", "user", &[]),
            merge("a", "b"),
            observe("b", "user", &[("bot", AttrValue::Bool(true))]),
            revoke("a", "b"),
        ],
    );
    let a = w.id_of(&key("a")).unwrap();
    let b = w.id_of(&key("b")).unwrap();
    assert_eq!(
        w.entities()[&a].attrs.get("bot"),
        Some(&AttrValue::Bool(true))
    );
    assert!(w.entities()[&b].attrs.is_empty());
}

#[test]
fn hub_cap_counts_distinct_sources_and_turns_edges_into_hub_refs() {
    let w = fold(
        World::with_hub_cap(2),
        &[
            relate("p1", "wiki", "on"),
            relate("p1", "wiki", "on"), // same source again: still one source
            relate("p2", "wiki", "on"),
            relate("p3", "wiki", "on"), // third distinct source: past the cap
        ],
    );
    let wiki = w.id_of(&key("wiki")).unwrap();
    let p3 = w.id_of(&key("p3")).unwrap();
    assert_eq!(
        w.relationships().values().copied().collect::<Vec<_>>(),
        [2, 1]
    );
    assert_eq!(w.entities()[&p3].hub_refs.get("on"), Some(&wiki));
    let counters = &w.hub_counters()[&wiki];
    assert_eq!(counters.in_degree(), 3);
    assert_eq!(counters.by_kind.get("on"), Some(&4));
    assert_eq!(counters.last_seen_offset, 4);
}

#[test]
fn world_round_trips_through_json() {
    let w = fold(
        World::with_hub_cap(1),
        &[
            observe("a", "user", &[("n", AttrValue::Int(-3))]),
            relate("a", "b", "edited"),
            relate("c", "b", "edited"),
            merge("a", "c"),
        ],
    );
    let json = serde_json::to_string(&w).unwrap();
    let back: World = serde_json::from_str(&json).unwrap();
    assert_eq!(back, w);
}

/// Regression snapshot of the fold's output shape. Review changes with `cargo insta review`;
/// never accept one to make a test pass without reading it.
#[test]
fn small_fold_snapshot() {
    let w = fold(
        World::with_hub_cap(2),
        &[
            observe("alice", "user", &[("edits", AttrValue::Int(1))]),
            observe("Main_Page", "page", &[("ns", AttrValue::Int(0))]),
            relate("alice", "Main_Page", "edited"),
            relate("bob", "Main_Page", "edited"),
            relate("carol", "Main_Page", "edited"),
            observe("alice_alt", "user", &[("bot", AttrValue::Bool(false))]),
            merge("alice", "alice_alt"),
            observe(
                "alice_alt",
                "user",
                &[("tz", AttrValue::Str("UTC".to_owned()))],
            ),
            revoke("alice", "alice_alt"),
        ],
    );
    insta::assert_json_snapshot!(w);
}
