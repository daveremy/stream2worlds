//! Checks 4 and 5: lint inheritance and no dependency overrides.
use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Manifest {
    pub(super) lints: Option<LintsTable>,
}

#[derive(Deserialize)]
pub(super) struct LintsTable {
    pub(super) workspace: Option<bool>,
}

/// Dependency overrides that would change a dependency's resolved source without changing its
/// declared identity.
pub(super) fn overrides(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for (file, banned) in [
        ("Cargo.toml", &["patch", "replace"][..]),
        (".cargo/config.toml", &["patch", "paths"][..]),
        (".cargo/config", &["patch", "paths"][..]),
    ] {
        let path = root.join(file);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        match toml::from_str::<toml::Table>(&text) {
            Ok(table) => {
                for key in banned {
                    if table.contains_key(*key) {
                        problems.push(format!(
                            "{file}: `{key}` overrides dependency sources, which the allowlist cannot see. Remove it; a genuine need gets a decision record and a check first."
                        ));
                    }
                }
            }
            Err(e) => problems.push(format!("{file}: {e}")),
        }
    }
    problems
}
