//! A stream mapping: data that tells a generic executor how one raw payload becomes claims
//! (decision 0021). Paths, type labels and relationship kinds are data, never code, so a
//! mapping carries a stream's domain knowledge without compiling it in (decision 0018).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The one mapping format version this crate reads and writes. Aliases or merge rules would be
/// version 2 (decision 0021).
pub const MAPPING_VERSION: u32 = 1;

/// Separates a natural key's components: the type label, then each JSON-encoded key part.
/// A structural separator, so a replay can split a key back into its parts.
pub const KEY_SEPARATOR: char = '\u{1f}';

/// How one stream's raw payloads map to entity and relationship claims.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamMapping {
    /// The format version; only [`MAPPING_VERSION`] is valid.
    pub version: u32,
    /// Paths whose string value holds JSON text, replaced by the parsed value in order before
    /// any rule runs. A format step, not domain knowledge.
    pub decode: Vec<FieldPath>,
    /// Entity rules, applied in order; claims keep this order.
    pub entities: Vec<EntityRule>,
    /// Relationship rules, emitted when both endpoint rules matched.
    pub relationships: Vec<RelationshipRule>,
}

/// A path into a JSON value: object keys and array indexes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldPath(pub Vec<Segment>);

/// One step of a [`FieldPath`]: a JSON string selects an object key, a JSON number an array
/// index.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Segment {
    /// An array index.
    Index(usize),
    /// An object key.
    Key(String),
}

/// One entity type's rule: where its identity and attributes live in a payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRule {
    /// The rule's id, unique in the mapping; relationship rules name it.
    pub id: String,
    /// The entity type label claimed for a match. Rules may share a label.
    pub type_label: String,
    /// The paths whose scalar values, together, identify the entity. All must be present.
    pub key: Vec<FieldPath>,
    /// Optional attributes; a missing or non-scalar value is left out.
    pub attrs: Vec<AttrRule>,
}

/// One attribute of an entity rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttrRule {
    /// The attribute name claimed.
    pub name: String,
    /// Where the value lives.
    pub path: FieldPath,
}

/// A relationship between two entity rules' matches in the same payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipRule {
    /// The source endpoint's entity rule id.
    pub from: String,
    /// The target endpoint's entity rule id.
    pub to: String,
    /// The relationship kind claimed.
    pub kind: String,
}

/// Why a mapping is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    /// The version is not [`MAPPING_VERSION`].
    #[error("mapping version {0} is not supported; this build reads version {MAPPING_VERSION}")]
    UnsupportedVersion(u32),
    /// Two entity rules share an id.
    #[error("entity rule id {0:?} is used twice; rule ids must be unique")]
    DuplicateRuleId(String),
    /// A relationship names an entity rule that does not exist.
    #[error("relationship endpoint {0:?} names no entity rule")]
    UnknownEndpoint(String),
    /// An entity rule has no key paths.
    #[error("entity rule {0:?} has no key paths; give it at least one")]
    EmptyKey(String),
    /// A path has no segments, or a key segment is empty.
    #[error("{0} has an empty path or an empty key segment")]
    EmptyPath(String),
    /// An id, label, attribute name or kind is empty.
    #[error("{0} is empty")]
    EmptyLabel(String),
    /// An attribute name appears twice in one rule.
    #[error("entity rule {rule:?} names attribute {name:?} twice")]
    DuplicateAttr {
        /// The rule.
        rule: String,
        /// The repeated attribute name.
        name: String,
    },
    /// A label or key segment contains [`KEY_SEPARATOR`].
    #[error("{0} contains the key separator U+001F")]
    Separator(String),
}

impl StreamMapping {
    /// Checks the rules a mapping must satisfy before an executor runs it.
    ///
    /// # Errors
    /// The first [`MappingError`] found, in declaration order.
    pub fn validate(&self) -> Result<(), MappingError> {
        if self.version != MAPPING_VERSION {
            return Err(MappingError::UnsupportedVersion(self.version));
        }
        for (i, path) in self.decode.iter().enumerate() {
            check_path(path, &format!("decode path {i}"))?;
        }
        let mut ids = BTreeSet::new();
        for rule in &self.entities {
            rule.validate()?;
            if !ids.insert(rule.id.as_str()) {
                return Err(MappingError::DuplicateRuleId(rule.id.clone()));
            }
        }
        for rel in &self.relationships {
            check_label(&rel.kind, "relationship kind")?;
            for end in [&rel.from, &rel.to] {
                if !ids.contains(end.as_str()) {
                    return Err(MappingError::UnknownEndpoint(end.clone()));
                }
            }
        }
        Ok(())
    }
}

impl EntityRule {
    fn validate(&self) -> Result<(), MappingError> {
        check_label(&self.id, "entity rule id")?;
        check_label(
            &self.type_label,
            &format!("type label of rule {:?}", self.id),
        )?;
        if self.key.is_empty() {
            return Err(MappingError::EmptyKey(self.id.clone()));
        }
        for path in &self.key {
            check_path(path, &format!("key path of rule {:?}", self.id))?;
        }
        let mut names = BTreeSet::new();
        for attr in &self.attrs {
            check_label(&attr.name, &format!("attribute name in rule {:?}", self.id))?;
            check_path(&attr.path, &format!("attribute {:?} path", attr.name))?;
            if !names.insert(attr.name.as_str()) {
                return Err(MappingError::DuplicateAttr {
                    rule: self.id.clone(),
                    name: attr.name.clone(),
                });
            }
        }
        Ok(())
    }
}

fn check_label(label: &str, what: &str) -> Result<(), MappingError> {
    if label.is_empty() {
        Err(MappingError::EmptyLabel(what.to_owned()))
    } else if label.contains(KEY_SEPARATOR) {
        Err(MappingError::Separator(what.to_owned()))
    } else {
        Ok(())
    }
}

fn check_path(path: &FieldPath, what: &str) -> Result<(), MappingError> {
    if path.0.is_empty() {
        return Err(MappingError::EmptyPath(what.to_owned()));
    }
    for segment in &path.0 {
        if let Segment::Key(key) = segment {
            if key.is_empty() {
                return Err(MappingError::EmptyPath(what.to_owned()));
            }
            if key.contains(KEY_SEPARATOR) {
                return Err(MappingError::Separator(what.to_owned()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn key(k: &str) -> Segment {
        Segment::Key(k.to_owned())
    }

    fn path(keys: &[&str]) -> FieldPath {
        FieldPath(keys.iter().map(|k| key(k)).collect())
    }

    fn rule(id: &str, label: &str) -> EntityRule {
        EntityRule {
            id: id.to_owned(),
            type_label: label.to_owned(),
            key: vec![path(&["id"])],
            attrs: vec![],
        }
    }

    fn mapping() -> StreamMapping {
        StreamMapping {
            version: MAPPING_VERSION,
            decode: vec![path(&["data"])],
            entities: vec![rule("a", "t"), rule("b", "t")],
            relationships: vec![RelationshipRule {
                from: "a".to_owned(),
                to: "b".to_owned(),
                kind: "k".to_owned(),
            }],
        }
    }

    #[test]
    fn a_valid_mapping_validates_and_rules_may_share_a_label() {
        assert_eq!(mapping().validate(), Ok(()));
    }

    #[test]
    fn segments_deserialize_by_json_type() -> TestResult {
        let parsed: FieldPath = serde_json::from_str(r#"["a", 0, "1"]"#)?;
        assert_eq!(
            parsed,
            FieldPath(vec![key("a"), Segment::Index(0), key("1")])
        );
        Ok(())
    }

    #[test]
    fn a_mapping_round_trips_through_json_in_declaration_order() -> TestResult {
        let text = serde_json::to_string(&mapping())?;
        assert!(text.starts_with(r#"{"version":1,"decode":[["data"]],"entities":"#));
        assert_eq!(serde_json::from_str::<StreamMapping>(&text)?, mapping());
        assert!(serde_json::from_str::<StreamMapping>(r#"{"version":1}"#).is_err());
        Ok(())
    }

    type Mutation = fn(&mut StreamMapping);

    #[test]
    fn invalid_mappings_name_their_fault() {
        let cases: Vec<(Mutation, MappingError)> = vec![
            (|m| m.version = 2, MappingError::UnsupportedVersion(2)),
            (
                |m| m.entities[1].id = "a".to_owned(),
                MappingError::DuplicateRuleId("a".to_owned()),
            ),
            (
                |m| m.relationships[0].to = "z".to_owned(),
                MappingError::UnknownEndpoint("z".to_owned()),
            ),
            (
                |m| m.entities[0].key.clear(),
                MappingError::EmptyKey("a".to_owned()),
            ),
            (
                |m| m.entities[0].key = vec![path(&[""])],
                MappingError::EmptyPath("key path of rule \"a\"".to_owned()),
            ),
            (
                |m| m.decode = vec![FieldPath(vec![])],
                MappingError::EmptyPath("decode path 0".to_owned()),
            ),
            (
                |m| m.entities[0].type_label = String::new(),
                MappingError::EmptyLabel("type label of rule \"a\"".to_owned()),
            ),
            (
                |m| m.entities[0].type_label = "x\u{1f}y".to_owned(),
                MappingError::Separator("type label of rule \"a\"".to_owned()),
            ),
            (
                |m| m.relationships[0].kind = String::new(),
                MappingError::EmptyLabel("relationship kind".to_owned()),
            ),
        ];
        for (mutate, expected) in cases {
            let mut m = mapping();
            mutate(&mut m);
            assert_eq!(m.validate(), Err(expected));
        }
    }

    #[test]
    fn a_repeated_attribute_name_is_rejected() {
        let mut m = mapping();
        let attr = AttrRule {
            name: "n".to_owned(),
            path: path(&["x"]),
        };
        m.entities[0].attrs = vec![attr.clone(), attr];
        assert_eq!(
            m.validate(),
            Err(MappingError::DuplicateAttr {
                rule: "a".to_owned(),
                name: "n".to_owned(),
            })
        );
    }
}
