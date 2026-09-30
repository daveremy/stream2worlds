use super::*;

fn fixture() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    fs::read_to_string(root.join(FIXTURE)).unwrap()
}

fn fires(harness: &Harness, needle: &str) {
    let problems = replay(&fixture(), harness);
    assert!(problems.iter().any(|p| p.contains(needle)), "{problems:?}");
}

#[test]
fn the_recorded_fixture_replays_clean() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert_eq!(check(&root), Vec::<String>::new());
}

#[test]
fn envelopes_join_multi_line_data_and_need_an_id() {
    let sse = ": comment\nevent: message\nid: 1\ndata: {\"a\":\ndata: 2}\n\ndata: {}\n\n";
    assert_eq!(
        envelopes(sse),
        Ok(vec![serde_json::json!({"data": "{\"a\":\n2}", "id": "1"})])
    );
}

/// A profiler that drops whichever entity rule sorts first by id: a decision keyed on a name.
fn drops_first_by_name(payloads: &[&[u8]]) -> (Profile, Discovery) {
    let (profile, discovery) = real_profiler(payloads);
    let Discovery::Mapping(mut m) = discovery else {
        return (profile, discovery);
    };
    let first = m
        .entities
        .iter()
        .map(|r| r.id.clone())
        .min()
        .unwrap_or_default();
    m.entities.retain(|r| r.id != first);
    m.relationships.retain(|r| r.from != first && r.to != first);
    (profile, Discovery::Mapping(m))
}

#[test]
fn a_name_keyed_profiler_fails() {
    let harness = Harness {
        profiler: drops_first_by_name,
        ..Harness::REAL
    };
    fires(&harness, "different mapping");
}

#[test]
fn a_renaming_that_skips_decoded_strings_fails() {
    let harness = Harness {
        descend: false,
        ..Harness::REAL
    };
    fires(&harness, "survived renaming");
}

fn drop_a_relationship(m: &mut StreamMapping) {
    m.relationships.pop();
}

#[test]
fn a_changed_expectation_fails() {
    let harness = Harness {
        mutate_expected: drop_a_relationship,
        ..Harness::REAL
    };
    fires(&harness, "different mapping");
}

fn always_abstains(payloads: &[&[u8]]) -> (Profile, Discovery) {
    (
        real_profiler(payloads).0,
        Discovery::Abstain("toy".to_owned()),
    )
}

#[test]
fn an_abstaining_profiler_fails() {
    let harness = Harness {
        profiler: always_abstains,
        ..Harness::REAL
    };
    fires(&harness, "abstained");
}

#[test]
fn a_renaming_that_hashes_date_times_fails() {
    let harness = Harness {
        stamps: Stamps::Hash,
        ..Harness::REAL
    };
    fires(&harness, "Timestamp in pass A");
}
