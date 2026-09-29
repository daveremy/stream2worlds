use super::*;

const TEXT: &str = "# header\ntolerance_percent = 5\nset_by = \"s2w#32\"\n\n[ir]\nfold_ir_per_event = 10000\nci_image = \"ubuntu-24.04\"\nevents = 100000\nrustc = \"rustc 1.98.1\"\nprofile = \"bench\"\n\n[memory]\nbytes_per_entity = 830 # measured\ntarget_bytes_per_entity = 300\nbudget_bytes_per_entity = 900\nbytes_per_relationship_reported = 234\nentities = 100000\n";

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
}

#[test]
fn ir_judge_passes_within_tolerance_and_fails_above_it() {
    let b = baseline();
    assert!(judge_ir(&b, 1_040_000_000).is_ok());
    let err = judge_ir(&b, 1_100_000_000).unwrap_err();
    assert!(
        err.contains("+10.0%") && err.contains("Baseline-growth"),
        "{err}"
    );
    let improved = judge_ir(&b, 800_000_000).unwrap();
    assert!(
        improved.contains("lower [ir] fold_ir_per_event to 8000"),
        "{improved}"
    );
}

#[test]
fn ir_judge_refuses_unknown_and_unset() {
    let mut b = baseline();
    assert!(judge_ir(&b, 0).unwrap_err().contains("UNKNOWN"));
    b.ir.fold_ir_per_event = 0;
    let err = judge_ir(&b, 885_928_832).unwrap_err();
    assert!(
        err.contains("baseline unset: measured 8860 Ir/event"),
        "{err}"
    );
}

#[test]
fn memory_judge_gates_baseline_budget_and_unknown() {
    let b = baseline();
    let (report, problems) = judge_memory(&b, &mem(830));
    assert!(problems.is_empty(), "{problems:?}");
    assert!(report[0].contains("2.77x") && report[0].contains("s2w#172"));
    let (_, problems) = judge_memory(&b, &mem(880));
    assert!(problems[0].contains("regressed +6.0%"), "{problems:?}");
    let (_, problems) = judge_memory(&b, &mem(950));
    assert!(problems.iter().any(|p| p.contains("hard budget 900")));
    let (_, problems) = judge_memory(&b, &mem(0));
    assert!(problems[0].contains("UNKNOWN"));
    let mut other = mem(830);
    other.entities = 5;
    assert!(judge_memory(&b, &other).1[0].contains("generator changed"));
    let (report, _) = judge_memory(&b, &mem(700));
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
    assert_eq!(grown_keys(None, &b).len(), 7);
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
    let lowered = tighten_text(TEXT, &mem(700)).unwrap();
    let b = parse(&lowered).unwrap();
    assert_eq!(b.memory.bytes_per_entity, 700);
    assert_eq!(b.memory.budget_bytes_per_entity, 900);
    assert_eq!(b.ir.fold_ir_per_event, 10_000);
    assert!(
        lowered.contains("bytes_per_entity = 700 # measured") && lowered.starts_with("# header")
    );
    let mut higher = mem(900);
    higher.bytes_per_relationship = 300;
    assert_eq!(tighten_text(TEXT, &higher), None);
}
