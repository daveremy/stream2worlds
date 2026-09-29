use super::*;

const TEXT: &str = "# header\ntolerance_percent = 5\nset_by = \"s2w#32\"\n\n[recorded]\nfixture_fnv1a64 = 0x10\n\n[ir]\nfold_ir_per_event = 10000\nci_image = \"ubuntu-24.04\"\nevents = 100000\nrustc = \"rustc 1.98.1\"\nprofile = \"bench\"\n\n[ir.recorded]\nfold_ir_per_event = 20000\nevents = 2\n\n[memory]\nbytes_per_entity = 830 # measured\ntarget_bytes_per_entity = 300\nbudget_bytes_per_entity = 900\nbytes_per_relationship_reported = 234\nentities = 100000\n\n[memory.recorded]\nbytes_per_entity = 400 # recorded\nbytes_per_relationship_reported = 500\nentities = 3\nrelationships = 4\n";

/// Two complete events and a comment: what [`judge_fixture`] counts.
const FIXTURE_BYTES: &[u8] = b": header\nid: 1\ndata: {}\n\nid: 2\ndata: {}\n\n";

fn recorded_mem(bytes: u64) -> MemMeasurement {
    MemMeasurement {
        bytes_per_entity: bytes,
        bytes_per_relationship: 500,
        entities: 3,
        relationships: 4,
    }
}

fn baseline() -> Baseline {
    parse(TEXT).unwrap()
}

fn mem(bytes: u64) -> MemMeasurement {
    MemMeasurement {
        bytes_per_entity: bytes,
        bytes_per_relationship: 234,
        entities: 100_000,
        relationships: 99_887,
    }
}

#[test]
fn parse_requires_every_field_and_refuses_unknown_ones() {
    assert_eq!(baseline().memory.budget_bytes_per_entity, 900);
    let missing = TEXT.replace("budget_bytes_per_entity = 900\n", "");
    assert!(
        parse(&missing)
            .unwrap_err()
            .contains("budget_bytes_per_entity")
    );
    let renamed = TEXT.replace("fold_ir_per_event", "fold_ir");
    assert!(parse(&renamed).is_err());
    assert!(parse(&TEXT.replace("\"bench\"", "\"release\"")).is_err());
    assert!(parse(&TEXT.replace("tolerance_percent = 5", "tolerance_percent = 0")).is_err());
    assert!(parse(&TEXT.replace("relationships = 4", "relationships = 0")).is_err());
}

#[test]
fn ir_judge_passes_within_tolerance_and_fails_above_it() {
    let b = baseline();
    assert!(judge_ir(&b, Supply::Synthetic, 1_040_000_000).is_ok());
    let err = judge_ir(&b, Supply::Synthetic, 1_100_000_000).unwrap_err();
    assert!(
        err.contains("+10.0%") && err.contains("Baseline-growth"),
        "{err}"
    );
    let improved = judge_ir(&b, Supply::Synthetic, 800_000_000).unwrap();
    assert!(
        improved.contains("lower [ir] fold_ir_per_event to 8000"),
        "{improved}"
    );
}

#[test]
fn ir_judge_refuses_unknown_and_unset() {
    let mut b = baseline();
    assert!(
        judge_ir(&b, Supply::Synthetic, 0)
            .unwrap_err()
            .contains("UNKNOWN")
    );
    b.ir.fold_ir_per_event = 0;
    let err = judge_ir(&b, Supply::Synthetic, 885_928_832).unwrap_err();
    assert!(
        err.contains("baseline unset: measured 8860 Ir/event"),
        "{err}"
    );
}

#[test]
fn memory_judge_gates_baseline_budget_and_unknown() {
    let b = baseline();
    let (report, problems) = judge_memory(&b, Supply::Synthetic, &mem(830));
    assert!(problems.is_empty(), "{problems:?}");
    assert!(report[0].contains("2.77x") && report[0].contains("s2w#172"));
    let (_, problems) = judge_memory(&b, Supply::Synthetic, &mem(880));
    assert!(problems[0].contains("regressed +6.0%"), "{problems:?}");
    let (_, problems) = judge_memory(&b, Supply::Synthetic, &mem(950));
    assert!(problems.iter().any(|p| p.contains("hard budget 900")));
    let (_, problems) = judge_memory(&b, Supply::Synthetic, &mem(0));
    assert!(problems[0].contains("UNKNOWN"));
    let mut other = mem(830);
    other.entities = 5;
    assert!(judge_memory(&b, Supply::Synthetic, &other).1[0].contains("generator changed"));
    let (report, _) = judge_memory(&b, Supply::Synthetic, &mem(700));
    assert!(report.iter().any(|r| r.contains("--tighten-baseline")));
}

#[test]
fn last_json_line_needs_a_json_line_with_the_fields() {
    let out = "running 1 test\n{\"bytes_per_entity\":830,\"bytes_per_relationship\":234,\"entities\":100000,\"relationships\":99887}\ntest result: ok\n";
    assert_eq!(last_json_line::<MemMeasurement>(out).unwrap(), mem(830));
    assert!(
        last_json_line::<MemMeasurement>("running 0 tests\ntest result: ok. 0 passed\n")
            .unwrap_err()
            .contains("no JSON line")
    );
    assert!(last_json_line::<MemMeasurement>("{\"other\":1}\n").is_err());
}

#[test]
fn summary_ir_reads_the_callgrind_total() {
    let json = r#"{"profiles":[{"tool":"Callgrind","data":{"total":{"metrics":{"Ir":{"values":{"new":885928832}}}}}}]}"#;
    assert_eq!(summary_ir(json).unwrap(), 885_928_832);
    assert!(summary_ir(&json.replace("Callgrind", "DHAT")).is_err());
    assert!(summary_ir(&json.replace("\"Ir\"", "\"Instructions\"")).is_err());
    assert!(summary_ir("not json").is_err());
}

#[test]
fn growth_counts_raised_and_new_values() {
    let b = baseline();
    assert_eq!(grown_keys(Some(&b), &b), Vec::<&str>::new());
    assert_eq!(grown_keys(None, &b).len(), 11);
    let mut raised = b.clone();
    raised.memory.budget_bytes_per_entity = 1000;
    raised.tolerance_percent = 6;
    raised.ir.fold_ir_per_event = 9000;
    assert_eq!(
        grown_keys(Some(&b), &raised),
        ["[memory] budget_bytes_per_entity", "tolerance_percent"]
    );
}

#[test]
fn raising_the_memory_target_is_growth() {
    let b = baseline();
    let mut raised = b.clone();
    raised.memory.target_bytes_per_entity = 830;
    assert_eq!(
        grown_keys(Some(&b), &raised),
        ["[memory] target_bytes_per_entity"]
    );
    let mut lowered = b.clone();
    lowered.memory.target_bytes_per_entity = 250;
    assert!(grown_keys(Some(&b), &lowered).is_empty());
}

#[test]
fn raising_a_measurement_size_is_growth() {
    let b = baseline();
    let mut raised = b.clone();
    raised.ir.events += 1;
    raised.memory.entities += 1;
    assert_eq!(
        grown_keys(Some(&b), &raised),
        ["[ir] events", "[memory] entities"]
    );
}

#[test]
fn memory_json_rejects_unknown_fields() {
    let line = "{\"bytes_per_entity\":1,\"bytes_per_relationship\":1,\"entities\":1,\"relationships\":1,\"extra\":1}\n";
    assert!(last_json_line::<MemMeasurement>(line).is_err());
}

#[test]
fn tighten_lowers_memory_values_only_and_keeps_comments() {
    let lowered = tighten_text(TEXT, Supply::Synthetic, &mem(700)).unwrap();
    let b = parse(&lowered).unwrap();
    assert_eq!(b.memory.bytes_per_entity, 700);
    assert_eq!(b.memory.budget_bytes_per_entity, 900);
    assert_eq!(b.ir.fold_ir_per_event, 10_000);
    assert!(
        lowered.contains("bytes_per_entity = 700 # measured") && lowered.starts_with("# header")
    );
    assert_eq!(
        b.memory.recorded.bytes_per_entity, 400,
        "the other supply's table"
    );
    let mut higher = mem(900);
    higher.bytes_per_relationship = 300;
    assert_eq!(tighten_text(TEXT, Supply::Synthetic, &higher), None);
}

#[test]
fn the_fixture_pin_refuses_changed_bytes_and_a_changed_count() {
    let mut b = baseline();
    b.recorded.fixture_fnv1a64 = s2w_model::Fnv64::new().write(FIXTURE_BYTES).finish();
    assert_eq!(judge_fixture(&b, FIXTURE_BYTES), Ok(()));
    let changed = judge_fixture(&b, b"id: 1\ndata: {}\n\n").unwrap_err();
    assert!(
        changed.contains("UNKNOWN") && changed.contains("human-owned"),
        "{changed}"
    );
    b.ir.recorded.events = 3;
    let count = judge_fixture(&b, FIXTURE_BYTES).unwrap_err();
    assert!(
        count.contains("holds 2 events but [ir.recorded] events = 3"),
        "{count}"
    );
}

#[test]
fn the_recorded_supply_is_judged_against_its_own_tables() {
    let mut b = baseline();
    let line = judge_ir(&b, Supply::Recorded, 40_000).unwrap();
    assert!(
        line.starts_with("fold Ir (recorded): 20000 Ir/event"),
        "{line}"
    );
    let err = judge_ir(&b, Supply::Recorded, 44_000).unwrap_err();
    assert!(
        err.contains("raise [ir.recorded] fold_ir_per_event"),
        "{err}"
    );
    b.ir.recorded.fold_ir_per_event = 0;
    let unset = judge_ir(&b, Supply::Recorded, 40_000).unwrap_err();
    assert!(
        unset.contains("set [ir.recorded] fold_ir_per_event = 20000"),
        "{unset}"
    );

    let (report, problems) = judge_memory(&b, Supply::Recorded, &recorded_mem(400));
    assert!(problems.is_empty() && report[0].starts_with("bytes/entity (recorded): 400 B"));
    let (_, problems) = judge_memory(&b, Supply::Recorded, &recorded_mem(430));
    assert!(
        problems[0].contains("[memory.recorded] bytes_per_entity"),
        "{problems:?}"
    );
    let (_, problems) = judge_memory(&b, Supply::Recorded, &recorded_mem(950));
    assert!(problems.iter().any(|p| p.contains("hard budget 900")));
    let mut dropped = recorded_mem(400);
    dropped.relationships = 3;
    let (_, problems) = judge_memory(&b, Supply::Recorded, &dropped);
    assert!(
        problems[0].contains("[memory.recorded] pins 3 entities and 4 relationships"),
        "{problems:?}"
    );
}

#[test]
fn recorded_keys_count_as_growth_and_tighten_separately() {
    let b = baseline();
    let mut raised = b.clone();
    raised.ir.recorded.fold_ir_per_event += 1;
    raised.memory.recorded.bytes_per_entity += 1;
    raised.ir.recorded.events += 1;
    raised.memory.recorded.entities += 1;
    assert_eq!(
        grown_keys(Some(&b), &raised),
        [
            "[ir.recorded] fold_ir_per_event",
            "[memory.recorded] bytes_per_entity",
            "[ir.recorded] events",
            "[memory.recorded] entities"
        ]
    );
    let lowered = tighten_text(TEXT, Supply::Recorded, &recorded_mem(350)).unwrap();
    let t = parse(&lowered).unwrap();
    assert_eq!(
        (
            t.memory.recorded.bytes_per_entity,
            t.memory.bytes_per_entity
        ),
        (350, 830)
    );
    assert!(lowered.contains("bytes_per_entity = 350 # recorded"));
}
