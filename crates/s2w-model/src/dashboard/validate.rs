//! Validation of a [`DashboardManifest`] (decision 0029). The shape rules need no context; the
//! reference rules check type labels, attribute names, key parts, relationships and sources
//! against accepted mappings, and paths against the input profile. The write path runs both
//! and refuses on the first fault; the read path runs the reference rules against the current
//! mappings alone and reports every fault as a stale entry.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    DashboardError, DashboardManifest, Label, MAX_BUILT_ON, MAX_EVENTS, MAX_QUESTIONS, MAX_ROLES,
    MAX_SENTENCE_FIELDS, MAX_SLOT_ITEMS, MAX_STRING_CHARS, MAX_TYPES, Sentence, Slots, Template,
};
use crate::{EntityRule, FieldPath, Segment, StreamMapping};

/// One source's accepted mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedMapping {
    /// The source id.
    pub source: String,
    /// The mapping's identity.
    pub identity: String,
    /// The mapping.
    pub mapping: StreamMapping,
}

/// What a manifest is validated against on the write path: the accepted mappings of the
/// world's member sources, and the paths of the proposer's input profile, per source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManifestContext {
    /// The accepted mappings.
    pub mappings: Vec<AcceptedMapping>,
    /// Each source's profiled paths.
    pub paths: BTreeMap<String, Vec<FieldPath>>,
}

impl DashboardManifest {
    /// Every rule of decision 0029: [`Self::validate_shape`], then every reference against
    /// `ctx`.
    ///
    /// # Errors
    /// The first fault: shape faults (in field order) before reference faults.
    pub fn validate(&self, ctx: &ManifestContext) -> Result<(), DashboardError> {
        self.validate_shape()?;
        let mut faults = Vec::new();
        References::new(&ctx.mappings, Some(&ctx.paths)).check(self, &mut faults);
        faults.into_iter().next().map_or(Ok(()), Err)
    }

    /// The read path's check (decision 0029): every entry naming a source, mapping, type,
    /// attribute, key part or relationship that `current` no longer has. Paths are not checked;
    /// the read path has no profile. Empty means not stale.
    #[must_use]
    pub fn stale_entries(&self, current: &[AcceptedMapping]) -> Vec<String> {
        let mut faults = Vec::new();
        References::new(current, None).check(self, &mut faults);
        faults.iter().map(ToString::to_string).collect()
    }

    /// The rules that need no context: string length and content, list caps, exactly one
    /// default role, distinct ids and types, each template's slots, and sentence placeholders.
    ///
    /// # Errors
    /// The first fault, in field order.
    pub fn validate_shape(&self) -> Result<(), DashboardError> {
        self.shape_built_on()?;
        text("domain.name", &self.domain.name)?;
        text("domain.summary", &self.domain.summary)?;
        let projection = &self.quintessential_projection;
        text("quintessential_projection.rationale", &projection.rationale)?;
        slots(
            "quintessential_projection.slots",
            projection.template,
            &projection.slots,
        )?;
        self.shape_roles()?;
        self.shape_types()?;
        self.shape_events()
    }

    fn shape_built_on(&self) -> Result<(), DashboardError> {
        cap("built_on", self.built_on.len(), 1, MAX_BUILT_ON)?;
        let mut sources = BTreeSet::new();
        for (i, built_on) in self.built_on.iter().enumerate() {
            let at = format!("built_on[{i}]");
            text(&format!("{at}.source"), &built_on.source)?;
            if !crate::hash::is_hex16(&built_on.mapping) {
                return Err(DashboardError::NotAnIdentity {
                    at: format!("{at}.mapping"),
                });
            }
            distinct(&mut sources, &format!("{at}.source"), &built_on.source)?;
        }
        Ok(())
    }

    fn shape_roles(&self) -> Result<(), DashboardError> {
        cap("roles", self.roles.len(), 1, MAX_ROLES)?;
        let mut ids = BTreeSet::new();
        for (i, role) in self.roles.iter().enumerate() {
            let at = format!("roles[{i}]");
            text(&format!("{at}.id"), &role.id)?;
            distinct(&mut ids, &format!("{at}.id"), &role.id)?;
            text(&format!("{at}.name"), &role.name)?;
            cap(
                &format!("{at}.questions"),
                role.questions.len(),
                0,
                MAX_QUESTIONS,
            )?;
            for (q, question) in role.questions.iter().enumerate() {
                text(&format!("{at}.questions[{q}]"), question)?;
            }
            slots(
                &format!("{at}.projection.slots"),
                role.projection.template,
                &role.projection.slots,
            )?;
        }
        let defaults = self.roles.iter().filter(|role| role.default).count();
        if defaults != 1 {
            return Err(DashboardError::DefaultRoles { count: defaults });
        }
        Ok(())
    }

    fn shape_types(&self) -> Result<(), DashboardError> {
        cap("types", self.types.len(), 0, MAX_TYPES)?;
        let mut labels = BTreeSet::new();
        for (i, row) in self.types.iter().enumerate() {
            let at = format!("types[{i}]");
            text(&format!("{at}.type"), &row.type_label)?;
            distinct(&mut labels, &format!("{at}.type"), &row.type_label)?;
            if let Some(noun) = &row.noun {
                text(&format!("{at}.noun"), noun)?;
            }
            if let Some(Label::Attr(label)) = &row.label {
                text(&format!("{at}.label.attr"), &label.attr)?;
            }
        }
        Ok(())
    }

    fn shape_events(&self) -> Result<(), DashboardError> {
        let events = self.events.as_deref().unwrap_or_default();
        cap("events", events.len(), 0, MAX_EVENTS)?;
        for (i, event) in events.iter().enumerate() {
            let at = format!("events[{i}]");
            text(&format!("{at}.source"), &event.source)?;
            if let Some(when) = &event.when {
                path(&format!("{at}.when.path"), &when.path)?;
                bounded(&format!("{at}.when.equals"), &when.equals)?;
            }
            sentence(&format!("{at}.sentence"), &event.sentence)?;
        }
        Ok(())
    }
}

/// Whether `value` is a string a manifest may hold where one is required: non-empty, within
/// [`MAX_STRING_CHARS`] and free of `<` and `>`. A proposer uses it to leave out a type label
/// or attribute name the validator would refuse.
#[must_use]
pub fn fits_text(value: &str) -> bool {
    text("", value).is_ok()
}

/// A non-empty string within [`MAX_STRING_CHARS`] and free of `<` and `>`.
fn text(at: &str, value: &str) -> Result<(), DashboardError> {
    if value.is_empty() {
        return Err(DashboardError::Empty { at: at.to_owned() });
    }
    bounded(at, value)
}

/// A string within [`MAX_STRING_CHARS`] and free of `<` and `>`; it may be empty.
fn bounded(at: &str, value: &str) -> Result<(), DashboardError> {
    if value.chars().count() > MAX_STRING_CHARS {
        return Err(DashboardError::TooLong {
            at: at.to_owned(),
            max: MAX_STRING_CHARS,
        });
    }
    if value.contains(['<', '>']) {
        return Err(DashboardError::AngleBracket { at: at.to_owned() });
    }
    Ok(())
}

fn cap(at: &str, len: usize, min: usize, max: usize) -> Result<(), DashboardError> {
    if len < min {
        return Err(DashboardError::NoItems { at: at.to_owned() });
    }
    if len > max {
        return Err(DashboardError::TooMany {
            at: at.to_owned(),
            max,
        });
    }
    Ok(())
}

fn distinct<'a>(
    seen: &mut BTreeSet<&'a str>,
    at: &str,
    value: &'a str,
) -> Result<(), DashboardError> {
    if seen.insert(value) {
        Ok(())
    } else {
        Err(DashboardError::Duplicate {
            at: at.to_owned(),
            value: value.to_owned(),
        })
    }
}

fn path(at: &str, path: &FieldPath) -> Result<(), DashboardError> {
    let empty_segment = path
        .0
        .iter()
        .any(|segment| matches!(segment, Segment::Key(key) if key.is_empty()));
    if path.0.is_empty() || empty_segment {
        return Err(DashboardError::EmptyPath { at: at.to_owned() });
    }
    Ok(())
}

/// The slot table's shape rules (decision 0029): required slots present, no slot outside the
/// template, list caps, distinct list items, and `actor_type` ≠ `subject_type`.
fn slots(at: &str, template: Template, slots: &Slots) -> Result<(), DashboardError> {
    let (required, optional) = template.slots();
    let present = slots.present();
    for slot in required {
        if !present.contains(slot) {
            return Err(DashboardError::SlotMissing {
                at: at.to_owned(),
                template: template.as_str(),
                slot,
            });
        }
    }
    for slot in &present {
        if !required.contains(slot) && !optional.contains(slot) {
            return Err(DashboardError::SlotNotAllowed {
                at: at.to_owned(),
                template: template.as_str(),
                slot,
            });
        }
    }
    for (name, value) in [
        ("subject_type", &slots.subject_type),
        ("actor_type", &slots.actor_type),
        ("type", &slots.type_label),
    ] {
        if let Some(value) = value {
            text(&format!("{at}.{name}"), value)?;
        }
    }
    if slots.subject_type.is_some() && slots.subject_type == slots.actor_type {
        return Err(DashboardError::SameType {
            at: format!("{at}.actor_type"),
        });
    }
    if let Some(links) = &slots.links {
        cap(&format!("{at}.links"), links.len(), 1, MAX_SLOT_ITEMS)?;
        for (i, [from, to]) in links.iter().enumerate() {
            text(&format!("{at}.links[{i}][0]"), from)?;
            text(&format!("{at}.links[{i}][1]"), to)?;
        }
    }
    for (name, list) in [("types", &slots.types), ("columns", &slots.columns)] {
        if let Some(list) = list {
            let list_at = format!("{at}.{name}");
            cap(&list_at, list.len(), 1, MAX_SLOT_ITEMS)?;
            let mut seen = BTreeSet::new();
            for (i, item) in list.iter().enumerate() {
                text(&format!("{list_at}[{i}]"), item)?;
                distinct(&mut seen, &format!("{list_at}[{i}]"), item)?;
            }
        }
    }
    for (name, value) in field_paths(slots) {
        path(&format!("{at}.{name}"), value)?;
    }
    Ok(())
}

fn field_paths(slots: &Slots) -> impl Iterator<Item = (&'static str, &FieldPath)> {
    [
        ("lat", &slots.lat),
        ("lon", &slots.lon),
        ("price", &slots.price),
        ("quantity", &slots.quantity),
        ("side", &slots.side),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.as_ref().map(|value| (name, value)))
}

/// `{n}` placeholders: each names an existing field, each field is shown, no stray brace.
fn sentence(at: &str, sentence: &Sentence) -> Result<(), DashboardError> {
    text(&format!("{at}.text"), &sentence.text)?;
    cap(
        &format!("{at}.fields"),
        sentence.fields.len(),
        0,
        MAX_SENTENCE_FIELDS,
    )?;
    for (i, field) in sentence.fields.iter().enumerate() {
        for field_path in field.paths() {
            path(&format!("{at}.fields[{i}]"), field_path)?;
        }
    }
    let fault = |reason: String| DashboardError::Placeholder {
        at: format!("{at}.text"),
        reason,
    };
    let mut shown = vec![false; sentence.fields.len()];
    let mut chars = sentence.text.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut digits = String::new();
                let closed = loop {
                    match chars.next() {
                        Some('}') => break true,
                        Some(d) if d.is_ascii_digit() && digits.len() < 3 => digits.push(d),
                        _ => break false,
                    }
                };
                let index = digits.parse::<usize>().ok().filter(|_| closed);
                let Some(index) = index else {
                    return Err(fault("a `{` does not open a `{n}` placeholder".to_owned()));
                };
                let Some(slot) = shown.get_mut(index) else {
                    return Err(fault(format!("placeholder {{{index}}} has no field")));
                };
                *slot = true;
            }
            '}' => return Err(fault("a `}` closes no placeholder".to_owned())),
            _ => {}
        }
    }
    if let Some(unshown) = shown.iter().position(|shown| !shown) {
        return Err(fault(format!("field {unshown} is never shown")));
    }
    Ok(())
}

/// The accepted mappings indexed for the reference rules.
struct References<'a> {
    mappings: &'a [AcceptedMapping],
    paths: Option<&'a BTreeMap<String, Vec<FieldPath>>>,
    rules: BTreeMap<&'a str, Vec<&'a EntityRule>>,
    relations: BTreeSet<(&'a str, &'a str)>,
}

impl<'a> References<'a> {
    fn new(
        mappings: &'a [AcceptedMapping],
        paths: Option<&'a BTreeMap<String, Vec<FieldPath>>>,
    ) -> Self {
        let mut rules: BTreeMap<&str, Vec<&EntityRule>> = BTreeMap::new();
        let mut relations = BTreeSet::new();
        for accepted in mappings {
            let entities = &accepted.mapping.entities;
            for rule in entities {
                rules
                    .entry(rule.type_label.as_str())
                    .or_default()
                    .push(rule);
            }
            let label_of = |id: &str| {
                entities
                    .iter()
                    .find(|rule| rule.id == id)
                    .map(|rule| rule.type_label.as_str())
            };
            for relationship in &accepted.mapping.relationships {
                if let (Some(from), Some(to)) =
                    (label_of(&relationship.from), label_of(&relationship.to))
                {
                    relations.insert((from, to));
                }
            }
        }
        Self {
            mappings,
            paths,
            rules,
            relations,
        }
    }

    fn check(&self, manifest: &DashboardManifest, out: &mut Vec<DashboardError>) {
        self.built_on(manifest, out);
        let projection = &manifest.quintessential_projection;
        self.slots("quintessential_projection.slots", &projection.slots, out);
        for (i, role) in manifest.roles.iter().enumerate() {
            self.slots(
                &format!("roles[{i}].projection.slots"),
                &role.projection.slots,
                out,
            );
        }
        self.types(manifest, out);
        self.events(manifest, out);
    }

    fn built_on(&self, manifest: &DashboardManifest, out: &mut Vec<DashboardError>) {
        for (i, built_on) in manifest.built_on.iter().enumerate() {
            let at = format!("built_on[{i}]");
            match self.mappings.iter().find(|m| m.source == built_on.source) {
                None => out.push(DashboardError::UnknownSource {
                    at,
                    source_id: built_on.source.clone(),
                }),
                Some(accepted) if accepted.identity != built_on.mapping => {
                    out.push(DashboardError::MappingMismatch {
                        at: format!("{at}.mapping"),
                        source_id: built_on.source.clone(),
                        expected: accepted.identity.clone(),
                        found: built_on.mapping.clone(),
                    });
                }
                Some(_) => {}
            }
        }
    }

    fn types(&self, manifest: &DashboardManifest, out: &mut Vec<DashboardError>) {
        for (i, row) in manifest.types.iter().enumerate() {
            let at = format!("types[{i}]");
            let Some(rules) = self.type_rules(&format!("{at}.type"), &row.type_label, out) else {
                continue;
            };
            let fault = match &row.label {
                Some(Label::Attr(label)) => attr(
                    format!("{at}.label.attr"),
                    &row.type_label,
                    rules,
                    &label.attr,
                ),
                Some(Label::Key(label)) if !rules.iter().any(|rule| label.key < rule.key.len()) => {
                    Some(DashboardError::KeyOutOfRange {
                        at: format!("{at}.label.key"),
                        type_label: row.type_label.clone(),
                        index: label.key,
                    })
                }
                Some(Label::Key(_)) | None => None,
            };
            out.extend(fault);
        }
    }

    fn events(&self, manifest: &DashboardManifest, out: &mut Vec<DashboardError>) {
        for (i, event) in manifest.events.iter().flatten().enumerate() {
            let at = format!("events[{i}]");
            if !self.mappings.iter().any(|m| m.source == event.source) {
                out.push(DashboardError::UnknownSource {
                    at: format!("{at}.source"),
                    source_id: event.source.clone(),
                });
                continue;
            }
            if let Some(when) = &event.when {
                self.path(
                    &format!("{at}.when.path"),
                    Some(&event.source),
                    &when.path,
                    out,
                );
            }
            for (f, field) in event.sentence.fields.iter().enumerate() {
                for field_path in field.paths() {
                    let at = format!("{at}.sentence.fields[{f}]");
                    self.path(&at, Some(&event.source), field_path, out);
                }
            }
        }
    }

    fn slots(&self, at: &str, slots: &Slots, out: &mut Vec<DashboardError>) {
        for (name, value) in [
            ("subject_type", &slots.subject_type),
            ("actor_type", &slots.actor_type),
        ] {
            if let Some(label) = value {
                self.type_rules(&format!("{at}.{name}"), label, out);
            }
        }
        for (i, [from, to]) in slots.links.iter().flatten().enumerate() {
            let known = self
                .type_rules(&format!("{at}.links[{i}][0]"), from, out)
                .is_some()
                & self
                    .type_rules(&format!("{at}.links[{i}][1]"), to, out)
                    .is_some();
            if known && !self.relations.contains(&(from.as_str(), to.as_str())) {
                out.push(DashboardError::UnknownRelationship {
                    at: format!("{at}.links[{i}]"),
                    from: from.clone(),
                    to: to.clone(),
                });
            }
        }
        for (i, label) in slots.types.iter().flatten().enumerate() {
            self.type_rules(&format!("{at}.types[{i}]"), label, out);
        }
        if let Some(label) = &slots.type_label
            && let Some(rules) = self.type_rules(&format!("{at}.type"), label, out)
        {
            for (i, column) in slots.columns.iter().flatten().enumerate() {
                out.extend(attr(format!("{at}.columns[{i}]"), label, rules, column));
            }
        }
        for (name, value) in field_paths(slots) {
            self.path(&format!("{at}.{name}"), None, value, out);
        }
    }

    fn type_rules(
        &self,
        at: &str,
        label: &str,
        out: &mut Vec<DashboardError>,
    ) -> Option<&[&'a EntityRule]> {
        let rules = self.rules.get(label).map(Vec::as_slice);
        if rules.is_none() {
            out.push(DashboardError::UnknownType {
                at: at.to_owned(),
                label: label.to_owned(),
            });
        }
        rules
    }

    /// A path must be in the profile of `source`, or of any source when `source` is `None`.
    /// Skipped on the read path, which has no profile.
    fn path(
        &self,
        at: &str,
        source: Option<&str>,
        path: &FieldPath,
        out: &mut Vec<DashboardError>,
    ) {
        let Some(paths) = self.paths else {
            return;
        };
        let found = match source {
            Some(source) => paths.get(source).is_some_and(|p| p.contains(path)),
            None => paths.values().any(|p| p.contains(path)),
        };
        if !found {
            out.push(DashboardError::UnknownPath { at: at.to_owned() });
        }
    }
}

/// `attr` must be an attribute name of one of the type's rules.
fn attr(at: String, type_label: &str, rules: &[&EntityRule], attr: &str) -> Option<DashboardError> {
    let known = rules
        .iter()
        .any(|rule| rule.attrs.iter().any(|a| a.name == attr));
    (!known).then(|| DashboardError::UnknownAttr {
        at,
        type_label: type_label.to_owned(),
        attr: attr.to_owned(),
    })
}
