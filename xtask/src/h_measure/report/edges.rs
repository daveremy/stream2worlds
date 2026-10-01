//! The report's relationship section (contract B3 "Relationships"): the edge rows for the
//! mapping and both ceilings, one row per key edge type, and the counts behind them. A key
//! without relationships prints one line, so a format-2 report changes by that line only.

use super::super::edges::{EdgeScore, Prf, TypeRow};
use super::super::grade::Grade;
use super::super::score::shown;
use super::table;

const ROWS: [&str; 9] = [
    "row",
    "key edges",
    "predicted",
    "TP",
    "FP",
    "FN",
    "P",
    "R",
    "F1",
];

const TYPES: [&str; 10] = [
    "key edge type",
    "key edges",
    "aligned predicted type",
    "TP",
    "FP",
    "FN",
    "P",
    "R",
    "F1",
    "ceiling R",
];

fn prf(score: &Prf) -> [String; 3] {
    [score.precision, score.recall, score.f1].map(shown)
}

fn row(name: &str, e: &EdgeScore) -> Vec<String> {
    let counts = [e.key_edges, e.predicted, e.tp, e.fp, e.missed].map(|n| n.to_string());
    [
        vec![name.to_owned()],
        counts.to_vec(),
        prf(&e.micro).to_vec(),
    ]
    .concat()
}

fn type_row(name: &str, t: &TypeRow, ceiling: Option<&TypeRow>) -> Vec<String> {
    let aligned = t
        .aligned
        .clone()
        .unwrap_or_else(|| "(unaligned)".to_owned());
    let counts = [t.tp, t.fp, t.missed].map(|n| n.to_string());
    let ceiling = shown(ceiling.and_then(|c| c.score.recall));
    [
        vec![name.to_owned(), t.key_edges.to_string(), aligned],
        counts.to_vec(),
        prf(&t.score).to_vec(),
        vec![ceiling],
    ]
    .concat()
}

/// The section, or the one "No relationships declared" line for a key without them.
pub(super) fn section(g: &Grade) -> String {
    let (Some(mapping), Some(ceiling), Some(linked)) =
        (&g.edges, &g.ceiling_edges, &g.ceiling_links_edges)
    else {
        return "No relationships declared by this key (format 2 or earlier).\n\n".to_owned();
    };
    let rows = [
        row("mapping", mapping),
        row("ceiling", ceiling),
        row("ceiling with links", linked),
    ];
    let types = mapping
        .per_type
        .iter()
        .map(|(name, t)| type_row(name, t, ceiling.per_type.get(name)));
    let empty: Vec<&String> = mapping
        .per_type
        .iter()
        .filter(|(_, t)| t.key_edges == 0)
        .map(|(name, _)| name)
        .collect();
    let unaligned: Vec<(&String, &usize)> = mapping.unaligned_predicted.iter().collect();
    format!(
        "Relationships (contract B3, unique typed directed edges):\n\n{}\n{}\nUnaligned predicted edge types (all false): {unaligned:?}. Predicted edges with a no-majority endpoint: {}. Dropped (unscored endpoint): {}. Unobservable key rows: {} (predicted edges dropped on them: {}). Key edge types with no edge in this corpus: {empty:?}.\n\n",
        table(&ROWS, rows),
        table(&TYPES, types),
        mapping.no_majority,
        mapping.dropped_unscored,
        mapping.unobservable,
        mapping.dropped_unobservable,
    )
}
