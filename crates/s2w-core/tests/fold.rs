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
    assert_eq!(w.entity_count(), 3);
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
        w.entity(a).unwrap().attrs.get("bot"),
        Some(&AttrValue::Bool(true))
    );
    assert!(w.entity(b).unwrap().attrs.is_empty());
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
            relate("p1", "wiki", "on"), // after the trip, even a pre-cap source becomes a hub_ref
        ],
    );
    let wiki = w.id_of(&key("wiki")).unwrap();
    let p1 = w.id_of(&key("p1")).unwrap();
    let p3 = w.id_of(&key("p3")).unwrap();
    // p1's pre-trip edge stays at its pre-trip weight; readers union it with p1's hub_refs.
    assert_eq!(
        w.relationships().values().copied().collect::<Vec<_>>(),
        [2, 1]
    );
    assert_eq!(w.entity(p3).unwrap().hub_ref("on"), Some(wiki));
    assert_eq!(w.entity(p1).unwrap().hub_ref("on"), Some(wiki));
    let counters = &w.hub_counters()[&wiki];
    assert_eq!(counters.in_degree(), 3);
    assert_eq!(counters.by_kind.get("on"), Some(&5));
    assert_eq!(counters.last_seen_offset, 5);
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

/// A deserialized world whose id counter disagrees with its entity count mints nothing: the id
/// is the entity's index, so a mint there would mis-index it. The fold stays total. (Exhausting
/// the id space itself now takes 2^64 minted entities; the old way to reach it, a snapshot with
/// the counter at `u64::MAX - 1` and no entities, is this case.)
#[test]
fn a_counter_out_of_step_with_entities_mints_nothing() {
    for counter in [1, u64::MAX - 1] {
        let mut json = serde_json::to_value(World::default()).unwrap();
        json["next_entity_id"] = serde_json::json!(counter);
        let w: World = serde_json::from_value(json).unwrap();

        let related = fold(w.clone(), &[relate("a", "b", "edited")]);
        assert!(related.keys().is_empty());
        assert!(related.relationships().is_empty());
        assert_eq!(related.offset(), 1);

        let observed = fold(w, &[observe("a", "user", &[])]);
        assert_eq!(observed.id_of(&key("a")), None);
        assert_eq!(observed.entity_count(), 0);
        assert_eq!(observed.offset(), 1);
    }
}

/// `entities` is indexed by id, so a deserialized world must have ids exactly `0..len`.
#[test]
fn entity_ids_must_be_dense_on_deserialize() {
    let w = fold(
        World::default(),
        &[observe("a", "user", &[]), observe("b", "user", &[])],
    );
    let json = serde_json::to_value(&w).unwrap();
    assert!(serde_json::from_value::<World>(json.clone()).is_ok());

    let state = json["entities"]["0"].clone();
    for ids in [&["0", "2"][..], &["1"], &["1", "2"]] {
        let mut bad = json.clone();
        bad["entities"] = ids
            .iter()
            .map(|id| ((*id).to_owned(), state.clone()))
            .collect();
        let err = serde_json::from_value::<World>(bad)
            .unwrap_err()
            .to_string();
        assert!(err.contains("entity ids are not dense"), "{ids:?}: {err}");
    }
}

/// An entity with no hub refs holds none in memory and writes `{}`; one with refs round-trips.
#[test]
fn hub_refs_round_trip_as_a_map() {
    let w = fold(
        World::with_hub_cap(1),
        &[relate("p1", "wiki", "on"), relate("p2", "wiki", "on")],
    );
    let p1 = w.id_of(&key("p1")).unwrap();
    let p2 = w.id_of(&key("p2")).unwrap();
    assert!(!w.entity(p1).unwrap().has_hub_refs());
    assert!(w.entity(p2).unwrap().has_hub_refs());
    let json = serde_json::to_value(&w).unwrap();
    assert_eq!(
        json["entities"][p1.get().to_string()]["hub_refs"],
        serde_json::json!({})
    );
    assert_eq!(
        json["entities"][p2.get().to_string()]["hub_refs"]["on"],
        serde_json::json!(w.id_of(&key("wiki")).unwrap().get())
    );
    assert_eq!(serde_json::from_value::<World>(json).unwrap(), w);
}

/// Whether `a` and `b` share the state of the entity minted for `k` (decision 0028); `None` if
/// either world lacks it.
fn shares(a: &World, b: &World, k: &str) -> Option<bool> {
    let id = a.id_of(&key(k))?;
    Some(std::sync::Arc::ptr_eq(a.entity_arc(id)?, b.entity_arc(id)?))
}

#[test]
fn a_cloned_world_shares_states_until_a_write_copies_one() {
    let original = fold(
        World::default(),
        &[
            observe("u", "user", &[("lang", AttrValue::Str("en".into()))]),
            observe("v", "user", &[]),
        ],
    );
    let copy = original.clone();
    assert_eq!(shares(&original, &copy, "u"), Some(true));
    assert_eq!(shares(&original, &copy, "v"), Some(true));

    let written = fold(
        copy,
        &[observe("u", "user", &[("bot", AttrValue::Bool(true))])],
    );
    assert_eq!(
        shares(&original, &written, "u"),
        Some(false),
        "the write copies u"
    );
    assert_eq!(
        shares(&original, &written, "v"),
        Some(true),
        "v was not written"
    );
    let u = original.id_of(&key("u")).unwrap();
    assert_eq!(original.entity(u).unwrap().attrs.get("bot"), None);
    assert_eq!(
        written.entity(u).unwrap().attrs.get("bot"),
        Some(&AttrValue::Bool(true))
    );
}

#[test]
fn a_write_that_changes_nothing_copies_nothing() {
    let attrs = [
        ("lang", AttrValue::Str("en".into())),
        ("n", AttrValue::Int(1)),
    ];
    let original = fold(
        World::with_hub_cap(1),
        &[
            observe("u", "user", &attrs),
            relate("p1", "wiki", "on"),
            relate("p2", "wiki", "on"), // past the cap: p2 holds a hub ref
        ],
    );
    let same = fold(
        original.clone(),
        &[
            observe("u", "user", &attrs),
            observe("u", "user", &attrs[..1]), // a subset of what u holds
            relate("p2", "wiki", "on"),        // the hub ref p2 already holds
        ],
    );
    assert_eq!(same.offset(), original.offset() + 3);
    assert_eq!(shares(&original, &same, "u"), Some(true));
    assert_eq!(shares(&original, &same, "p2"), Some(true));

    // Each kind of change still copies: a new value, a new type, a new hub ref kind.
    for event in [
        observe("u", "user", &[("n", AttrValue::Int(2))]),
        observe("u", "page", &[]),
    ] {
        assert_eq!(
            shares(&original, &fold(original.clone(), &[event]), "u"),
            Some(false)
        );
    }
    assert_eq!(
        shares(
            &original,
            &fold(original.clone(), &[relate("p2", "wiki", "in")]),
            "p2"
        ),
        Some(false)
    );
}
