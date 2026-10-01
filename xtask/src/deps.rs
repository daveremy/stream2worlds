//! Checks 1 and 2: the dependency allowlist (`xtask/allowlist.toml`) and the README stack table.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::Package;

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

#[derive(Deserialize, Default)]
pub(super) struct Allowlist {
    #[serde(default)]
    pub(super) crates: BTreeMap<String, CrateDeps>,
    #[serde(default)]
    pub(super) external: BTreeMap<String, External>,
}

#[derive(Deserialize, Default)]
pub(super) struct CrateDeps {
    #[serde(default)]
    normal: BTreeSet<String>,
    #[serde(default)]
    dev: BTreeSet<String>,
    #[serde(default)]
    build: BTreeSet<String>,
}

#[derive(Deserialize)]
pub(super) struct External {
    pub(super) readme_row: String,
}

/// Compares one crate's declared dependency edges with the allowlist, and checks each edge's
/// identity: internal edges must resolve to the workspace member of that name, external edges
/// must come from crates.io.
pub(super) fn check_edges(
    pkg: &Package,
    allow: &Allowlist,
    members: &BTreeMap<&str, PathBuf>,
    used_external: &mut BTreeSet<String>,
    problems: &mut Vec<String>,
) {
    let Some(listed) = allow.crates.get(&pkg.name) else {
        problems.push(format!(
            "{}: not in xtask/allowlist.toml. Add a [crates.{}] table listing its dependencies.",
            pkg.name, pkg.name
        ));
        return;
    };
    let mut actual: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for dep in &pkg.dependencies {
        actual
            .entry(dep.kind.as_deref().unwrap_or("normal"))
            .or_default()
            .insert(dep.name.clone());
        match (members.get(dep.name.as_str()), &dep.path) {
            (Some(member_dir), Some(path)) if same_dir(member_dir, path) => {}
            (Some(_), _) => problems.push(format!(
                "{}: dependency '{}' shares a workspace crate's name but does not resolve to that crate. Point it at the workspace member.",
                pkg.name, dep.name
            )),
            (None, _) if dep.source.as_deref() == Some(CRATES_IO) => {
                used_external.insert(dep.name.clone());
            }
            (None, _) => problems.push(format!(
                "{}: external dependency '{}' does not come from crates.io (source: {}). Git and path dependencies outside the workspace need a decision record first.",
                pkg.name,
                dep.name,
                dep.source.as_deref().unwrap_or("a path")
            )),
        }
    }
    for (kind, allowed) in [
        ("normal", &listed.normal),
        ("dev", &listed.dev),
        ("build", &listed.build),
    ] {
        let have = actual.remove(kind).unwrap_or_default();
        for extra in have.difference(allowed) {
            problems.push(format!(
                "{}: {kind} dependency '{extra}' is not allowed. If it belongs, add it to [crates.{}].{kind} in xtask/allowlist.toml (and name it in its README stack row if external); if not, remove it. Layer rules: the crate's AGENTS.md.",
                pkg.name, pkg.name
            ));
        }
        for stale in allowed.difference(&have) {
            problems.push(format!(
                "{}: allowlist lists {kind} dependency '{stale}' that the crate does not use. Remove it from xtask/allowlist.toml.",
                pkg.name
            ));
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// The README's Technical architecture table: first-column label → the rest of the row.
pub(super) fn stack_table(readme: &str) -> BTreeMap<String, String> {
    let Some(start) = readme.find("\n## Technical architecture") else {
        return BTreeMap::new();
    };
    let rest = &readme[start + 1..];
    let end = rest
        .get(3..)
        .and_then(|r| r.find("\n## "))
        .map_or(rest.len(), |i| i + 3);
    rest[..end]
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Part ") && !l.starts_with("|---"))
        .filter_map(|l| {
            let mut cells = l.split('|').skip(1);
            let label = cells.next()?.trim().trim_matches('*').to_owned();
            Some((label, cells.collect::<Vec<_>>().join("|")))
        })
        .collect()
}

/// Whether the row names `name` as an exact backticked token, such as `serde`.
pub(super) fn row_has_token(table: &BTreeMap<String, String>, row: &str, name: &str) -> bool {
    table
        .get(row)
        .is_some_and(|cells| cells.contains(&format!("`{name}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const README: &str = "intro\n\n## Technical architecture\n\n| Part | Choice | Status | Why |\n|---|---|---|---|\n| Workspace | `s2w-model` ← `s2w` | building | x |\n| **Serialization and errors** | `serde`, `thiserror` | building | x |\n\n## Next\n| Workspace | `ghost` | | |\n";

    #[test]
    fn stack_table_reads_rows_of_its_own_section_only() {
        let t = stack_table(README);
        assert_eq!(
            t.keys().cloned().collect::<Vec<_>>(),
            vec!["Serialization and errors", "Workspace"]
        );
        assert!(!row_has_token(&t, "Workspace", "ghost"));
    }

    #[test]
    fn tokens_match_exactly_not_by_substring() {
        let t = stack_table(README);
        assert!(row_has_token(&t, "Workspace", "s2w"));
        assert!(row_has_token(&t, "Workspace", "s2w-model"));
        assert!(!row_has_token(&t, "Workspace", "s2w-core"));
        assert!(!row_has_token(&t, "Serialization and errors", "serde_json"));
        assert!(row_has_token(&t, "Serialization and errors", "serde"));
    }
}
