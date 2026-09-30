//! Stream mapping links at the bridge and the fold (decision 0027, #245 PR 2): a stored
//! version-2 mapping routes, its engine claims merges in-band, and the existing fold joins two
//! different values into one entity, first link wins per absorbed key.

use s2w_app::bridge::{Bridge, BridgeConfig, EngineRegistry};
use s2w_app::query::{QueryState, Timeline};
use s2w_app::routes::{self, ENVELOPE_FORMAT, MappingEnvelope, STREAM_MAPPING_CLASS};
use s2w_core::{EntityId, NaturalKey, World, WorldEvent, fold};
use s2w_log::{
    Actor, Decider, EventLog, LogPosition, NewDecision, NewProposal, Outcome, ProposalStore,
    SqliteEventLog, SqliteProposalStore, SqliteVerdictStore,
};
use s2w_model::{Cursor, KeyPart, RawEvent, SourceId, StreamMapping, Timestamp};
use s2w_system1::{Engine, MappingEngine, Verdict};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

const SOURCE: &str = "test.linked";
const EVENTS: u64 = 20;
/// Sites in the raw sample: seven `wiki_id` values, each with one `meta.domain`.
const SITES: usize = 7;

fn linked_fixture() -> Fallible<StreamMapping> {
    Ok(serde_json::from_str(include_str!(
        "../../s2w-system1/testdata/sample-links.mapping.json"
    ))?)
}

fn raw_event(i: u8, payload: &[u8]) -> Fallible<RawEvent> {
    Ok(RawEvent {
        source: SourceId::new(SOURCE)?,
        cursor: Cursor::new(vec![i])?,
        received_at: Timestamp::from_millis(1_000 + i64::from(i)),
        payload: payload.to_vec(),
    })
}

/// The recorded raw sample, one event per line.
fn events() -> Fallible<Vec<RawEvent>> {
    include_str!("../../s2w-system1/testdata/raw-sample.jsonl")
        .lines()
        .zip(1_u8..)
        .map(|(line, i)| raw_event(i, line.as_bytes()))
        .collect()
}

/// Stores `mapping` for [`SOURCE`] as proposal `id` with a human accept.
fn accept(dir: &TestDirectory, id: &str, mapping: StreamMapping) -> TestResult {
    let envelope = MappingEnvelope {
        format: ENVELOPE_FORMAT,
        source: SOURCE.to_owned(),
        mapping,
    };
    let mut store = SqliteProposalStore::open(&dir.0)?;
    store.append_proposal(&NewProposal {
        id: id.to_owned(),
        class: STREAM_MAPPING_CLASS.to_owned(),
        actor: Actor::Human { id: "h".to_owned() },
        snapshot_offset: LogPosition::from_u64(1).ok_or("position")?,
        payload: serde_json::to_vec(&envelope)?,
        proposed_at_ms: 0,
    })?;
    store.append_decision(&NewDecision {
        proposal_id: id.to_owned(),
        decider: Decider::Human,
        outcome: Outcome::Accept,
        basis: "reviewed".to_owned(),
        decided_at_ms: 0,
    })?;
    Ok(())
}

/// Runs a fresh bridge over `dir` to the end of its log and returns the served world.
fn run_bridge(dir: &TestDirectory, registry: EngineRegistry) -> Fallible<World> {
    let state = QueryState::new(Timeline::new(s2w_app::DEFAULT_HUB_IN_DEGREE_CAP));
    let mut bridge = Bridge::new(
        SqliteEventLog::open(&dir.0)?,
        SqliteVerdictStore::open(&dir.0)?,
        registry,
        state.clone(),
        BridgeConfig {
            batch: 4,
            ..BridgeConfig::default()
        },
    )?;
    loop {
        let report = bridge.poll_once()?;
        if let Some(error) = report.error {
            return Err(error.into());
        }
        if report.stats.consumed == 0 {
            break;
        }
    }
    assert_eq!(bridge.stats().evaluated, EVENTS);
    Ok(state.world_at(None)?)
}

/// The entity a key resolves to, through every merge.
fn resolved(world: &World, key: &NaturalKey) -> Fallible<EntityId> {
    Ok(world.resolve(world.id_of(key).ok_or_else(|| format!("{key:?} unknown"))?))
}

/// The `site` keys of every sample line: `(wiki_id key, meta.domain key)`, deduplicated.
fn site_keys() -> Fallible<Vec<(NaturalKey, NaturalKey)>> {
    let mut pairs = Vec::new();
    for line in include_str!("../../s2w-system1/testdata/raw-sample.jsonl").lines() {
        let outer: serde_json::Value = serde_json::from_str(line)?;
        let data: serde_json::Value =
            serde_json::from_str(outer["data"].as_str().ok_or("data is not a string")?)?;
        let part = |value: &serde_json::Value| -> Fallible<NaturalKey> {
            let text = value.as_str().ok_or("a site key part is not a string")?;
            Ok(NaturalKey::from_parts(
                "site",
                &[KeyPart::Str(text.to_owned())],
            )?)
        };
        let pair = (part(&data["wiki_id"])?, part(&data["meta"]["domain"])?);
        if !pairs.contains(&pair) {
            pairs.push(pair);
        }
    }
    Ok(pairs)
}

/// A stored version-2 mapping routes in `serve`'s bridge, and its merges join each site's two
/// encodings into one entity: the survivor's. Every `hosted_on` edge points at a survivor.
#[test]
fn a_stored_linked_mapping_routes_and_merges_at_the_bridge() -> TestResult {
    let dir = TestDirectory::new("bridge")?;
    SqliteEventLog::open(&dir.0)?.append_batch(events()?)?;
    accept(&dir, "p-linked", linked_fixture()?)?;
    let world = run_bridge(&dir, routes::registry(&routes::load(&dir.0)?)?)?;

    let sites = site_keys()?;
    assert_eq!(sites.len(), SITES);
    assert_eq!(world.merges().len(), SITES, "one merge per site");
    let mut survivors = Vec::new();
    for (survivor, absorbed) in &sites {
        let id = world.id_of(survivor).ok_or("survivor unknown")?;
        assert_ne!(world.id_of(absorbed), Some(id), "two keys, two minted ids");
        assert_eq!(
            resolved(&world, absorbed)?,
            id,
            "{absorbed:?} joins {survivor:?}"
        );
        survivors.push(id);
    }
    let mut targets: Vec<EntityId> = world
        .relationships()
        .keys()
        .filter(|rel| rel.kind == "hosted_on")
        .map(|rel| rel.to)
        .collect();
    targets.extend(
        world
            .entities()
            .filter_map(|(_, state)| state.hub_ref("hosted_on")),
    );
    assert!(!targets.is_empty());
    assert!(
        targets.iter().all(|to| survivors.contains(to)),
        "{targets:?} vs {survivors:?}"
    );
    Ok(())
}

/// First link wins per absorbed key (decision 0027, semantics 4): an absorbed value shared by
/// two survivors joins the first one it co-occurs with, never the second, and never chains the
/// two survivors together. Each payload's edge to the absorbed rule binds to the entity the
/// absorbed key resolves to when it is folded, since merges precede relationships.
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
        match engine.evaluate(&raw_event(i, payload.as_bytes())?) {
            Verdict::Propose { claims: c, .. } => claims.extend(c),
            Verdict::Abstain { reason } => return Err(format!("{reason:?}").into()),
        }
    }
    let merges = claims
        .iter()
        .filter(|c| matches!(c, WorldEvent::EntitiesMerged { .. }))
        .count();
    assert_eq!(
        merges, 2,
        "the engine claims both; the fold keeps the first"
    );

    let world = fold(
        World::with_hub_cap(s2w_app::DEFAULT_HUB_IN_DEGREE_CAP),
        &claims,
    );
    let key = |text: &str| NaturalKey::from_parts("T", &[KeyPart::Str(text.to_owned())]);
    let (u1, u2, s) = (key("u1")?, key("u2")?, key("s")?);
    assert_eq!(resolved(&world, &s)?, resolved(&world, &u1)?);
    assert_ne!(resolved(&world, &u2)?, resolved(&world, &u1)?);
    assert_eq!(world.merges().len(), 1);
    let targets: Vec<EntityId> = world.relationships().keys().map(|rel| rel.to).collect();
    assert_eq!(
        targets,
        vec![resolved(&world, &u1)?; 2],
        "both edges bind to u1"
    );
    Ok(())
}

/// A fresh directory under the system temp dir, removed on drop (even when a test panics).
struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Fallible<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "s2w-app-mapping-links-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}
