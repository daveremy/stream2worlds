//! The bridge's verdict store (decision 0012): verdicts are stored before their claims are
//! served, and a restart serves stored verdicts instead of re-running engines — across an
//! engine version bump, a panic, a newly registered engine, a failed commit and corruption.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use s2w_app::bridge::{
    Bridge, BridgeConfig, BridgeStats, EngineRegistry, PollReport, RegistryError, Route,
};
use s2w_app::query::{QueryState, Timeline};
use s2w_core::{AttrValue, NaturalKey, World};
use s2w_log::{
    EventLog, InMemoryEventLog, InMemoryVerdictStore, LogError, LogPosition, LogReader,
    SqliteEventLog, SqliteVerdictStore, StoredEvent, StoredVerdict, VerdictStore,
};
use s2w_model::{Cursor, RawEvent, SourceId, Timestamp, WorldEvent};
use s2w_system1::{Confidence, Engine, Verdict};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

const SOURCE: &str = "test.things";

/// Observes one entity keyed by the payload, tagged with the engine's version, and counts its
/// calls. Two versions of it map the same payload to different claims.
struct Tagger {
    name: &'static str,
    version: u32,
    calls: Arc<AtomicU64>,
    panics: bool,
}

impl Tagger {
    fn new(name: &'static str, version: u32) -> (Self, Arc<AtomicU64>) {
        let calls = Arc::new(AtomicU64::new(0));
        let engine = Self {
            name,
            version,
            calls: Arc::clone(&calls),
            panics: false,
        };
        (engine, calls)
    }
}

impl Engine for Tagger {
    fn name(&self) -> &'static str {
        self.name
    }
    fn version(&self) -> u32 {
        self.version
    }
    fn evaluate(&self, event: &RawEvent) -> Verdict {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panics, "engine defect");
        let key = String::from_utf8_lossy(&event.payload).into_owned();
        Verdict::Propose {
            claims: vec![WorldEvent::EntityObserved {
                key: NaturalKey::new(key),
                entity_type: "thing".into(),
                attrs: BTreeMap::from([(
                    format!("{}-tag", self.name),
                    AttrValue::Str(format!("v{}", self.version)),
                )]),
            }],
            confidence: Confidence::CERTAIN,
        }
    }
}

fn registry_of(engines: Vec<Tagger>) -> Fallible<EngineRegistry> {
    let mut registry = EngineRegistry::new();
    for engine in engines {
        registry.register(Route::Exact(SOURCE.to_owned()), Box::new(engine))?;
    }
    Ok(registry)
}

fn events(prefix: &str, range: std::ops::Range<u8>) -> Fallible<Vec<RawEvent>> {
    range
        .map(|i| {
            Ok(RawEvent {
                source: SourceId::new(SOURCE)?,
                cursor: Cursor::new(vec![i])?,
                received_at: Timestamp::from_millis(1_000 + i64::from(i)),
                payload: format!("{prefix}{i}").into_bytes(),
            })
        })
        .collect()
}

fn new_state() -> QueryState {
    QueryState::new(Timeline::new(s2w_app::DEFAULT_HUB_IN_DEGREE_CAP))
}

/// Runs a fresh bridge to the end of `log`, returning its totals and the world it served.
fn run_bridge<R: LogReader, V: VerdictStore>(
    log: R,
    verdicts: V,
    registry: EngineRegistry,
) -> Fallible<(BridgeStats, World)> {
    let state = new_state();
    let mut bridge = Bridge::new(log, verdicts, registry, state.clone(), batch_of(2))?;
    drain(&mut bridge)?;
    Ok((bridge.stats(), state.world_at(None)?))
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

fn batch_of(batch: usize) -> BridgeConfig {
    BridgeConfig {
        batch,
        ..BridgeConfig::default()
    }
}

fn stored_events<R: LogReader>(log: &R) -> Fallible<Vec<StoredEvent>> {
    Ok(log.read_after(None)?.collect::<Result<_, _>>()?)
}

fn all_rows<V: VerdictStore>(store: &V, through: LogPosition) -> Fallible<Vec<StoredVerdict>> {
    Ok(store.read_range(None, through)?)
}

fn tag(world: &World, key: &str, attr: &str) -> Option<AttrValue> {
    let id = world.id_of(&NaturalKey::new(key))?;
    world
        .entities()
        .iter()
        .find(|(e, _)| **e == id)
        .and_then(|(_, state)| state.attrs.get(attr).cloned())
}

/// A fresh directory under the system temp dir, removed on drop (even when a test panics).
struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Fallible<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "s2w-app-verdicts-{name}-{}-{nanos}",
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

// 1
#[test]
fn a_restart_replays_stored_verdicts_and_never_re_runs_an_engine() -> TestResult {
    let dir = TestDirectory::new("replay")?;
    let mut log = SqliteEventLog::open(&dir.0)?;
    log.append_batch(events("e", 0..5)?)?;

    let (first_engine, first_calls) = Tagger::new("tagger", 1);
    let (first, world_first) = run_bridge(
        log,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![first_engine])?,
    )?;
    assert_eq!(first_calls.load(Ordering::SeqCst), 5);
    assert_eq!((first.evaluated, first.replayed), (5, 0));
    let cursor = SqliteVerdictStore::open(&dir.0)?.cursor()?;

    let (second_engine, second_calls) = Tagger::new("tagger", 1);
    let (second, world_second) = run_bridge(
        SqliteEventLog::open(&dir.0)?,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![second_engine])?,
    )?;
    assert_eq!(second_calls.load(Ordering::SeqCst), 0, "no engine call");
    assert_eq!((second.evaluated, second.replayed), (0, 5));
    assert_eq!(second.proposed_claims, 5);
    assert_eq!(SqliteVerdictStore::open(&dir.0)?.cursor()?, cursor);
    assert_eq!(world_second, world_first, "the same world is served");
    Ok(())
}

// 2 and 11: the issue's done-when.
#[test]
fn a_version_bump_keeps_serving_old_verdicts_and_evaluates_only_new_events() -> TestResult {
    let dir = TestDirectory::new("bump")?;
    let mut log = SqliteEventLog::open(&dir.0)?;
    log.append_batch(events("e", 0..3)?)?;
    let (v1, _) = Tagger::new("tagger", 1);
    let (_, world_v1) = run_bridge(
        log,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![v1])?,
    )?;
    assert_eq!(
        tag(&world_v1, "e0", "tagger-tag"),
        Some(AttrValue::Str("v1".into()))
    );

    // Restart with v2 of the same engine over the same log: already-evaluated positions serve
    // the v1 verdicts, and the served world is the pre-restart world.
    let (v2, v2_calls) = Tagger::new("tagger", 2);
    let (bumped, world_v2) = run_bridge(
        SqliteEventLog::open(&dir.0)?,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![v2])?,
    )?;
    assert_eq!(v2_calls.load(Ordering::SeqCst), 0);
    assert_eq!(bumped.replayed_stale_version, 3);
    assert_eq!(world_v2, world_v1);

    // New events get v2; old ones still serve v1.
    let mut log = SqliteEventLog::open(&dir.0)?;
    log.append_batch(events("e", 3..5)?)?;
    let stored = stored_events(&log)?;
    let (v2, v2_calls) = Tagger::new("tagger", 2);
    let (grown, world) = run_bridge(
        log,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![v2])?,
    )?;
    assert_eq!(v2_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        (
            grown.replayed,
            grown.replayed_stale_version,
            grown.evaluated
        ),
        (3, 3, 2)
    );
    assert_eq!(
        tag(&world, "e2", "tagger-tag"),
        Some(AttrValue::Str("v1".into()))
    );
    assert_eq!(
        tag(&world, "e3", "tagger-tag"),
        Some(AttrValue::Str("v2".into()))
    );

    let last = stored.last().ok_or("events were stored")?.position;
    let versions: Vec<u32> = all_rows(&SqliteVerdictStore::open(&dir.0)?, last)?
        .iter()
        .map(|row| row.version)
        .collect();
    assert_eq!(versions, [1, 1, 1, 2, 2]);
    Ok(())
}

// 3
#[test]
fn a_panic_is_persisted_and_replayed_without_calling_the_engine() -> TestResult {
    let dir = TestDirectory::new("panic")?;
    let mut log = SqliteEventLog::open(&dir.0)?;
    log.append_batch(events("e", 0..2)?)?;
    let (mut panicking, _) = Tagger::new("tagger", 1);
    panicking.panics = true;
    let (first, _) = run_bridge(
        log,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![panicking])?,
    )?;
    assert_eq!((first.engine_panics, first.evaluated), (2, 2));

    let (fixed, calls) = Tagger::new("tagger", 1);
    let (second, world) = run_bridge(
        SqliteEventLog::open(&dir.0)?,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![fixed])?,
    )?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        (
            second.engine_panics,
            second.replayed,
            second.proposed_claims
        ),
        (2, 2, 0)
    );
    assert!(world.entities().is_empty());
    Ok(())
}

// 4
#[test]
fn a_newly_registered_engine_evaluates_old_events_once() -> TestResult {
    let dir = TestDirectory::new("new-engine")?;
    let mut log = SqliteEventLog::open(&dir.0)?;
    log.append_batch(events("e", 0..3)?)?;
    let last = stored_events(&log)?.last().ok_or("stored")?.position;
    let (a, _) = Tagger::new("a", 1);
    run_bridge(
        log,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![a])?,
    )?;

    let (a, a_calls) = Tagger::new("a", 1);
    let (b, b_calls) = Tagger::new("b", 1);
    let (stats, world) = run_bridge(
        SqliteEventLog::open(&dir.0)?,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![a, b])?,
    )?;
    assert_eq!(
        (
            a_calls.load(Ordering::SeqCst),
            b_calls.load(Ordering::SeqCst)
        ),
        (0, 3)
    );
    assert_eq!((stats.replayed, stats.evaluated), (3, 3));
    assert_eq!(
        tag(&world, "e0", "b-tag"),
        Some(AttrValue::Str("v1".into()))
    );
    assert_eq!(all_rows(&SqliteVerdictStore::open(&dir.0)?, last)?.len(), 6);

    let (a, a_calls) = Tagger::new("a", 1);
    let (b, b_calls) = Tagger::new("b", 1);
    run_bridge(
        SqliteEventLog::open(&dir.0)?,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![a, b])?,
    )?;
    assert_eq!(
        (
            a_calls.load(Ordering::SeqCst),
            b_calls.load(Ordering::SeqCst)
        ),
        (0, 0)
    );
    Ok(())
}

/// An in-memory store whose commits fail while `failing` is set.
struct FlakyStore {
    inner: InMemoryVerdictStore,
    failing: Arc<AtomicBool>,
}

impl VerdictStore for FlakyStore {
    fn cursor(&self) -> Result<Option<LogPosition>, LogError> {
        self.inner.cursor()
    }
    fn read_range(
        &self,
        after: Option<LogPosition>,
        through: LogPosition,
    ) -> Result<Vec<StoredVerdict>, LogError> {
        self.inner.read_range(after, through)
    }
    fn commit_batch(
        &mut self,
        rows: &[StoredVerdict],
        through: LogPosition,
    ) -> Result<(), LogError> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(LogError::Io("disk full".into()));
        }
        self.inner.commit_batch(rows, through)
    }
}

// 5
#[test]
fn a_failed_commit_serves_nothing_and_the_next_poll_retries() -> TestResult {
    let mut log = InMemoryEventLog::new();
    log.append_batch(events("e", 0..3)?)?;
    let failing = Arc::new(AtomicBool::new(true));
    let store = FlakyStore {
        inner: InMemoryVerdictStore::new(),
        failing: Arc::clone(&failing),
    };
    let (engine, calls) = Tagger::new("tagger", 1);
    let state = new_state();
    let mut bridge = Bridge::new(
        log,
        store,
        registry_of(vec![engine])?,
        state.clone(),
        batch_of(10),
    )?;

    let failed = bridge.poll_once()?;
    assert_eq!(
        failed,
        PollReport {
            stats: BridgeStats::default(),
            error: Some(LogError::Io("disk full".into())),
        }
    );
    assert_eq!(state.branches()?[0].head, 0, "no claim served");
    assert_eq!(bridge.stats(), BridgeStats::default());

    failing.store(false, Ordering::SeqCst);
    let retried = bridge.poll_once()?;
    assert!(retried.error.is_none());
    assert_eq!((retried.stats.consumed, retried.stats.evaluated), (3, 3));
    assert_eq!(state.branches()?[0].head, 3);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        6,
        "never-stored verdicts are evaluated again"
    );
    assert_eq!(
        bridge.verdicts().inner.cursor()?.map(LogPosition::as_u64),
        Some(3)
    );
    Ok(())
}

fn expect_corrupt(report: &PollReport) -> TestResult {
    match &report.error {
        Some(LogError::Corrupt(_)) => Ok(()),
        other => Err(format!("expected a Corrupt error, got {other:?}").into()),
    }
}

// 6
#[test]
fn a_store_ahead_of_the_log_is_corrupt() -> TestResult {
    let dir = TestDirectory::new("ahead")?;
    let mut long = InMemoryEventLog::new();
    long.append_batch(events("e", 0..3)?)?;
    let (engine, _) = Tagger::new("tagger", 1);
    run_bridge(
        long,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![engine])?,
    )?;

    let mut short = InMemoryEventLog::new();
    short.append_batch(events("e", 0..1)?)?;
    let (engine, calls) = Tagger::new("tagger", 1);
    let state = new_state();
    let mut bridge = Bridge::new(
        short,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![engine])?,
        state.clone(),
        batch_of(10),
    )?;
    let report = bridge.poll_once()?;
    expect_corrupt(&report)?;
    assert_eq!(report.stats, BridgeStats::default());
    assert_eq!(state.branches()?[0].head, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

/// A log of `n` events and an in-memory store pre-loaded with `rows(stored events)`.
fn fixture(
    n: u8,
    rows: impl FnOnce(&[StoredEvent]) -> Fallible<Vec<StoredVerdict>>,
) -> Fallible<(InMemoryEventLog, InMemoryVerdictStore)> {
    let mut log = InMemoryEventLog::new();
    log.append_batch(events("e", 0..n)?)?;
    let stored = stored_events(&log)?;
    let mut store = InMemoryVerdictStore::new();
    let last = stored.last().ok_or("stored")?.position;
    store.commit_batch(&rows(&stored)?, last)?;
    Ok((log, store))
}

fn row(event: &StoredEvent, version: u32, verdict: Vec<u8>) -> StoredVerdict {
    StoredVerdict {
        position: event.position,
        event_hash: event.content_hash,
        engine: "tagger".into(),
        version,
        verdict,
        provenance: None,
    }
}

fn verdict_bytes(engine: &Tagger, event: &StoredEvent) -> Fallible<Vec<u8>> {
    Ok(serde_json::to_vec(&engine.evaluate(&event.event))?)
}

// 7
#[test]
fn undecodable_verdict_bytes_are_corrupt_and_never_re_evaluated() -> TestResult {
    let (log, store) = fixture(1, |stored| {
        Ok(vec![row(&stored[0], 1, b"not json".to_vec())])
    })?;
    let (engine, calls) = Tagger::new("tagger", 1);
    let state = new_state();
    let mut bridge = Bridge::new(
        log,
        store,
        registry_of(vec![engine])?,
        state.clone(),
        batch_of(10),
    )?;
    let report = bridge.poll_once()?;
    expect_corrupt(&report)?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.branches()?[0].head, 0);
    Ok(())
}

// 8
#[test]
fn the_registry_runs_a_name_once_and_refuses_a_second_version() -> TestResult {
    let (first, calls) = Tagger::new("tagger", 1);
    let (same, _) = Tagger::new("tagger", 1);
    let mut registry = EngineRegistry::new();
    registry
        .register(Route::Prefix("test.".to_owned()), Box::new(first))?
        .register(Route::Exact(SOURCE.to_owned()), Box::new(same))?;
    let (other, _) = Tagger::new("tagger", 2);
    assert!(matches!(
        registry.register(Route::Exact(SOURCE.to_owned()), Box::new(other)),
        Err(RegistryError::VersionConflict {
            registered: 1,
            rejected: 2,
            ..
        })
    ));

    let mut log = InMemoryEventLog::new();
    log.append_batch(events("e", 0..2)?)?;
    let (stats, _) = run_bridge(log, InMemoryVerdictStore::new(), registry)?;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "one call per event, not two"
    );
    assert_eq!(stats.evaluated, 2);
    Ok(())
}

// 9
#[test]
fn a_log_swapped_under_the_store_is_corrupt() -> TestResult {
    let dir = TestDirectory::new("swapped")?;
    let mut original = InMemoryEventLog::new();
    original.append_batch(events("e", 0..2)?)?;
    let (engine, _) = Tagger::new("tagger", 1);
    run_bridge(
        original,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![engine])?,
    )?;

    // Same length and more: different payloads at the stored positions.
    let mut swapped = InMemoryEventLog::new();
    swapped.append_batch(events("other", 0..3)?)?;
    let (engine, calls) = Tagger::new("tagger", 1);
    let state = new_state();
    let mut bridge = Bridge::new(
        swapped,
        SqliteVerdictStore::open(&dir.0)?,
        registry_of(vec![engine])?,
        state.clone(),
        batch_of(10),
    )?;
    let report = bridge.poll_once()?;
    expect_corrupt(&report)?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.branches()?[0].head, 0);
    Ok(())
}

// 10
#[test]
fn several_stored_versions_serve_the_first_written() -> TestResult {
    let (v1, _) = Tagger::new("tagger", 1);
    let (v2, _) = Tagger::new("tagger", 2);
    let (log, store) = fixture(1, |stored| {
        Ok(vec![
            row(&stored[0], 1, verdict_bytes(&v1, &stored[0])?),
            row(&stored[0], 2, verdict_bytes(&v2, &stored[0])?),
        ])
    })?;
    // Registered at a version above both, so serving the lowest seq is not the same as
    // serving the registered or the highest version.
    let (v3, calls) = Tagger::new("tagger", 3);
    let (stats, world) = run_bridge(log, store, registry_of(vec![v3])?)?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!((stats.replayed, stats.replayed_stale_version), (1, 1));
    assert_eq!(
        tag(&world, "e0", "tagger-tag"),
        Some(AttrValue::Str("v1".into()))
    );
    Ok(())
}

#[test]
fn unrouted_events_store_nothing_but_advance_the_cursor() -> TestResult {
    let mut log = InMemoryEventLog::new();
    log.append_batch(events("e", 0..2)?)?;
    let state = new_state();
    let mut bridge = Bridge::new(
        log,
        InMemoryVerdictStore::new(),
        EngineRegistry::new(),
        state,
        batch_of(10),
    )?;
    drain(&mut bridge)?;
    assert_eq!(bridge.stats().unrouted, 2);
    let cursor = bridge.verdicts().cursor()?.ok_or("cursor advanced")?;
    assert_eq!(cursor.as_u64(), 2);
    assert!(bridge.verdicts().read_range(None, cursor)?.is_empty());
    Ok(())
}
