use std::sync::Mutex;

use super::*;
use crate::tests::TestDirectory;
use s2w_model::Timestamp;

type Notes = Arc<Mutex<Vec<String>>>;

fn sink() -> (Notes, NoteSink) {
    let notes: Notes = Arc::default();
    let sink: NoteSink = {
        let notes = notes.clone();
        Arc::new(move |message: &str| notes.lock().expect("notes").push(message.to_owned()))
    };
    (notes, sink)
}

fn state() -> QueryState {
    QueryState::new(Timeline::new(crate::DEFAULT_HUB_IN_DEGREE_CAP))
}

fn append_entity(state: &QueryState, key: &str) {
    state
        .append(
            Timestamp::from_millis(1),
            s2w_core::WorldEvent::EntityObserved {
                key: s2w_core::NaturalKey::new(key),
                entity_type: "thing".to_owned(),
                attrs: Default::default(),
            },
        )
        .expect("append");
}

fn position(n: u64) -> LogPosition {
    LogPosition::from_u64(n).expect("nonzero position")
}

fn config(every: u64, shutdown_min: u64) -> SnapshotConfig {
    SnapshotConfig {
        enabled: true,
        every,
        shutdown_min,
    }
}

/// Offsets of the snapshot files written into `log_dir`, oldest first.
fn written(log_dir: &Path) -> Vec<u64> {
    store::list(&store::dir(log_dir), 7)
        .unwrap_or_default()
        .into_iter()
        .map(|(offset, _)| offset)
        .collect()
}

fn has_note(notes: &Notes, needle: &str) -> bool {
    notes
        .lock()
        .expect("notes")
        .iter()
        .any(|note| note.contains(needle))
}

#[test]
fn a_due_snapshot_is_written_at_the_checkpoint_offset() {
    let dir = TestDirectory::new("snapshotter-due");
    let (notes, sink) = sink();
    let state = state();
    append_entity(&state, "a");
    append_entity(&state, "b");
    let (_, head, _) = state.bounds().expect("bounds");
    let mut snapshotter = Snapshotter::start(dir.path(), config(2, 1), 7, sink).expect("start");
    snapshotter.after_poll(Some((position(1), 11)), 1, &state);
    assert!(written(dir.path()).is_empty(), "one event is not yet due");
    snapshotter.after_poll(Some((position(2), 22)), 1, &state);
    snapshotter.finish(&state);
    assert_eq!(written(dir.path()), vec![head]);
    let loaded = store::load_latest(&store::dir(dir.path()), 7, |_| Ok(())).expect("load");
    let (_, snapshot) = loaded.snapshot.expect("snapshot");
    assert_eq!(
        (
            snapshot.position,
            snapshot.position_event_hash,
            snapshot.feed_hash
        ),
        (2, 22, 7)
    );
    assert!(snapshot.cursors.is_empty());
    // The periodic write covered every consumed event, so the stop writes nothing more.
    assert!(has_note(&notes, "snapshot written at offset"));
    assert!(!has_note(&notes, "no final snapshot"));
}

#[test]
fn nothing_is_written_before_the_first_consumed_event() {
    let dir = TestDirectory::new("snapshotter-empty");
    let (_notes, sink) = sink();
    let state = state();
    let mut snapshotter = Snapshotter::start(dir.path(), config(1, 1), 7, sink).expect("start");
    snapshotter.after_poll(None, 0, &state);
    snapshotter.finish(&state);
    assert!(written(dir.path()).is_empty());
}

#[test]
fn a_head_that_moved_past_the_checkpoint_is_refused() {
    let dir = TestDirectory::new("snapshotter-moved");
    let (notes, sink) = sink();
    let state = state();
    append_entity(&state, "a");
    let mut snapshotter = Snapshotter::start(dir.path(), config(100, 1), 7, sink).expect("start");
    snapshotter.after_poll(Some((position(1), 11)), 1, &state);
    // Something appended after the poll without the snapshotter seeing it.
    append_entity(&state, "b");
    snapshotter.finish(&state);
    assert!(written(dir.path()).is_empty());
    assert!(has_note(
        &notes,
        "final snapshot skipped: the head is at offset"
    ));
}

#[test]
fn a_stop_below_the_threshold_writes_no_final_snapshot() {
    let dir = TestDirectory::new("snapshotter-threshold");
    let (notes, sink) = sink();
    let state = state();
    append_entity(&state, "a");
    let mut snapshotter = Snapshotter::start(dir.path(), config(100, 5), 7, sink).expect("start");
    snapshotter.after_poll(Some((position(3), 11)), 3, &state);
    snapshotter.finish(&state);
    assert!(written(dir.path()).is_empty());
    assert!(has_note(
        &notes,
        "no final snapshot: 3 events since the last one"
    ));
}

#[test]
fn a_stop_at_the_threshold_writes_a_final_snapshot() {
    let dir = TestDirectory::new("snapshotter-final");
    let (_notes, sink) = sink();
    let state = state();
    append_entity(&state, "a");
    let (_, head, _) = state.bounds().expect("bounds");
    let mut snapshotter = Snapshotter::start(dir.path(), config(100, 5), 7, sink).expect("start");
    snapshotter.after_poll(Some((position(5), 11)), 5, &state);
    snapshotter.finish(&state);
    assert_eq!(written(dir.path()), vec![head]);
}

#[test]
fn a_failed_write_leaves_the_final_snapshot_due() {
    let dir = TestDirectory::new("snapshotter-failed");
    // A file where the snapshot directory should be makes every write fail.
    let blocker = store::dir(dir.path());
    std::fs::create_dir_all(blocker.parent().expect("parent")).expect("log dir");
    std::fs::write(&blocker, b"not a directory").expect("blocker");
    let (notes, sink) = sink();
    let state = state();
    append_entity(&state, "a");
    let mut snapshotter = Snapshotter::start(dir.path(), config(1, 1), 7, sink).expect("start");
    snapshotter.after_poll(Some((position(1), 11)), 1, &state);
    snapshotter.finish(&state);
    let notes = notes.lock().expect("notes");
    let failures = notes.iter().filter(|n| n.contains("failed")).count();
    // The periodic write failed and did not count as written, so the stop tried again.
    assert_eq!(failures, 2, "{notes:?}");
}
