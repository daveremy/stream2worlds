//! Structure discovery, H-lite (decision 0022; research 0002 §6 stages 1 to 4, plus per-event
//! value-equality aliases and co-occurrence relationships).
//!
//! [`discover`] reads a window of raw payloads, exactly as the log stores them, and proposes a
//! [`StreamMapping`] or abstains. It reads statistics, never meaning: every decision rests on
//! value equality, presence and stream order, so renaming every key and hashing every string
//! yields the same mapping under the renamed paths (`cargo xtask check` 12). Type labels and
//! attribute names are built from the key names the stream carries, as data.
//!
//! Pure: no I/O, no clock, no randomness, no hash-order iteration.
#![deny(clippy::print_stdout, clippy::print_stderr)]

mod assemble;
mod flatten;
mod roles;

use s2w_model::{FieldPath, Segment, StreamMapping};

pub use roles::Role;

/// The profiler's version, recorded on every proposal it makes (`Actor::Agent { model: "h-lite",
/// version }`, decision 0025). Bump it with any change to `Config::default()` or to a rule, so
/// grading by (actor, version) (decision 0019) never pools two profilers' proposals. Not the
/// crate version: the workspace keeps every crate at 0.0.0.
pub const PROFILER_VERSION: &str = "1";

/// Thresholds. Percentages are whole percent, compared on integer ratios rounded down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Fewer parsed events than this and the profiler abstains.
    pub min_events: usize,
    /// Minimum events carrying a path, or a pair of paths, before it counts as evidence.
    pub min_support: usize,
    /// Minimum repeated values before a dependency is tested.
    pub min_groups: usize,
    /// Distinct/count at or above this: an event id.
    pub event_id_pct: usize,
    /// Distinct/count at or above this, below `event_id_pct`: abstain.
    pub grey_uniqueness_pct: usize,
    /// Interleaved repeats below this share: a sequence, not an identifier.
    pub min_recurrence_pct: usize,
    /// A dependent constant in at least this share of groups makes an entity or attribute.
    pub fd_accept_pct: usize,
    /// Between this and `fd_accept_pct`: abstain.
    pub fd_grey_pct: usize,
    /// Equal in at least this share of shared events: two paths are aliases.
    pub alias_pct: usize,
    /// Parses as a JSON object in at least this share of events: a decode step. 100 by default:
    /// the executor abstains on a whole event whose decode step fails.
    pub decode_pct: usize,
    /// Most values an event-type field may have.
    pub category_max: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_events: 1000,
            min_support: 20,
            min_groups: 5,
            event_id_pct: 98,
            grey_uniqueness_pct: 90,
            min_recurrence_pct: 10,
            fd_accept_pct: 95,
            fd_grey_pct: 80,
            alias_pct: 99,
            decode_pct: 100,
            category_max: 32,
        }
    }
}

/// One path's statistics and role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathProfile {
    /// The path, including any decode prefix.
    pub path: FieldPath,
    /// Events that carry it.
    pub count: usize,
    /// Distinct keyable values.
    pub distinct: usize,
    /// What the profiler decided it is.
    pub role: Role,
}

/// What the profiler measured, including every abstention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// Payloads that parsed as a JSON object.
    pub events: usize,
    /// Payloads that did not.
    pub skipped: usize,
    /// Root fields whose string value holds JSON.
    pub decode: Vec<FieldPath>,
    /// The field whose value best explains which optional fields an event carries (stage 3).
    pub event_type: Option<FieldPath>,
    /// Every path, sorted.
    pub paths: Vec<PathProfile>,
}

/// The profiler's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Discovery {
    /// A mapping that passed `validate()`.
    Mapping(StreamMapping),
    /// No mapping, and why. The honest failure.
    Abstain(String),
}

/// Profiles `payloads` (stored bytes, in stream order) and proposes a mapping or abstains.
#[must_use]
pub fn discover(payloads: &[&[u8]], cfg: &Config) -> (Profile, Discovery) {
    let table = flatten::Table::build(payloads, cfg);
    let roles: Vec<Role> = (0..table.paths.len())
        .map(|p| {
            roles::single_column(&table.columns[p], cfg)
                .unwrap_or_else(|| roles::dependency_role(&table, p, cfg))
        })
        .collect();
    let mut paths: Vec<PathProfile> = table
        .paths
        .iter()
        .zip(&table.columns)
        .zip(&roles)
        .map(|((path, column), role)| PathProfile {
            path: path.clone(),
            count: column.cells.len(),
            distinct: column.texts.len(),
            role: *role,
        })
        .collect();
    paths.sort_by(|a, b| a.path.cmp(&b.path));
    let profile = Profile {
        events: table.events,
        skipped: table.skipped,
        decode: table.decode.clone(),
        event_type: roles::event_type(&table, cfg).map(|p| table.paths[p].clone()),
        paths,
    };
    let discovery = if table.events < cfg.min_events {
        Discovery::Abstain(format!(
            "{} events, fewer than {}",
            table.events, cfg.min_events
        ))
    } else {
        match assemble::assemble(&table, &roles, cfg) {
            Ok(mapping) => Discovery::Mapping(mapping),
            Err(reason) => Discovery::Abstain(reason),
        }
    };
    (profile, discovery)
}

/// A rule id or attribute name for `path`: its segments joined by `.`, with `\` and `.` inside
/// a segment escaped by `\`, so distinct key paths never share an id (the profiler emits
/// no index segments: arrays are skipped).
#[must_use]
pub fn rule_id(path: &FieldPath) -> String {
    path.0
        .iter()
        .map(|s| match s {
            Segment::Key(k) => escape(k, &['.']),
            Segment::Index(i) => i.to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Type labels for entity types, one per class of key paths, in the same order. A label is the
/// sorted, distinct `parent/leaf` tails of the class's paths joined by `+` (`\`, `/` and `+`
/// escaped), built from the stream's own key names until System 2 names types. When two classes
/// would share a label, both use their full paths instead, which are distinct.
#[must_use]
pub fn type_labels(classes: &[Vec<FieldPath>]) -> Vec<String> {
    let short: Vec<String> = classes.iter().map(|c| label(c, 2)).collect();
    short
        .iter()
        .zip(classes)
        .map(|(l, c)| {
            if short.iter().filter(|o| *o == l).count() > 1 {
                label(c, usize::MAX)
            } else {
                l.clone()
            }
        })
        .collect()
}

/// The last `depth` key segments of each path, joined by `/`; distinct tails joined by `+`.
fn label(paths: &[FieldPath], depth: usize) -> String {
    let tails: std::collections::BTreeSet<String> = paths
        .iter()
        .map(|p| {
            let keys: Vec<String> =
                p.0.iter()
                    .filter_map(|s| match s {
                        Segment::Key(k) => Some(escape(k, &['/', '+'])),
                        Segment::Index(_) => None,
                    })
                    .collect();
            keys[keys.len().saturating_sub(depth)..].join("/")
        })
        .collect();
    tails.into_iter().collect::<Vec<_>>().join("+")
}

/// `text` with `\` and every char in `special` prefixed by `\`.
fn escape(text: &str, special: &[char]) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\\' || special.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests;
