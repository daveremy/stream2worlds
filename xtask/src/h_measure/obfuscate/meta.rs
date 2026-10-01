//! The replicate's private metadata file (contract B2.2): everything needed to audit the
//! transformation except the key itself. It names the domains, so it is published only after
//! every mapping it could inform is committed.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::rules::Unobservable;
use super::transform::{FieldTable, Stats};

/// The metadata format this build writes and reads.
pub(super) const META_FORMAT: u32 = 1;

/// One replicate's metadata.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Meta {
    pub format: u32,
    pub replicate: String,
    /// sha256 of the key bytes: names the key, never reveals it.
    pub key_sha256: String,
    pub rules_sha256: String,
    pub shift_seconds: i64,
    /// Every field path and its output name.
    pub fields: Vec<FieldRow>,
    /// Every leaf path and how its values were transformed (its identifier domain, or why not).
    pub treatments: Vec<Treatment>,
    /// Paths holding numbers no rule declared, kept unchanged: check none is an identifier.
    pub undeclared_numbers: Vec<Vec<String>>,
    /// Values hashed whole as text because their URL rule did not match, per path.
    pub fallbacks: Vec<PathCount>,
    /// Values hashed as their own because the record lacked their rule's `from` path
    /// (`own_if_absent`), per path.
    pub own_values: Vec<PathCount>,
    /// Rules whose path no input holds.
    pub unused_rules: Vec<Vec<String>>,
    /// Relationships and paths the obfuscated stream cannot show.
    pub unobservable: Vec<Unobservable>,
    /// Input corpus name to its pinned sha256.
    pub inputs: BTreeMap<String, String>,
    /// Input corpus name to the obfuscated corpus written for it.
    pub outputs: BTreeMap<String, Written>,
    /// Plain answer-key file to the renamed key written for it.
    pub keys: BTreeMap<String, Written>,
}

/// One field-table row.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FieldRow {
    pub path: Vec<String>,
    pub name: String,
}

/// One leaf path's transformations.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Treatment {
    pub path: Vec<String>,
    pub how: Vec<String>,
}

/// One path's count: URL fallbacks, or own values hashed for an absent `from` path.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PathCount {
    pub path: Vec<String>,
    pub count: usize,
}

/// A file the run wrote.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Written {
    pub file: String,
    /// SSE frames, for a corpus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<usize>,
    pub sha256: String,
}

impl Meta {
    /// Folds a later run of the same replicate (same key, rules and field table, checked by the
    /// caller) into this record, so the metadata lists every window obfuscated under it
    /// (contract B2.2). A corpus or answer key the record already holds is refused: each window
    /// is obfuscated once per replicate. Per-path statistics are combined: treatments and
    /// undeclared numbers are united, fallback and own-value counts summed, and a rule stays unused only if
    /// no run's input held its path.
    pub(super) fn absorb(&mut self, run: Meta) -> Result<(), String> {
        let again: Vec<&String> = (run.outputs.keys())
            .filter(|c| self.outputs.contains_key(*c))
            .chain(run.keys.keys().filter(|k| self.keys.contains_key(*k)))
            .collect();
        if !again.is_empty() {
            return Err(format!(
                "the metadata already records {again:?}; a window is obfuscated once per replicate"
            ));
        }
        self.inputs.extend(run.inputs);
        self.outputs.extend(run.outputs);
        self.keys.extend(run.keys);
        let mut how: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();
        for t in self.treatments.drain(..).chain(run.treatments) {
            how.entry(t.path).or_default().extend(t.how);
        }
        self.treatments = (how.into_iter())
            .map(|(path, how)| Treatment {
                path,
                how: how.into_iter().collect(),
            })
            .collect();
        self.fallbacks = summed(self.fallbacks.drain(..).chain(run.fallbacks));
        self.own_values = summed(self.own_values.drain(..).chain(run.own_values));
        let numbers: BTreeSet<Vec<String>> = (self
            .undeclared_numbers
            .drain(..)
            .chain(run.undeclared_numbers))
        .collect();
        self.undeclared_numbers = numbers.into_iter().collect();
        self.unused_rules.retain(|r| run.unused_rules.contains(r));
        Ok(())
    }

    /// The field table this metadata records; a path or a name listed twice is refused.
    pub(super) fn table(&self) -> Result<FieldTable, String> {
        let mut table = BTreeMap::new();
        let mut names = BTreeSet::new();
        for row in &self.fields {
            if !names.insert(&row.name)
                || table.insert(row.path.clone(), row.name.clone()).is_some()
            {
                return Err(format!(
                    "the metadata's field table lists {:?} or {:?} twice",
                    row.path, row.name
                ));
            }
        }
        Ok(FieldTable(table))
    }
}

/// Per-path counts with each path's counts summed, in path order.
fn summed(counts: impl Iterator<Item = PathCount>) -> Vec<PathCount> {
    let mut sums: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    for c in counts {
        *sums.entry(c.path).or_default() += c.count;
    }
    counted(&sums)
}

/// Per-path counts as metadata rows, in path order.
pub(super) fn counted(counts: &BTreeMap<Vec<String>, usize>) -> Vec<PathCount> {
    counts
        .iter()
        .map(|(path, count)| PathCount {
            path: path.clone(),
            count: *count,
        })
        .collect()
}

/// The field table and the per-path treatments, from the field table and the run's statistics.
pub(super) fn rows(table: &FieldTable, stats: &Stats) -> (Vec<FieldRow>, Vec<Treatment>) {
    let fields = table
        .0
        .iter()
        .map(|(path, name)| FieldRow {
            path: path.clone(),
            name: name.clone(),
        })
        .collect();
    let treatments = stats
        .treatments
        .iter()
        .map(|(path, how)| Treatment {
            path: path.clone(),
            how: how.iter().cloned().collect(),
        })
        .collect();
    (fields, treatments)
}
