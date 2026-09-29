//! The snapshot golden equivalence (decision 0024): folding a log from offset 0 gives the same
//! world as restoring a snapshot taken at offset `o` and appending the tail, for every `o`
//! tried, over the human-owned golden log and over generated streams. The snapshot goes through
//! the real file format (`encode` then `decode`). No fixture is regenerated.

use std::collections::BTreeMap;

use proptest::prelude::*;
use s2w_app::query::{QueryError, TimeRange, Timeline};
use s2w_app::snapshot::{SNAPSHOT_FORMAT, SnapshotV1, codec, fold_hash, world_hash};
use s2w_core::{AttrValue, NaturalKey, World, WorldEvent, fold};
use s2w_model::Timestamp;

const GOLDEN: &str = include_str!("../../s2w-core/tests/fixtures/golden-fold-v1.json");
const GOLDEN_CAP: u64 = 3;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Appends `events` with timestamps `ts` (clamping applies) to `timeline`.
fn append_all(timeline: &mut Timeline, events: &[WorldEvent], ts: &[i64]) {
    for (event, at) in events.iter().zip(ts) {
        timeline.append(Timestamp::from_millis(*at), event.clone());
    }
}

/// The timeline restored from a snapshot written at `o`, with the tail appended.
fn restored(
    cap: u64,
    events: &[WorldEvent],
    ts: &[i64],
    o: usize,
) -> Result<Timeline, Box<dyn std::error::Error>> {
    let mut prefix = Timeline::new(cap);
    append_all(&mut prefix, events.get(..o).ok_or("o")?, ts);
    let world = prefix.head_world().clone();
    let snapshot = SnapshotV1 {
        format: SNAPSHOT_FORMAT,
        fold_hash: fold_hash(cap),
        feed_hash: 0,
        hub_cap: cap,
        offset: world.offset(),
        position: 0,
        position_event_hash: 0,
        cursors: Vec::new(),
        time: prefix.head_time(),
        world,
    };
    let decoded = codec::decode(&codec::encode(&snapshot)?)?;
    assert_eq!(decoded, snapshot);
    let mut timeline = Timeline::from_snapshot(decoded.world, decoded.time);
    append_all(
        &mut timeline,
        events.get(o..).ok_or("tail")?,
        ts.get(o..).ok_or("ts")?,
    );
    Ok(timeline)
}

/// Asserts the restored timeline serves exactly what the full one does from `o` on.
fn assert_equivalent(
    cap: u64,
    events: &[WorldEvent],
    ts: &[i64],
    o: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut full = Timeline::new(cap);
    append_all(&mut full, events, ts);
    let head = full.head();
    let expected = world_hash(&fold(World::with_hub_cap(cap), events))?;
    assert_eq!(world_hash(&full.world_at(head)?)?, expected);

    let timeline = restored(cap, events, ts, o)?;
    let base = u64::try_from(o)?;
    assert_eq!(timeline.head(), head);
    assert_eq!(world_hash(&timeline.world_at(head)?)?, expected, "o={o}");
    for at in [base, base.midpoint(head), head] {
        assert_eq!(timeline.world_at(at)?, full.world_at(at)?, "o={o} at={at}");
    }
    assert_eq!(
        timeline.time_range(),
        TimeRange {
            base,
            ..full.time_range()
        },
        "o={o}: the time index survives the snapshot, clamping included"
    );
    assert_eq!(timeline.events_after(base)?, full.events_after(base)?);
    if base > 0 {
        assert_eq!(
            timeline.world_at(base - 1),
            Err(QueryError::OffsetBeforeBase { at: base - 1, base })
        );
    }
    Ok(())
}

/// Timestamps that go backwards now and then, so clamping crosses the snapshot boundary.
fn wobbly_ts(n: usize) -> Vec<i64> {
    (0..n)
        .map(|i| {
            let i = i64::try_from(i).unwrap_or(0);
            i * 10 - if i % 3 == 1 { 25 } else { 0 }
        })
        .collect()
}

#[test]
fn golden_log_restores_to_the_same_world_at_every_split() -> TestResult {
    let events: Vec<WorldEvent> = serde_json::from_str(GOLDEN)?;
    let head = events.len();
    let ts = wobbly_ts(head);
    for o in [0, 1, head / 2, head - 1, head] {
        assert_equivalent(GOLDEN_CAP, &events, &ts, o)?;
    }
    Ok(())
}

#[test]
fn history_after_a_restore_matches_the_full_history_past_the_base() -> TestResult {
    let events: Vec<WorldEvent> = serde_json::from_str(GOLDEN)?;
    let ts = wobbly_ts(events.len());
    let o = events.len() / 2;
    let mut full = Timeline::new(GOLDEN_CAP);
    append_all(&mut full, &events, &ts);
    let timeline = restored(GOLDEN_CAP, &events, &ts, o)?;
    let (base, head) = (u64::try_from(o)?, full.head());
    let world = full.world_at(head)?;
    for id in world.entities().keys().map(|id| id.get()) {
        let tail: Vec<_> = full
            .history(id, head)?
            .into_iter()
            .filter(|entry| entry.offset > base)
            .collect();
        assert_eq!(timeline.history(id, head)?, tail, "entity {id}");
    }
    assert_eq!(
        timeline.history(0, base - 1),
        Err(QueryError::OffsetBeforeBase { at: base - 1, base })
    );
    Ok(())
}

#[test]
fn time_below_the_base_is_gone_and_at_or_after_it_is_served() -> TestResult {
    let events: Vec<WorldEvent> = serde_json::from_str(GOLDEN)?;
    let ts: Vec<i64> = (0..events.len())
        .map(|i| i64::try_from(i).unwrap_or(0) * 1000)
        .collect();
    let o = 10;
    let mut full = Timeline::new(GOLDEN_CAP);
    append_all(&mut full, &events, &ts);
    let timeline = restored(GOLDEN_CAP, &events, &ts, o)?;
    let base_last = *ts.get(o - 1).ok_or("ts")?;
    assert_eq!(
        timeline.offset_at(Timestamp::from_millis(base_last - 1)),
        Err(QueryError::TimeBeforeBase {
            ts: base_last - 1,
            base: 10
        })
    );
    for at in [base_last, base_last + 1, base_last + 1000, i64::MAX] {
        let at = Timestamp::from_millis(at);
        assert_eq!(timeline.offset_at(at)?, full.offset_at(at)?);
    }
    assert!(timeline.events_after(9).is_err());
    Ok(())
}

const KEYS: [&str; 5] = ["a", "b", "c", "d", "e"];

fn arb_key() -> impl Strategy<Value = NaturalKey> {
    prop::sample::select(&KEYS[..]).prop_map(NaturalKey::new)
}

fn arb_event() -> impl Strategy<Value = WorldEvent> {
    prop_oneof![
        (arb_key(), any::<i8>()).prop_map(|(key, n)| WorldEvent::EntityObserved {
            key,
            entity_type: "t".to_owned(),
            attrs: BTreeMap::from([("n".to_owned(), AttrValue::Int(i64::from(n)))]),
        }),
        (arb_key(), arb_key()).prop_map(|(from, to)| WorldEvent::RelationshipObserved {
            from,
            to,
            kind: "k".to_owned(),
        }),
        (arb_key(), arb_key())
            .prop_map(|(survivor, absorbed)| WorldEvent::EntitiesMerged { survivor, absorbed }),
        (arb_key(), arb_key())
            .prop_map(|(survivor, absorbed)| WorldEvent::MergeRevoked { survivor, absorbed }),
    ]
}

fn arb_log() -> impl Strategy<Value = (Vec<WorldEvent>, Vec<i64>, prop::sample::Index)> {
    prop::collection::vec((arb_event(), -50_i64..50), 0..60).prop_flat_map(|pairs| {
        let (events, ts): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
        (Just(events), Just(ts), any::<prop::sample::Index>())
    })
}

proptest! {
    #[test]
    fn generated_streams_restore_to_the_same_world(
        (events, ts, split) in arb_log(),
        cap in 1_u64..4,
    ) {
        let head = events.len();
        let chosen = split.index(head + 1);
        for o in [0, 1.min(head), head / 2, head.saturating_sub(1), head, chosen] {
            assert_equivalent(cap, &events, &ts, o)
                .map_err(|e| TestCaseError::fail(format!("split at {o} of {head}: {e}")))?;
        }
    }
}
