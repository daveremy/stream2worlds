use std::sync::{Arc, Mutex};

use super::*;
use crate::serve::tests::{MAPPED, accept_mapping, mapping_a, mapping_b};
use crate::tests::TestDirectory;

fn sink() -> (NoteSink, Arc<Mutex<Vec<String>>>) {
    let notes = Arc::new(Mutex::new(Vec::new()));
    let sunk = notes.clone();
    let sink: NoteSink = Arc::new(move |note: &str| {
        sunk.lock().expect("notes").push(note.to_owned());
    });
    (sink, notes)
}

struct Quiet;

impl Reporter for Quiet {
    fn flushed(&mut self, _: u64, _: u64, _: u64, _: Option<&str>) {}
    fn duplicate(&mut self, _: u64) {}
    fn note(&mut self, _: &str) {}
    fn source_error(&mut self, _: &str, _: bool) {}
}

/// A decision written between the watermark read and the rows read is applied by that check,
/// and the next check sees the watermark moved again, re-reads, and finds nothing new: the
/// watermark-then-rows order can re-read a row, never miss one.
#[test]
fn a_row_written_between_the_watermark_and_the_rows_is_never_missed() {
    let dir = TestDirectory::new("rebuild-watermark-race");
    accept_mapping(dir.path(), "p-a", mapping_a());
    let (registry, mut watcher) =
        RouteWatcher::start(dir.path().to_path_buf(), &mut Quiet, |_, _| false).expect("start");
    let fp_a = registry.feed_fingerprint();
    // Move the watermark without changing the routes, so the next check reads the rows.
    accept_mapping(dir.path(), "p-a2", mapping_a());
    let (notes, noted) = sink();
    let path = dir.path().to_path_buf();
    let change = watcher
        .check_with(
            || routes::watermark(&path),
            || {
                accept_mapping(&path, "p-b", mapping_b());
                routes::load(&path)
            },
            &notes,
        )
        .expect("B, written after the watermark read, is applied");
    let fp_b = change.registry.feed_fingerprint();
    assert_ne!(fp_a, fp_b);
    assert!(
        change
            .resolution
            .routes
            .contains_key(&SourceId::new(MAPPED).expect("source"))
    );
    // What `Rebuild::swap` records once the change is installed.
    watcher.feed = fp_b;
    watcher.resolution = change.resolution;

    assert!(watcher.check(&notes).is_none(), "B is already served");
    let noted = noted.lock().expect("notes");
    assert_eq!(
        noted
            .iter()
            .filter(
                |note| note.contains("routes: unchanged after proposal store change; no rebuild")
            )
            .count(),
        1,
        "{noted:?}"
    );
    drop(noted);
    assert!(watcher.check(&notes).is_none(), "nothing written since");
}

/// A store that cannot be read is noted once, keeps the current routes, and its recovery is
/// noted.
#[test]
fn an_unreadable_store_is_noted_once_and_its_recovery_noted() {
    let dir = TestDirectory::new("rebuild-unreadable");
    accept_mapping(dir.path(), "p-a", mapping_a());
    let (_, mut watcher) =
        RouteWatcher::start(dir.path().to_path_buf(), &mut Quiet, |_, _| false).expect("start");
    let (notes, noted) = sink();
    let fail = || Err(AppError::Usage("store locked".to_owned()));
    for _ in 0..3 {
        assert!(
            watcher
                .check_with(fail, || unreachable!(), &notes)
                .is_none()
        );
    }
    assert!(watcher.check(&notes).is_none());
    let noted = noted.lock().expect("notes");
    assert_eq!(
        noted
            .iter()
            .filter(|n| n.contains("cannot read the proposal store"))
            .count(),
        1,
        "{noted:?}"
    );
    assert!(
        noted.iter().any(|n| n.contains("readable again")),
        "{noted:?}"
    );
}

/// Only sources whose effective mapping changed to a new one are reported as rebuilding; a
/// revoked source is unrouted again, not rebuilding.
#[test]
fn rebuilding_lists_changed_sources_only() {
    let dir = TestDirectory::new("rebuild-listing");
    accept_mapping(dir.path(), "p-a", mapping_a());
    let a = routes::load(dir.path()).expect("A");
    accept_mapping(dir.path(), "p-b", mapping_b());
    let b = routes::load(dir.path()).expect("B");
    let none = Resolution::default();

    let listed = rebuilding(&a, &b, 7);
    let source = SourceId::new(MAPPED).expect("source");
    assert_eq!(
        listed.get(&source),
        Some(&Rebuilding {
            identity: b.routes[&source].identity.clone(),
            since_position: 7,
        })
    );
    assert!(rebuilding(&b, &b, 7).is_empty(), "unchanged");
    assert!(rebuilding(&b, &none, 7).is_empty(), "revoked");
}
