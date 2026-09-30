//! The deterministic dashboard proposer (decision 0029): the fallback when no System 2
//! proposer is configured. Like the profiler it reads statistics, never meaning: every choice
//! rests on counts, distinct counts and string shares, and no tie is broken by a name, so
//! renaming keys and hashing strings changes its manifest only by the same renaming.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use s2w_model::{
    AttrLabel, BuiltOn, DashboardManifest, Domain, KeyLabel, Kind, Label, MAX_TYPES, ManifestInput,
    ManifestOutcome, ManifestProposer, PathStats, Projection, ProposerId, ProposerTrace,
    QuintessentialProjection, Role, Slots, Template, TypeRow, fits_text,
};

use crate::Profile;

/// The fallback's actor model name.
pub const FALLBACK_MODEL: &str = "dashboard-fallback";
/// The fallback's actor version. Bump it with any change to what it proposes.
pub const FALLBACK_VERSION: &str = "1";
/// The longest mean string length, in bytes, of an attribute the fallback picks as a label.
const LABEL_MAX_MEAN_LEN: u64 = 80;

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
    /// Whether some rule's first key part is mostly strings.
    string_key: bool,
    /// Candidate label attributes: name, then the best statistics seen under that name.
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
        types: rows.iter().map(|(label, e)| type_row(label, e)).collect(),
        events: None,
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
            if let Some(first) = rule.key.first().and_then(|p| stats.get(p)) {
                evidence.count = evidence.count.max(first.distinct);
                evidence.string_key |= mostly_strings(first);
            }
            for attr in &rule.attrs {
                let Some(s) = stats.get(&attr.path) else {
                    continue;
                };
                let usable = fits_text(&attr.name)
                    && !keys.contains(&&attr.path)
                    && mostly_strings(s)
                    && s.str_len_mean <= LABEL_MAX_MEAN_LEN;
                if usable {
                    let best = evidence.attrs.entry(&attr.name).or_insert(s);
                    if s.distinct > best.distinct {
                        *best = s;
                    }
                }
            }
        }
    }
    types
}

/// A type's row: its label is the attribute with the most distinct values (a unique maximum),
/// else its first key part when that is mostly strings, else none; primary when it has one.
fn type_row(label: &str, evidence: &TypeEvidence) -> TypeRow {
    let choice = unique_max(&evidence.attrs)
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
        noun: None,
        label: choice,
        kind: Some(Kind::Other),
    }
}

/// At least 90% of the path's values are strings.
fn mostly_strings(stats: &PathStats) -> bool {
    stats.count > 0 && stats.str_count.saturating_mul(10) >= stats.count.saturating_mul(9)
}

/// The candidate with the most distinct values, or `None` on a tie: a tie is never broken by
/// a name.
fn unique_max<'a>(attrs: &BTreeMap<&'a str, &PathStats>) -> Option<&'a str> {
    let mut best: Option<(&str, u64)> = None;
    let mut tied = false;
    for (name, stats) in attrs {
        match best {
            Some((_, distinct)) if stats.distinct < distinct => {}
            Some((_, distinct)) if stats.distinct == distinct => tied = true,
            _ => {
                best = Some((name, stats.distinct));
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
