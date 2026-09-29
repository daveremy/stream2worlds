//! The answer-key spec (s2w#56): data that says which `(record, field path)` pairs mention an
//! entity and which values identify it. Domain knowledge lives in the spec file, never here
//! (decision 0018); this module reads any stream's key the same way.

use std::collections::BTreeSet;

use s2w_discover::rule_id;
use s2w_model::{
    EntityRule, FieldPath, KEY_SEPARATOR, KeyPart, MAPPING_VERSION, Segment, StreamMapping,
};
use s2w_system1::decode::key_part;
use serde::Deserialize;
use serde_json::Value;

/// The newest key-spec version, the one [`KeySpec::from_mapping`] writes. Version 1 adds
/// [`MentionRule::no_identity`]; a version-0 spec reads exactly as it always did.
pub(crate) const KEY_VERSION: u32 = 1;

/// Every key-spec version this harness reads.
pub(crate) const KEY_VERSIONS: [u32; 2] = [0, KEY_VERSION];

/// A key spec. Every mention rule's path names where a mention sits; its identity paths name the
/// values that identify the entity. Two mention rules of one type whose identity values are equal
/// mention one entity, which is how aliases with different values join (`server` and a canonical
/// id, say).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeySpec {
    /// The format version; one of [`KEY_VERSIONS`].
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
/// hold a key part, and the value at `path` is not one of `no_identity`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MentionRule {
    /// Where the mention sits.
    pub path: FieldPath,
    /// The values that identify the entity, in order. May include `path` itself.
    pub identity: Vec<FieldPath>,
    /// Format 1: sentinel values at `path` that mean "no identity". A record holding one there
    /// mentions nothing at `path`: no mention, so neither a singleton nor a merge. Compared as
    /// key parts, so `0` and `"0"` differ. Only on a rule whose `path` is an identity path.
    #[serde(default)]
    pub no_identity: Vec<Value>,
}

impl MentionRule {
    /// Whether `part`, the key part at this rule's `path`, is one of its `no_identity` values.
    pub(crate) fn excludes(&self, part: &KeyPart) -> bool {
        self.no_identity
            .iter()
            .any(|sentinel| key_part(sentinel).as_ref() == Some(part))
    }
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
                no_identity: Vec::new(),
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

    /// The oracle v0 mapping: the best the mapping format can do with this key. One entity rule
    /// per mention rule whose path is among its identity paths, keyed by those paths with the
    /// mention path moved last (where [`super::mentions::mapping_mentions`] places the mention),
    /// labelled with the key's type. A mention rule whose path is not an identity path is an
    /// alias of another value, which no v0 rule can join, so it gets no rule: grading this
    /// mapping measures the format's ceiling, not a discoverer. Moving the mention path last
    /// reorders the key parts, so two mention rules of one type on the same multi-path identity
    /// (`a` and `b`, both identified by `[a, b]`) get differently ordered keys and the oracle
    /// splits their entity: a second limit of the format, pinned by a fixture. The mapping
    /// format cannot exclude a value, so a rule with `no_identity` still gets its entity rule;
    /// [`super::score::grade`] drops the key's excluded mentions from the oracle's partition, so
    /// the ceiling honours the exclusion.
    pub(crate) fn oracle(&self) -> Result<StreamMapping, String> {
        self.validate()?;
        let mut entities = Vec::new();
        for kind in &self.types {
            for rule in &kind.mentions {
                let Some(at) = rule.identity.iter().position(|p| *p == rule.path) else {
                    continue;
                };
                let mut key = rule.identity.clone();
                let mention = key.remove(at);
                key.push(mention);
                entities.push(EntityRule {
                    id: format!("oracle-{}", entities.len()),
                    type_label: kind.label.clone(),
                    key,
                    attrs: Vec::new(),
                });
            }
        }
        let mapping = StreamMapping {
            version: MAPPING_VERSION,
            decode: self.decode.clone(),
            entities,
            relationships: Vec::new(),
        };
        mapping
            .validate()
            .map_err(|e| format!("the oracle mapping is not valid: {e}"))?;
        Ok(mapping)
    }

    /// The unscored paths as mention path ids.
    pub(crate) fn unscored_ids(&self) -> BTreeSet<String> {
        self.unscored.iter().map(rule_id).collect()
    }

    /// Fails closed on anything that would make the key partition ambiguous.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !KEY_VERSIONS.contains(&self.version) {
            return Err(format!(
                "key spec version {} is not one of {KEY_VERSIONS:?}",
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
                self.validate_no_identity(&kind.label, rule)?;
            }
        }
        Ok(())
    }
}

impl KeySpec {
    /// A rule's `no_identity` list: format 1 only, on an identity path, every value a key part
    /// (string, integer or boolean) listed once. Anything else could never match, or would
    /// match ambiguously, so it fails closed.
    fn validate_no_identity(&self, label: &str, rule: &MentionRule) -> Result<(), String> {
        if rule.no_identity.is_empty() {
            return Ok(());
        }
        if self.version == 0 {
            return Err(format!(
                "type {label:?}: mention path {:?} has no_identity, which needs key format 1",
                rule.path
            ));
        }
        if !rule.identity.contains(&rule.path) {
            return Err(format!(
                "type {label:?}: mention path {:?} has no_identity but is not one of its identity paths",
                rule.path
            ));
        }
        let mut seen = BTreeSet::new();
        for sentinel in &rule.no_identity {
            let Some(part) = key_part(sentinel) else {
                return Err(format!(
                    "type {label:?}: no_identity value {sentinel} at {:?} is not a string, integer or boolean",
                    rule.path
                ));
            };
            if !seen.insert(part) {
                return Err(format!(
                    "type {label:?}: no_identity value {sentinel} at {:?} is listed twice",
                    rule.path
                ));
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
