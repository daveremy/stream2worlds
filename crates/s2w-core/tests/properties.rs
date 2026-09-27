//! Property tests: the identity model against an independent reference model, and prefix
//! resume through serialization.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use s2w_core::{AttrValue, EntityId, NaturalKey, World, WorldEvent, fold, fold_one};

const KEYS: [&str; 6] = ["a", "b", "c", "d", "e", "f"];

fn arb_key() -> impl Strategy<Value = NaturalKey> {
    prop::sample::select(&KEYS[..]).prop_map(NaturalKey::new)
}

fn arb_event() -> impl Strategy<Value = WorldEvent> {
    prop_oneof![
        2 => (arb_key(), prop::sample::select(&["user", "page"][..]), any::<i8>()).prop_map(
            |(key, t, n)| WorldEvent::EntityObserved {
                key,
                entity_type: t.to_owned(),
                attrs: BTreeMap::from([("n".to_owned(), AttrValue::Int(i64::from(n)))]),
            }
        ),
        2 => (arb_key(), arb_key(), prop::sample::select(&["edited", "links"][..])).prop_map(
            |(from, to, kind)| WorldEvent::RelationshipObserved {
                from,
                to,
                kind: kind.to_owned(),
            }
        ),
        // Merges and revokes are weighted up so chains, cycles and double merges are common.
        3 => (arb_key(), arb_key())
            .prop_map(|(survivor, absorbed)| WorldEvent::EntitiesMerged { survivor, absorbed }),
        2 => (arb_key(), arb_key())
            .prop_map(|(survivor, absorbed)| WorldEvent::MergeRevoked { survivor, absorbed }),
    ]
}

/// The reference model: which keys are known and which merges are live, in natural-key terms.
/// It never reads the fold's `merges` or calls `World::resolve`.
#[derive(Default)]
struct Reference {
    known: BTreeSet<NaturalKey>,
    /// Live merges as (absorbed, survivor), in the order they were applied.
    live: Vec<(NaturalKey, NaturalKey)>,
}

impl Reference {
    /// The partition of known keys implied by the live merges, rebuilt from scratch with a
    /// naive union-find. Maps each key to the component's root: the one member that is not the
    /// absorbed side of any live merge.
    fn roots(&self) -> BTreeMap<NaturalKey, NaturalKey> {
        let mut parent: BTreeMap<&NaturalKey, &NaturalKey> =
            self.known.iter().map(|k| (k, k)).collect();
        fn find<'a>(
            parent: &BTreeMap<&'a NaturalKey, &'a NaturalKey>,
            k: &'a NaturalKey,
        ) -> &'a NaturalKey {
            let mut cur = k;
            while let Some(&p) = parent.get(cur) {
                if p == cur {
                    break;
                }
                cur = p;
            }
            cur
        }
        for (a, s) in &self.live {
            let (ra, rs) = (find(&parent, a), find(&parent, s));
            parent.insert(ra, rs);
        }
        let absorbed: BTreeSet<&NaturalKey> = self.live.iter().map(|(a, _)| a).collect();
        let mut members: BTreeMap<&NaturalKey, Vec<&NaturalKey>> = BTreeMap::new();
        for k in &self.known {
            members.entry(find(&parent, k)).or_default().push(k);
        }
        let mut roots = BTreeMap::new();
        for group in members.values() {
            let heads: Vec<&&NaturalKey> =
                group.iter().filter(|k| !absorbed.contains(**k)).collect();
            assert_eq!(
                heads.len(),
                1,
                "reference component {group:?} has heads {heads:?}"
            );
            for k in group {
                roots.insert((*k).clone(), (*heads[0]).clone());
            }
        }
        roots
    }

    fn apply(&mut self, event: &WorldEvent) {
        match event {
            WorldEvent::EntityObserved { key, .. } => {
                self.known.insert(key.clone());
            }
            WorldEvent::RelationshipObserved { from, to, .. } => {
                self.known.insert(from.clone());
                self.known.insert(to.clone());
            }
            WorldEvent::EntitiesMerged { survivor, absorbed } => {
                if !self.known.contains(survivor) || !self.known.contains(absorbed) {
                    return;
                }
                if self.live.iter().any(|(a, _)| a == absorbed) {
                    return;
                }
                let roots = self.roots();
                if roots[survivor] == roots[absorbed] {
                    return;
                }
                self.live.push((absorbed.clone(), survivor.clone()));
            }
            WorldEvent::MergeRevoked { survivor, absorbed } => {
                self.live.retain(|(a, s)| !(a == absorbed && s == survivor));
            }
        }
    }
}

/// Every stored merge chain ends within `merges.len()` steps and never revisits an id.
fn assert_acyclic(w: &World) {
    for &start in w.merges().keys() {
        let mut seen = BTreeSet::from([start]);
        let mut cur = start;
        while let Some(&next) = w.merges().get(&cur) {
            assert!(seen.insert(next), "merge cycle through {start:?}");
            cur = next;
        }
        assert!(seen.len() <= w.merges().len() + 1);
    }
}

proptest! {
    #[test]
    fn identity_matches_the_reference_model(events in prop::collection::vec(arb_event(), 0..60)) {
        let mut world = World::default();
        let mut reference = Reference::default();
        let mut seen_ids: BTreeMap<NaturalKey, EntityId> = BTreeMap::new();

        for event in &events {
            world = fold_one(world, event);
            reference.apply(event);

            // Keys are write-once.
            for (k, id) in &seen_ids {
                prop_assert_eq!(world.keys().get(k), Some(id));
            }
            seen_ids.clone_from(world.keys());
            prop_assert_eq!(
                world.keys().keys().cloned().collect::<BTreeSet<_>>(),
                reference.known.clone()
            );

            assert_acyclic(&world);

            let roots = reference.roots();
            for (k, id) in world.keys() {
                let expected = world.keys()[&roots[k]];
                prop_assert_eq!(world.resolve(*id), expected, "key {:?} after {:?}", k, event);
            }
        }
    }

    #[test]
    fn a_serialized_prefix_resumes_to_the_same_world(
        events in prop::collection::vec(arb_event(), 0..60),
        split in any::<prop::sample::Index>(),
        cap in 1u64..4,
    ) {
        let k = split.index(events.len() + 1);
        let whole = fold(World::with_hub_cap(cap), &events);

        let prefix = fold(World::with_hub_cap(cap), &events[..k]);
        let json = serde_json::to_string(&prefix).unwrap();
        let resumed: World = serde_json::from_str(&json).unwrap();
        let resumed = fold(resumed, &events[k..]);

        prop_assert_eq!(&resumed, &whole);
        prop_assert_eq!(
            serde_json::to_string_pretty(&resumed).unwrap(),
            serde_json::to_string_pretty(&whole).unwrap()
        );
    }
}
