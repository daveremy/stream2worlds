//! The live bridge against a recorded fixture: log → System 1 → `QueryState`, with no network.
//! Runs the same scenario over the in-memory log and the durable SQLite log. Since decision
//! 0018 the preset-sourced events run no engine from the default registry, so this exercises
//! the bridge's routing, batching and resume mechanics over a realistic multi-source log
//! (routed `stdin`, unrouted preset traffic, unrouted other sources).

use std::cell::Cell;
use std::time::Duration;

use s2w_app::bridge::{Bridge, BridgeConfig, BridgeError, BridgeStats, EngineRegistry, Route};
use s2w_app::query::{QueryState, Timeline};
use s2w_log::{
    EventLog, InMemoryEventLog, InMemoryVerdictStore, LogError, LogPosition, LogReader,
    SqliteEventLog, SqliteVerdictStore, StoredEvent, VerdictStore,
};
use s2w_model::{Cursor, NaturalKey, RawEvent, SourceId, Timestamp};
use s2w_system1::{AbstainReason, Engine, Verdict};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const FIXTURE: &str = include_str!("fixtures/wikipedia-page-change.jsonl");
const WIKI: &str = "wikipedia.page_change";

/// Merges the moved page into the deleted one's survivor slot, both named by natural key.
const MERGE: &str =
    r#"{"EntitiesMerged":{"survivor":"ptwiki:page:6733701","absorbed":"frwiki:page:6998844"}}"#;

fn event(source: &str, i: u8, payload: &[u8]) -> Result<RawEvent, Box<dyn std::error::Error>> {
    Ok(RawEvent {
        source: SourceId::new(source)?,
        cursor: Cursor::new(vec![i])?,
        received_at: Timestamp::from_millis(1_000 + i64::from(i) * 100),
        payload: payload.to_vec(),
    })
}

/// The seven events, in log order: five recorded page changes (unrouted since decision 0018),
/// one `stdin` merge claim (routed to the JSON-claims engine), one event on another unrouted
/// source.
fn fixture_events() -> Result<Vec<RawEvent>, Box<dyn std::error::Error>> {
    let lines: Vec<&str> = FIXTURE.lines().collect();
    assert_eq!(lines.len(), 5, "fixture holds five page changes");

    // Fail loudly if a fixture edit drops the coverage the assertions below rely on.
    let parsed: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line))
        .collect::<Result<_, _>>()?;
    assert!(parsed.iter().all(|p| p["performer"].is_object()));

    let mut events = Vec::new();
    for (i, line) in (0u8..).zip(&lines) {
        events.push(event(WIKI, i, line.as_bytes())?);
    }
    events.push(event("stdin", 5, MERGE.as_bytes())?);
    events.push(event("kafka.orders", 6, b"{\"order\":1}")?);
    Ok(events)
}

fn new_state() -> QueryState {
    QueryState::new(Timeline::new(s2w_app::DEFAULT_HUB_IN_DEGREE_CAP))
}

fn batch_of(batch: usize) -> BridgeConfig {
    BridgeConfig {
        batch,
        ..BridgeConfig::default()
    }
}

fn replay_and_check<R: LogReader, V: VerdictStore>(log: R, verdicts: V) -> TestResult {
    let state = new_state();
    let observer = state.clone();
    let mut bridge = Bridge::new(
        log,
        verdicts,
        EngineRegistry::with_defaults(),
        state,
        batch_of(4),
    )?;

    let first = bridge.poll_once()?;
    let second = bridge.poll_once()?;
    let third = bridge.poll_once()?;
    assert_eq!((first.stats.consumed, second.stats.consumed), (4, 3));
    assert_eq!(third.stats.consumed, 0, "nothing left after the batch cap");
    assert!(first.error.is_none() && second.error.is_none() && third.error.is_none());

    let totals = bridge.stats();
    assert_eq!(
        totals,
        BridgeStats {
            consumed: 7,
            // Decision 0018's accepted consequence: the five preset events and the kafka
            // event run no engine from the default registry, so the one `stdin` claim is
            // the only proposal in the log.
            proposed_claims: 1,
            unrouted: 6,
            evaluated: 1,
            ..BridgeStats::default()
        }
    );

    // The claim reached the shared state: the handle a server would hold sees the head.
    assert_eq!(observer.branches()?[0].head, 1);

    // The merge claim names keys no observation minted, so it folds to a no-op world: the
    // claim is served, and the world stays empty, exactly as the fold's contract says.
    let world = observer.world_at(None)?;
    assert!(world.entities().is_empty());
    Ok(())
}

#[test]
fn bridge_replays_the_in_memory_log() -> TestResult {
    let mut log = InMemoryEventLog::new();
    log.append_batch(fixture_events()?)?;
    replay_and_check(log, InMemoryVerdictStore::new())
}

/// A fresh directory under the system temp dir, removed on drop (even when a test panics).
struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("s2w-app-{name}-{}-{nanos}", std::process::id()));
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
fn bridge_replays_the_sqlite_log() -> TestResult {
    let directory = TestDirectory::new("bridge")?;
    let mut log = SqliteEventLog::open(&directory.0)?;
    log.append_batch(fixture_events()?)?;
    replay_and_check(log, SqliteVerdictStore::open(&directory.0)?)
}

#[test]
fn bridge_refuses_a_timeline_that_already_has_events() -> TestResult {
    let state = new_state();
    state.append(
        Timestamp::from_millis(0),
        s2w_model::WorldEvent::EntitiesMerged {
            survivor: NaturalKey::new("a"),
            absorbed: NaturalKey::new("b"),
        },
    )?;
    let refused = Bridge::new(
        InMemoryEventLog::new(),
        InMemoryVerdictStore::new(),
        EngineRegistry::with_defaults(),
        state,
        BridgeConfig::default(),
    );
    assert!(matches!(
        refused,
        Err(BridgeError::TimelineNotEmpty { head: 1 })
    ));
    Ok(())
}

struct Panics;
impl Engine for Panics {
    fn name(&self) -> &'static str {
        "panics"
    }
    fn version(&self) -> u32 {
        1
    }
    fn evaluate(&self, _: &RawEvent) -> Verdict {
        panic!("engine defect")
    }
}

#[test]
fn a_panicking_engine_is_an_abstention_and_the_next_engine_still_runs() -> TestResult {
    let mut log = InMemoryEventLog::new();
    log.append(event("stdin", 0, MERGE.as_bytes())?)?;
    let stored = log
        .read_after(None)?
        .next()
        .ok_or("the event was stored")??;
    let mut registry = EngineRegistry::new();
    registry.register(Route::Exact("stdin"), Box::new(Panics))?;
    registry.register(
        Route::Exact("stdin"),
        Box::new(s2w_system1::JsonClaimsEngine),
    )?;
    let mut bridge = Bridge::new(
        log,
        InMemoryVerdictStore::new(),
        registry,
        new_state(),
        BridgeConfig::default(),
    )?;
    let report = bridge.poll_once()?;
    assert_eq!(report.stats.engine_panics, 1);
    assert_eq!(report.stats.proposed_claims, 1);
    assert_eq!(report.stats.abstained.not_mine, 0);

    let records = s2w_app::bridge::evaluate_stored(&stored, &[&Panics]);
    assert_eq!(
        records[0].verdict,
        Verdict::Abstain {
            reason: AbstainReason::Panicked("engine defect".into())
        }
    );
    Ok(())
}

/// Yields one event, then a log error, once; afterwards reads normally.
struct FailsOnce {
    log: InMemoryEventLog,
    failed: Cell<bool>,
}

impl LogReader for FailsOnce {
    fn read_after(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        let events = self.log.read_after(from)?;
        if self.failed.replace(true) {
            return Ok(events);
        }
        Ok(Box::new(events.take(1).chain(std::iter::once(Err(
            LogError::Io("disk hiccup".into()),
        )))))
    }
}

#[test]
fn a_log_error_ends_the_poll_and_the_next_poll_resumes_after_the_last_consumed_event() -> TestResult
{
    let mut log = InMemoryEventLog::new();
    log.append_batch(fixture_events()?)?;
    let reader = FailsOnce {
        log,
        failed: Cell::new(false),
    };
    let state = new_state();
    let observer = state.clone();
    let mut bridge = Bridge::new(
        reader,
        InMemoryVerdictStore::new(),
        EngineRegistry::with_defaults(),
        state,
        batch_of(100),
    )?;

    let first = bridge.poll_once()?;
    assert_eq!(first.stats.consumed, 1);
    assert_eq!(first.error, Some(LogError::Io("disk hiccup".into())));
    let second = bridge.poll_once()?;
    assert_eq!(
        second.stats.consumed, 6,
        "resumes after the one event, no re-fold"
    );
    assert!(second.error.is_none());
    assert_eq!(observer.branches()?[0].head, 1);
    Ok(())
}

#[test]
fn run_drains_the_log_and_stops_on_shutdown() -> TestResult {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut log = InMemoryEventLog::new();
            log.append_batch(fixture_events()?)?;
            let state = new_state();
            let observer = state.clone();
            let config = BridgeConfig {
                poll: Duration::from_millis(5),
                max_backoff: Duration::from_millis(20),
                batch: 2,
            };
            let bridge = Bridge::new(
                log,
                InMemoryVerdictStore::new(),
                EngineRegistry::with_defaults(),
                state,
                config,
            )?;
            let (stop, shutdown) = tokio::sync::watch::channel(false);
            let task = tokio::spawn(bridge.run(shutdown));

            for _ in 0..200 {
                if observer.branches()?[0].head == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            stop.send(true)?;
            let stats = task.await??;
            assert_eq!(stats.consumed, 7);
            assert_eq!(stats.proposed_claims, 1);
            Ok(())
        })
}
