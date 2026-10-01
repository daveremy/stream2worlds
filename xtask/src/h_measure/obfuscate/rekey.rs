//! The answer key for an obfuscated corpus: the plain key with every path renamed through the
//! replicate's field table, every `no_identity` sentinel transformed as a value at its path
//! would be, the rules file's unobservable paths added to `unscored`, and every relationship row
//! (format 3) renamed, marked unobservable where the rules file says the obfuscated stream cannot
//! show it. The key format is unchanged, so `score` reads it as any other key (contract B3:
//! scored up to renaming).

use s2w_model::{FieldPath, Segment};

use super::rules::Unobservable;
use super::transform::{FieldTable, Transformer};
use crate::h_measure::key::{KeySpec, RelationshipRow, UnscoredPath, UnscoredPrefix};

/// The envelope field that holds a frame's event, as `discover_replay::envelopes` stores it.
const ENVELOPE: &str = "data";

/// `spec` for the obfuscated corpus `transformer` writes.
pub(super) fn rekey(
    spec: &KeySpec,
    transformer: &mut Transformer<'_>,
    unobservable: &[Unobservable],
) -> Result<KeySpec, String> {
    let envelope = FieldPath(vec![Segment::Key(ENVELOPE.to_owned())]);
    if spec.decode != [envelope.clone()] {
        return Err(format!(
            "the key decodes {:?}; the transformer rewrites each frame's {ENVELOPE:?} JSON, so it renames only a key whose decode is exactly [[{ENVELOPE:?}]]",
            spec.decode
        ));
    }
    let mut out = spec.clone();
    out.relationships = spec
        .relationships
        .iter()
        .map(|row| renamed_row(transformer.table(), row, unobservable))
        .collect::<Result<_, _>>()?;
    for kind in &mut out.types {
        for rule in &mut kind.mentions {
            let chain = chain(&rule.path)?;
            rule.no_identity = rule
                .no_identity
                .iter()
                .map(|sentinel| transformer.leaf(&chain, sentinel, None))
                .collect::<Result<_, _>>()?;
            rule.path = rename(transformer.table(), &rule.path)?;
            rule.identity = rule
                .identity
                .iter()
                .map(|path| rename(transformer.table(), path))
                .collect::<Result<_, _>>()?;
        }
    }
    let mut unscored = spec.unscored.clone();
    for row in unobservable {
        let Some(path) = &row.path else { continue };
        let mut full = envelope.clone();
        full.0.extend(path.iter().cloned().map(Segment::Key));
        if !covered(&unscored, &full) {
            unscored.push(UnscoredPath::Exact(full));
        }
    }
    out.unscored = unscored
        .iter()
        .map(|entry| renamed_entry(transformer.table(), entry))
        .collect::<Result<_, _>>()?;
    out.validate()
        .map_err(|e| format!("the renamed key does not validate: {e}"))?;
    Ok(out)
}

/// `row` renamed, and marked unobservable (unless it already is) by the first rules-file row
/// that hides it: a relationship row whose `from` and `to` each name a field holding the key
/// row's endpoint, or a path row holding either endpoint. Matching is on the endpoints only: a
/// key row's type is a free label, and a key carries one row per `(from, to)` pair. (In r1's
/// rules file every relationship rule starts at a text or URL field, which no key mention path
/// is, so there only a path rule can hide an observable row.)
fn renamed_row(
    table: &FieldTable,
    row: &RelationshipRow,
    unobservable: &[Unobservable],
) -> Result<RelationshipRow, String> {
    let (from, to) = (chain(&row.from)?, chain(&row.to)?);
    let holds = |field: &[String], end: &[String]| end.starts_with(field);
    let named = |name: &str| name.split('.').map(str::to_owned).collect::<Vec<_>>();
    let hidden = || {
        unobservable
            .iter()
            .find_map(|rule| match (&rule.path, &rule.from, &rule.to) {
                (Some(path), _, _) if holds(path, &from) || holds(path, &to) => Some(format!(
                    "obfuscation: {} is unobservable ({})",
                    path.join("."),
                    rule.reason
                )),
                (None, Some(f), Some(t)) if holds(&named(f), &from) && holds(&named(t), &to) => {
                    Some(format!("obfuscation: {f} -> {t} ({})", rule.reason))
                }
                _ => None,
            })
    };
    Ok(RelationshipRow {
        label: row.label.clone(),
        from: rename(table, &row.from)?,
        to: rename(table, &row.to)?,
        unobservable: row.unobservable.clone().or_else(hidden),
    })
}

fn renamed_entry(table: &FieldTable, entry: &UnscoredPath) -> Result<UnscoredPath, String> {
    Ok(match entry {
        UnscoredPath::Exact(path) => UnscoredPath::Exact(rename(table, path)?),
        UnscoredPath::Prefix(UnscoredPrefix { prefix }) => UnscoredPath::Prefix(UnscoredPrefix {
            prefix: rename(table, prefix)?,
        }),
    })
}

/// Whether `path` is already an exact entry or under a prefix entry.
fn covered(entries: &[UnscoredPath], path: &FieldPath) -> bool {
    entries.iter().any(|entry| match entry {
        UnscoredPath::Exact(exact) => exact == path,
        UnscoredPath::Prefix(UnscoredPrefix { prefix }) => path.0.starts_with(&prefix.0),
    })
}

/// The object keys of a key path under the envelope, array indexes dropped: the path's row in
/// the field table.
fn chain(path: &FieldPath) -> Result<Vec<String>, String> {
    let rest = under_envelope(path)?;
    Ok(rest
        .iter()
        .filter_map(|segment| match segment {
            Segment::Key(key) => Some(key.clone()),
            Segment::Index(_) => None,
        })
        .collect())
}

fn under_envelope(path: &FieldPath) -> Result<&[Segment], String> {
    match path.0.split_first() {
        Some((Segment::Key(first), rest)) if first == ENVELOPE && !rest.is_empty() => Ok(rest),
        _ => Err(format!(
            "key path {path:?} is not under {ENVELOPE:?}; the transformer renames only paths inside a frame's event"
        )),
    }
}

/// `path` with every object key under the envelope renamed; indexes kept.
fn rename(table: &FieldTable, path: &FieldPath) -> Result<FieldPath, String> {
    let rest = under_envelope(path)?;
    let mut chain = Vec::new();
    let mut out = vec![Segment::Key(ENVELOPE.to_owned())];
    for segment in rest {
        out.push(match segment {
            Segment::Key(key) => {
                chain.push(key.clone());
                Segment::Key(table.name(&chain)?.to_owned())
            }
            Segment::Index(index) => Segment::Index(*index),
        });
    }
    Ok(FieldPath(out))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use s2w_model::{FieldPath, Segment};

    use super::{FieldTable, RelationshipRow, Unobservable, renamed_row};

    fn path(keys: &[&str]) -> FieldPath {
        let mut out = vec![Segment::Key("data".to_owned())];
        out.extend(keys.iter().map(|k| Segment::Key((*k).to_owned())));
        FieldPath(out)
    }

    fn table() -> FieldTable {
        let mut names = BTreeMap::new();
        for (i, chain) in [
            &["doc"][..],
            &["site"],
            &["ver"],
            &["ver", "new"],
            &["note"],
        ]
        .iter()
        .enumerate()
        {
            let chain: Vec<String> = chain.iter().map(|k| (*k).to_owned()).collect();
            names.insert(chain, format!("f{i}"));
        }
        FieldTable(names)
    }

    fn row(from: &[&str], to: &[&str], unobservable: Option<&str>) -> RelationshipRow {
        RelationshipRow {
            label: "e".to_owned(),
            from: path(from),
            to: path(to),
            unobservable: unobservable.map(str::to_owned),
        }
    }

    fn edge_rule(from: &str, to: &str) -> Unobservable {
        Unobservable {
            path: None,
            from: Some(from.to_owned()),
            to: Some(to.to_owned()),
            kind: Some("names".to_owned()),
            reason: "gone".to_owned(),
        }
    }

    fn path_rule(keys: &[&str]) -> Unobservable {
        Unobservable {
            path: Some(keys.iter().map(|k| (*k).to_owned()).collect()),
            from: None,
            to: None,
            kind: None,
            reason: "gone".to_owned(),
        }
    }

    fn hidden(row: &RelationshipRow, rules: &[Unobservable]) -> Option<String> {
        renamed_row(&table(), row, rules).unwrap().unobservable
    }

    #[test]
    fn a_row_is_renamed_and_stays_observable_when_no_rule_names_it() {
        let rules = [edge_rule("note", "doc"), path_rule(&["note"])];
        let out = renamed_row(&table(), &row(&["ver", "new"], &["doc"], None), &rules).unwrap();
        assert_eq!(out.from, path(&["f2", "f3"]));
        assert_eq!(out.to, path(&["f0"]));
        assert_eq!(out.unobservable, None);
    }

    #[test]
    fn an_edge_rule_hides_a_row_whose_endpoints_its_fields_hold() {
        // `ver` holds `ver.new`, as the rules file's `revision` holds `revision.new`.
        let got = hidden(
            &row(&["ver", "new"], &["doc"], None),
            &[edge_rule("ver", "doc")],
        );
        assert_eq!(got.as_deref(), Some("obfuscation: ver -> doc (gone)"));
        // Direction matters: the reverse rule names a different edge.
        assert_eq!(
            hidden(
                &row(&["ver", "new"], &["doc"], None),
                &[edge_rule("doc", "ver")]
            ),
            None
        );
    }

    #[test]
    fn a_path_rule_hides_a_row_with_either_endpoint_under_it() {
        let got = hidden(
            &row(&["site"], &["ver", "new"], None),
            &[path_rule(&["ver"])],
        );
        assert_eq!(
            got.as_deref(),
            Some("obfuscation: ver is unobservable (gone)")
        );
    }

    #[test]
    fn a_row_keeps_its_own_unobservable_reason() {
        let got = hidden(
            &row(&["note"], &["doc"], Some("free text")),
            &[edge_rule("note", "doc")],
        );
        assert_eq!(got.as_deref(), Some("free text"));
    }
}
