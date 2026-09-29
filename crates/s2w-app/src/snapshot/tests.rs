//! Unit tests for the snapshot format, validity rules and store (plan tests 1 and 3).

use std::collections::BTreeMap;
use std::path::PathBuf;

use s2w_core::{AttrValue, NaturalKey, World, WorldEvent, fold};
use s2w_log::{AppendOutcome, EventLog, InMemoryEventLog, LogPosition};
use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};

use super::codec::{self, MAGIC};
use super::store;
use super::{Expected, Invalid, SNAPSHOT_FORMAT, SnapshotV1, check_fold, check_log, fold_hash};
use crate::query::BaseTime;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CAP: u64 = 3;
const FEED: u64 = 0xfeed;
const EXPECTED: Expected = Expected {
    hub_cap: CAP,
    feed_hash: FEED,
};

fn events(n: usize) -> Vec<WorldEvent> {
    (0..n)
        .map(|i| WorldEvent::EntityObserved {
            key: NaturalKey::new(format!("k{}", i % 4)),
            entity_type: "t".to_owned(),
            attrs: BTreeMap::from([(
                "i".to_owned(),
                AttrValue::Int(i64::try_from(i).unwrap_or(0)),
            )]),
        })
        .collect()
}

fn snapshot_at(n: usize) -> Result<SnapshotV1, Box<dyn std::error::Error>> {
    let world = fold(World::with_hub_cap(CAP), &events(n));
    Ok(SnapshotV1 {
        format: SNAPSHOT_FORMAT,
        fold_hash: fold_hash(CAP),
        feed_hash: FEED,
        hub_cap: CAP,
        offset: world.offset(),
        position: 2,
        position_event_hash: 0,
        cursors: vec![(SourceId::new("stdin")?, Cursor::new(b"2".to_vec())?)],
        time: BaseTime {
            first_ts: Some(10),
            last_ts: Some(20),
            clamped: 1,
        },
        world,
    })
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        Self(std::env::temp_dir().join(format!(
            "s2w-snapshot-{name}-{}-{nanos}",
            std::process::id()
        )))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn encode_decode_round_trips() -> TestResult {
    let snapshot = snapshot_at(7)?;
    let bytes = codec::encode(&snapshot)?;
    assert_eq!(bytes.get(..8), Some(&MAGIC[..]));
    assert_eq!(codec::decode(&bytes)?, snapshot);
    check_fold(&snapshot, EXPECTED)?;
    Ok(())
}

#[test]
fn every_corruption_is_rejected() -> TestResult {
    let bytes = codec::encode(&snapshot_at(7)?)?;
    let mut flipped = bytes.clone();
    if let Some(byte) = flipped.get_mut(20) {
        *byte ^= 0x01;
    }
    let mut magic = bytes.clone();
    if let Some(byte) = magic.first_mut() {
        *byte = b'X';
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    let truncated = bytes.get(..bytes.len() - 1).unwrap_or_default().to_vec();
    for (what, bad) in [
        ("flipped payload byte", flipped),
        ("wrong magic", magic),
        ("trailing byte", trailing),
        ("truncated", truncated),
        ("empty", Vec::new()),
        ("header only", bytes.get(..12).unwrap_or_default().to_vec()),
    ] {
        assert!(
            matches!(codec::decode(&bad), Err(Invalid::Corrupt(_))),
            "{what} must be rejected as corrupt"
        );
    }
    Ok(())
}

#[test]
fn wrong_format_fold_or_feed_is_ignored() -> TestResult {
    let mut other_format = snapshot_at(3)?;
    other_format.format = SNAPSHOT_FORMAT + 1;
    assert_eq!(
        codec::decode(&codec::encode(&other_format)?),
        Err(Invalid::Format {
            found: SNAPSHOT_FORMAT + 1
        })
    );
    let mut other_fold = snapshot_at(3)?;
    other_fold.fold_hash ^= 1;
    assert!(matches!(
        check_fold(&other_fold, EXPECTED),
        Err(Invalid::Fold { .. })
    ));
    let other_cap = Expected {
        hub_cap: CAP + 1,
        ..EXPECTED
    };
    assert!(matches!(
        check_fold(&snapshot_at(3)?, other_cap),
        Err(Invalid::Fold { .. })
    ));
    let other_feed = Expected {
        feed_hash: FEED + 1,
        ..EXPECTED
    };
    assert!(matches!(
        check_fold(&snapshot_at(3)?, other_feed),
        Err(Invalid::Feed { .. })
    ));
    let mut inconsistent = snapshot_at(3)?;
    inconsistent.offset = 2;
    assert!(matches!(
        check_fold(&inconsistent, EXPECTED),
        Err(Invalid::Corrupt(_))
    ));
    Ok(())
}

fn raw(i: u8) -> Result<RawEvent, Box<dyn std::error::Error>> {
    Ok(RawEvent {
        source: SourceId::new("stdin")?,
        cursor: Cursor::new(vec![b'0' + i])?,
        received_at: Timestamp::from_millis(i64::from(i)),
        payload: vec![b'p', i],
    })
}

/// A log of `n` events and their positions.
fn make_log(n: u8) -> Result<(InMemoryEventLog, Vec<LogPosition>), Box<dyn std::error::Error>> {
    let mut log = InMemoryEventLog::new();
    let mut positions = Vec::new();
    for i in 0..n {
        match log.append(raw(i)?)? {
            AppendOutcome::Inserted(p) => positions.push(p),
            other => return Err(format!("unexpected {other:?}").into()),
        }
    }
    Ok((log, positions))
}

fn bound(positions: &[LogPosition], log: &InMemoryEventLog, index: usize) -> SnapshotV1 {
    let mut snapshot = SnapshotV1 {
        position: positions.get(index).map_or(0, |p| p.as_u64()),
        ..snapshot_at(3).unwrap_or_else(|e| panic!("{e}"))
    };
    snapshot.position_event_hash = log
        .replay(None)
        .ok()
        .and_then(|mut events| events.nth(index))
        .and_then(Result::ok)
        .map_or(0, |event| event.content_hash);
    snapshot
}

#[test]
fn a_snapshot_matching_the_log_is_valid() -> TestResult {
    let (log, positions) = make_log(3)?;
    let head = positions.last().copied();
    // The first event: read from the start of the log.
    check_log(&bound(&positions, &log, 0), &log, None, head)?;
    // A later event: read after its predecessor.
    check_log(
        &bound(&positions, &log, 2),
        &log,
        positions.get(1).copied(),
        head,
    )?;
    Ok(())
}

#[test]
fn a_log_mismatch_is_ignored() -> TestResult {
    let (log, positions) = make_log(3)?;
    let head = positions.last().copied();
    let mut replaced = bound(&positions, &log, 1);
    replaced.position_event_hash ^= 1;
    let previous = positions.first().copied();
    assert!(matches!(
        check_log(&replaced, &log, previous, head),
        Err(Invalid::Log(_))
    ));
    // The log ends before the snapshot's position: it was rolled back or replaced by a
    // shorter one since the snapshot was written against the longer log.
    let (longer, long_positions) = make_log(5)?;
    let rolled_back = bound(&long_positions, &longer, 4);
    assert!(matches!(
        check_log(
            &rolled_back,
            &log,
            long_positions.get(3).copied(),
            long_positions.get(4).copied()
        ),
        Err(Invalid::Log(_))
    ));
    // The verdict store's cursor has not reached the position.
    let valid = bound(&positions, &log, 1);
    assert!(matches!(
        check_log(&valid, &log, previous, previous),
        Err(Invalid::Log(_))
    ));
    assert!(matches!(
        check_log(&valid, &log, previous, None),
        Err(Invalid::Log(_))
    ));
    Ok(())
}

#[test]
fn write_is_atomic_and_load_picks_the_newest_valid() -> TestResult {
    let dir = TempDir::new("load");
    let snapshots = store::dir(&dir.0);
    assert!(
        store::load_latest(&snapshots, |_| Ok(()))?
            .snapshot
            .is_none()
    );
    for n in [2, 5, 9] {
        store::write(&snapshots, &snapshot_at(n)?)?;
    }
    let names: Vec<String> = std::fs::read_dir(&snapshots)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    assert_eq!(names.len(), 3, "no temporary file is left: {names:?}");
    // A crash mid-write leaves only a temporary name, which is never loaded.
    std::fs::write(
        snapshots.join(".snapshot-00000000000000000099.s2w.tmp"),
        b"partial",
    )?;
    // A corrupt newest file is skipped, not deleted.
    let corrupt = snapshots.join(store::file_name(50));
    std::fs::write(&corrupt, b"S2WSNAP1 garbage")?;
    let loaded = store::load_latest(&snapshots, |s| check_fold(s, EXPECTED))?;
    let (_, snapshot) = loaded.snapshot.ok_or("a valid snapshot")?;
    assert_eq!(snapshot.offset, 9);
    assert_eq!(loaded.skipped.len(), 1);
    assert!(corrupt.exists());
    // A rule the caller enforces skips down to an older file.
    let loaded = store::load_latest(&snapshots, |s| {
        if s.offset > 5 {
            Err(Invalid::Log("test".to_owned()))
        } else {
            Ok(())
        }
    })?;
    assert_eq!(loaded.snapshot.map(|(_, s)| s.offset), Some(5));
    assert_eq!(loaded.skipped.len(), 2);
    Ok(())
}

#[test]
fn a_file_name_that_disagrees_with_its_payload_is_skipped() -> TestResult {
    let dir = TempDir::new("rename");
    let snapshots = store::dir(&dir.0);
    let path = store::write(&snapshots, &snapshot_at(4)?)?;
    std::fs::rename(&path, snapshots.join(store::file_name(8)))?;
    let loaded = store::load_latest(&snapshots, |_| Ok(()))?;
    assert!(loaded.snapshot.is_none());
    assert!(matches!(
        loaded.skipped.as_slice(),
        [(_, Invalid::Corrupt(_))]
    ));
    Ok(())
}

#[test]
fn prune_keeps_the_newest_three() -> TestResult {
    let dir = TempDir::new("prune");
    let snapshots = store::dir(&dir.0);
    for n in [1, 2, 3, 4, 5] {
        store::write(&snapshots, &snapshot_at(n)?)?;
    }
    std::fs::write(snapshots.join("unrelated.txt"), b"keep me")?;
    let removed = store::prune(&snapshots, store::KEEP)?;
    assert_eq!(removed.len(), 2);
    let kept: Vec<u64> = store::list(&snapshots)?
        .into_iter()
        .map(|(o, _)| o)
        .collect();
    assert_eq!(kept, [5, 4, 3]);
    assert!(snapshots.join("unrelated.txt").exists());
    Ok(())
}

#[test]
fn clean_tmp_removes_only_crashed_writes() -> TestResult {
    let dir = TempDir::new("clean-tmp");
    let snapshots = store::dir(&dir.0);
    assert!(
        store::clean_tmp(&snapshots)?.is_empty(),
        "a missing dir has none"
    );
    store::write(&snapshots, &snapshot_at(2)?)?;
    let stale = snapshots.join(format!(".{}.tmp", store::file_name(3)));
    std::fs::write(&stale, b"half a snapshot")?;
    for other in [".snapshot-3.s2w.tmp", "snapshot-x.tmp", "notes.tmp"] {
        std::fs::write(snapshots.join(other), b"not ours")?;
    }
    assert_eq!(store::clean_tmp(&snapshots)?, std::slice::from_ref(&stale));
    assert!(!stale.exists());
    assert!(snapshots.join(store::file_name(2)).exists());
    for other in [".snapshot-3.s2w.tmp", "snapshot-x.tmp", "notes.tmp"] {
        assert!(
            snapshots.join(other).exists(),
            "{other} is not ours to remove"
        );
    }
    Ok(())
}

/// The borrowed snapshot encodes to exactly the owned one's bytes, and the single-buffer encode
/// lays them out as `MAGIC | len | payload | FNV` (#179). Pins field order and the slice/`Vec`
/// and `&World`/`World` equivalence the writer relies on.
#[test]
fn a_borrowed_snapshot_encodes_to_the_owned_snapshots_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let snapshot = snapshot_at(12)?;
    let payload = postcard::to_stdvec(&snapshot)?;
    assert_eq!(postcard::to_stdvec(&snapshot.as_ref_v1())?, payload);
    let mut expected = MAGIC.to_vec();
    expected.extend_from_slice(&u32::try_from(payload.len())?.to_le_bytes());
    expected.extend_from_slice(&payload);
    expected.extend_from_slice(&super::fnv1a64(&payload).to_le_bytes());
    assert_eq!(codec::encode_ref(&snapshot.as_ref_v1())?, expected);
    assert_eq!(codec::encode(&snapshot)?, expected);
    Ok(())
}

/// `EntityState::attrs` is an `AttrMap` in memory (s2w#172) but must stay a `BTreeMap` on the
/// wire: the same postcard bytes (snapshots, `world_hash`) and the same JSON, and a snapshot
/// written by either decodes into the other.
#[test]
fn attr_map_bytes_are_btreemap_bytes() -> TestResult {
    let many: BTreeMap<String, AttrValue> = [
        ("zeta", AttrValue::Str("last".into())),
        ("alpha", AttrValue::Int(i64::MIN)),
        ("mid", AttrValue::Bool(false)),
        ("attr_07", AttrValue::Str(String::new())),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let one = BTreeMap::from([("k".to_owned(), AttrValue::Int(1))]);
    for source in [BTreeMap::new(), one, many] {
        let attrs = s2w_core::AttrMap::from(source.clone());
        let bytes = postcard::to_stdvec(&source)?;
        assert_eq!(postcard::to_stdvec(&attrs)?, bytes);
        assert_eq!(serde_json::to_vec(&attrs)?, serde_json::to_vec(&source)?);
        assert_eq!(postcard::from_bytes::<s2w_core::AttrMap>(&bytes)?, attrs);
    }
    Ok(())
}
