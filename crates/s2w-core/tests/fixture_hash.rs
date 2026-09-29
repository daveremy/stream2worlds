//! Pins [`FOLD_FIXTURE_HASH`] to the golden fixtures' bytes (decision 0024).

use s2w_core::FOLD_FIXTURE_HASH;
use s2w_model::Fnv64;

#[test]
fn fold_fixture_hash_matches_the_golden_fixtures() {
    let events = include_bytes!("fixtures/golden-fold-v1.json");
    let expected = include_bytes!("fixtures/golden-fold-v1.snapshot.json");
    let actual = Fnv64::new().write(events).write(expected).finish();
    assert_eq!(
        FOLD_FIXTURE_HASH, actual,
        "the golden fixtures changed, so the fold changed: set FOLD_FIXTURE_HASH in \
         crates/s2w-core/src/world.rs to {actual:#x}. Stored world snapshots then invalidate \
         and the next start replays from the log (decision 0024). Never edit the fixtures to \
         match this constant."
    );
}
