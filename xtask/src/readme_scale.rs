//! Check 17: the README's Technical architecture "Scale" row states the numbers in
//! `xtask/scale-baseline.toml` (s2w#324).
//!
//! The baseline file is the authority. The row is prose, so this check reads each figure out of
//! the text and compares it with the file, instead of rendering the row from the file: a
//! rendered block would put the row's wording under a generator. Each figure is the number
//! written just before a fixed phrase, inside the segment for its supply. A phrase that is
//! missing or has no number before it is a failure, never a pass.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::scale::{self, BASELINE, Baseline};

/// Reads the baseline and the README Scale row and returns one message per disagreement.
pub(super) fn check(root: &Path, table: &BTreeMap<String, String>) -> Vec<String> {
    let text = match fs::read_to_string(root.join(BASELINE)) {
        Ok(text) => text,
        Err(e) => return vec![format!("{BASELINE}: {e}")],
    };
    let baseline = match scale::parse(&text) {
        Ok(b) => b,
        Err(e) => return vec![e],
    };
    let Some(row) = table.get("Scale") else {
        return vec![
            "README.md: the Technical architecture table has no Scale row; the baseline figures have nowhere to be checked. Restore the row.".to_owned(),
        ];
    };
    judge(row, &baseline)
}

/// The number written immediately before `marker` in `seg`: digits and thousands commas.
fn number_before(seg: &str, marker: &str) -> Option<u64> {
    let head = &seg[..seg.find(marker)?];
    let token: String = head
        .trim_end()
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    token.replace(',', "").parse().ok()
}

/// The text between `from` and `to` (or the end of `row` when `to` is absent).
fn segment<'a>(row: &'a str, from: &str, to: &str) -> Option<&'a str> {
    let rest = &row[row.find(from)? + from.len()..];
    Some(rest.find(to).map_or(rest, |i| &rest[..i]))
}

type Figure<'a> = (
    &'static str,
    Option<&'a str>,
    &'static str,
    u64,
    &'static str,
);

/// Every figure the row states: (label, README segment, marker, baseline value, baseline key).
fn figures<'a>(row: &'a str, b: &Baseline) -> Vec<Figure<'a>> {
    let mut all = supply_figures(row, b);
    all.extend(other_figures(row, b));
    all
}

fn supply_figures<'a>(row: &'a str, b: &Baseline) -> Vec<Figure<'a>> {
    let synthetic = segment(row, "Synthetic:", "Recorded:");
    let recorded = segment(row, "Recorded:", "Bytes are");
    vec![
        (
            "synthetic Ir per event",
            synthetic,
            " Ir/event",
            b.ir.fold_ir_per_event,
            "[ir] fold_ir_per_event",
        ),
        (
            "synthetic bytes per entity",
            synthetic,
            " bytes per entity",
            b.memory.bytes_per_entity,
            "[memory] bytes_per_entity",
        ),
        (
            "recorded Ir per raw event",
            recorded,
            " Ir per raw event",
            b.ir.recorded.fold_ir_per_event,
            "[ir.recorded] fold_ir_per_event",
        ),
        (
            "recorded bytes per entity",
            recorded,
            " bytes per entity",
            b.memory.recorded.bytes_per_entity,
            "[memory.recorded] bytes_per_entity",
        ),
        (
            "recorded bytes per relationship",
            recorded,
            " B per relationship",
            b.memory.recorded.bytes_per_relationship_reported,
            "[memory.recorded] bytes_per_relationship_reported",
        ),
    ]
}

fn other_figures<'a>(row: &'a str, b: &Baseline) -> Vec<Figure<'a>> {
    let synthetic = segment(row, "Synthetic:", "Recorded:");
    let parse = segment(row, "Parse instructions per raw event", ". Fork cost");
    vec![
        (
            "parse Ir per raw event",
            parse,
            " Ir per raw event",
            b.parse.parse_ir_per_event,
            "[parse] parse_ir_per_event",
        ),
        (
            "planning target bytes",
            synthetic,
            " B planning figure",
            b.memory.target_bytes_per_entity,
            "[memory] target_bytes_per_entity",
        ),
        (
            "hard ceiling bytes",
            Some(row),
            " B ceiling",
            b.memory.budget_bytes_per_entity,
            "[memory] budget_bytes_per_entity",
        ),
    ]
}

fn judge(row: &str, b: &Baseline) -> Vec<String> {
    let mut problems = Vec::new();
    for (label, seg, marker, want, key) in figures(row, b) {
        let found = seg.and_then(|s| number_before(s, marker));
        if found != Some(want) {
            problems.push(format!(
                "README.md Scale row: {label} reads {} but {BASELINE} {key} is {want}. The baseline is the authority: edit the row to say {want} (the number written before \"{}\").",
                found.map_or_else(|| "nothing readable".to_owned(), |n| n.to_string()),
                marker.trim_start()
            ));
        }
    }
    problems.extend(ratios(row, b));
    problems
}

/// The two "(1.23×" ratios, recomputed from the baseline the way the row prints them.
fn ratios(row: &str, b: &Baseline) -> Vec<String> {
    let target = b.memory.target_bytes_per_entity;
    let mut problems = Vec::new();
    for (label, bytes) in [
        ("synthetic", b.memory.bytes_per_entity),
        ("recorded", b.memory.recorded.bytes_per_entity),
    ] {
        let hundredths = (bytes * 100 + target / 2) / target;
        let want = format!("({}.{:02}×", hundredths / 100, hundredths % 100);
        if !row.contains(&want) {
            problems.push(format!(
                "README.md Scale row: no \"{want}\" ratio for the {label} bytes per entity ({bytes} B over the {target} B target, {BASELINE}). Edit the ratio in the row to match."
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn committed_row() -> (String, Baseline) {
        let root = crate::workspace_root();
        let readme = fs::read_to_string(root.join("README.md")).unwrap();
        let text = fs::read_to_string(root.join(BASELINE)).unwrap();
        let row = crate::stack_table(&readme).remove("Scale").unwrap();
        (row, scale::parse(&text).unwrap())
    }

    #[test]
    fn the_committed_readme_matches_the_baseline() {
        let (row, b) = committed_row();
        assert_eq!(judge(&row, &b), Vec::<String>::new());
    }

    #[test]
    fn a_stale_bytes_figure_fails_and_names_the_key() {
        let (row, b) = committed_row();
        let stale = row.replace("369 bytes per entity", "360 bytes per entity");
        let problems = judge(&stale, &b);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("[memory] bytes_per_entity"),
            "{problems:?}"
        );
        assert!(problems[0].contains("reads 360"), "{problems:?}");
    }

    #[test]
    fn each_other_figure_and_ratio_is_checked() {
        let (row, b) = committed_row();
        for (from, to) in [
            ("5,764 Ir/event", "5,700 Ir/event"),
            ("15,285 Ir per raw event", "15,000 Ir per raw event"),
            ("409 B per relationship", "400 B per relationship"),
            ("215,248 Ir per raw event", "1 Ir per raw event"),
            ("(1.16×", "(1.20×"),
            ("600 B ceiling", "900 B ceiling"),
        ] {
            assert!(row.contains(from), "fixture lost {from}");
            let problems = judge(&row.replace(from, to), &b);
            assert_eq!(problems.len(), 1, "{from}: {problems:?}");
        }
    }

    #[test]
    fn a_missing_phrase_fails_instead_of_passing() {
        let (row, b) = committed_row();
        let problems = judge(&row.replace(" bytes per entity", " B/entity"), &b);
        assert!(!problems.is_empty());
        assert!(problems.iter().any(|p| p.contains("nothing readable")));
    }
}
