use super::*;
use crate::raw;
use s2w_model::{AttrRule, FieldPath, KEY_SEPARATOR, MAPPING_VERSION, RelationshipRule, Segment};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SEP: char = KEY_SEPARATOR;

fn path(keys: &[&str]) -> FieldPath {
    FieldPath(keys.iter().map(|k| Segment::Key((*k).to_owned())).collect())
}

fn rule(id: &str, label: &str, key: &[&[&str]]) -> EntityRule {
    EntityRule {
        id: id.to_owned(),
        type_label: label.to_owned(),
        key: key.iter().map(|p| path(p)).collect(),
        attrs: vec![],
    }
}

/// A two-rule mapping over a decoded `body` string: `a.id` and `b.id`, related `a` → `b`.
fn mapping() -> StreamMapping {
    let mut a = rule("ra", "ta", &[&["body", "a", "id"]]);
    a.attrs = vec![
        AttrRule {
            name: "s".to_owned(),
            path: path(&["body", "a", "s"]),
        },
        AttrRule {
            name: "n".to_owned(),
            path: path(&["body", "a", "n"]),
        },
        AttrRule {
            name: "f".to_owned(),
            path: path(&["body", "a", "f"]),
        },
    ];
    StreamMapping {
        version: MAPPING_VERSION,
        decode: vec![path(&["body"])],
        entities: vec![
            a,
            rule("rb", "tb", &[&["body", "b", "ns"], &["body", "b", "id"]]),
        ],
        relationships: vec![RelationshipRule {
            from: "ra".to_owned(),
            to: "rb".to_owned(),
            kind: "k".to_owned(),
        }],
    }
}

fn engine() -> Result<MappingEngine, MappingEngineError> {
    MappingEngine::new(mapping())
}

/// Wraps `inner` JSON as the string-valued `body` of an envelope.
fn enveloped(inner: &str) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&serde_json::json!({ "body": inner, "id": "1" }))
}

fn key(parts: &[&str]) -> NaturalKey {
    NaturalKey::new(parts.join(&SEP.to_string()))
}

#[test]
fn a_decoded_envelope_yields_entities_attrs_and_a_relationship() -> TestResult {
    let payload = enveloped(r#"{"a":{"id":"x","s":"hi","n":3,"f":1.5},"b":{"ns":"n1","id":42}}"#)?;
    let verdict = engine()?.evaluate(&raw(&payload)?);
    let a = key(&["ta", r#""x""#]);
    let b = key(&["tb", r#""n1""#, "42"]);
    let expected = vec![
        WorldEvent::EntityObserved {
            key: a.clone(),
            entity_type: "ta".to_owned(),
            attrs: BTreeMap::from([
                ("n".to_owned(), AttrValue::Int(3)),
                ("s".to_owned(), AttrValue::Str("hi".to_owned())),
            ]),
        },
        WorldEvent::EntityObserved {
            key: b.clone(),
            entity_type: "tb".to_owned(),
            attrs: BTreeMap::new(),
        },
        WorldEvent::RelationshipObserved {
            from: a,
            to: b,
            kind: "k".to_owned(),
        },
    ];
    assert_eq!(
        verdict,
        Verdict::Propose {
            claims: expected,
            confidence: Confidence::CERTAIN,
        }
    );
    Ok(())
}

#[test]
fn one_matched_rule_proposes_without_the_relationship() -> TestResult {
    let payload = enveloped(r#"{"a":{"id":true}}"#)?;
    let Verdict::Propose { claims, .. } = engine()?.evaluate(&raw(&payload)?) else {
        return Err("expected a proposal".into());
    };
    assert_eq!(claims.len(), 1);
    assert!(
        matches!(&claims[0], WorldEvent::EntityObserved { key: k, .. } if *k == key(&["ta", "true"]))
    );
    Ok(())
}

#[test]
fn a_missing_path_or_non_scalar_key_matches_no_rule() -> TestResult {
    for inner in [
        r#"{"other":1}"#,
        r#"{"a":{"id":1.5}}"#,
        r#"{"a":{"id":null}}"#,
        r#"{"a":{"id":{"x":1}}}"#,
        r#"{"a":{"id":18446744073709551615}}"#,
        r#"{"b":{"id":1}}"#,
        "[1,2]",
    ] {
        let verdict = engine()?.evaluate(&raw(&enveloped(inner)?)?);
        assert_eq!(
            verdict,
            Verdict::Abstain {
                reason: AbstainReason::Insufficient("no entity rule matched".to_owned())
            },
            "{inner}"
        );
    }
    Ok(())
}

#[test]
fn bad_json_and_bad_decode_targets_are_unparseable() -> TestResult {
    let engine = engine()?;
    let not_json = raw(b"{")?;
    let body_not_json = raw(&enveloped("{nope")?)?;
    let body_not_string = raw(br#"{"body":{"a":{"id":"x"}}}"#)?;
    for event in [not_json, body_not_json, body_not_string] {
        assert!(matches!(
            engine.evaluate(&event),
            Verdict::Abstain {
                reason: AbstainReason::Unparseable(_)
            }
        ));
    }
    // An absent decode path is skipped, not an error: the payload is still read, and here
    // simply matches no rule.
    let bare = raw(br#"{"a":{"id":"x"}}"#)?;
    assert_eq!(
        engine.evaluate(&bare),
        Verdict::Abstain {
            reason: AbstainReason::Insufficient("no entity rule matched".to_owned())
        }
    );
    Ok(())
}

#[test]
fn keys_are_namespaced_by_type_and_by_json_type() -> TestResult {
    let mut m = mapping();
    m.decode.clear();
    m.relationships.clear();
    m.entities = vec![
        rule("r1", "t1", &[&["id"]]),
        rule("r2", "t2", &[&["id"]]),
        rule("r3", "t1", &[&["alt"]]),
    ];
    let engine = MappingEngine::new(m)?;
    let Verdict::Propose { claims, .. } = engine.evaluate(&raw(br#"{"id":"123","alt":123}"#)?)
    else {
        return Err("expected a proposal".into());
    };
    let keys: Vec<&NaturalKey> = claims
        .iter()
        .filter_map(|c| match c {
            WorldEvent::EntityObserved { key, .. } => Some(key),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys,
        [
            &key(&["t1", r#""123""#]),
            &key(&["t2", r#""123""#]),
            &key(&["t1", "123"]),
        ]
    );
    Ok(())
}

#[test]
fn array_indexes_and_string_escapes_are_handled() -> TestResult {
    let mut m = mapping();
    m.decode.clear();
    m.relationships.clear();
    m.entities = vec![EntityRule {
        id: "r".to_owned(),
        type_label: "t".to_owned(),
        key: vec![FieldPath(vec![
            Segment::Key("list".to_owned()),
            Segment::Index(1),
        ])],
        attrs: vec![],
    }];
    let engine = MappingEngine::new(m)?;
    let verdict = engine.evaluate(&raw("{\"list\":[\"a\",\"b\\u001fc\"]}".as_bytes())?);
    let Verdict::Propose { claims, .. } = verdict else {
        return Err("expected a proposal".into());
    };
    // The separator inside a string part is JSON-escaped, so the key still splits cleanly.
    let expected = key(&["t", r#""b\u001fc""#]);
    assert!(matches!(&claims[0], WorldEvent::EntityObserved { key: k, .. } if *k == expected));
    Ok(())
}

#[test]
fn evaluation_is_deterministic_and_provenance_names_the_mapping() -> TestResult {
    let payload = raw(&enveloped(r#"{"a":{"id":"x"},"b":{"ns":"n","id":1}}"#)?)?;
    let (first, second) = (engine()?, engine()?);
    assert_eq!(first.evaluate(&payload), second.evaluate(&payload));
    assert_eq!(first.provenance(), second.provenance());
    let provenance = String::from_utf8(first.provenance().unwrap_or_default())?;
    assert!(
        provenance.starts_with(r#"{"mapping_hash":""#),
        "{provenance}"
    );
    assert_eq!(provenance.len(), r#"{"mapping_hash":""}"#.len() + 16);
    let mut other = mapping();
    other.relationships[0].kind = "k2".to_owned();
    assert_ne!(MappingEngine::new(other)?.provenance(), first.provenance());
    assert_eq!((first.name(), first.version()), ("mapping", 1));
    Ok(())
}

#[test]
fn an_invalid_mapping_is_refused() {
    let mut m = mapping();
    m.version = 9;
    assert!(matches!(
        MappingEngine::new(m),
        Err(MappingEngineError::Invalid(
            MappingError::UnsupportedVersion(9)
        ))
    ));
}

/// The committed fixture pair used by `cargo xtask check`'s raw obfuscation replay proposes
/// entities for every line: the fixture is not vacuous at the engine layer.
#[test]
fn the_committed_fixture_mapping_matches_every_sample_line() -> TestResult {
    let mapping: StreamMapping =
        serde_json::from_str(include_str!("../../../testdata/sample.mapping.json"))?;
    let engine = MappingEngine::new(mapping)?;
    let lines = include_str!("../../../testdata/raw-sample.jsonl");
    let mut count = 0;
    for line in lines.lines() {
        count += 1;
        let verdict = engine.evaluate(&raw(line.as_bytes())?);
        assert!(matches!(verdict, Verdict::Propose { .. }), "{verdict:?}");
    }
    assert_eq!(count, 20);
    Ok(())
}
