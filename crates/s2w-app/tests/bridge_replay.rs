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

    // Fail loudly if a fixture edit drops the coverage the assertions below rely on: five
    // parseable JSON lines. What shape each holds does not matter — since decision 0018 this
    // traffic is unrouted and reaches no engine, so the bridge only ever treats it as bytes.
    lines
        .iter()
        .map(|line| serde_json::from_str::<serde_json::Value>(line))
        .collect::<Result<Vec<_>, _>>()?;

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

    // The same replay, per source: each unrouted source keeps its newest raw events, and the
    // routed one counts consumption with none kept.
    let per_source = bridge.source_stats();
    let preset = &per_source[&SourceId::new(WIKI)?];
    assert_eq!((preset.consumed, preset.unrouted), (5, 5));
    assert_eq!(preset.recent_unrouted.len(), 5);
    let stdin = &per_source[&SourceId::new("stdin")?];
    assert_eq!(
        (stdin.consumed, stdin.unrouted, stdin.recent_unrouted.len()),
        (1, 0, 0)
    );
    let orders = &per_source[&SourceId::new("kafka.orders")?];
    assert_eq!(
        (
            orders.consumed,
            orders.unrouted,
            orders.recent_unrouted.len()
        ),
        (1, 1, 1)
    );

    // The claim reached the shared state: the handle a server would hold sees the head.
    assert_eq!(observer.branches()?[0].head, 1);

    // The merge claim names keys no observation minted, so it folds to a no-op world: the
    // claim is served, and the world stays empty, exactly as the fold's contract says.
    let world = observer.world_at(None)?;
    assert_eq!(world.entity_count(), 0);
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

/// Per-source stats keep only the newest unrouted events, however long an unrouted stream
/// runs, so the sources view stays bounded and shows the stream is alive.
#[test]
fn per_source_stats_keep_the_newest_unrouted_events_capped() -> TestResult {
    let mut log = InMemoryEventLog::new();
    let mut events = Vec::new();
    for i in 0..25u8 {
        // Distinct payloads: the log dedupes by source plus payload content hash.
        events.push(event(WIKI, i, format!("{{\"seq\":{i}}}").as_bytes())?);
    }
    log.append_batch(events)?;
    let mut bridge = Bridge::new(
        log,
        InMemoryVerdictStore::new(),
        EngineRegistry::with_defaults(),
        new_state(),
        batch_of(100),
    )?;
    bridge.poll_once()?;

    let stats = bridge.source_stats();
    let preset = &stats[&SourceId::new(WIKI)?];
    assert_eq!((preset.consumed, preset.unrouted), (25, 25));
    // Log order internally (oldest first): the five earliest were evicted, the newest kept.
    let offsets = preset
        .recent_unrouted
        .iter()
        .map(|stored| stored.position.as_u64())
        .collect::<Vec<_>>();
    assert_eq!(offsets, (6..=25).collect::<Vec<_>>());
    assert_eq!(
        preset
            .recent_unrouted
            .back()
            .map(|stored| stored.event.payload.as_slice()),
        Some(&br#"{"seq":24}"#[..])
    );
    Ok(())
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
    registry.register(Route::Exact("stdin".to_owned()), Box::new(Panics))?;
    registry.register(
        Route::Exact("stdin".to_owned()),
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

    fn read_head(&self) -> Result<Option<LogPosition>, LogError> {
        self.log.read_head()
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

/// Answers `inner`'s real rows, plus one bogus `json_claims` row bound to a real event (so it
/// passes `judge_event`'s position/hash consistency checks) whose verdict bytes are not valid
/// JSON — engineering the `LogError::Corrupt` that `judge_event` raises from the *engine
/// verdict decode* step, which runs after the per-source counters are touched.
struct CorruptRow<V> {
    inner: V,
    at: LogPosition,
    event_hash: i64,
}

impl<V: VerdictStore> VerdictStore for CorruptRow<V> {
    fn cursor(&self) -> Result<Option<LogPosition>, LogError> {
        self.inner.cursor()
    }

    fn read_range(
        &self,
        after: Option<LogPosition>,
        through: LogPosition,
    ) -> Result<Vec<s2w_log::StoredVerdict>, LogError> {
        let mut rows = self.inner.read_range(after, through)?;
        rows.push(s2w_log::StoredVerdict {
            position: self.at,
            event_hash: self.event_hash,
            engine: "json_claims".into(),
            version: 2,
            verdict: b"not json".to_vec(),
            provenance: None,
        });
        Ok(rows)
    }

    fn commit_batch(
        &mut self,
        rows: &[s2w_log::StoredVerdict],
        through: LogPosition,
    ) -> Result<(), LogError> {
        self.inner.commit_batch(rows, through)
    }
}

/// #143 round 1: a corrupt stored verdict on one event must not leak that event's counts into
/// the batch's per-source stats. Before the fix, `judge_event` incremented `judged.per_source`
/// directly, ahead of the fallible engine-verdict decode that can `?`-return `Corrupt` for the
/// same event — so a source whose event was never durably committed still showed up with a
/// phantom `consumed` count once the batch's good prefix was absorbed.
#[test]
fn a_corrupt_verdict_row_does_not_leak_that_events_source_into_per_source_stats() -> TestResult {
    let mut log = InMemoryEventLog::new();
    log.append_batch(fixture_events()?)?;
    let events: Vec<StoredEvent> = log.read_after(None)?.collect::<Result<_, _>>()?;
    // Index 5 is the `stdin` event (see `fixture_events`): the only source `with_defaults`
    // routes to an engine, so the corrupt row can be bound to a real registered engine.
    let stdin_event = &events[5];

    let mut bridge = Bridge::new(
        log,
        CorruptRow {
            inner: InMemoryVerdictStore::new(),
            at: stdin_event.position,
            event_hash: stdin_event.content_hash,
        },
        EngineRegistry::with_defaults(),
        new_state(),
        batch_of(100),
    )?;
    let report = bridge.poll_once()?;
    assert!(
        matches!(report.error, Some(LogError::Corrupt(_))),
        "the bogus row must be reported, not silently accepted"
    );
    assert_eq!(
        report.stats.consumed, 5,
        "only the five wiki events before the corrupt row commit"
    );

    let stats = bridge.source_stats();
    assert!(
        !stats.contains_key(&SourceId::new("stdin")?),
        "stdin's event was never committed, so it must not appear in per-source stats at all"
    );
    let preset = &stats[&SourceId::new(WIKI)?];
    assert_eq!((preset.consumed, preset.unrouted), (5, 5));
    Ok(())
}

#[test]
fn resume_refuses_a_timeline_with_events_past_its_base() -> TestResult {
    let state = new_state();
    state.append(
        Timestamp::from_millis(0),
        s2w_model::WorldEvent::EntitiesMerged {
            survivor: NaturalKey::new("a"),
            absorbed: NaturalKey::new("b"),
        },
    )?;
    let refused = Bridge::resume(
        InMemoryEventLog::new(),
        InMemoryVerdictStore::new(),
        EngineRegistry::with_defaults(),
        state,
        BridgeConfig::default(),
        LogPosition::from_u64(1).ok_or("position")?,
    );
    assert!(matches!(
        refused,
        Err(BridgeError::TimelineNotEmpty { head: 1 })
    ));
    Ok(())
}

#[test]
fn resume_skips_the_covered_prefix_and_marks_the_consumed_event_hash() -> TestResult {
    let mut log = InMemoryEventLog::new();
    log.append(event("stdin", 1, MERGE.as_bytes())?)?;
    log.append(event("kafka.orders", 2, b"{\"order\":1}")?)?;
    let stored: Vec<_> = log.replay(None)?.collect::<Result<_, _>>()?;
    let mut bridge = Bridge::resume(
        log,
        InMemoryVerdictStore::new(),
        EngineRegistry::with_defaults(),
        new_state(),
        BridgeConfig::default(),
        stored[0].position,
    )?;
    assert_eq!(
        bridge.mark(),
        None,
        "resume knows the position, not its hash"
    );
    let report = bridge.poll_once()?;
    assert_eq!(
        report.stats.consumed, 1,
        "only the event after the snapshot"
    );
    assert_eq!(
        bridge.mark(),
        Some((stored[1].position, stored[1].content_hash))
    );
    Ok(())
}
