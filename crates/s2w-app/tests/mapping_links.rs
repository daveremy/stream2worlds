//! First link wins per absorbed key (decision 0027, semantics 4), through the real engine and
//! the real fold. The bridge-level test is in `bridge_mapping_routes.rs`.

use s2w_core::{NaturalKey, World, fold};
use s2w_model::{Cursor, KeyPart, RawEvent, SourceId, StreamMapping, Timestamp};
use s2w_system1::{Engine, MappingEngine, Verdict};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// An absorbed value shared by two survivors joins the first one it co-occurs with, never the
/// second, and never chains the two survivors together. Each payload's edge to the absorbed rule
/// binds to the entity the absorbed key resolves to when it is folded, since merges precede
/// relationships.
#[test]
fn a_shared_absorbed_value_joins_only_the_first_survivor() -> TestResult {
    let mapping: StreamMapping = serde_json::from_str(
        r#"{"version":2,"decode":[],"entities":[
            {"id":"by","type_label":"E","key":[["by"]],"attrs":[]},
            {"id":"long","type_label":"T","key":[["long"]],"attrs":[]},
            {"id":"short","type_label":"T","key":[["short"]],"attrs":[]}],
          "relationships":[{"from":"by","to":"short","kind":"k"}],
          "links":[{"survivor":"long","absorbed":"short"}]}"#,
    )?;
    let engine = MappingEngine::new(mapping)?;
    let mut claims = Vec::new();
    for (payload, i) in [
        r#"{"by":"e1","long":"u1","short":"s"}"#,
        r#"{"by":"e2","long":"u2","short":"s"}"#,
    ]
    .iter()
    .zip(1_u8..)
    {
        let event = RawEvent {
            source: SourceId::new("test.links")?,
            cursor: Cursor::new(vec![i])?,
            received_at: Timestamp::from_millis(i64::from(i)),
            payload: payload.as_bytes().to_vec(),
        };
        match engine.evaluate(&event) {
            Verdict::Propose { claims: c, .. } => claims.extend(c),
            Verdict::Abstain { reason } => return Err(format!("{reason:?}").into()),
        }
    }
    let merges = claims
        .iter()
        .filter(|c| matches!(c, s2w_core::WorldEvent::EntitiesMerged { .. }))
        .count();
    assert_eq!(
        merges, 2,
        "the engine claims both; the fold keeps the first"
    );

    let world = fold(
        World::with_hub_cap(s2w_app::DEFAULT_HUB_IN_DEGREE_CAP),
        &claims,
    );
    let resolved = |text: &str| -> Result<_, Box<dyn std::error::Error>> {
        let key = NaturalKey::from_parts("T", &[KeyPart::Str(text.to_owned())])?;
        Ok(world.resolve(world.id_of(&key).ok_or("unknown key")?))
    };
    let (u1, u2, s) = (resolved("u1")?, resolved("u2")?, resolved("s")?);
    assert_eq!(s, u1);
    assert_ne!(u2, u1);
    assert_eq!(world.merges().len(), 1);
    let targets: Vec<_> = world.relationships().keys().map(|rel| rel.to).collect();
    assert_eq!(targets, vec![u1; 2], "both edges bind to u1");
    Ok(())
}
