//! Synthetic streams with neutral one-letter names. Each field is built to exercise one role:
//! `e` names the event, `t` is a sequence, `a` (aliased at `x.a`) and `d.b` (inside a decoded
//! string) are entities with attributes `an` and `d.bn`, `a` determines `d.b`, `c` and `cc`
//! determine each other, `g` and `h` sit in the grey uniqueness band (only `h` has a dependent,
//! `hn`, and passes the entity test but is too unique to key a type), `r` repeats with
//! nothing depending on it, `k` explains when `o` is present.

use std::collections::BTreeMap;

use s2w_model::{EntityRule, FieldPath, Segment, StreamMapping};
use serde_json::{Value, json};

use super::*;

/// Deterministic pseudo-random values (a 64-bit LCG), so the test needs no randomness crate.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

fn stream(n: u64) -> Vec<Value> {
    let mut rng = Lcg(7);
    (0..n)
        .map(|i| {
            let a = rng.below(60);
            let b = a % 15;
            let c = rng.below(30);
            let g = if i % 14 == 13 { i - 10 } else { i };
            let h = if i % 14 == 6 { i - 5 } else { i };
            let r = rng.below(300);
            let k = ["p", "q", "r"][usize::try_from(i % 3).unwrap()];
            let inner = json!({"b": format!("b{b}"), "bn": format!("m{}", b / 2)});
            let mut event = json!({
                "e": format!("e{i}"),
                "t": 1_000 + i / 3,
                "a": format!("a{a}"),
                "x": {"a": format!("a{a}")},
                "an": format!("n{}", a / 2),
                "d": inner.to_string(),
                "c": c,
                "cc": format!("c{}", c * 7 + 1),
                "g": g,
                "h": h,
                "hn": h / 2,
                "r": r,
                "k": k,
            });
            if i % 3 == 0 {
                event["o"] = json!(format!("o{}", rng.below(50)));
            }
            event
        })
        .collect()
}

fn run(events: &[Value], extra: &[&[u8]]) -> (Profile, Discovery) {
    run_with(events, extra, &Config::default())
}

fn run_with(events: &[Value], extra: &[&[u8]], cfg: &Config) -> (Profile, Discovery) {
    let bytes: Vec<Vec<u8>> = events.iter().map(|v| v.to_string().into_bytes()).collect();
    let mut refs: Vec<&[u8]> = bytes.iter().map(Vec::as_slice).collect();
    refs.extend_from_slice(extra);
    discover(&refs, cfg)
}

fn path(keys: &[&str]) -> FieldPath {
    FieldPath(keys.iter().map(|k| Segment::Key((*k).to_owned())).collect())
}

fn role(profile: &Profile, keys: &[&str]) -> Role {
    let p = path(keys);
    profile
        .paths
        .iter()
        .find(|x| x.path == p)
        .map(|x| x.role)
        .expect("path profiled")
}

fn mapping(discovery: Discovery) -> StreamMapping {
    match discovery {
        Discovery::Mapping(m) => m,
        Discovery::Abstain(reason) => panic!("abstained: {reason}"),
    }
}

fn entity<'m>(m: &'m StreamMapping, id: &str) -> &'m EntityRule {
    m.entities
        .iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("no entity {id}"))
}

#[test]
fn roles_separate_event_ids_sequences_entities_and_the_grey_band() {
    let (profile, _) = run(&stream(1200), &[]);
    assert_eq!(role(&profile, &["e"]), Role::EventId);
    assert_eq!(role(&profile, &["t"]), Role::Sequence);
    assert_eq!(role(&profile, &["a"]), Role::Entity);
    assert_eq!(role(&profile, &["d", "b"]), Role::Entity);
    assert_eq!(role(&profile, &["g"]), Role::GreyUniqueness);
    assert_eq!(role(&profile, &["h"]), Role::NearUnique);
    assert_eq!(role(&profile, &["r"]), Role::NoDependents);
    assert_eq!(role(&profile, &["k"]), Role::FewGroups);
}

#[test]
fn aliases_share_one_label_and_attributes_follow_the_key() {
    let m = mapping(run(&stream(1200), &[]).1);
    let (a, xa) = (entity(&m, "a"), entity(&m, "x.a"));
    assert_eq!(a.type_label, "a+x/a");
    assert_eq!(xa.type_label, a.type_label);
    assert!(a.attrs.iter().any(|x| x.name == "an"), "{:?}", a.attrs);
    assert!(entity(&m, "d.b").attrs.iter().any(|x| x.name == "d.bn"));
    assert!(
        m.entities
            .iter()
            .all(|e| e.id != "e" && e.id != "t" && e.id != "g")
    );
}

/// A stream where `s` (six values, with `f` = its family, three values) decides which of the
/// optional fields `u` and `v` an event carries, and `w` is carried only by `s`'s first value.
/// `b` (20 values, attribute `bn`) and `w` (8 values, attribute `wn`) are entities whose values
/// decide nothing about shape. With `stray`, one event in ten of `s`'s last value also carries
/// `u`, so that value's group is no longer all-or-none.
fn shaped(n: u64, stray: bool) -> Vec<Value> {
    let mut rng = Lcg(11);
    (0..n)
        .map(|i| {
            let s = rng.below(6);
            let b = rng.below(20);
            let mut event = json!({
                "s": format!("s{s}"),
                "f": format!("f{}", s / 2),
                "b": format!("b{b}"),
                "bn": format!("n{}", b / 2),
            });
            if matches!(s, 0 | 2) || (stray && s == 5 && i.is_multiple_of(10)) {
                event["u"] = json!(format!("u{}", rng.below(40)));
            }
            if matches!(s, 1 | 2) {
                event["v"] = json!(format!("v{}", rng.below(40)));
            }
            if s == 0 {
                let w = rng.below(8);
                event["w"] = json!(format!("w{w}"));
                event["wn"] = json!(format!("m{}", w / 2));
            }
            event
        })
        .collect()
}

/// `devices` ids (attribute `dn`), each of which always or never carries `z`.
fn devices(n: u64, devices: u64) -> Vec<Value> {
    let mut rng = Lcg(13);
    (0..n)
        .map(|_| {
            let d = rng.below(devices);
            let mut event = json!({"d": format!("d{d}"), "dn": format!("n{}", d / 2)});
            if d.is_multiple_of(2) {
                event["z"] = json!(format!("z{}", rng.below(40)));
            }
            event
        })
        .collect()
}

#[test]
fn a_small_key_whose_values_decide_the_event_shape_is_a_category_not_a_type() {
    let (profile, discovery) = run(&shaped(1200, false), &[]);
    assert_eq!(role(&profile, &["s"]), Role::Category);
    assert_eq!(role(&profile, &["b"]), Role::Entity);
    // No optional path among the events that carry `w`: it cannot decide their shape.
    assert_eq!(role(&profile, &["w"]), Role::Entity);
    let m = mapping(discovery);
    assert!(m.entities.iter().all(|e| e.id != "s"), "s keys no type");
    assert!(entity(&m, "b").attrs.iter().any(|a| a.name == "bn"));
}

#[test]
fn one_value_that_is_not_all_or_none_keeps_the_key_an_entity() {
    let (profile, _) = run(&shaped(1200, true), &[]);
    assert_eq!(role(&profile, &["s"]), Role::Entity);
}

#[test]
fn only_a_key_with_at_most_category_max_values_can_be_a_category() {
    // Every device always or never carries `z`. Past `category_max` values it stays an entity;
    // at or below it, it reads as a category: the accepted false demotion (decision 0022).
    let (many, _) = run(&devices(3000, 60), &[]);
    assert_eq!(role(&many, &["d"]), Role::Entity);
    let (few, _) = run(&devices(3000, 20), &[]);
    assert_eq!(role(&few, &["d"]), Role::Category);
}

#[test]
fn the_category_max_boundary_is_inclusive() {
    let (at, _) = run(&devices(3000, 32), &[]);
    assert_eq!(role(&at, &["d"]), Role::Category);
    let (past, _) = run(&devices(3000, 33), &[]);
    assert_eq!(role(&past, &["d"]), Role::Entity);
}

/// How `recurring` lays out `q`'s values and chooses its follower `y`.
#[derive(Clone, Copy)]
enum Recur {
    /// Each of `q`'s 30 values is drawn at random, so its events come back apart across the
    /// stream; `y` (six values) is constant under 26 of them.
    Spread,
    /// As `Spread`, but each value of `q` is confined to a stretch of 200 events (a burst),
    /// alternating with one other value.
    Bunched,
    /// As `Spread`, but one value of `y` is carried by about two thirds of the events.
    NearConstant,
    /// As `Spread`, and `q`'s even values always carry `o`, its odd values never.
    Shaped,
}

/// `q` recurs; `y` follows it in 26 of its 30 repeat groups but with six values over 26
/// constant groups is not informative, so `q` fails the first entity test (s2w#250 PR 2).
fn recurring(n: u64, mode: Recur) -> Vec<Value> {
    let mut rng = Lcg(17);
    (0..n)
        .map(|i| {
            let q = match mode {
                Recur::Bunched => 2 * (i / 200) + rng.below(2),
                _ => rng.below(30),
            };
            let noise = rng.below(6);
            let y = match mode {
                Recur::NearConstant if q % 3 != 0 => "y-common".to_owned(),
                Recur::NearConstant => format!("y{}", (q / 3) % 5),
                _ if q < 4 => format!("y{noise}"),
                _ => format!("y{}", q % 6),
            };
            let mut event = json!({"q": format!("q{q}"), "y": y});
            if matches!(mode, Recur::Shaped) && q % 2 == 0 {
                event["o"] = json!(format!("o{}", rng.below(40)));
            }
            event
        })
        .collect()
}

#[test]
fn a_key_that_recurs_apart_and_is_followed_by_a_varying_path_is_an_entity() {
    let (profile, discovery) = run(&recurring(3000, Recur::Spread), &[]);
    assert_eq!(role(&profile, &["q"]), Role::Entity);
    assert_eq!(role(&profile, &["y"]), Role::NoDependents);
    let m = mapping(discovery);
    entity(&m, "q");
}

#[test]
fn a_key_whose_repeats_are_bursts_has_no_dependents() {
    let (profile, _) = run(&recurring(3000, Recur::Bunched), &[]);
    assert_eq!(role(&profile, &["q"]), Role::NoDependents);
}

#[test]
fn a_key_followed_only_by_a_near_constant_path_has_no_dependents() {
    let (profile, _) = run(&recurring(3000, Recur::NearConstant), &[]);
    assert_eq!(role(&profile, &["q"]), Role::NoDependents);
}

#[test]
fn a_small_shape_deciding_key_that_passes_only_the_second_test_is_a_category() {
    let (profile, _) = run(&recurring(3000, Recur::Shaped), &[]);
    assert_eq!(role(&profile, &["q"]), Role::Category);
}

/// 1000 events. `q` has 20 values, each followed by `y` (six values, so not informative).
/// Five values are carried exactly twice, 100 events apart (a tenth of the stream); fifteen
/// come back three events apart. Every other event carries only `x`.
fn spread_boundary() -> Vec<Value> {
    let mut at: BTreeMap<u64, u64> = BTreeMap::new();
    for j in 0..5 {
        at.insert(10 * j, j);
        at.insert(10 * j + 100, j);
    }
    for t in 0..5 {
        for k in 0..3 {
            let q = 5 + 3 * t + k;
            at.insert(300 + 10 * t + k, q);
            at.insert(303 + 10 * t + k, q);
        }
    }
    (0..1000)
        .map(|i| match at.get(&i) {
            Some(q) => json!({"q": format!("q{q}"), "y": format!("y{}", q % 6)}),
            None => json!({"x": format!("x{}", i % 7)}),
        })
        .collect()
}

#[test]
fn the_spread_thresholds_are_inclusive() {
    let events = spread_boundary();
    let exact = Config::default();
    assert_eq!((exact.spread_groups_pct, exact.spread_window_pct), (25, 10));
    assert_eq!(
        role(&run_with(&events, &[], &exact).0, &["q"]),
        Role::Entity
    );
    let wider = Config {
        spread_window_pct: 11,
        ..Config::default()
    };
    assert_eq!(
        role(&run_with(&events, &[], &wider).0, &["q"]),
        Role::NoDependents
    );
    let more = Config {
        spread_groups_pct: 26,
        ..Config::default()
    };
    assert_eq!(
        role(&run_with(&events, &[], &more).0, &["q"]),
        Role::NoDependents
    );
}

/// `shaped` plus `p` (60 values, each fixing `s`), which recurs and is followed by `s` (six
/// values, not informative over 60 groups): `s` is a `Category` and `p` passes only the
/// second entity test.
fn shaped_with_recurring_key(n: u64) -> Vec<Value> {
    let mut rng = Lcg(19);
    shaped(n, false)
        .into_iter()
        .map(|mut event| {
            let s: u64 = event["s"].as_str().unwrap()[1..].parse().unwrap();
            event["p"] = json!(format!("p{}", s + 6 * rng.below(10)));
            event
        })
        .collect()
}

#[test]
fn a_category_is_still_another_types_attribute() {
    let (profile, discovery) = run(&shaped_with_recurring_key(1200), &[]);
    assert_eq!(role(&profile, &["s"]), Role::Category);
    assert_eq!(role(&profile, &["p"]), Role::Entity);
    let m = mapping(discovery);
    assert!(entity(&m, "p").attrs.iter().any(|a| a.name == "s"), "{m:?}");
}

#[test]
fn a_near_unique_key_names_no_type_unless_the_threshold_allows_it() {
    let m = mapping(run(&stream(1200), &[]).1);
    assert!(m.entities.iter().all(|e| e.id != "h"), "{:?}", m.entities);
    assert!(
        m.relationships.iter().all(|r| r.from != "h" && r.to != "h"),
        "{:?}",
        m.relationships
    );
    // The threshold is what removed it: above 100% no path is near-unique, and `h` keys a type.
    let bytes: Vec<Vec<u8>> = stream(1200)
        .iter()
        .map(|v| v.to_string().into_bytes())
        .collect();
    let refs: Vec<&[u8]> = bytes.iter().map(Vec::as_slice).collect();
    let cfg = Config {
        type_uniqueness_pct: 101,
        ..Config::default()
    };
    let (profile, discovery) = discover(&refs, &cfg);
    assert_eq!(role(&profile, &["h"]), Role::Entity);
    assert!(
        entity(&mapping(discovery), "h")
            .attrs
            .iter()
            .any(|x| x.name == "hn")
    );
}

#[test]
fn the_near_unique_threshold_is_inclusive_on_the_rounded_down_ratio() {
    // `h` is 92% distinct (integer, rounded down): 92 makes it near-unique, 93 lets it key a type.
    let bytes: Vec<Vec<u8>> = stream(1200)
        .iter()
        .map(|v| v.to_string().into_bytes())
        .collect();
    let refs: Vec<&[u8]> = bytes.iter().map(Vec::as_slice).collect();
    for (pct, want) in [(92, Role::NearUnique), (93, Role::Entity)] {
        let cfg = Config {
            type_uniqueness_pct: pct,
            ..Config::default()
        };
        let (profile, _) = discover(&refs, &cfg);
        assert_eq!(role(&profile, &["h"]), want, "type_uniqueness_pct {pct}");
    }
}

#[test]
fn one_to_one_classes_merge_under_the_integer_key() {
    let m = mapping(run(&stream(1200), &[]).1);
    assert!(m.entities.iter().all(|e| e.id != "cc"));
    assert!(entity(&m, "c").attrs.iter().any(|x| x.name == "cc"));
}

#[test]
fn many_to_one_points_from_the_many_side() {
    let m = mapping(run(&stream(1200), &[]).1);
    for from in ["a", "x.a"] {
        assert!(
            m.relationships
                .iter()
                .any(|r| r.from == from && r.to == "d.b" && r.kind == "n:1"),
            "{:?}",
            m.relationships
        );
    }
    assert!(
        !m.relationships
            .iter()
            .any(|r| r.from == "d.b" && r.to == "a")
    );
}

#[test]
fn decode_steps_event_type_and_skipped_payloads_are_reported() {
    let (profile, discovery) = run(&stream(1200), &[b"not json", b"[1,2]"]);
    assert_eq!(profile.skipped, 2);
    assert_eq!(profile.events, 1200);
    assert_eq!(profile.decode, vec![path(&["d"])]);
    assert_eq!(profile.event_type, Some(path(&["k"])));
    assert_eq!(mapping(discovery).decode, vec![path(&["d"])]);
}

#[test]
fn too_few_events_abstains() {
    let (_, discovery) = run(&stream(999), &[]);
    assert!(
        matches!(discovery, Discovery::Abstain(ref r) if r.contains("999")),
        "{discovery:?}"
    );
}

#[test]
fn no_identifier_abstains() {
    let events: Vec<Value> = (0..1200)
        .map(|i| json!({"e": format!("e{i}"), "t": i / 3}))
        .collect();
    assert!(matches!(run(&events, &[]).1, Discovery::Abstain(_)));
}

#[test]
fn same_input_same_output() {
    let events = stream(1200);
    assert_eq!(run(&events, &[]), run(&events, &[]));
}

#[test]
fn ids_and_labels_escape_their_separators() {
    assert_ne!(rule_id(&path(&["a.b"])), rule_id(&path(&["a", "b"])));
    assert_eq!(rule_id(&path(&["a.b", "c\\"])), "a\\.b.c\\\\");
    let labels = type_labels(&[vec![path(&["a+b"])], vec![path(&["a"]), path(&["b"])]]);
    assert_ne!(labels[0], labels[1]);
    let clash = type_labels(&[vec![path(&["p", "q", "r"])], vec![path(&["s", "q", "r"])]]);
    assert_eq!(clash, vec!["p/q/r", "s/q/r"]);
}

/// Renames every key (reversing their sort order) and hashes every string, inside the decoded
/// string too: the obfuscation `cargo xtask check` 12 applies to the recorded fixture.
struct Obfuscate(BTreeMap<String, String>);

impl Obfuscate {
    fn new(events: &[Value]) -> Self {
        fn keys(v: &Value, out: &mut std::collections::BTreeSet<String>) {
            if let Value::Object(map) = v {
                for (k, inner) in map {
                    out.insert(k.clone());
                    keys(inner, out);
                }
            } else if let Value::String(s) = v
                && let Ok(inner) = serde_json::from_str::<Value>(s)
            {
                keys(&inner, out);
            }
        }
        let mut all = std::collections::BTreeSet::new();
        events.iter().for_each(|e| keys(e, &mut all));
        let n = all.len();
        Self(
            all.into_iter()
                .enumerate()
                .map(|(i, k)| (k, format!("f{:03}", n - i)))
                .collect(),
        )
    }

    fn value(&self, v: &Value) -> Value {
        match v {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, x)| (self.0[k].clone(), self.value(x)))
                    .collect(),
            ),
            Value::String(s) => match serde_json::from_str::<Value>(s) {
                Ok(inner @ Value::Object(_)) => Value::String(self.value(&inner).to_string()),
                _ => Value::String(format!("h{:016x}", fnv(s))),
            },
            other => other.clone(),
        }
    }

    fn path(&self, p: &FieldPath) -> FieldPath {
        FieldPath(
            p.0.iter()
                .map(|s| match s {
                    Segment::Key(k) => Segment::Key(self.0[k].clone()),
                    Segment::Index(i) => Segment::Index(*i),
                })
                .collect(),
        )
    }

    /// `m` as the profiler must propose it for the obfuscated stream: renamed paths, with ids,
    /// labels and names re-derived from them, in canonical order.
    fn mapping(&self, m: &StreamMapping) -> StreamMapping {
        let mut classes: BTreeMap<&str, Vec<FieldPath>> = BTreeMap::new();
        for e in &m.entities {
            classes
                .entry(&e.type_label)
                .or_default()
                .push(self.path(&e.key[0]));
        }
        let renamed: Vec<Vec<FieldPath>> = classes.values().cloned().collect();
        let labels: BTreeMap<&str, String> =
            classes.keys().copied().zip(type_labels(&renamed)).collect();
        let ids: BTreeMap<&str, String> = m
            .entities
            .iter()
            .map(|e| (e.id.as_str(), rule_id(&self.path(&e.key[0]))))
            .collect();
        let mut out = m.clone();
        out.decode = m.decode.iter().map(|p| self.path(p)).collect();
        for e in &mut out.entities {
            e.id.clone_from(&ids[e.id.as_str()]);
            e.type_label.clone_from(&labels[e.type_label.as_str()]);
            e.key = vec![self.path(&e.key[0])];
            for a in &mut e.attrs {
                a.path = self.path(&a.path);
                a.name = rule_id(&a.path);
            }
        }
        for r in &mut out.relationships {
            r.from.clone_from(&ids[r.from.as_str()]);
            r.to.clone_from(&ids[r.to.as_str()]);
        }
        canonical(out)
    }
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn canonical(mut m: StreamMapping) -> StreamMapping {
    m.entities.sort_by(|a, b| a.id.cmp(&b.id));
    for e in &mut m.entities {
        e.attrs.sort_by(|a, b| a.name.cmp(&b.name));
    }
    m.relationships
        .sort_by(|a, b| (&a.from, &a.to, &a.kind).cmp(&(&b.from, &b.to, &b.kind)));
    m
}

#[test]
fn renaming_keys_and_hashing_strings_only_renames_the_mapping() {
    let plain = stream(1200);
    let obf = Obfuscate::new(&plain);
    let hidden: Vec<Value> = plain.iter().map(|v| obf.value(v)).collect();
    let (pa, a) = run(&plain, &[]);
    let (pb, b) = run(&hidden, &[]);
    let a = mapping(a);
    assert!(
        a.entities.len() >= 4 && !a.relationships.is_empty(),
        "vacuous: {a:?}"
    );
    assert_eq!(canonical(mapping(b)), obf.mapping(&a));
    assert_eq!(pb.event_type, pa.event_type.as_ref().map(|p| obf.path(p)));
}

#[test]
fn renaming_is_invariant_with_a_category_and_a_second_test_entity() {
    let plain = shaped_with_recurring_key(1200);
    let obf = Obfuscate::new(&plain);
    let hidden: Vec<Value> = plain.iter().map(|v| obf.value(v)).collect();
    let (pa, a) = run(&plain, &[]);
    let (pb, b) = run(&hidden, &[]);
    assert_eq!(role(&pa, &["s"]), Role::Category);
    assert_eq!(role(&pa, &["p"]), Role::Entity);
    let a = mapping(a);
    assert_eq!(canonical(mapping(b)), obf.mapping(&a));
    assert_eq!(pb.event_type, pa.event_type.as_ref().map(|p| obf.path(p)));
}

/// The base stream plus `n`, unique per event, and `w`, in 90% of events: for `share` in 100 of
/// events it carries the `n` of the event five back (each `n` is carried at most once, so `w`
/// stays unique too), otherwise a value never seen at `n` (a reference to before the window).
fn carried(n: u64, share: u64) -> Vec<Value> {
    let mut events = stream(n);
    for (i, event) in (0u64..).zip(events.iter_mut()) {
        event["n"] = json!(format!("i{}", 10_000 + i));
        if i % 10 != 0 {
            event["w"] = if i % 100 < share && i >= 5 {
                json!(format!("i{}", 10_000 + i - 5))
            } else {
                json!(format!("z{i}"))
            };
        }
    }
    events
}

fn containment<'p>(
    profile: &'p Profile,
    referrer: &str,
    referenced: &str,
) -> Option<&'p Containment> {
    let (a, b) = (path(&[referrer]), path(&[referenced]));
    profile
        .contained
        .iter()
        .find(|c| c.referrer == a && c.referenced == b)
}

#[test]
fn a_carried_identifier_joins_its_source_in_one_type() {
    let (profile, discovery) = run(&carried(1200, 30), &[]);
    assert_eq!(role(&profile, &["n"]), Role::EventId);
    assert_eq!(role(&profile, &["w"]), Role::EventId);
    let link = containment(&profile, "w", "n").expect("measured");
    assert!(link.accepted, "{link:?}");
    assert_eq!(link.carry_pct, 100);
    let reverse = containment(&profile, "n", "w").expect("both directions measured");
    assert!(!reverse.accepted && reverse.carry_pct == 0, "{reverse:?}");
    let m = mapping(discovery);
    assert_eq!(entity(&m, "n").type_label, "n+w");
    assert_eq!(entity(&m, "w").type_label, "n+w");
    assert!(
        entity(&m, "n").attrs.is_empty(),
        "a unique key has no repeat groups"
    );
}

#[test]
fn stage_5b_thresholds_are_inclusive() {
    let events = carried(1200, 30);
    let (profile, _) = run(&events, &[]);
    let link = containment(&profile, "w", "n").expect("measured").clone();
    let at = |cfg: Config| {
        let (p, _) = run_with(&events, &[], &cfg);
        containment(&p, "w", "n").expect("measured").accepted
    };
    let cfg = Config::default;
    assert!(at(Config {
        contain_pct: link.coverage_pct,
        ..cfg()
    }));
    assert!(!at(Config {
        contain_pct: link.coverage_pct + 1,
        ..cfg()
    }));
    assert!(at(Config {
        carry_pct: link.carry_pct,
        ..cfg()
    }));
    assert!(!at(Config {
        carry_pct: link.carry_pct + 1,
        ..cfg()
    }));
}

#[test]
fn a_few_carried_values_or_a_capped_comparison_links_nothing() {
    let (profile, discovery) = run(&carried(1200, 5), &[]);
    let link = containment(&profile, "w", "n").expect("measured");
    assert!(!link.accepted && link.coverage_pct < 10, "{link:?}");
    assert!(!mapping(discovery).entities.iter().any(|e| e.id == "w"));
    let cfg = Config {
        contain_cap: 5,
        ..Config::default()
    };
    let (profile, _) = run_with(&carried(1200, 30), &[], &cfg);
    assert!(
        containment(&profile, "w", "n").is_none(),
        "under min_support"
    );
}

#[test]
fn chance_overlap_without_carry_order_links_nothing() {
    // Two unique identifiers drawn from one pool in unrelated orders: the sets overlap almost
    // entirely, but a shared value is first seen at either path about equally often.
    let mut events = stream(1200);
    for (i, event) in (0u64..).zip(events.iter_mut()) {
        event["u"] = json!(format!("v{}", (i * 7919) % 1201));
        event["v"] = json!(format!("v{}", (i * 104_729 + 13) % 1201));
    }
    let (profile, discovery) = run(&events, &[]);
    for (a, b) in [("u", "v"), ("v", "u")] {
        let c = containment(&profile, a, b).expect("measured");
        assert!(c.coverage_pct >= 90 && !c.accepted, "{c:?}");
    }
    assert!(
        !mapping(discovery)
            .entities
            .iter()
            .any(|e| e.id == "u" || e.id == "v")
    );
}

#[test]
fn per_event_aliases_are_not_containment() {
    let (profile, _) = run(&stream(1200), &[]);
    let c = profile
        .contained
        .iter()
        .find(|c| c.referrer == path(&["a"]) && c.referenced == path(&["x", "a"]))
        .expect("alias pair measured");
    assert_eq!((c.carry_pct, c.accepted), (0, false));
}

#[test]
fn two_offset_counters_are_the_accepted_false_class() {
    // Decision 0022 v5: a counter that runs ahead of another passes the carry test with no
    // reference between them. Named, not fixed: no pair like it appears on the dev windows.
    let mut events = stream(1200);
    for (i, event) in (0u64..).zip(events.iter_mut()) {
        event["u"] = json!(i);
        event["v"] = json!(i + 50);
    }
    let (profile, discovery) = run(&events, &[]);
    assert!(containment(&profile, "u", "v").expect("measured").accepted);
    let m = mapping(discovery);
    assert_eq!(entity(&m, "u").type_label, entity(&m, "v").type_label);
}

#[test]
fn renaming_is_invariant_with_containment() {
    let plain = carried(1200, 30);
    let obf = Obfuscate::new(&plain);
    let hidden: Vec<Value> = plain.iter().map(|v| obf.value(v)).collect();
    let (pa, a) = run(&plain, &[]);
    let (pb, b) = run(&hidden, &[]);
    let a = mapping(a);
    assert_eq!(entity(&a, "n").type_label, entity(&a, "w").type_label);
    assert_eq!(canonical(mapping(b)), obf.mapping(&a));
    let accepted = |p: &Profile| p.contained.iter().filter(|c| c.accepted).count();
    assert_eq!(accepted(&pa), accepted(&pb));
}
