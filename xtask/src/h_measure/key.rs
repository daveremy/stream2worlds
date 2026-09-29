//! The answer-key spec (s2w#56): data that says which `(record, field path)` pairs mention an
//! entity and which values identify it. Domain knowledge lives in the spec file, never here
//! (decision 0018); this module reads any stream's key the same way.

use std::collections::BTreeSet;

use s2w_discover::rule_id;
use s2w_model::{FieldPath, KEY_SEPARATOR, Segment, StreamMapping};
use serde::Deserialize;

/// The one key-spec version this harness reads.
pub(crate) const KEY_VERSION: u32 = 0;

/// A key spec. Every mention rule's path names where a mention sits; its identity paths name the
/// values that identify the entity. Two mention rules of one type whose identity values are equal
/// mention one entity, which is how aliases with different values join (`server` and a canonical
/// id, say).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeySpec {
    /// The format version; only [`KEY_VERSION`] is valid.
    pub version: u32,
    /// Paths whose string value holds JSON text, parsed in order before any rule runs (as a
    /// stream mapping's `decode`).
    #[serde(default)]
    pub decode: Vec<FieldPath>,
    /// Entity types, each with its mention rules.
    pub types: Vec<KeyType>,
    /// Paths whose mentions are not scored on either side: ambiguous or unobservable parts of the
    /// stream (contract B3).
    #[serde(default)]
    pub unscored: Vec<FieldPath>,
}

/// One key entity type.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyType {
    /// The type's name, as data.
    #[serde(rename = "type")]
    pub label: String,
    /// Where this type's mentions sit and what identifies them.
    pub mentions: Vec<MentionRule>,
}

/// One mention rule: a record mentions an entity at `path` when `path` and every identity path
/// hold a key part.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MentionRule {
    /// Where the mention sits.
    pub path: FieldPath,
    /// The values that identify the entity, in order. May include `path` itself.
    pub identity: Vec<FieldPath>,
}

impl KeySpec {
    /// The key a mapping states about itself: one type per type label, and per entity rule one
    /// mention at its last key path identified by all its key paths, the convention
    /// [`super::mentions::mapping_mentions`] scores. Grading a mapping against this key must
    /// find the two partitions equal; the self-test checks that, so the two executors cannot
    /// drift apart. Rules that repeat a type's mention path with the same identity collapse to
    /// one rule; any other clash fails validation.
    pub(crate) fn from_mapping(mapping: &StreamMapping) -> Result<Self, String> {
        mapping
            .validate()
            .map_err(|e| format!("the mapping is not valid: {e}"))?;
        let mut types: Vec<KeyType> = Vec::new();
        for rule in &mapping.entities {
            // Unreachable after `validate` (an empty key is `EmptyKey`); kept fail-closed.
            let Some(last) = rule.key.last() else {
                return Err(format!("entity rule {:?} has no key paths", rule.id));
            };
            let mention = MentionRule {
                path: last.clone(),
                identity: rule.key.clone(),
            };
            match types.iter_mut().find(|kind| kind.label == rule.type_label) {
                Some(kind) if kind.mentions.contains(&mention) => {}
                Some(kind) => kind.mentions.push(mention),
                None => types.push(KeyType {
                    label: rule.type_label.clone(),
                    mentions: vec![mention],
                }),
            }
        }
        let spec = Self {
            version: KEY_VERSION,
            decode: mapping.decode.clone(),
            types,
            unscored: Vec::new(),
        };
        spec.validate()?;
        Ok(spec)
    }

    /// Fails closed on anything that would make the key partition ambiguous.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.version != KEY_VERSION {
            return Err(format!(
                "key spec version {} is not {KEY_VERSION}",
                self.version
            ));
        }
        if self.types.is_empty() {
            return Err("key spec has no types".to_owned());
        }
        if !self.decode.iter().all(well_formed) {
            return Err("a decode path is empty or has an empty or U+001F key".to_owned());
        }
        let mut unscored = BTreeSet::new();
        for path in &self.unscored {
            if !well_formed(path) {
                return Err("an unscored path is empty or has an empty or U+001F key".to_owned());
            }
            // Compared as the executors' mention id, so `["a", 1]` and `["a", "1"]` are one path.
            if !unscored.insert(rule_id(path)) {
                return Err(format!("unscored path {path:?} is listed twice"));
            }
        }
        let mut labels = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for kind in &self.types {
            if kind.label.is_empty() || kind.label.contains(KEY_SEPARATOR) {
                return Err(format!(
                    "type label {:?} is empty or holds U+001F",
                    kind.label
                ));
            }
            if !labels.insert(kind.label.as_str()) {
                return Err(format!("type {:?} is listed twice", kind.label));
            }
            if kind.mentions.is_empty() {
                return Err(format!("type {:?} has no mention rules", kind.label));
            }
            for rule in &kind.mentions {
                if !well_formed(&rule.path)
                    || rule.identity.is_empty()
                    || !rule.identity.iter().all(well_formed)
                {
                    return Err(format!(
                        "type {:?}: a mention rule has an empty path or identity, or an empty or U+001F key",
                        kind.label
                    ));
                }
                let id = rule_id(&rule.path);
                if !paths.insert(id.clone()) {
                    return Err(format!(
                        "mention path {:?} is listed twice: one (record, path) would mention two entities",
                        rule.path
                    ));
                }
                if unscored.contains(&id) {
                    return Err(format!("mention path {:?} is also unscored", rule.path));
                }
            }
        }
        Ok(())
    }
}

/// A path the mapping format would accept too: not empty, and no key segment empty or holding
/// U+001F (as `StreamMapping::validate`).
fn well_formed(path: &FieldPath) -> bool {
    !path.0.is_empty()
        && path.0.iter().all(|segment| match segment {
            Segment::Key(key) => !key.is_empty() && !key.contains(KEY_SEPARATOR),
            Segment::Index(_) => true,
        })
}
