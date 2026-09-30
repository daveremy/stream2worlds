//! The deterministic dashboard proposer (decision 0029): the fallback when no System 2
//! proposer is configured. Like the profiler it reads statistics, never meaning: every choice
//! rests on counts, distinct counts and string shares, and no tie is broken by a name, so
//! renaming keys and hashing strings changes its manifest only by the same renaming.
//!
//! Version 2 skips date-time paths as labels (the profiler's `Timestamp` role, decision
//! 0030), breaks a distinct-count tie by the shorter mean string length, names each type's
//! noun after its type label, and gives each source a sentence: its event type and its busiest
//! labelled type's key.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use s2w_model::{
    AttrLabel, BuiltOn, DashboardManifest, Domain, EventSentence, KeyLabel, Kind, Label,
    MAX_BUILT_ON, MAX_EVENTS, MAX_TYPES, ManifestInput, ManifestOutcome, ManifestProposer,
    PathStats, Projection, ProposerId, ProposerTrace, QuintessentialProjection, Role, Sentence,
    SentenceField, Slots, SourceInput, Template, TypeRow, fits_text,
};

use crate::{Profile, Role as PathRole};

/// The fallback's actor model name.
pub const FALLBACK_MODEL: &str = "dashboard-fallback";
/// The fallback's actor version. Bump it with any change to what it proposes.
pub const FALLBACK_VERSION: &str = "3";
/// The longest mean string length, in bytes, of an attribute the fallback picks as a label.
const LABEL_MAX_MEAN_LEN: u64 = 80;
/// The fewest distinct values an attribute the fallback picks as a label may have: a sample-size
/// guard, since a share of distinct values over fewer values than this is noise.
const MIN_LABEL_DISTINCT: u64 = 8;

/// Copies the profiler's numbers into the dashboard input's own statistics type.
#[must_use]
pub fn path_stats(profile: &Profile) -> Vec<PathStats> {
    let wide = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    profile
        .paths
        .iter()
        .map(|p| PathStats {
            path: p.path.clone(),
            count: wide(p.count),
            distinct: wide(p.distinct),
            str_count: wide(p.str_count),
            str_len_mean: wide(p.str_len_mean),
            timestamp: p.role == PathRole::Timestamp,
        })
        .collect()
}

/// The deterministic proposer: the `feed` projection, one default role with no questions, and
/// a label per type where the statistics support one. It answers the domain level only.
#[derive(Clone, Copy, Debug, Default)]
pub struct FallbackProposer;

impl ManifestProposer for FallbackProposer {
    fn id(&self) -> ProposerId {
        ProposerId {
            model: FALLBACK_MODEL.to_owned(),
            version: FALLBACK_VERSION.to_owned(),
        }
    }

    fn propose(&self, input: &ManifestInput) -> ManifestOutcome {
        // `built_on` names every mapped source, so past its cap no manifest could validate:
        // abstain rather than file a proposal that burns an attempt.
        if input.sources.len() > MAX_BUILT_ON {
            return ManifestOutcome::Abstain(format!(
                "{} mapped sources exceed the manifest's {MAX_BUILT_ON}",
                input.sources.len()
            ));
        }
        match fallback(input) {
            Ok(manifest) => ManifestOutcome::Manifest {
                manifest: Box::new(manifest),
                trace: ProposerTrace::default(),
            },
            Err(reason) => ManifestOutcome::Abstain(reason),
        }
    }
}

/// One type label's evidence across every rule that carries it.
#[derive(Default)]
struct TypeEvidence<'a> {
    /// Distinct values of a rule's first key path; the largest over the label's rules.
    count: u64,
    /// Whether some rule's first key part is mostly strings and not a date-time.
    string_key: bool,
    /// Candidate label attributes: name, then the best statistics seen under that name. Like
    /// `count`, a merged maximum: an attribute's statistics may come from another rule with the
    /// same label than the one whose key set `count`.
    attrs: BTreeMap<&'a str, &'a PathStats>,
}

fn fallback(input: &ManifestInput) -> Result<DashboardManifest, String> {
    if input.sources.is_empty() {
        return Err("no member source has an accepted mapping".to_owned());
    }
    let mut rows: Vec<(&str, TypeEvidence)> = evidence(input).into_iter().collect();
    if rows.is_empty() {
        return Err("no accepted mapping has a type label a manifest can hold".to_owned());
    }
    // Stable: equal counts keep label order. Only the cap at MAX_TYPES reads that order, and
    // only when labels tie at the cut.
    rows.sort_by_key(|row| Reverse(row.1.count));
    rows.truncate(MAX_TYPES);
    let subject = match rows.as_slice() {
        [only] => Some(only.0),
        [first, second, ..] if first.1.count > second.1.count => Some(first.0),
        _ => None,
    };
    let slots = Slots {
        subject_type: subject.map(str::to_owned),
        ..Slots::default()
    };
    let types: Vec<TypeRow> = rows.iter().map(|(label, e)| type_row(label, e)).collect();
    let events = sentences(input, &types);
    Ok(DashboardManifest {
        built_on: input
            .sources
            .iter()
            .map(|s| BuiltOn {
                source: s.source.clone(),
                mapping: s.mapping_identity.clone(),
            })
            .collect(),
        domain: Domain {
            name: "Unnamed domain".to_owned(),
            summary: format!(
                "{} source(s) and {} entity type(s), described from statistics alone.",
                input.sources.len(),
                rows.len()
            ),
        },
        quintessential_projection: QuintessentialProjection {
            template: Template::Feed,
            rationale: "The deterministic fallback reads statistics, not meaning, so it shows \
                        the live feed of events."
                .to_owned(),
            slots: slots.clone(),
        },
        roles: vec![Role {
            id: "r1".to_owned(),
            name: "Observer".to_owned(),
            default: true,
            questions: Vec::new(),
            projection: Projection {
                template: Template::Feed,
                slots,
            },
        }],
        types,
        events,
    })
}

/// Every type label a manifest can hold, with its evidence merged over the rules that carry
/// it. Keyed by label only to merge; nothing reads the key order as a choice.
fn evidence(input: &ManifestInput) -> BTreeMap<&str, TypeEvidence<'_>> {
    let mut types: BTreeMap<&str, TypeEvidence> = BTreeMap::new();
    for source in &input.sources {
        let stats: BTreeMap<_, _> = source.paths.iter().map(|s| (&s.path, s)).collect();
        let keys: Vec<_> = source
            .mapping
            .entities
            .iter()
            .flat_map(|r| &r.key)
            .collect();
        for rule in &source.mapping.entities {
            if !fits_text(&rule.type_label) {
                continue;
            }
            let evidence = types.entry(&rule.type_label).or_default();
            // With no profiled key, how often the rule's entities occur cannot be judged, so
            // none of its attributes may name them.
            let Some(first) = rule.key.first().and_then(|p| stats.get(p)) else {
                continue;
            };
            evidence.count = evidence.count.max(first.distinct);
            evidence.string_key |= mostly_strings(first) && !first.timestamp;
            for attr in &rule.attrs {
                let Some(s) = stats.get(&attr.path) else {
                    continue;
                };
                // Coverage: an attribute on more than twice as many events as the rule's key
                // describes the event, not the entity. Its distinct count is taken over those
                // other events too, so the share test below would compare two populations.
                let usable = s.count <= first.count.saturating_mul(2)
                    && fits_text(&attr.name)
                    && !keys.contains(&&attr.path)
                    && !s.timestamp
                    && mostly_strings(s)
                    && s.str_len_mean <= LABEL_MAX_MEAN_LEN;
                if usable {
                    let best = evidence.attrs.entry(&attr.name).or_insert(s);
                    if (Reverse(s.distinct), s.str_len_mean)
                        < (Reverse(best.distinct), best.str_len_mean)
                    {
                        *best = s;
                    }
                }
            }
        }
    }
    types
}

/// A type's row: its label is the attribute with the most distinct values, the shorter mean
/// string length breaking a tie (a unique best), among attributes with at least
/// `MIN_LABEL_DISTINCT` distinct values and at least half as many as the type has entities,
/// else its first key part when that is mostly strings and not a date-time, else none;
/// primary when it has one. Its noun is its type label.
fn type_row(label: &str, evidence: &TypeEvidence) -> TypeRow {
    // An attribute with fewer distinct values than half the type's entities names a category
    // (an edit kind, a content model), not an entity.
    let naming: BTreeMap<_, _> = evidence
        .attrs
        .iter()
        .filter(|(_, s)| {
            evidence.count > 0
                && s.distinct >= MIN_LABEL_DISTINCT
                && s.distinct.saturating_mul(2) >= evidence.count
        })
        .map(|(name, s)| (*name, *s))
        .collect();
    let choice = unique_max(&naming)
        .map(|attr| {
            Label::Attr(AttrLabel {
                attr: attr.to_owned(),
            })
        })
        .or_else(|| {
            evidence
                .string_key
                .then_some(Label::Key(KeyLabel { key: 0 }))
        });
    TypeRow {
        type_label: label.to_owned(),
        primary: choice.is_some(),
        noun: Some(label.to_owned()),
        label: choice,
        kind: Some(Kind::Other),
    }
}

/// At least 90% of the path's values are strings.
fn mostly_strings(stats: &PathStats) -> bool {
    stats.count > 0 && stats.str_count.saturating_mul(10) >= stats.count.saturating_mul(9)
}

/// The candidate with the most distinct values, then the shortest mean string length, or
/// `None` when two tie on both: a tie is never broken by a name.
fn unique_max<'a>(attrs: &BTreeMap<&'a str, &PathStats>) -> Option<&'a str> {
    let rank = |stats: &PathStats| (Reverse(stats.distinct), stats.str_len_mean);
    let mut best: Option<(&str, (Reverse<u64>, u64))> = None;
    let mut tied = false;
    for (name, stats) in attrs {
        let r = rank(stats);
        match best {
            Some((_, b)) if r > b => {}
            Some((_, b)) if r == b => tied = true,
            _ => {
                best = Some((name, r));
                tied = false;
            }
        }
    }
    if tied {
        None
    } else {
        best.map(|(name, _)| name)
    }
}

/// One sentence per source, `type key`, for the source's busiest primary type: the rule whose
/// first key path has the most distinct values (a unique maximum). When the source has an
/// event-type path, a sentence that also names the event type comes first; the renderer falls
/// through to the plain one on an event that lacks it. `None` when no source gets one.
fn sentences(input: &ManifestInput, types: &[TypeRow]) -> Option<Vec<EventSentence>> {
    let mut out = Vec::new();
    for source in &input.sources {
        let Some((type_label, key)) = busiest(source, types) else {
            continue;
        };
        let event_type = source
            .event_type
            .as_ref()
            .filter(|p| source.paths.iter().any(|s| &s.path == *p));
        if let Some(event_type) = event_type {
            out.push(EventSentence {
                source: source.source.clone(),
                when: None,
                sentence: Sentence {
                    text: format!("{{0}}: {type_label} {{1}}"),
                    fields: vec![
                        SentenceField::Path(event_type.clone()),
                        SentenceField::Path(key.clone()),
                    ],
                },
            });
        }
        out.push(EventSentence {
            source: source.source.clone(),
            when: None,
            sentence: Sentence {
                text: format!("{type_label} {{0}}"),
                fields: vec![SentenceField::Path(key.clone())],
            },
        });
    }
    // Two entries per source at most; the cap on sources keeps this far under MAX_EVENTS.
    out.truncate(MAX_EVENTS);
    (!out.is_empty()).then_some(out)
}

/// The source's primary type whose rule has the most distinct first-key values, with that key
/// path; `None` when two types tie or no primary type's key was profiled as a non-date-time. A type label that holds a
/// brace or does not fit a sentence's text is skipped.
fn busiest<'a>(
    source: &'a SourceInput,
    types: &[TypeRow],
) -> Option<(&'a str, &'a s2w_model::FieldPath)> {
    let primary = |label: &str| types.iter().any(|t| t.primary && t.type_label == label);
    let mut best: Option<(&str, &s2w_model::FieldPath, u64)> = None;
    let mut tied = false;
    for rule in &source.mapping.entities {
        let label = rule.type_label.as_str();
        if !primary(label)
            || label.contains(['{', '}'])
            || !fits_text(&format!("{{0}}: {label} {{1}}"))
        {
            continue;
        }
        let Some(key) = rule.key.first() else {
            continue;
        };
        // A date-time key would make the sentence read "type 2026-09-30T…".
        let Some(stats) = source.paths.iter().find(|s| &s.path == key && !s.timestamp) else {
            continue;
        };
        match best {
            // Rules that share a type label name one entity type, so a second one is not a
            // tie: the one with more distinct values, then the shallower key path, stands for
            // it (a structural fact, not a name).
            Some((l, k, d)) if l == label => {
                if (Reverse(stats.distinct), key.0.len()) < (Reverse(d), k.0.len()) {
                    best = Some((label, key, stats.distinct));
                    // Now a strict maximum over every type seen, when its count grew.
                    tied &= stats.distinct == d;
                }
            }
            Some((_, _, d)) if stats.distinct < d => {}
            Some((_, _, d)) if stats.distinct == d => tied = true,
            _ => {
                best = Some((label, key, stats.distinct));
                tied = false;
            }
        }
    }
    if tied {
        None
    } else {
        best.map(|(l, k, _)| (l, k))
    }
}
