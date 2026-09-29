//! Seeded synthetic `WorldEvent` generator for the scale measurements (s2w#32).
//!
//! Shared by `benches/scale_ir.rs`, `benches/scale_wall.rs` and `tests/scale_mem.rs` through
//! `#[path]`, so every measurement folds the same events. It is not shipped code and adds no
//! crate-graph edge: `s2w-app` already depends on `s2w-model`.
//!
//! The distributions are chosen, not observed; no recorded stream sample is committed. The
//! choices, all seeded by [`SEED`] through a hand-rolled splitmix64 (no `rand` dependency):
//!
//! - Entity-observed events: each one mints a new entity. The key is a 20-byte string, about
//!   the length of a typical page title in the recorded fixtures. The type is one of four
//!   generic names (`kind_a` to `kind_d`), uniform.
//! - Attributes per entity: 1 to [`MAX_ATTRS`], log-uniform (Zipf with exponent 1): most
//!   entities carry one or two attributes, a few carry many. Mean about 3.2.
//! - Attribute names come from a pool of [`ATTR_NAMES`] generic names, also Zipf-skewed, so a
//!   few names dominate. Values are a third each text (8 to 32 bytes), integer and flag.
//! - Relationship-observed events: the source endpoint is uniform over the already-generated
//!   entities; the target is Zipf-skewed over them, so a few hub entities collect most edges.
//!   The kind is one of three generic names. They never mint an entity, so their heap cost
//!   is separable from the entities' (issue #32 plan, §1.4).

use std::collections::BTreeMap;

use s2w_model::{AttrValue, NaturalKey, WorldEvent};

/// The seed every measurement uses. Changing it changes every measured number.
pub(crate) const SEED: u64 = 0x5332_5733_3200_0001;

/// Events folded by the instruction-count benchmark.
pub(crate) const IR_EVENTS: usize = 100_000;

/// Entity-observed events folded by the memory test's stage (a).
pub(crate) const MEM_ENTITIES: usize = 100_000;

/// Relationship-observed events folded by the memory test's stage (b).
pub(crate) const MEM_RELATIONSHIPS: usize = 100_000;

/// Events the wall-clock append benchmark writes, one transaction each.
pub(crate) const WALL_EVENTS: usize = 2_000;

/// The most attributes one entity-observed event carries.
const MAX_ATTRS: usize = 8;

/// How many distinct attribute names exist.
const ATTR_NAMES: usize = 16;

/// Entity types, uniform.
const ENTITY_TYPES: [&str; 4] = ["kind_a", "kind_b", "kind_c", "kind_d"];

/// Relationship kinds, uniform.
const RELATIONSHIP_KINDS: [&str; 3] = ["rel_a", "rel_b", "rel_c"];

/// A splitmix64 stream: tiny, seedable, and good enough for test data.
pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    /// A stream starting from `seed`.
    pub(crate) const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64 random bits.
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`; 0 when `n` is 0.
    fn below(&mut self, n: usize) -> usize {
        let n64 = u64::try_from(n).unwrap_or(u64::MAX);
        if n64 == 0 {
            return 0;
        }
        usize::try_from(self.next_u64() % n64).unwrap_or(0)
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        // The top 53 bits fill an f64 mantissa exactly.
        (self.next_u64() >> 11) as f64 / (1_u64 << 53) as f64
    }

    /// Log-uniform in `0..n` (Zipf with exponent 1): `r` is drawn with probability
    /// `log(1 + 1/(r+1)) / log(n+1)`, roughly proportional to `1/(r+1)`. 0 when `n` is 0.
    fn zipf(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        // `(n+1)^u` for `u` in `[0, 1)` lies in `[1, n+1)`, so its floor is a rank in `1..=n`.
        let rank = ((n + 1) as f64).powf(self.unit()) as usize;
        rank.saturating_sub(1).min(n - 1)
    }
}

/// The natural key of generated entity `index`, 20 bytes.
pub(crate) fn entity_key(index: usize) -> NaturalKey {
    NaturalKey::new(format!("entity-{index:013}"))
}

/// One attribute value: text, integer or flag, a third each.
fn attr_value(rng: &mut SplitMix64) -> AttrValue {
    match rng.below(3) {
        0 => {
            let length = 8 + rng.below(25);
            AttrValue::Str("v".repeat(length))
        }
        1 => AttrValue::Int(i64::try_from(rng.next_u64() >> 1).unwrap_or(0)),
        _ => AttrValue::Bool(rng.below(2) == 1),
    }
}

/// `count` entity-observed events, each minting a new entity (keys `entity_key(0..count)`).
pub(crate) fn entity_events(count: usize, seed: u64) -> Vec<WorldEvent> {
    let mut rng = SplitMix64::new(seed);
    (0..count)
        .map(|index| {
            let wanted = 1 + rng.zipf(MAX_ATTRS);
            let mut attrs = BTreeMap::new();
            // Zipf-drawn names can repeat; retry a bounded number of times so the count stays
            // close to `wanted` without looping forever.
            for _ in 0..wanted * 4 {
                if attrs.len() == wanted {
                    break;
                }
                let name = format!("attr_{:02}", rng.zipf(ATTR_NAMES));
                attrs.insert(name, attr_value(&mut rng));
            }
            WorldEvent::EntityObserved {
                key: entity_key(index),
                entity_type: ENTITY_TYPES[rng.below(ENTITY_TYPES.len())].to_owned(),
                attrs,
            }
        })
        .collect()
}

/// `count` relationship-observed events between the first `entities` generated entities. They
/// never mint an entity when those entities were folded first.
pub(crate) fn relationship_events(count: usize, entities: usize, seed: u64) -> Vec<WorldEvent> {
    let mut rng = SplitMix64::new(seed ^ 0xA5A5_A5A5_A5A5_A5A5);
    (0..count)
        .map(|_| WorldEvent::RelationshipObserved {
            from: entity_key(rng.below(entities)),
            to: entity_key(rng.zipf(entities)),
            kind: RELATIONSHIP_KINDS[rng.below(RELATIONSHIP_KINDS.len())].to_owned(),
        })
        .collect()
}

/// The instruction-count benchmark's mix: 80% entity-observed (new entities), then 20%
/// relationship-observed events among them, `total` events in all.
pub(crate) fn mixed_events(total: usize, seed: u64) -> Vec<WorldEvent> {
    let entities = total - total / 5;
    let mut events = entity_events(entities, seed);
    events.extend(relationship_events(total / 5, entities, seed));
    events
}
