//! A stream mapping: data that tells a generic executor how one raw payload becomes claims
//! (decision 0021). Paths, type labels and relationship kinds are data, never code, so a
//! mapping carries a stream's domain knowledge without compiling it in (decision 0018).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Fnv64, KEY_FORMAT, KEY_SEPARATOR};

/// The base mapping format version: a mapping without links (decision 0021). A writer that
/// states no link writes this version, so its mapping identity is the one it had before version
/// [`MAPPING_VERSION_LINKS`] existed (decision 0027).
pub const MAPPING_VERSION: u32 = 1;

/// The mapping format version that adds [`StreamMapping::links`] (decision 0027), and the
/// newest version this build reads.
pub const MAPPING_VERSION_LINKS: u32 = 2;

/// How one stream's raw payloads map to entity and relationship claims.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamMapping {
    /// The format version: [`MAPPING_VERSION`] or [`MAPPING_VERSION_LINKS`].
    pub version: u32,
    /// Paths whose string value holds JSON text, replaced by the parsed value in order before
    /// any rule runs. A format step, not domain knowledge.
    pub decode: Vec<FieldPath>,
    /// Entity rules, applied in order; claims keep this order.
    pub entities: Vec<EntityRule>,
    /// Relationship rules, emitted when both endpoint rules matched.
    pub relationships: Vec<RelationshipRule>,
    /// Link rules (version [`MAPPING_VERSION_LINKS`] only, decision 0027): pairs of entity rules
    /// whose different keys name one entity when both match in one payload. Omitted from the
    /// JSON when empty, so a version-1 mapping's bytes and identity are unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LinkRule>,
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

/// Two entity rules with one type label whose keys name one entity (decision 0027). When both
/// match in one payload with different keys, the absorbed key joins the survivor's entity.
/// Per type the links form a star: a rule is absorbed at most once and is never also a
/// survivor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkRule {
    /// The entity rule whose entity keeps its id.
    pub survivor: String,
    /// The entity rule whose key joins the survivor's entity.
    pub absorbed: String,
}

/// Why a mapping is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    /// The version is neither [`MAPPING_VERSION`] nor [`MAPPING_VERSION_LINKS`].
    #[error(
        "mapping version {0} is not supported; this build reads versions {MAPPING_VERSION} and {MAPPING_VERSION_LINKS}"
    )]
    UnsupportedVersion(u32),
    /// A mapping below [`MAPPING_VERSION_LINKS`] has links.
    #[error("mapping version {0} cannot have links; links need version {MAPPING_VERSION_LINKS}")]
    LinksNeedVersion(u32),
    /// A link names an entity rule that does not exist.
    #[error("link names no entity rule {0:?}")]
    UnknownLinkRule(String),
    /// A link joins a rule to itself.
    #[error("link joins entity rule {0:?} to itself")]
    SelfLink(String),
    /// A link joins two rules with different type labels.
    #[error(
        "link joins entity rules {survivor:?} and {absorbed:?}, which have different type labels"
    )]
    LinkAcrossLabels {
        /// The survivor rule.
        survivor: String,
        /// The absorbed rule.
        absorbed: String,
    },
    /// A rule is absorbed by more than one link (a repeated link included).
    #[error("entity rule {0:?} is absorbed by more than one link")]
    AbsorbedTwice(String),
    /// A rule is both a link's survivor and another link's absorbed rule.
    #[error("entity rule {0:?} is both a link survivor and absorbed; links form a star per type")]
    SurvivorAbsorbed(String),
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
    /// The mapping could not be encoded to its canonical JSON bytes.
    #[error("mapping could not be encoded: {0}")]
    Encode(String),
}

impl StreamMapping {
    /// Checks the rules a mapping must satisfy before an executor runs it.
    ///
    /// # Errors
    /// The first [`MappingError`] found, in declaration order.
    pub fn validate(&self) -> Result<(), MappingError> {
        if self.version != MAPPING_VERSION && self.version != MAPPING_VERSION_LINKS {
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
        self.validate_links()
    }

    fn validate_links(&self) -> Result<(), MappingError> {
        if self.links.is_empty() {
            return Ok(());
        }
        if self.version < MAPPING_VERSION_LINKS {
            return Err(MappingError::LinksNeedVersion(self.version));
        }
        let label = |id: &str| {
            self.entities
                .iter()
                .find(|rule| rule.id == id)
                .map(|rule| rule.type_label.as_str())
                .ok_or_else(|| MappingError::UnknownLinkRule(id.to_owned()))
        };
        let mut absorbed = BTreeSet::new();
        for link in &self.links {
            let (survivor_label, absorbed_label) = (label(&link.survivor)?, label(&link.absorbed)?);
            if link.survivor == link.absorbed {
                return Err(MappingError::SelfLink(link.survivor.clone()));
            }
            if survivor_label != absorbed_label {
                return Err(MappingError::LinkAcrossLabels {
                    survivor: link.survivor.clone(),
                    absorbed: link.absorbed.clone(),
                });
            }
            if !absorbed.insert(link.absorbed.as_str()) {
                return Err(MappingError::AbsorbedTwice(link.absorbed.clone()));
            }
        }
        match self
            .links
            .iter()
            .find(|link| absorbed.contains(link.survivor.as_str()))
        {
            Some(link) => Err(MappingError::SurvivorAbsorbed(link.survivor.clone())),
            None => Ok(()),
        }
    }
}

impl StreamMapping {
    /// The mapping identity (decision 0023): FNV-1a 64 as 16 hex digits over, in order,
    /// [`KEY_FORMAT`] and the mapping's own `version` (little-endian `u32`, decision 0027) and
    /// the mapping's canonical
    /// JSON (`serde_json` of the struct, fields in declaration order), each as one
    /// length-prefixed field. Whitespace or key order in the text a mapping was read from never
    /// changes it; a key-format or mapping-format bump always does. It names the engine that
    /// runs the mapping (`mapping-<identity>`), so stored verdicts and world snapshots made
    /// under one mapping are never served under another.
    ///
    /// # Errors
    /// The first [`MappingError`] from [`Self::validate`]: only a valid mapping has an
    /// identity. [`MappingError::Encode`] if the canonical bytes cannot be produced.
    pub fn identity(&self) -> Result<String, MappingError> {
        self.validate()?;
        let canonical =
            serde_json::to_vec(self).map_err(|error| MappingError::Encode(error.to_string()))?;
        Ok(identity_digest(KEY_FORMAT, self.version, &canonical))
    }
}

/// [`StreamMapping::identity`]'s hash, with its three inputs as arguments so a test can vary
/// each one.
fn identity_digest(key_format: u32, mapping_version: u32, canonical: &[u8]) -> String {
    let digest = Fnv64::new()
        .write_field(&key_format.to_le_bytes())
        .write_field(&mapping_version.to_le_bytes())
        .write_field(canonical)
        .finish();
    format!("{digest:016x}")
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
            links: vec![],
        }
    }

    fn link(survivor: &str, absorbed: &str) -> LinkRule {
        LinkRule {
            survivor: survivor.to_owned(),
            absorbed: absorbed.to_owned(),
        }
    }

    /// A version-2 mapping: rules `a`, `b` and `c` share label `t`, `d` has label `u`; `a`
    /// absorbs `b` and `c`.
    fn linked() -> StreamMapping {
        let mut m = mapping();
        m.version = MAPPING_VERSION_LINKS;
        m.entities.extend([rule("c", "t"), rule("d", "u")]);
        m.links = vec![link("a", "b"), link("a", "c")];
        m
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
            (|m| m.version = 0, MappingError::UnsupportedVersion(0)),
            (|m| m.version = 3, MappingError::UnsupportedVersion(3)),
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
    fn a_version_2_mapping_with_links_validates_and_so_does_one_without() {
        assert_eq!(linked().validate(), Ok(()));
        let mut empty = linked();
        empty.links.clear();
        assert_eq!(empty.validate(), Ok(()));
    }

    #[test]
    fn invalid_links_name_their_fault() {
        let cases: Vec<(Mutation, MappingError)> = vec![
            (
                |m| m.version = MAPPING_VERSION,
                MappingError::LinksNeedVersion(MAPPING_VERSION),
            ),
            (
                |m| m.links[0].absorbed = "z".to_owned(),
                MappingError::UnknownLinkRule("z".to_owned()),
            ),
            (
                |m| m.links[0].survivor = "z".to_owned(),
                MappingError::UnknownLinkRule("z".to_owned()),
            ),
            (
                |m| m.links[0] = link("b", "b"),
                MappingError::SelfLink("b".to_owned()),
            ),
            (
                |m| m.links[1] = link("a", "d"),
                MappingError::LinkAcrossLabels {
                    survivor: "a".to_owned(),
                    absorbed: "d".to_owned(),
                },
            ),
            (
                |m| m.links[1] = link("a", "b"),
                MappingError::AbsorbedTwice("b".to_owned()),
            ),
            (
                |m| m.links[1] = link("c", "b"),
                MappingError::AbsorbedTwice("b".to_owned()),
            ),
            (
                |m| m.links[1] = link("b", "c"),
                MappingError::SurvivorAbsorbed("b".to_owned()),
            ),
            (
                |m| m.links[1] = link("c", "a"),
                MappingError::SurvivorAbsorbed("a".to_owned()),
            ),
        ];
        for (mutate, expected) in cases {
            let mut m = linked();
            mutate(&mut m);
            assert_eq!(m.validate(), Err(expected.clone()), "{m:?}");
            assert_eq!(m.identity(), Err(expected));
        }
    }

    #[test]
    fn a_version_2_mapping_round_trips_with_and_without_links() -> TestResult {
        let text = serde_json::to_string(&linked())?;
        assert!(text.starts_with(r#"{"version":2,"decode":"#));
        assert!(text.ends_with(
            r#""links":[{"survivor":"a","absorbed":"b"},{"survivor":"a","absorbed":"c"}]}"#
        ));
        assert_eq!(serde_json::from_str::<StreamMapping>(&text)?, linked());

        let mut empty = linked();
        empty.links.clear();
        let text = serde_json::to_string(&empty)?;
        assert!(!text.contains("links"), "{text}");
        assert_eq!(serde_json::from_str::<StreamMapping>(&text)?, empty);
        assert!(
            serde_json::from_str::<LinkRule>(r#"{"survivor":"a","absorbed":"b","x":1}"#).is_err()
        );
        Ok(())
    }

    /// Decision 0027's compatibility promise: adding `links` changes neither the bytes nor the
    /// identity of a version-1 mapping, so no stored verdict, snapshot or world is re-keyed.
    /// The expected text and identity were computed before the `links` field existed.
    #[test]
    fn a_version_1_mapping_keeps_its_bytes_and_identity() -> TestResult {
        let text = serde_json::to_string(&mapping())?;
        assert_eq!(
            text,
            r#"{"version":1,"decode":[["data"]],"entities":[{"id":"a","type_label":"t","key":[["id"]],"attrs":[]},{"id":"b","type_label":"t","key":[["id"]],"attrs":[]}],"relationships":[{"from":"a","to":"b","kind":"k"}]}"#
        );
        assert_eq!(mapping().identity()?, V1_IDENTITY);
        assert_eq!(
            serde_json::from_str::<StreamMapping>(&text)?.identity()?,
            V1_IDENTITY
        );
        Ok(())
    }

    /// `mapping()`'s identity before decision 0027, computed outside this crate from the
    /// pre-0027 byte layout (the same computation reproduces the pinned fixture identity).
    const V1_IDENTITY: &str = "10702d70ca8543ed";

    /// A version-2 mapping hashes its own version, so its identity differs from the same
    /// rules at version 1, links or none.
    #[test]
    fn a_version_2_identity_hashes_version_2_and_is_pinned() -> TestResult {
        let m = linked();
        let canonical = serde_json::to_vec(&m)?;
        assert_eq!(
            m.identity()?,
            identity_digest(KEY_FORMAT, MAPPING_VERSION_LINKS, &canonical)
        );
        assert_eq!(m.identity()?, V2_IDENTITY);

        let mut v2 = mapping();
        v2.version = MAPPING_VERSION_LINKS;
        assert_eq!(
            serde_json::to_vec(&v2)?[12..],
            serde_json::to_vec(&mapping())?[12..]
        );
        assert_ne!(v2.identity()?, mapping().identity()?);
        Ok(())
    }

    /// `linked()`'s identity, computed outside this crate like [`V1_IDENTITY`].
    const V2_IDENTITY: &str = "488c37af3f9b92de";

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

    #[test]
    fn identity_ignores_the_text_layout_and_tracks_the_mapping_bytes() -> TestResult {
        let compact = serde_json::to_string(&mapping())?;
        let spaced = serde_json::to_string_pretty(&mapping())?;
        assert_ne!(compact, spaced, "the fixtures must differ as text");
        let a: StreamMapping = serde_json::from_str(&compact)?;
        let b: StreamMapping = serde_json::from_str(&spaced)?;
        assert_eq!(a.identity()?, b.identity()?);
        assert_eq!(a.identity()?.len(), 16);

        let mut other = mapping();
        other.entities[1].type_label = "u".to_owned();
        assert_ne!(other.identity()?, a.identity()?);
        Ok(())
    }

    #[test]
    fn identity_hashes_the_key_format_the_mapping_version_and_the_bytes() -> TestResult {
        let canonical = serde_json::to_vec(&mapping())?;
        let identity = mapping().identity()?;
        assert_eq!(
            identity,
            identity_digest(KEY_FORMAT, MAPPING_VERSION, &canonical)
        );
        assert_ne!(
            identity,
            identity_digest(KEY_FORMAT + 1, MAPPING_VERSION, &canonical)
        );
        assert_ne!(
            identity,
            identity_digest(KEY_FORMAT, MAPPING_VERSION + 1, &canonical)
        );
        let mut changed = canonical.clone();
        changed.push(b' ');
        assert_ne!(
            identity,
            identity_digest(KEY_FORMAT, MAPPING_VERSION, &changed)
        );
        Ok(())
    }

    #[test]
    fn only_a_valid_mapping_has_an_identity() {
        let mut m = mapping();
        m.version = MAPPING_VERSION_LINKS + 1;
        assert_eq!(
            m.identity(),
            Err(MappingError::UnsupportedVersion(MAPPING_VERSION_LINKS + 1))
        );
    }
}
