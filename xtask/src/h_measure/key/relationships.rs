//! A key's `relationships` (format 3, s2w#388, contract B3 "Relationships"): typed, directed
//! edges between two mention paths of one record. A record holds a row's edge when both of its
//! paths hold a gold mention there, the test the engine applies to a `RelationshipRule` (both
//! endpoint rules matched in one payload). The edge is `(type, from entity, to entity)`, and the
//! key's edge set is the unique set of those over the corpus.

use std::collections::{BTreeMap, BTreeSet};

use s2w_discover::rule_id;
use s2w_model::{EntityRule, FieldPath, KEY_SEPARATOR, RelationshipRule, StreamMapping};
use serde::{Deserialize, Serialize};

use super::well_formed;

/// One `relationships` row.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelationshipRow {
    /// The edge type, as data. Rows sharing a type form one key edge type.
    #[serde(rename = "type")]
    pub label: String,
    /// The source endpoint's mention path.
    pub from: FieldPath,
    /// The target endpoint's mention path. Direction is as written: the reverse is another edge.
    pub to: FieldPath,
    /// Why the stream cannot show this edge. An unobservable row places no gold edge and may
    /// name any well-formed path, a mention path or not; it is counted, never scored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unobservable: Option<String>,
}

impl RelationshipRow {
    /// Whether the row places gold edges: it is not marked unobservable.
    pub(crate) fn observable(&self) -> bool {
        self.unobservable.is_none()
    }
}

/// Fails closed on a row that could not place a well-defined edge. `mention_paths` holds the
/// spec's mention path ids ([`rule_id`]).
pub(super) fn validate(
    rows: &[RelationshipRow],
    version: u32,
    mention_paths: &BTreeSet<String>,
) -> Result<(), String> {
    if !rows.is_empty() && version < 3 {
        return Err(format!(
            "key spec version {version} has relationships, which need key format 3 or later"
        ));
    }
    // Each (from, to) pair's type, so a repeat names the row it repeats.
    let mut pairs: BTreeMap<(String, String), &str> = BTreeMap::new();
    for row in rows {
        if row.label.is_empty() || row.label.contains(KEY_SEPARATOR) {
            return Err(format!(
                "relationship type {:?} is empty or holds U+001F",
                row.label
            ));
        }
        if !well_formed(&row.from) || !well_formed(&row.to) {
            return Err(format!(
                "relationship {:?}: a from or to path is empty or has an empty or U+001F key",
                row.label
            ));
        }
        let (from, to) = (rule_id(&row.from), rule_id(&row.to));
        if from == to {
            return Err(format!(
                "relationship {:?}: from and to are one path {:?}",
                row.label, row.from
            ));
        }
        match &row.unobservable {
            Some(reason) if reason.trim().is_empty() => {
                return Err(format!(
                    "relationship {:?} from {:?} to {:?}: unobservable needs a reason",
                    row.label, row.from, row.to
                ));
            }
            Some(_) => {}
            None => {
                for (end, path, id) in [("from", &row.from, &from), ("to", &row.to, &to)] {
                    if !mention_paths.contains(id) {
                        return Err(format!(
                            "relationship {:?}: {end} path {path:?} is not a mention path of the key (mark the row unobservable if the stream cannot show it)",
                            row.label
                        ));
                    }
                }
            }
        }
        if let Some(earlier) = pairs.insert((from, to), &row.label) {
            return Err(if earlier == row.label {
                format!(
                    "relationship {:?} from {:?} to {:?} is listed twice",
                    row.label, row.from, row.to
                )
            } else {
                format!(
                    "relationships {earlier:?} and {:?} share from {:?} and to {:?}: one (from, to) pair carries one edge type",
                    row.label, row.from, row.to
                )
            });
        }
    }
    Ok(())
}

/// The rows a mapping states about itself: per relationship rule, its kind from the last key
/// path of its `from` rule to the last key path of its `to` rule (where
/// [`crate::h_measure::mentions::mapping_mentions`] places each endpoint's mention). Rules that
/// repeat a row collapse to one; any other clash fails validation.
pub(super) fn of_mapping(mapping: &StreamMapping) -> Result<Vec<RelationshipRow>, String> {
    let last = |id: &str| {
        mapping
            .entities
            .iter()
            .find(|rule| rule.id == id)
            .and_then(|rule| rule.key.last().cloned())
            // Unreachable after `validate` (an endpoint names a rule, a key is not empty).
            .ok_or_else(|| format!("relationship endpoint {id:?} names no entity rule with a key"))
    };
    let mut rows: Vec<RelationshipRow> = Vec::new();
    for rule in &mapping.relationships {
        let row = RelationshipRow {
            label: rule.kind.clone(),
            from: last(&rule.from)?,
            to: last(&rule.to)?,
            unobservable: None,
        };
        if !rows.contains(&row) {
            rows.push(row);
        }
    }
    Ok(rows)
}

/// The oracle's relationship rules: one per observable row whose two mention paths both got an
/// oracle entity rule, kind the row's type. Each oracle rule keys its mention path last (an alias
/// rule keys it alone), and a mention path gets at most one rule, so a rule's last key path names
/// its mention path. A row with an endpoint that got no rule (an alias, in the oracle without
/// links) gets none, so the ceiling shows that limit of the format as it shows the alias limit.
pub(super) fn oracle_rules(
    rows: &[RelationshipRow],
    entities: &[EntityRule],
) -> Vec<RelationshipRule> {
    let rule_of: BTreeMap<String, &str> = entities
        .iter()
        .filter_map(|rule| Some((rule_id(rule.key.last()?), rule.id.as_str())))
        .collect();
    rows.iter()
        .filter(|row| row.observable())
        .filter_map(|row| {
            Some(RelationshipRule {
                from: (*rule_of.get(&rule_id(&row.from))?).to_owned(),
                to: (*rule_of.get(&rule_id(&row.to))?).to_owned(),
                kind: row.label.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
