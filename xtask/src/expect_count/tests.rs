//! Prove-it-fires: check 18's counter, judges, growth rule and tightening.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    Baseline, Counts, count_source, grown, growth_finding, judge, render, rust_files, tighten_to,
};

/// Test sources spell the attribute `EXPECT`, so the human instrument (`git grep` for the
/// attribute's text) never counts them; this restores the real name before parsing.
fn count(src: &str) -> Result<Vec<(String, usize)>, String> {
    count_source(&src.replace("EXPECT", "expect"))
}

fn lints(src: &str) -> Vec<String> {
    count(src)
        .unwrap()
        .into_iter()
        .map(|(lint, _)| lint)
        .collect()
}

fn map(rows: &[(&str, usize)]) -> BTreeMap<String, usize> {
    rows.iter().map(|(l, n)| ((*l).to_owned(), *n)).collect()
}

fn counts(rows: &[(&str, usize)]) -> Counts {
    rows.iter()
        .map(|(l, n)| {
            let sites = (1..=*n).map(|i| format!("src/lib.rs:{i}")).collect();
            ((*l).to_owned(), sites)
        })
        .collect()
}

#[test]
fn counts_outer_inner_statement_field_and_expression_attributes() {
    let src = r#"
#![EXPECT(missing_docs, reason = "inner")]
#[EXPECT(clippy::too_many_lines, reason = "outer")]
fn f() {
    #[EXPECT(unused_variables, reason = "statement")]
    let x = 1;
    let _ = #[EXPECT(unused_parens, reason = "expression")] (1);
}
struct S {
    #[EXPECT(dead_code, reason = "field")]
    a: u8,
}
"#;
    let found = count(src).unwrap();
    let names: Vec<&str> = found.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(
        names,
        [
            "missing_docs",
            "clippy::too_many_lines",
            "unused_variables",
            "unused_parens",
            "dead_code"
        ]
    );
    assert_eq!(found[0].1, 2, "{found:?}");
    assert_eq!(found[1].1, 3, "{found:?}");
}

#[test]
fn counts_cfg_attr_arms_and_each_lint_of_a_two_lint_attribute() {
    let src = r#"
#[cfg_attr(test, expect(dead_code, reason = "t"))]
fn a() {}
#[cfg_attr(all(), cfg_attr(test, expect(clippy::box_collection)), inline)]
fn b() {}
#[EXPECT(clippy::cast_precision_loss, clippy::too_many_arguments, reason = "two")]
fn c() {}
"#;
    assert_eq!(
        lints(src),
        [
            "dead_code",
            "clippy::box_collection",
            "clippy::cast_precision_loss",
            "clippy::too_many_arguments"
        ]
    );
}

#[test]
fn ignores_expect_in_strings_comments_and_other_attributes() {
    let src = r##"
/// Write `#[EXPECT(dead_code)]` to silence it.
// #[EXPECT(dead_code)]
#[allow(dead_code)]
#[cfg(test)]
fn f() -> &'static str { "#[EXPECT(dead_code, reason = \"no\")]" }
"##;
    assert!(lints(src).is_empty(), "{:?}", lints(src));
}

#[test]
fn an_unparsable_file_is_an_error() {
    assert!(count("fn f( {").is_err());
    assert!(count("#[EXPECT(= 1)] fn f() {}").is_err());
}

#[test]
fn fires_on_growth_and_on_a_lint_missing_from_the_baseline() {
    let base = map(&[("dead_code", 1)]);
    let (over, under) = judge(&base, &counts(&[("dead_code", 2), ("missing_docs", 1)]));
    assert!(under.is_empty(), "{under:?}");
    assert_eq!(over.len(), 2, "{over:?}");
    assert!(
        over[0].starts_with(
            "2 #[expect] naming dead_code vs baseline 1: fix the lint at src/lib.rs:1, src/lib.rs:2"
        ),
        "{over:?}"
    );
    assert!(
        over[1].starts_with("1 #[expect] naming missing_docs vs baseline 0"),
        "{over:?}"
    );
    assert!(over[1].contains("Baseline-growth: s2w#<N>"), "{over:?}");
}

#[test]
fn fires_on_stale_slack_including_a_lint_gone_to_zero() {
    let base = map(&[("dead_code", 3), ("missing_docs", 1)]);
    let (over, under) = judge(&base, &counts(&[("dead_code", 2)]));
    assert!(over.is_empty(), "{over:?}");
    assert_eq!(
        under,
        [
            "dead_code is at 2, baseline 3: run cargo xtask check --tighten-baseline and commit xtask/expect-baseline.toml",
            "missing_docs is at 0, baseline 1: run cargo xtask check --tighten-baseline and commit xtask/expect-baseline.toml"
        ]
    );
    let (over, under) = judge(&base, &counts(&[("dead_code", 3), ("missing_docs", 1)]));
    assert!(over.is_empty() && under.is_empty());
}

#[test]
fn growth_needs_a_trailer() {
    let base = map(&[("dead_code", 2), ("missing_docs", 1)]);
    let now = map(&[
        ("dead_code", 3),
        ("missing_docs", 0),
        ("clippy::box_collection", 1),
    ]);
    let grew = grown(&base, &now);
    assert_eq!(grew, ["clippy::box_collection", "dead_code"]);
    assert!(grown(&base, &map(&[("dead_code", 1)])).is_empty());
    // An absent base (the file is new) makes every entry growth.
    assert_eq!(grown(&BTreeMap::new(), &base).len(), 2);

    assert!(growth_finding(&grew, Ok("fix: x\n\nBaseline-growth: s2w#156\n".into())).is_none());
    let refused = growth_finding(&grew, Ok("fix: x\n\nBaseline-growth: s2w#\n".into())).unwrap();
    assert!(
        refused.starts_with("expect baseline grew (clippy::box_collection, dead_code)"),
        "{refused}"
    );
    let unread = growth_finding(&grew, Err("bad revision".into())).unwrap();
    assert!(
        unread.ends_with("; cannot read commit range: bad revision"),
        "{unread}"
    );
}

#[test]
fn tightening_lowers_drops_zeros_and_never_raises() {
    let base = map(&[
        ("dead_code", 3),
        ("missing_docs", 1),
        ("clippy::box_collection", 2),
    ]);
    let actual = map(&[("dead_code", 2), ("clippy::box_collection", 5)]);
    assert_eq!(
        tighten_to(&base, &actual),
        map(&[("dead_code", 2), ("clippy::box_collection", 2)])
    );
}

#[test]
fn render_round_trips_through_the_baseline_parser() {
    let lints = map(&[("clippy::too_many_lines", 32), ("dead_code", 7)]);
    let text = render(&lints);
    assert!(text.starts_with("# Shrink-only (s2w#156)"), "{text}");
    let parsed: Baseline = toml::from_str(&text).unwrap();
    assert_eq!(parsed.lints, lints);
}

#[test]
fn the_committed_baseline_is_in_rendered_form() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join("expect-baseline.toml")).unwrap();
    let parsed: Baseline = toml::from_str(&text).unwrap();
    assert_eq!(render(&parsed.lints), text);
}

#[test]
fn the_walk_finds_nested_files_and_skips_build_and_hidden_dirs() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "s2w-expects-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    for name in [
        "src/lib.rs",
        "src/a/b.rs",
        "tests/t.rs",
        "build.rs",
        "target/debug/x.rs",
        "web/node_modules/y.rs",
        ".hidden/z.rs",
        "src/notes.txt",
    ] {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
    }
    let mut files = BTreeSet::new();
    rust_files(&dir, &mut files).unwrap();
    let found: Vec<String> = files
        .iter()
        .map(|p| p.strip_prefix(&dir).unwrap().display().to_string())
        .collect();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        found,
        ["build.rs", "src/a/b.rs", "src/lib.rs", "tests/t.rs"]
    );
}
