//! The live bridge against a recorded Wikimedia fixture: log → System 1 → `QueryState`, with no
//! network. Runs the same scenario over the in-memory log and the durable SQLite log.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::time::Duration;

use s2w_app::bridge::{Bridge, BridgeConfig, BridgeError, BridgeStats, EngineRegistry, Route};
use s2w_app::query::{QueryState, Timeline};
use s2w_core::{AttrValue, NaturalKey, World};
use s2w_log::{
    EventLog, InMemoryEventLog, LogError, LogPosition, LogReader, SqliteEventLog, StoredEvent,
};
use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
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

/// The seven events, in log order: five Wikimedia page changes, one `stdin` merge claim, one
/// event on a source no engine is routed for.
fn fixture_events() -> Result<Vec<RawEvent>, Box<dyn std::error::Error>> {
    let lines: Vec<&str> = FIXTURE.lines().collect();
    assert_eq!(lines.len(), 5, "fixture holds five page changes");

    // Fail loudly if a fixture edit drops the coverage the assertions below rely on.
    let parsed: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line))
        .collect::<Result<_, _>>()?;
    let kinds: Vec<&str> = parsed
        .iter()
        .map(|p| p["page_change_kind"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(kinds, ["create", "edit", "edit", "move", "delete"]);
    assert!(
        parsed.iter().all(|p| p["performer"].is_object()),
        "every page change carries a performer, so each yields three claims"
    );
    assert_eq!(
        parsed[1]["performer"]["user_id"],
        parsed[2]["performer"]["user_id"]
    );
    assert_eq!(parsed[1]["page"]["page_id"], parsed[2]["page"]["page_id"]);
    assert_eq!(parsed[1]["wiki_id"], parsed[2]["wiki_id"]);

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

fn id(world: &World, key: &str) -> Result<u64, String> {
    world
        .id_of(&NaturalKey::new(key))
        .map(s2w_core::EntityId::get)
        .ok_or_else(|| format!("no entity for {key}"))
}

fn edge_weight(world: &World, from: &str, to: &str, kind: &str) -> Result<u64, String> {
    let (from, to) = (id(world, from)?, id(world, to)?);
    Ok(world
        .relationships()
        .iter()
        .find(|(r, _)| r.from.get() == from && r.to.get() == to && r.kind == kind)
        .map_or(0, |(_, weight)| *weight))
}

fn replay_and_check<R: LogReader>(log: R) -> TestResult {
    let state = new_state();
    let observer = state.clone();
    let mut bridge = Bridge::new(log, EngineRegistry::with_defaults(), state, batch_of(4))?;

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
            proposed_claims: 5 * 3 + 1,
            unrouted: 1,
            ..BridgeStats::default()
        }
    );

    // Every claim reached the shared state: the handle a server would hold sees the head.
    assert_eq!(observer.branches()?[0].head, 16);

    let world = observer.world_at(None)?;
    // 4 users and 4 pages were minted; the merge folds the moved page and the deleted page into
    // one, leaving 7 distinct entities.
    assert_eq!(world.entities().len(), 8);
    let distinct: BTreeSet<_> = world.entities().keys().map(|&e| world.resolve(e)).collect();
    assert_eq!(distinct.len(), 7);

    let deleted = id(&world, "frwiki:page:6998844")?;
    let state_of_deleted = world
        .entities()
        .iter()
        .find(|(e, _)| e.get() == deleted)
        .map(|(_, s)| s)
        .ok_or("deleted page is still an entity")?;
    assert_eq!(
        state_of_deleted.attrs.get("deleted"),
        Some(&AttrValue::Bool(true))
    );

    assert_eq!(
        edge_weight(
            &world,
            "wikidatawiki:user:1976141",
            "wikidatawiki:page:135135298",
            "edit"
        )?,
        2,
        "the repeat edit is one edge seen twice"
    );
    assert_eq!(
        edge_weight(&world, "ptwiki:user:71355", "ptwiki:page:6733701", "move")?,
        1
    );
    assert_eq!(
        edge_weight(
            &world,
            "frwiki:user:621253",
            "frwiki:page:6998844",
            "delete"
        )?,
        1
    );
    assert_eq!(
        edge_weight(
            &world,
            "commonswiki:user:6679151",
            "commonswiki:page:200412070",
            "create"
        )?,
        1
    );

    // Order lock: the first event's three claims mint its user before its page, so the prefix
    // at offset 3 is exactly that user (id 0) and page (id 1) and their one edge.
    let prefix = observer.world_at(Some(3))?;
    assert_eq!(prefix.entities().len(), 2);
    assert_eq!(id(&prefix, "commonswiki:user:6679151")?, 0);
    assert_eq!(id(&prefix, "commonswiki:page:200412070")?, 1);
    assert_eq!(prefix.relationships().len(), 1);
    Ok(())
}

#[test]
fn bridge_replays_the_in_memory_log() -> TestResult {
    let mut log = InMemoryEventLog::new();
    log.append_batch(fixture_events()?)?;
    replay_and_check(log)
}

#[test]
fn bridge_replays_the_sqlite_log() -> TestResult {
    let directory = std::env::temp_dir().join(format!("s2w-bridge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory)?;
    let result = (|| {
        let mut log = SqliteEventLog::open(&directory)?;
        log.append_batch(fixture_events()?)?;
        replay_and_check(log)
    })();
    let _ = std::fs::remove_dir_all(&directory);
    result
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
    let mut registry = EngineRegistry::new();
    registry.register(Route::Exact("stdin"), Box::new(Panics));
    registry.register(
        Route::Exact("stdin"),
        Box::new(s2w_system1::JsonClaimsEngine),
    );
    let mut bridge = Bridge::new(log, registry, new_state(), BridgeConfig::default())?;
    let report = bridge.poll_once()?;
    assert_eq!(report.stats.engine_panics, 1);
    assert_eq!(report.stats.proposed_claims, 1);
    assert_eq!(report.stats.abstained.not_mine, 0);

    let records = s2w_app::bridge::evaluate_stored(
        &StoredEvent {
            position: first_position()?,
            event: event("stdin", 0, MERGE.as_bytes())?,
        },
        &[&Panics],
    );
    assert_eq!(
        records[0].verdict,
        Verdict::Abstain {
            reason: AbstainReason::Panicked("engine defect".into())
        }
    );
    Ok(())
}

fn first_position() -> Result<LogPosition, Box<dyn std::error::Error>> {
    let mut log = InMemoryEventLog::new();
    match log.append(event("x", 0, b"{}")?)? {
        s2w_log::AppendOutcome::Inserted(position)
        | s2w_log::AppendOutcome::Duplicate(position) => Ok(position),
    }
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
    assert_eq!(observer.branches()?[0].head, 16);
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
            let bridge = Bridge::new(log, EngineRegistry::with_defaults(), state, config)?;
            let (stop, shutdown) = tokio::sync::watch::channel(false);
            let task = tokio::spawn(bridge.run(shutdown));

            for _ in 0..200 {
                if observer.branches()?[0].head == 16 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            stop.send(true)?;
            let stats = task.await??;
            assert_eq!(stats.consumed, 7);
            assert_eq!(stats.proposed_claims, 16);
            Ok(())
        })
}
