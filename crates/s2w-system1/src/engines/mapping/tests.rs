use super::*;
use crate::raw;
use s2w_model::{
    AttrRule, FieldPath, KEY_SEPARATOR, LinkRule, MAPPING_VERSION, MAPPING_VERSION_LINKS,
    RelationshipRule, Segment,
};

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
        links: Vec::new(),
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
    assert_eq!(first.name(), format!("mapping-{}", mapping().identity()?));
    assert_eq!(first.version(), 1);
    Ok(())
}

#[test]
fn provenance_names_the_proposal_and_the_name_does_not() -> TestResult {
    let bare = engine()?;
    let named = engine()?.with_proposal_id("p-\"1");
    assert_eq!(named.name(), bare.name());
    let provenance: serde_json::Value =
        serde_json::from_slice(&named.provenance().unwrap_or_default())?;
    let bare_provenance: serde_json::Value =
        serde_json::from_slice(&bare.provenance().unwrap_or_default())?;
    assert_eq!(provenance["proposal_id"], "p-\"1");
    assert_eq!(provenance["mapping_hash"], bare_provenance["mapping_hash"]);
    assert!(bare_provenance.get("proposal_id").is_none());
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

/// `mapping()` at version 2 with an alias rule for `ta` at `a.alias`, linked `ra` ← `ra-alias`
/// (decision 0027).
fn linked() -> StreamMapping {
    let mut linked = mapping();
    linked.version = MAPPING_VERSION_LINKS;
    linked
        .entities
        .push(rule("ra-alias", "ta", &[&["body", "a", "alias"]]));
    linked.links = vec![LinkRule {
        survivor: "ra".to_owned(),
        absorbed: "ra-alias".to_owned(),
    }];
    linked
}

fn claims_of(verdict: Verdict) -> Vec<WorldEvent> {
    match verdict {
        Verdict::Propose { claims, .. } => claims,
        Verdict::Abstain { .. } => Vec::new(),
    }
}

/// A link whose two rules match with different keys claims one merge, after every entity and
/// before every relationship (decision 0027, semantics 3), so this payload's edge binds to the
/// survivor's entity.
#[test]
fn a_link_claims_a_merge_between_the_entities_and_the_relationships() -> TestResult {
    let engine = MappingEngine::new(linked())?;
    let payload = enveloped(r#"{"a":{"id":"x","alias":"y"},"b":{"ns":"n1","id":42}}"#)?;
    let a = key(&["ta", r#""x""#]);
    let alias = key(&["ta", r#""y""#]);
    let b = key(&["tb", r#""n1""#, "42"]);
    let observed = |key: &NaturalKey, label: &str| WorldEvent::EntityObserved {
        key: key.clone(),
        entity_type: label.to_owned(),
        attrs: BTreeMap::new(),
    };
    let expected = vec![
        observed(&a, "ta"),
        observed(&b, "tb"),
        observed(&alias, "ta"),
        WorldEvent::EntitiesMerged {
            survivor: a.clone(),
            absorbed: alias,
        },
        WorldEvent::RelationshipObserved {
            from: a,
            to: b,
            kind: "k".to_owned(),
        },
    ];
    assert_eq!(
        engine.evaluate(&raw(&payload)?),
        Verdict::Propose {
            claims: expected,
            confidence: Confidence::CERTAIN,
        }
    );
    Ok(())
}

/// A link claims nothing when its two keys are equal or when either rule did not match.
#[test]
fn a_link_claims_nothing_on_equal_keys_or_a_missing_side() -> TestResult {
    let engine = MappingEngine::new(linked())?;
    let is_merge = |claim: &WorldEvent| matches!(claim, WorldEvent::EntitiesMerged { .. });
    for inner in [
        r#"{"a":{"id":"x","alias":"x"}}"#,
        r#"{"a":{"id":"x"}}"#,
        r#"{"a":{"alias":"y"}}"#,
        r#"{"a":{"id":"x","alias":1.5}}"#,
    ] {
        let claims = claims_of(engine.evaluate(&raw(&enveloped(inner)?)?));
        assert!(!claims.is_empty(), "{inner}");
        assert!(!claims.iter().any(is_merge), "{inner}: {claims:?}");
    }
    Ok(())
}

/// Several links claim their merges in link order, not rule order.
#[test]
fn merges_are_claimed_in_link_order() -> TestResult {
    let mut mapping = linked();
    mapping
        .entities
        .insert(1, rule("ra-other", "ta", &[&["body", "a", "other"]]));
    mapping.links.push(LinkRule {
        survivor: "ra".to_owned(),
        absorbed: "ra-other".to_owned(),
    });
    let engine = MappingEngine::new(mapping)?;
    let payload = enveloped(r#"{"a":{"id":"x","alias":"y","other":"z"}}"#)?;
    let merges: Vec<NaturalKey> = claims_of(engine.evaluate(&raw(&payload)?))
        .into_iter()
        .filter_map(|claim| match claim {
            WorldEvent::EntitiesMerged { absorbed, .. } => Some(absorbed),
            _ => None,
        })
        .collect();
    assert_eq!(merges, vec![key(&["ta", r#""y""#]), key(&["ta", r#""z""#])]);
    Ok(())
}

/// A version-2 mapping without links runs, under its own name: the identity names the version.
#[test]
fn an_unlinked_version_2_mapping_runs_under_its_own_name() -> TestResult {
    let mut unlinked = linked();
    unlinked.links.clear();
    let v2 = MappingEngine::new(unlinked.clone())?;
    unlinked.version = MAPPING_VERSION;
    assert_ne!(v2.name(), MappingEngine::new(unlinked)?.name());
    Ok(())
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

/// The fixture mapping's identity is pinned (decision 0023). It changes only with the fixture,
/// `KEY_FORMAT` or the mapping's `version`; any of those renames the engine and orphans its stored
/// verdicts, so a change here must be deliberate.
#[test]
fn the_committed_fixture_mapping_identity_is_pinned() -> TestResult {
    let mapping: StreamMapping =
        serde_json::from_str(include_str!("../../../testdata/sample.mapping.json"))?;
    assert_eq!(
        MappingEngine::new(mapping)?.name(),
        "mapping-6815cb3fc24b0848"
    );
    Ok(())
}

/// A second pinned identity, over labels that `serde_json` must escape (`"`, `\`) or encode
/// as multi-byte UTF-8, so a change in how the canonical bytes are written cannot silently
/// rename every engine (decision 0023).
#[test]
fn an_identity_over_escaped_and_non_ascii_labels_is_pinned() -> TestResult {
    let mapping: StreamMapping = serde_json::from_str(
        r#"{"version":1,"decode":[],"entities":[{"id":"ed\"it\\or","type_label":"usér","key":[["naïve"]],"attrs":[{"name":"ünï","path":["a\"b"]}]}],"relationships":[]}"#,
    )?;
    assert_eq!(
        MappingEngine::new(mapping)?.name(),
        "mapping-8b7f79bf35a70060"
    );
    Ok(())
}

/// The linked fixture (check 11's second pair, decision 0027) proposes on every sample line and
/// claims a merge on every line: `wiki_id` and `meta.domain` are two encodings of one site
/// that never share text. Its identity is pinned for the same reason as the first fixture's,
/// and was reproduced by an FNV-1a computation outside this codebase.
#[test]
fn the_committed_linked_fixture_merges_on_every_line_and_its_identity_is_pinned() -> TestResult {
    let mapping: StreamMapping =
        serde_json::from_str(include_str!("../../../testdata/sample-links.mapping.json"))?;
    assert_eq!(mapping.version, MAPPING_VERSION_LINKS);
    let engine = MappingEngine::new(mapping)?;
    let lines = include_str!("../../../testdata/raw-sample.jsonl");
    let mut merges = 0;
    for line in lines.lines() {
        let claims = claims_of(engine.evaluate(&raw(line.as_bytes())?));
        assert!(!claims.is_empty(), "{line}");
        merges += claims
            .iter()
            .filter(|claim| matches!(claim, WorldEvent::EntitiesMerged { .. }))
            .count();
    }
    assert_eq!(merges, 20);
    assert_eq!(engine.name(), "mapping-25768f1123cac8c0");
    Ok(())
}
