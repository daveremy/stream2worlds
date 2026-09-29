//! Routes from stored stream mappings (decision 0023) at the bridge: a mapping engine is named
//! by its mapping identity, so a restart under the same mapping replays its stored verdicts,
//! and a restart under a different mapping never serves the old mapping's verdicts. The
//! mutant these tests exist for is one bare engine name shared by every mapping.

use s2w_app::bridge::{Bridge, BridgeConfig, BridgeStats, EngineRegistry, Route};
use s2w_app::query::{QueryState, Timeline};
use s2w_app::routes::{self, ENVELOPE_FORMAT, MappingEnvelope, STREAM_MAPPING_CLASS};
use s2w_app::snapshot::world_hash;
use s2w_log::{
    Actor, Decider, EventLog, LogPosition, LogReader, NewDecision, NewProposal, Outcome,
    ProposalStore, SqliteEventLog, SqliteProposalStore, SqliteVerdictStore, VerdictStore,
};
use s2w_model::{Cursor, RawEvent, SourceId, StreamMapping, Timestamp};
use s2w_system1::{Engine, MappingEngine, Verdict};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

const SOURCE: &str = "test.mapped";
const EVENTS: u64 = 20;

fn mapping_a() -> Fallible<StreamMapping> {
    Ok(serde_json::from_str(include_str!(
        "../../s2w-system1/testdata/sample.mapping.json"
    ))?)
}

/// Mapping A with its first entity rule's type label changed: another identity, another world.
fn mapping_b() -> Fallible<StreamMapping> {
    let mut mapping = mapping_a()?;
    let rule = mapping.entities.first_mut().ok_or("no entity rule")?;
    rule.type_label = format!("{}-b", rule.type_label);
    Ok(mapping)
}

/// The recorded raw sample, one event per line.
fn events() -> Fallible<Vec<RawEvent>> {
    include_str!("../../s2w-system1/testdata/raw-sample.jsonl")
        .lines()
        .zip(1_u8..)
        .map(|(line, i)| {
            Ok(RawEvent {
                source: SourceId::new(SOURCE)?,
                cursor: Cursor::new(vec![i])?,
                received_at: Timestamp::from_millis(1_000 + i64::from(i)),
                payload: line.as_bytes().to_vec(),
            })
        })
        .collect()
}

/// A log directory holding the sample.
fn logged(dir: &TestDirectory) -> TestResult {
    let mut log = SqliteEventLog::open(&dir.0)?;
    log.append_batch(events()?)?;
    Ok(())
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

/// The registry `serve` would build from `dir`'s proposal store.
fn stored_routes(dir: &TestDirectory) -> Fallible<EngineRegistry> {
    Ok(routes::registry(&routes::load(&dir.0)?)?)
}

/// The mutant decision 0023 rules out: every mapping engine under one bare name.
struct BareName(MappingEngine);

impl Engine for BareName {
    fn name(&self) -> &str {
        "mapping"
    }
    fn version(&self) -> u32 {
        self.0.version()
    }
    fn evaluate(&self, event: &RawEvent) -> Verdict {
        self.0.evaluate(event)
    }
    fn provenance(&self) -> Option<Vec<u8>> {
        self.0.provenance()
    }
}

fn bare(mapping: StreamMapping) -> Fallible<EngineRegistry> {
    let mut registry = EngineRegistry::with_defaults();
    registry.register(
        Route::Exact(SOURCE.to_owned()),
        Box::new(BareName(MappingEngine::new(mapping)?)),
    )?;
    Ok(registry)
}

/// Runs a fresh bridge over `dir` to the end of its log: its totals and its world's hash.
fn run_bridge(dir: &TestDirectory, registry: EngineRegistry) -> Fallible<(BridgeStats, u64)> {
    let state = QueryState::new(Timeline::new(s2w_app::DEFAULT_HUB_IN_DEGREE_CAP));
    let config = BridgeConfig {
        batch: 4,
        ..BridgeConfig::default()
    };
    let mut bridge = Bridge::new(
        SqliteEventLog::open(&dir.0)?,
        SqliteVerdictStore::open(&dir.0)?,
        registry,
        state.clone(),
        config,
    )?;
    drain(&mut bridge)?;
    Ok((bridge.stats(), world_hash(&state.world_at(None)?)?))
}

fn drain<R: LogReader, V: VerdictStore>(bridge: &mut Bridge<R, V>) -> TestResult {
    loop {
        let report = bridge.poll_once()?;
        if let Some(error) = report.error {
            return Err(error.into());
        }
        if report.stats.consumed == 0 {
            return Ok(());
        }
    }
}

/// The world hash of a cold fold of the sample under `registry`, in a fresh directory.
fn cold(name: &str, registry: EngineRegistry) -> Fallible<u64> {
    let dir = TestDirectory::new(name)?;
    logged(&dir)?;
    let (stats, hash) = run_bridge(&dir, registry)?;
    assert_eq!(
        (stats.evaluated, stats.replayed),
        (EVENTS, 0),
        "a cold fold"
    );
    Ok(hash)
}

/// A fresh directory under the system temp dir, removed on drop (even when a test panics).
struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Fallible<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "s2w-app-mapping-routes-{name}-{}-{nanos}",
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

#[test]
fn a_restart_under_the_same_stored_mapping_replays_its_verdicts() -> TestResult {
    let dir = TestDirectory::new("same")?;
    logged(&dir)?;
    accept(&dir, "p-a", mapping_a()?)?;
    let (first, hash) = run_bridge(&dir, stored_routes(&dir)?)?;
    assert_eq!((first.evaluated, first.replayed), (EVENTS, 0));

    let (second, restarted) = run_bridge(&dir, stored_routes(&dir)?)?;
    assert_eq!(
        (second.evaluated, second.replayed),
        (0, EVENTS),
        "no engine call"
    );
    assert_eq!(restarted, hash, "the same world is served");
    Ok(())
}

#[test]
fn a_restart_under_another_stored_mapping_never_serves_the_old_verdicts() -> TestResult {
    let cold_stored = |name, id, mapping| -> Fallible<u64> {
        let dir = TestDirectory::new(name)?;
        accept(&dir, id, mapping)?;
        cold(name, stored_routes(&dir)?)
    };
    let (cold_a, cold_b) = (
        cold_stored("cold-a", "p-a", mapping_a()?)?,
        cold_stored("cold-b", "p-b", mapping_b()?)?,
    );
    assert_ne!(
        cold_a, cold_b,
        "non-vacuity: the two mappings fold different worlds"
    );

    let dir = TestDirectory::new("remapped")?;
    logged(&dir)?;
    accept(&dir, "p-a", mapping_a()?)?;
    run_bridge(&dir, stored_routes(&dir)?)?;
    accept(&dir, "p-b", mapping_b()?)?;
    let (second, hash) = run_bridge(&dir, stored_routes(&dir)?)?;
    assert_eq!(
        (second.evaluated, second.replayed),
        (EVENTS, 0),
        "B is a new engine: A's stored verdicts are not read"
    );
    assert_eq!(hash, cold_b, "restart under B == cold fold under B");
    Ok(())
}

#[test]
fn a_bare_engine_name_would_serve_mapping_a_verdicts_as_mapping_b() -> TestResult {
    let cold_b = cold("bare-cold-b", bare(mapping_b()?)?)?;
    let dir = TestDirectory::new("bare")?;
    logged(&dir)?;
    run_bridge(&dir, bare(mapping_a()?)?)?;
    let (second, hash) = run_bridge(&dir, bare(mapping_b()?)?)?;
    assert!(second.replayed > 0, "A's verdicts replay under B's name");
    assert_eq!(second.evaluated, 0, "B never runs");
    assert_ne!(hash, cold_b, "the mutant serves the wrong world");
    Ok(())
}

#[test]
fn the_feed_fingerprint_differs_between_two_stored_mappings() -> TestResult {
    let (a, b) = (TestDirectory::new("fp-a")?, TestDirectory::new("fp-b")?);
    accept(&a, "p", mapping_a()?)?;
    accept(&b, "p", mapping_b()?)?;
    let (a, b) = (stored_routes(&a)?, stored_routes(&b)?);
    assert_ne!(a.feed_fingerprint(), b.feed_fingerprint());
    assert_ne!(
        a.feed_fingerprint(),
        EngineRegistry::with_defaults().feed_fingerprint(),
        "a route changes the fingerprint"
    );
    Ok(())
}
