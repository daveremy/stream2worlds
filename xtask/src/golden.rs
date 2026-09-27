//! Check 6, golden replay: the fold is deterministic and resumes from a serialized prefix.
//!
//! Folds the human-owned golden log twice and requires both results to match each other and
//! the committed snapshot byte for byte. Then, for several split points, folds a prefix,
//! round-trips the world through JSON, folds the rest, and requires the same bytes again.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use s2w_core::{World, WorldEvent, fold};

pub(crate) const LOG: &str = "crates/s2w-core/tests/fixtures/golden-fold-v1.json";
pub(crate) const SNAPSHOT: &str = "crates/s2w-core/tests/fixtures/golden-fold-v1.snapshot.json";

/// The golden log folds from a hub cap of 3 so a hub fits in a log a human can read. This tests
/// the fold's behaviour at the cap, not the production default (`World::default`).
pub(crate) const HUB_CAP: u64 = 3;

const POLICY: &str = "The golden snapshot is human-owned: this check never rewrites it. If the fold change is intentional, bump FOLD_VERSION if the same log now folds differently, have a human review the new world, and update the snapshot file by hand in the same PR.";

pub(crate) fn check(root: &Path) -> Vec<String> {
    let read = |rel: &str| fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"));
    match (read(LOG), read(SNAPSHOT)) {
        (Ok(log), Ok(snapshot)) => replay(&log, &snapshot),
        (log, snapshot) => [log.err(), snapshot.err()].into_iter().flatten().collect(),
    }
}

/// The canonical bytes of a world: pretty JSON plus a trailing newline.
pub(crate) fn canonical(world: &World) -> Result<String, String> {
    serde_json::to_string_pretty(world)
        .map(|s| s + "\n")
        .map_err(|e| format!("golden replay: cannot serialize the world: {e}"))
}

pub(crate) fn replay(log: &str, snapshot: &str) -> Vec<String> {
    let events: Vec<WorldEvent> = match serde_json::from_str(log) {
        Ok(events) => events,
        Err(e) => return vec![format!("{LOG}: not a JSON array of WorldEvents: {e}")],
    };
    let mut problems = coverage(&events);

    let start = || World::with_hub_cap(HUB_CAP);
    let world = fold(start(), &events);
    if !world.entities().values().any(|e| !e.hub_refs.is_empty()) {
        problems.push(format!(
            "golden replay: folding {LOG} trips no hub cap (no entity has a hub_ref at cap {HUB_CAP}). The fixture must exercise the cap; add a target with more than {HUB_CAP} distinct sources."
        ));
    }
    let (first, second) = match (canonical(&world), canonical(&fold(start(), &events))) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            problems.extend([a.err(), b.err()].into_iter().flatten());
            return problems;
        }
    };
    if first != second {
        problems.push(format!(
            "golden replay: folding {LOG} twice gave different bytes. The fold is not deterministic; look for hash-order iteration or hidden state."
        ));
    }
    if first != snapshot {
        problems.push(format!(
            "golden replay: folding {LOG} no longer matches {SNAPSHOT} ({}). {POLICY}",
            first_difference(snapshot, &first)
        ));
    }

    for k in split_points(events.len()) {
        let (head, tail) = events.split_at(k);
        let resumed = serde_json::to_string(&fold(start(), head))
            .and_then(|json| serde_json::from_str::<World>(&json))
            .map_err(|e| e.to_string())
            .and_then(|world| canonical(&fold(world, tail)));
        match resumed {
            Ok(bytes) if bytes == first => {}
            Ok(bytes) => problems.push(format!(
                "golden replay: folding {k} events, round-tripping the world through JSON, then folding the rest differs from one uninterrupted fold ({}). Some world state is not serialized, or is serialized lossily.",
                first_difference(&first, &bytes)
            )),
            Err(e) => problems.push(format!(
                "golden replay: resuming after {k} events failed: {e}. Every World must round-trip through JSON."
            )),
        }
    }
    problems
}

/// A fixture that lacks an event variant could be hollowed out and still pass.
fn coverage(events: &[WorldEvent]) -> Vec<String> {
    let seen: BTreeSet<&str> = events.iter().map(variant).collect();
    [
        "EntityObserved",
        "RelationshipObserved",
        "EntitiesMerged",
        "MergeRevoked",
    ]
    .into_iter()
    .filter(|v| !seen.contains(v))
    .map(|v| {
        format!("{LOG}: has no {v} event. The golden log must exercise every WorldEvent variant.")
    })
    .collect()
}

fn variant(event: &WorldEvent) -> &'static str {
    match event {
        WorldEvent::EntityObserved { .. } => "EntityObserved",
        WorldEvent::RelationshipObserved { .. } => "RelationshipObserved",
        WorldEvent::EntitiesMerged { .. } => "EntitiesMerged",
        WorldEvent::MergeRevoked { .. } => "MergeRevoked",
    }
}

/// Every split from the empty prefix to the whole log. The golden log is small, so all of them.
fn split_points(len: usize) -> impl Iterator<Item = usize> {
    0..=len
}

fn first_difference(expected: &str, actual: &str) -> String {
    let line = expected
        .lines()
        .zip(actual.lines())
        .position(|(e, a)| e != a)
        .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
    format!("first difference at line {}", line + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG_TEXT: &str = include_str!("../../crates/s2w-core/tests/fixtures/golden-fold-v1.json");
    const SNAPSHOT_TEXT: &str =
        include_str!("../../crates/s2w-core/tests/fixtures/golden-fold-v1.snapshot.json");

    #[test]
    fn the_committed_fixture_passes() {
        assert_eq!(replay(LOG_TEXT, SNAPSHOT_TEXT), Vec::<String>::new());
    }

    #[test]
    fn a_changed_snapshot_fires() {
        let tampered = SNAPSHOT_TEXT.replacen("\"offset\": ", "\"offset\": 1", 1);
        let problems = replay(LOG_TEXT, &tampered);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("human-owned"));
    }

    #[test]
    fn a_hollowed_fixture_fires() {
        let problems = replay("[]", SNAPSHOT_TEXT);
        assert!(problems.iter().any(|p| p.contains("no MergeRevoked")));
        assert!(problems.iter().any(|p| p.contains("trips no hub cap")));
    }
}
