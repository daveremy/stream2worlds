//! The entity types discovery finds on the recorded fixture, and each type's share of the world
//! events it produces (s2w#282). A standing check on profiler drift: the committed mapping
//! (`recorded.mapping.json`) is pinned, so no other per-PR test sees what `discover` itself now
//! makes of the fixture. s2w#282 found a profiler change (#261) that promoted edit counters, byte
//! sizes and free text to entity types and doubled the head world (+6.1M world events, +347 MiB)
//! with every per-PR test green.
//!
//! The test discovers a mapping over the fixture's first [`DISCOVER_WINDOW`] events (as serve and
//! `backfill_memory` do), evaluates every fixture event through it, and counts, for each entity
//! type: its `EntityObserved` events and the `RelationshipObserved` events with that type at
//! either end. It prints the table, then compares the set of types with the committed baseline
//! `tests/discovered_types.baseline.txt` and fails on any type added or removed, naming
//! each one with its share. A new type is not wrong by itself: if the change is intended, update
//! the baseline in the same PR (`S2W_DISCOVERED_TYPES_BLESS=1` rewrites it) so the reviewer sees
//! the new type and what it costs.
//!
//! Shares are of all world events one pass over the fixture produces. A relationship counts
//! toward both of its endpoints' types, so the shares add up to more than 100%. The
//! `backfill_memory` load cycles the same fixture, so its shares are the same and its world events
//! scale by 1.5x10^5 / 11,667. The baseline is derived output, not a fixture: unlike
//! `tests/fixtures/`, updating it in the PR that changes discovery is the intended workflow.

#[path = "support/recorded.rs"]
#[expect(
    dead_code,
    reason = "this test uses the loader, not the committed mapping"
)]
mod recorded;

// `allow-unwrap-in-tests` applies inside `#[cfg(test)]` items only.
#[cfg(test)]
mod discovered_types {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Write as _;

    use s2w_app::discover::DISCOVER_WINDOW;
    use s2w_discover::{Config, Discovery, discover};
    use s2w_model::{NaturalKey, WorldEvent};
    use s2w_system1::{Engine, MappingEngine, Verdict};

    use super::recorded::{Fallible, load};

    const BASELINE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/discovered_types.baseline.txt"
    );
    const BLESS: &str = "S2W_DISCOVERED_TYPES_BLESS";
    /// The type column for a relationship endpoint no `EntityObserved` named.
    const UNOBSERVED: &str = "(unobserved key)";

    #[derive(Default)]
    struct Row {
        entity_events: u64,
        relationship_events: u64,
    }

    /// Per entity type counts over one pass of the fixture, and the pass's world events.
    struct Tally {
        rows: BTreeMap<String, Row>,
        world_events: u64,
    }

    impl Tally {
        /// `row`'s share of the pass's world events, in percent.
        fn share(&self, row: &Row) -> f64 {
            100.0 * (row.entity_events + row.relationship_events) as f64
                / self.world_events.max(1) as f64
        }
    }

    /// Discovers a mapping over the fixture's discovery window and tallies every event's claims.
    fn tally_fixture() -> Fallible<(usize, Tally)> {
        let events = load()?;
        let payloads: Vec<&[u8]> = events[..DISCOVER_WINDOW]
            .iter()
            .map(|e| e.payload.as_slice())
            .collect();
        let mapping = match discover(&payloads, &Config::default()).1 {
            Discovery::Mapping(mapping) => mapping,
            Discovery::Abstain(reason) => panic!("discovery abstained on the fixture: {reason}"),
        };
        let engine = MappingEngine::new(mapping)?;
        let mut claims: Vec<WorldEvent> = Vec::new();
        for event in events {
            if let Verdict::Propose { claims: c, .. } = engine.evaluate(event) {
                claims.extend(c);
            }
        }
        Ok((events.len(), tally(&claims)))
    }

    fn tally(claims: &[WorldEvent]) -> Tally {
        // Key -> type, from every observation (the latest wins, as in the fold).
        let mut key_type: BTreeMap<&NaturalKey, &str> = BTreeMap::new();
        for claim in claims {
            if let WorldEvent::EntityObserved {
                key, entity_type, ..
            } = claim
            {
                key_type.insert(key, entity_type);
            }
        }
        let mut rows: BTreeMap<String, Row> = BTreeMap::new();
        for claim in claims {
            match claim {
                WorldEvent::EntityObserved { entity_type, .. } => {
                    rows.entry(entity_type.clone()).or_default().entity_events += 1;
                }
                WorldEvent::RelationshipObserved { from, to, .. } => {
                    let ends: BTreeSet<&str> = [from, to]
                        .into_iter()
                        .map(|k| key_type.get(k).copied().unwrap_or(UNOBSERVED))
                        .collect();
                    for end in ends {
                        rows.entry(end.to_owned()).or_default().relationship_events += 1;
                    }
                }
                WorldEvent::EntitiesMerged { .. } | WorldEvent::MergeRevoked { .. } => {}
            }
        }
        Tally {
            rows,
            world_events: claims.len() as u64,
        }
    }

    /// The table, one row per type, largest share first.
    fn table(raw_events: usize, t: &Tally) -> Fallible<String> {
        let mut out = format!(
            "{raw_events} fixture events -> {} world events, {} entity types\n\
             share\tentity\trelationship\ttype\n",
            t.world_events,
            t.rows.len()
        );
        let mut by_share: Vec<(&String, &Row)> = t.rows.iter().collect();
        by_share.sort_by(|a, b| t.share(b.1).total_cmp(&t.share(a.1)).then(a.0.cmp(b.0)));
        for (ty, row) in by_share {
            writeln!(
                out,
                "{:5.1}%\t{}\t{}\t{ty}",
                t.share(row),
                row.entity_events,
                row.relationship_events
            )?;
        }
        Ok(out)
    }

    /// Every type added to or removed from `baseline`, one line each; empty when they match.
    fn drift(t: &Tally, baseline: &BTreeSet<&str>) -> Fallible<String> {
        let found: BTreeSet<&str> = t.rows.keys().map(String::as_str).collect();
        let mut out = String::new();
        for ty in found.difference(baseline) {
            let row = &t.rows[*ty];
            writeln!(
                out,
                "  added   {ty}: {:.1}% of world events ({} entity, {} relationship)",
                t.share(row),
                row.entity_events,
                row.relationship_events
            )?;
        }
        for ty in baseline.difference(&found) {
            writeln!(out, "  removed {ty}")?;
        }
        Ok(out)
    }

    #[test]
    fn discovered_entity_types_match_the_baseline() -> Fallible<()> {
        let (raw_events, t) = tally_fixture()?;
        println!("{}", table(raw_events, &t)?);
        if std::env::var_os(BLESS).is_some() {
            let mut body = String::from(
                "# Entity types `discover` finds on the recorded fixture (tests/discovered_types.rs).\n\
                 # Rewritten by S2W_DISCOVERED_TYPES_BLESS=1; review every added line's cost.\n",
            );
            for ty in t.rows.keys() {
                writeln!(body, "{ty}")?;
            }
            std::fs::write(BASELINE, body)?;
            return Ok(());
        }
        let text = std::fs::read_to_string(BASELINE)?;
        let baseline: BTreeSet<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        let drift = drift(&t, &baseline)?;
        assert!(
            drift.is_empty(),
            "discovered entity types differ from {BASELINE}:\n{drift}\
             If intended, rerun with {BLESS}=1 and commit the baseline; say in the PR what each \
             added type is and whether it pays for its share (s2w#282)."
        );
        Ok(())
    }
}
