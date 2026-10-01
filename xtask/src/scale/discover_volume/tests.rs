use super::super::tests::TEXT;
use super::*;
use crate::scale::{grown_keys, parse};

fn baseline() -> Baseline {
    parse(TEXT).unwrap()
}

fn measured(heap_bytes: u64) -> VolumeMeasurement {
    VolumeMeasurement {
        heap_bytes,
        rss_bytes: 650_000_000,
        entities: 154_018,
        relationships: 1_412_074,
        links: 0,
        events: 100_000,
        window: 10_000,
        profiler_version: "9".to_owned(),
        variant: "fresh".to_owned(),
    }
}

#[test]
fn the_table_is_required_and_refuses_zeros_and_unknown_keys() {
    let text = TEXT;
    assert_eq!(baseline().discover_volume.budget_bytes, 490_000_000);
    let i = text.find("\n[discover_volume]").unwrap();
    assert!(parse(&text[..=i]).unwrap_err().contains("discover_volume"));
    let (head, tail) = text.split_at(i);
    for key in [
        "heap_bytes",
        "budget_bytes",
        "entities",
        "relationships",
        "events",
        "window",
    ] {
        let at = tail.find(&format!("\n{key} = ")).unwrap() + 1;
        let end = at + tail[at..].find('\n').unwrap();
        let zeroed = format!("{head}{}{key} = 0{}", &tail[..at], &tail[end..]);
        assert!(
            parse(&zeroed).unwrap_err().contains("[discover_volume]"),
            "{key}"
        );
    }
    assert!(parse(&text.replace("window = 10000", "window = 10000\nlinks = 0")).is_err());
    assert!(
        parse(&text.replace("rss_bytes_reported = 409000000", "rss_bytes_reported = 0")).is_ok()
    );
}

#[test]
fn passes_within_tolerance_and_reports_the_figures() {
    let (report, problems) = judge(&baseline(), &measured(410_000_000));
    assert_eq!(problems, Vec::<String>::new());
    assert!(report[0].contains("410000000 B (391.0 MiB)"), "{report:?}");
    assert!(report[1].contains("+2.5%"), "{report:?}");
}

#[test]
fn over_tolerance_fails_naming_the_key_and_the_trailer() {
    let (_, problems) = judge(&baseline(), &measured(440_000_000));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("regressed +10.0%")
            && problems[0].contains("[discover_volume] heap_bytes to 440000000")
            && problems[0].contains("Baseline-growth"),
        "{problems:?}"
    );
}

#[test]
fn over_budget_fails_even_when_the_baseline_was_raised() {
    let mut b = baseline();
    b.discover_volume.heap_bytes = 495_000_000;
    let (_, problems) = judge(&b, &measured(500_000_000));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("exceeds the hard budget 490000000 B")
            && problems[0].contains("decision 0025"),
        "{problems:?}"
    );
}

#[test]
fn a_zero_is_unknown_and_never_passes() {
    for m in [
        measured(0),
        VolumeMeasurement {
            entities: 0,
            ..measured(400_000_000)
        },
    ] {
        let (report, problems) = judge(&baseline(), &m);
        assert!(report.is_empty());
        assert!(problems[0].contains("UNKNOWN"), "{problems:?}");
    }
}

#[test]
fn a_moved_pin_says_re_measure() {
    for (m, key) in [
        (
            VolumeMeasurement {
                entities: 154_019,
                ..measured(400_000_000)
            },
            "entities = 154019",
        ),
        (
            VolumeMeasurement {
                relationships: 1,
                ..measured(400_000_000)
            },
            "relationships = 1",
        ),
        (
            VolumeMeasurement {
                events: 50_000,
                ..measured(400_000_000)
            },
            "events = 50000",
        ),
        (
            VolumeMeasurement {
                window: 5_000,
                ..measured(400_000_000)
            },
            "window = 5000",
        ),
    ] {
        let (_, problems) = judge(&baseline(), &m);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains(key) && problems[0].contains("re-measure"),
            "{problems:?}"
        );
    }
}

#[test]
fn only_the_fresh_variant_is_judged() {
    let m = VolumeMeasurement {
        variant: "repeat".to_owned(),
        ..measured(100_000_000)
    };
    let (report, problems) = judge(&baseline(), &m);
    assert!(report.is_empty());
    assert!(problems[0].contains("fresh upper bound"), "{problems:?}");
}

#[test]
fn an_improvement_past_tolerance_hints_and_tightens_heap_bytes_only() {
    let m = measured(300_000_000);
    let (report, problems) = judge(&baseline(), &m);
    assert!(problems.is_empty(), "{problems:?}");
    assert!(
        report[1].contains("cargo xtask discover-volume --tighten-baseline"),
        "{report:?}"
    );
    let text = TEXT;
    let lowered = tighten_text(text, &m).unwrap();
    assert!(
        lowered.contains("heap_bytes = 300000000 # measured"),
        "{lowered}"
    );
    assert!(lowered.contains("rss_bytes_reported = 409000000\n"));
    // [memory] and its recorded table are untouched.
    assert!(lowered.contains("bytes_per_entity = 830 # measured"));
    assert_eq!(
        parse(&lowered).unwrap().discover_volume.heap_bytes,
        300_000_000
    );
    // Never raises.
    assert_eq!(tighten_text(text, &measured(450_000_000)), None);
}

#[test]
fn raising_a_guarded_key_is_growth() {
    let base = baseline();
    assert!(grown_keys(Some(&base), &base).is_empty());
    for (key, edit) in [
        (
            "[discover_volume] heap_bytes",
            "heap_bytes = 400000000",
            "heap_bytes = 400000001",
        ),
        (
            "[discover_volume] budget_bytes",
            "budget_bytes = 490000000",
            "budget_bytes = 490000001",
        ),
        (
            "[discover_volume] events",
            "events = 100000\nwindow",
            "events = 100001\nwindow",
        ),
        (
            "[discover_volume] window",
            "window = 10000",
            "window = 10001",
        ),
    ]
    .map(|(k, from, to)| (k, (from, to)))
    {
        let raised = parse(&TEXT.replace(edit.0, edit.1)).unwrap();
        assert_eq!(grown_keys(Some(&base), &raised), vec![key]);
    }
    // The pins and the reported RSS are not guarded: they move with the code, both ways.
    let pins = parse(
        &TEXT
            .replace("entities = 154018", "entities = 154019")
            .replace(
                "rss_bytes_reported = 409000000",
                "rss_bytes_reported = 409000001",
            ),
    )
    .unwrap();
    assert!(grown_keys(Some(&base), &pins).is_empty());
    assert!(grown_keys(None, &base).contains(&"[discover_volume] heap_bytes"));
}

#[test]
fn the_json_line_parses_and_rejects_unknown_fields() {
    let line = r#"{"entities":154018,"events":100000,"heap_bytes":399243516,"links":0,"profiler_version":"9","relationships":1412074,"rss_bytes":651313152,"variant":"fresh","window":10000}"#;
    let m: VolumeMeasurement =
        crate::scale::last_json_line(&format!("noise\n{line}\ntest result: ok. 1 passed;\n"))
            .unwrap();
    assert_eq!(m.heap_bytes, 399_243_516);
    assert_eq!(m.profiler_version, "9");
    let extra = line.replace("\"window\"", "\"extra\":1,\"window\"");
    assert!(crate::scale::last_json_line::<VolumeMeasurement>(&extra).is_err());
    assert!(crate::scale::last_json_line::<VolumeMeasurement>("test result: ok.\n").is_err());
}
