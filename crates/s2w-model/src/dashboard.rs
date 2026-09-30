//! The dashboard manifest (decision 0029): how a world is presented, as persisted data. Every
//! key is generic and every value comes from a proposer, so this module names no domain. The
//! type labels, attribute names and paths a manifest holds are checked against the accepted
//! mappings (decision 0021) and the proposer's input profile, never against a built-in list.
//!
//! Format 1 is frozen by [`DashboardManifest::identity`]'s pinned test: change a field and the
//! pinned identities move, which needs a decision record.

mod propose;
mod render;
mod validate;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use crate::{FieldPath, Fnv64};

pub use propose::{
    ManifestInput, ManifestOutcome, ManifestProposer, PathStats, ProposerId, ProposerTrace,
    SourceInput,
};
pub use render::{TRUNCATE_CHARS, render_sentence, sentence_for};
pub use validate::{AcceptedMapping, ManifestContext, fits_text};

/// The one manifest format this build reads and writes.
pub const DASHBOARD_FORMAT: u32 = 1;
/// The longest string a manifest may hold, in characters.
pub const MAX_STRING_CHARS: usize = 200;
/// The most `built_on` entries.
pub const MAX_BUILT_ON: usize = 32;
/// The most roles.
pub const MAX_ROLES: usize = 8;
/// The most questions per role.
pub const MAX_QUESTIONS: usize = 5;
/// The most type rows.
pub const MAX_TYPES: usize = 64;
/// The most event sentences.
pub const MAX_EVENTS: usize = 64;
/// The most fields one sentence shows.
pub const MAX_SENTENCE_FIELDS: usize = 8;
/// The most items in a list slot (`links`, `types`, `columns`).
pub const MAX_SLOT_ITEMS: usize = 8;

/// A world's dashboard manifest, format 1 (decision 0029).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardManifest {
    /// The accepted mappings this manifest was built from.
    pub built_on: Vec<BuiltOn>,
    /// What the world is.
    pub domain: Domain,
    /// The form a native of the domain recognises as what it is.
    pub quintessential_projection: QuintessentialProjection,
    /// Who uses a view of this world; exactly one is the default.
    pub roles: Vec<Role>,
    /// Presentation of each entity type; a row may hold only `type` and `primary`.
    pub types: Vec<TypeRow>,
    /// One sentence template per event type. Optional as a whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<EventSentence>>,
}

/// One accepted mapping a manifest was built from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltOn {
    /// The source id.
    pub source: String,
    /// The mapping's identity (`StreamMapping::identity`, 16 lowercase hex digits).
    pub mapping: String,
}

/// What the world is, in the proposer's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Domain {
    /// A short name.
    pub name: String,
    /// One sentence.
    pub summary: String,
}

/// The world's quintessential projection: a template, its slots, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuintessentialProjection {
    /// The projection template.
    pub template: Template,
    /// Why a native of the domain recognises this form.
    pub rationale: String,
    /// The template's slots (decision 0029, slot table).
    pub slots: Slots,
}

/// A projection template with its slots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    /// The projection template.
    pub template: Template,
    /// The template's slots (decision 0029, slot table).
    pub slots: Slots,
}

/// The closed set of projection templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Template {
    /// One subject at a time, with its actors and links.
    Document,
    /// A stream of recent activity.
    Feed,
    /// Entities and their relationships.
    Graph,
    /// Positions on a map.
    Map,
    /// Two-sided price levels.
    Ladder,
    /// Rows of one type.
    Table,
}

impl Template {
    /// The template's name as serialized.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Feed => "feed",
            Self::Graph => "graph",
            Self::Map => "map",
            Self::Ladder => "ladder",
            Self::Table => "table",
        }
    }

    /// The template's (required, optional) slot names (decision 0029, slot table).
    #[must_use]
    pub const fn slots(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::Document => (&["subject_type"], &["actor_type", "links"]),
            Self::Feed => (&[], &["subject_type", "actor_type"]),
            Self::Graph => (&["types"], &[]),
            Self::Table => (&["type"], &["columns"]),
            Self::Map => (&["lat", "lon"], &["subject_type"]),
            Self::Ladder => (&["price", "quantity"], &["side"]),
        }
    }
}

/// Every slot any template has. Which ones a template allows is [`Template::slots`]; a slot
/// name outside this struct is refused when decoding.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slots {
    /// A type label: the subject (`document`, `feed`, `map`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<String>,
    /// A type label: the actor (`document`, `feed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_type: Option<String>,
    /// Relationships as `[from type, to type]` pairs (`document`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<[String; 2]>>,
    /// Distinct type labels (`graph`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<String>>,
    /// A type label (`table`).
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_label: Option<String>,
    /// Attribute names of the `type` slot's type (`table`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    /// A path in the input profile (`map`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<FieldPath>,
    /// A path in the input profile (`map`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<FieldPath>,
    /// A path in the input profile (`ladder`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<FieldPath>,
    /// A path in the input profile (`ladder`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<FieldPath>,
    /// A path in the input profile (`ladder`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<FieldPath>,
}

impl Slots {
    /// The names of the slots that hold a value, in declaration order.
    #[must_use]
    pub fn present(&self) -> Vec<&'static str> {
        let flags = [
            ("subject_type", self.subject_type.is_some()),
            ("actor_type", self.actor_type.is_some()),
            ("links", self.links.is_some()),
            ("types", self.types.is_some()),
            ("type", self.type_label.is_some()),
            ("columns", self.columns.is_some()),
            ("lat", self.lat.is_some()),
            ("lon", self.lon.is_some()),
            ("price", self.price.is_some()),
            ("quantity", self.quantity.is_some()),
            ("side", self.side.is_some()),
        ];
        flags
            .into_iter()
            .filter_map(|(name, set)| set.then_some(name))
            .collect()
    }
}

/// One role: who uses a view, what they ask, and which projection answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    /// Unique among the manifest's roles.
    pub id: String,
    /// The role's name.
    pub name: String,
    /// Whether this is the default role; exactly one is.
    pub default: bool,
    /// Up to [`MAX_QUESTIONS`] questions this role asks.
    pub questions: Vec<String>,
    /// The projection that answers them.
    pub projection: Projection,
}

/// Presentation of one entity type. Only `type` and `primary` are required; the viewer falls
/// back field by field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeRow {
    /// The type label, as an accepted mapping's entity rule names it.
    #[serde(rename = "type")]
    pub type_label: String,
    /// Whether the type is one of the world's primary entities.
    pub primary: bool,
    /// A singular noun for one entity of the type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noun: Option<String>,
    /// Where an entity's human-readable label comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,
    /// The kind of thing the type is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
}

/// Where an entity's label comes from: one of its attributes, or one part of its key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Label {
    /// An attribute of the type.
    Attr(AttrLabel),
    /// A part of the type's key.
    Key(KeyLabel),
}

/// A label taken from an attribute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttrLabel {
    /// An attribute name of the type's entity rules.
    pub attr: String,
}

/// A label taken from a key part.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyLabel {
    /// A key part index of the type's entity rules.
    pub key: usize,
}

/// The closed set of entity kinds. The viewer maps a kind to an icon, never a domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A person or account.
    Person,
    /// A document or article.
    Document,
    /// A category or group.
    Category,
    /// A place.
    Place,
    /// An organisation.
    Organisation,
    /// An occurrence.
    Event,
    /// Anything else.
    Other,
}

/// One event type's sentence template.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSentence {
    /// The source id whose events it renders.
    pub source: String,
    /// Which event type, by the profiler's event-type path; absent means every event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    /// The template.
    pub sentence: Sentence,
}

/// Selects one event type: the value at `path` equals `equals`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct When {
    /// The event-type path.
    pub path: FieldPath,
    /// The event type's value.
    pub equals: String,
}

/// A structured sentence: `{n}` placeholders in `text` name `fields[n]`. Paths never appear in
/// free text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sentence {
    /// The text with `{n}` placeholders.
    pub text: String,
    /// The fields the placeholders show; each is shown at least once.
    pub fields: Vec<SentenceField>,
}

/// One sentence field: a path's value, or a formatter over paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SentenceField {
    /// The value at a path.
    Path(FieldPath),
    /// The signed integer difference `delta[0] - delta[1]`.
    Delta(DeltaField),
    /// The value at a path, cut at 120 characters.
    Truncate(TruncateField),
}

/// The `delta` formatter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeltaField {
    /// The minuend and subtrahend paths.
    pub delta: [FieldPath; 2],
}

/// The `truncate` formatter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TruncateField {
    /// The path.
    pub truncate: FieldPath,
}

impl SentenceField {
    /// The paths the field reads.
    #[must_use]
    pub fn paths(&self) -> Vec<&FieldPath> {
        match self {
            Self::Path(path) | Self::Truncate(TruncateField { truncate: path }) => vec![path],
            Self::Delta(DeltaField { delta }) => delta.iter().collect(),
        }
    }
}

/// Why a manifest is not valid. `at` names the field, JSON-pointer style.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DashboardError {
    /// The manifest could not be encoded to canonical JSON.
    #[error("the manifest could not be encoded: {0}")]
    Encode(String),
    /// An empty string where a value is required.
    #[error("{at} is empty")]
    Empty {
        /// The field.
        at: String,
    },
    /// A string longer than [`MAX_STRING_CHARS`].
    #[error("{at} is longer than {max} characters")]
    TooLong {
        /// The field.
        at: String,
        /// The limit.
        max: usize,
    },
    /// A string holding `<` or `>`.
    #[error("{at} holds `<` or `>`")]
    AngleBracket {
        /// The field.
        at: String,
    },
    /// A list longer than its cap.
    #[error("{at} holds more than {max} items")]
    TooMany {
        /// The field.
        at: String,
        /// The cap.
        max: usize,
    },
    /// An empty list where at least one item is required.
    #[error("{at} is empty; at least one item is required")]
    NoItems {
        /// The field.
        at: String,
    },
    /// A value repeated where values must be distinct.
    #[error("{at} repeats {value:?}")]
    Duplicate {
        /// The field.
        at: String,
        /// The repeated value.
        value: String,
    },
    /// Not exactly one default role.
    #[error("roles has {count} default roles; exactly one is required")]
    DefaultRoles {
        /// How many roles have `default: true`.
        count: usize,
    },
    /// A mapping identity that is not 16 lowercase hex digits.
    #[error("{at} is not a mapping identity (16 lowercase hex digits)")]
    NotAnIdentity {
        /// The field.
        at: String,
    },
    /// A source with no accepted mapping.
    #[error("{at}: source {source_id:?} has no accepted mapping")]
    UnknownSource {
        /// The field.
        at: String,
        /// The source id.
        source_id: String,
    },
    /// A `built_on` identity that is not the source's accepted mapping.
    #[error("{at}: mapping {found} is not source {source_id:?}'s accepted mapping {expected}")]
    MappingMismatch {
        /// The field.
        at: String,
        /// The source id.
        source_id: String,
        /// The accepted mapping's identity.
        expected: String,
        /// The manifest's identity.
        found: String,
    },
    /// A type label no accepted mapping has.
    #[error("{at}: no accepted mapping has type {label:?}")]
    UnknownType {
        /// The field.
        at: String,
        /// The label.
        label: String,
    },
    /// An attribute name the type's rules do not have.
    #[error("{at}: type {type_label:?} has no attribute {attr:?}")]
    UnknownAttr {
        /// The field.
        at: String,
        /// The type label.
        type_label: String,
        /// The attribute name.
        attr: String,
    },
    /// A key part index past every rule's key of the type.
    #[error("{at}: type {type_label:?} has no key part {index}")]
    KeyOutOfRange {
        /// The field.
        at: String,
        /// The type label.
        type_label: String,
        /// The index.
        index: usize,
    },
    /// A `[from, to]` pair no accepted mapping relates.
    #[error("{at}: no accepted mapping relates {from:?} to {to:?}")]
    UnknownRelationship {
        /// The field.
        at: String,
        /// The from type label.
        from: String,
        /// The to type label.
        to: String,
    },
    /// `actor_type` names the same type as `subject_type`.
    #[error("{at}: actor_type must differ from subject_type")]
    SameType {
        /// The field.
        at: String,
    },
    /// A required slot is missing.
    #[error("{at}: template {template} requires slot {slot}")]
    SlotMissing {
        /// The field.
        at: String,
        /// The template.
        template: &'static str,
        /// The slot.
        slot: &'static str,
    },
    /// A slot the template does not have.
    #[error("{at}: template {template} has no slot {slot}")]
    SlotNotAllowed {
        /// The field.
        at: String,
        /// The template.
        template: &'static str,
        /// The slot.
        slot: &'static str,
    },
    /// An empty path, or a path with an empty key segment.
    #[error("{at} is an empty path or has an empty segment")]
    EmptyPath {
        /// The field.
        at: String,
    },
    /// A path the input profile does not hold.
    #[error("{at}: the input profile has no such path")]
    UnknownPath {
        /// The field.
        at: String,
    },
    /// A sentence whose placeholders and fields do not match.
    #[error("{at}: {reason}")]
    Placeholder {
        /// The field.
        at: String,
        /// What is wrong.
        reason: String,
    },
}

impl DashboardManifest {
    /// The manifest's identity (decision 0029): FNV-1a 64 over the length-prefixed format
    /// (`u32` little-endian) and canonical JSON, as 16 lowercase hex digits. The canonical JSON
    /// is `serde_json` of the struct in declaration order with absent optional fields skipped,
    /// so whitespace and key order in a stored payload never move it.
    ///
    /// # Errors
    /// [`DashboardError::Encode`] if serialization fails.
    pub fn identity(&self) -> Result<String, DashboardError> {
        let canonical =
            serde_json::to_vec(self).map_err(|error| DashboardError::Encode(error.to_string()))?;
        let digest = Fnv64::new()
            .write_field(&DASHBOARD_FORMAT.to_le_bytes())
            .write_field(&canonical)
            .finish();
        Ok(format!("{digest:016x}"))
    }
}
