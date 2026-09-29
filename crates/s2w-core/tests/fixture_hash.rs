//! Pins [`FOLD_FIXTURE_HASH`] to the golden fixtures' bytes (decision 0021).

use s2w_core::FOLD_FIXTURE_HASH;

fn fnv1a64(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in parts.iter().flat_map(|part| part.iter()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

#[test]
fn fold_fixture_hash_matches_the_golden_fixtures() {
    let events = include_bytes!("fixtures/golden-fold-v1.json");
    let expected = include_bytes!("fixtures/golden-fold-v1.snapshot.json");
    let actual = fnv1a64(&[events, expected]);
    assert_eq!(
        FOLD_FIXTURE_HASH, actual,
        "the golden fixtures changed, so the fold changed: set FOLD_FIXTURE_HASH in \
         crates/s2w-core/src/world.rs to {actual:#x}. Stored world snapshots then invalidate \
         and the next start replays from the log (decision 0021). Never edit the fixtures to \
         match this constant."
    );
}
