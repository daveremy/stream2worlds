//! Synthetic streams with neutral one-letter names. Each field is built to exercise one role:
//! `e` names the event, `t` is a sequence, `a` (aliased at `x.a`) and `d.b` (inside a decoded
//! string) are entities with attributes `an` and `d.bn`, `a` determines `d.b`, `c` and `cc`
//! determine each other, `g` and `h` sit in the grey uniqueness band (only `h` has a dependent,
//! `hn`), `r` repeats with nothing depending on it, `k` explains when `o` is present.

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
    let bytes: Vec<Vec<u8>> = events.iter().map(|v| v.to_string().into_bytes()).collect();
    let mut refs: Vec<&[u8]> = bytes.iter().map(Vec::as_slice).collect();
    refs.extend_from_slice(extra);
    discover(&refs, &Config::default())
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
    assert_eq!(role(&profile, &["h"]), Role::Entity);
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
