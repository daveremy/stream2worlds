//! The answer key for an obfuscated corpus: the plain key with every path renamed through the
//! replicate's field table, every `no_identity` sentinel transformed as a value at its path
//! would be, and the rules file's unobservable paths added to `unscored`. The key format is
//! unchanged, so `score` reads it as any other key (contract B3: scored up to renaming).

use s2w_model::{FieldPath, Segment};

use super::rules::Unobservable;
use super::transform::{FieldTable, Transformer};
use crate::h_measure::key::{KeySpec, UnscoredPath, UnscoredPrefix};

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
    // Renaming edge rows, and matching the rules file's unobservable relationship rows to
    // them, is s2w#388 PR 3; until then a format-3 key with relationships fails closed rather
    // than keeping plain paths the obfuscated corpus does not hold.
    if !spec.relationships.is_empty() {
        return Err(
            "the key declares relationships, which this build does not rename yet (s2w#388 PR 3)"
                .to_owned(),
        );
    }
    let mut out = spec.clone();
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
