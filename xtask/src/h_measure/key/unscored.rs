//! A key's `unscored` entries (contract B3): exact paths in every format, and from format 2 a
//! prefix form that covers a path and everything under it (s2w#224), so a path that occurs only
//! in a corpus the key's author never read is still unscored.

use std::collections::{BTreeMap, BTreeSet};

use s2w_discover::rule_id;
use s2w_model::FieldPath;
use serde::Deserialize;

use super::well_formed;

/// One `unscored` entry.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum UnscoredPath {
    /// A JSON array: exactly this path (every format).
    Exact(FieldPath),
    /// `{"prefix": [...]}` (format 2): this path and every path under it, including paths the
    /// key's author never saw.
    Prefix(UnscoredPrefix),
}

/// The body of [`UnscoredPath::Prefix`]. A struct of its own because `deny_unknown_fields` has
/// no effect on a variant of an `untagged` enum, only on a struct.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnscoredPrefix {
    /// The covered path. Every path that starts with its segments is covered too.
    pub prefix: FieldPath,
}

impl UnscoredPath {
    fn path(&self) -> &FieldPath {
        match self {
            Self::Exact(path) | Self::Prefix(UnscoredPrefix { prefix: path }) => path,
        }
    }
}

/// A key's unscored entries as mention path ids ([`rule_id`]), the form the scorer drops
/// predicted mentions by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Unscored {
    exact: BTreeSet<String>,
    prefixes: BTreeSet<String>,
}

impl Unscored {
    /// Exactly these mention path ids, with no prefix.
    #[cfg(test)]
    pub(crate) fn exact(ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            exact: ids.into_iter().collect(),
            prefixes: BTreeSet::new(),
        }
    }

    /// The ids of `entries`, unchecked.
    pub(super) fn of(entries: &[UnscoredPath]) -> Self {
        let mut unscored = Self::default();
        for entry in entries {
            let id = rule_id(entry.path());
            match entry {
                UnscoredPath::Exact(_) => unscored.exact.insert(id),
                UnscoredPath::Prefix(_) => unscored.prefixes.insert(id),
            };
        }
        unscored
    }

    /// The ids of `entries` in a spec of format `version`, failing closed: every entry well
    /// formed and listed once, a prefix only from format 2, and no entry under another entry's
    /// prefix (it would be listed twice). Compared as the executors' mention ids, so `["a", 1]`
    /// and `["a", "1"]` are one path.
    pub(super) fn validated(entries: &[UnscoredPath], version: u32) -> Result<Self, String> {
        let mut unscored = Self::default();
        let mut seen: BTreeMap<String, &FieldPath> = BTreeMap::new();
        for entry in entries {
            let path = entry.path();
            if !well_formed(path) {
                return Err("an unscored path is empty or has an empty or U+001F key".to_owned());
            }
            let is_prefix = matches!(entry, UnscoredPath::Prefix(_));
            if is_prefix && version < 2 {
                return Err(format!(
                    "unscored prefix {path:?} needs key format 2: format {version} matches unscored paths exactly"
                ));
            }
            let id = rule_id(path);
            if seen.contains_key(&id) {
                return Err(format!("unscored path {path:?} is listed twice"));
            }
            // No id repeats, so a match below is never the entry itself.
            let nested = match unscored.prefixes.iter().find(|prefix| under(&id, prefix)) {
                Some(prefix) => Some((path, prefix.clone())),
                None if is_prefix => seen
                    .iter()
                    .find(|(inner, _)| under(inner, &id))
                    .map(|(_, inner)| (*inner, id.clone())),
                None => None,
            };
            if let Some((inner, prefix)) = nested {
                return Err(format!(
                    "unscored path {inner:?} is under unscored prefix {prefix:?}: drop the entry, the prefix covers it"
                ));
            }
            if is_prefix {
                unscored.prefixes.insert(id.clone());
            } else {
                unscored.exact.insert(id.clone());
            }
            seen.insert(id, path);
        }
        Ok(unscored)
    }

    /// Whether mention path id `id` is unscored: listed exactly, or at or under a prefix.
    pub(crate) fn covers(&self, id: &str) -> bool {
        self.exact.contains(id) || self.prefixes.iter().any(|prefix| under(id, prefix))
    }
}

/// Whether path id `id` is `prefix` or a path under it. An id joins its segments with `.`, and
/// [`rule_id`] escapes every `.` and `\` inside a key, so an unescaped `.` right after the whole
/// prefix is a segment boundary: `a.b.c` is under `a.b`, while the keys `b.c` (id `a.b\.c`) and
/// `bc` (id `a.bc`) are not.
fn under(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

#[cfg(test)]
mod tests;
