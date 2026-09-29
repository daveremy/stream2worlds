//! Check 8: clippy configuration consistency. Reads TOML only.
//!
//! A `clippy.toml` (or `.clippy.toml`) in a crate's directory REPLACES the workspace one rather
//! than merging with it, so a function-size threshold set only in the root file silently does
//! not apply to a crate that has its own file. This check finds each workspace member's
//! effective config the way clippy does (the nearest `clippy.toml` or `.clippy.toml` walking up
//! from the manifest directory to the workspace root) and requires every threshold key of the
//! root config with an equal value. `CLIPPY_CONF_DIR` overrides that lookup entirely, so a set
//! value fails the check.

use std::fs;
use std::path::{Path, PathBuf};

/// The size and shape thresholds every effective config must carry, equal to the root's.
const THRESHOLD_KEYS: [&str; 3] = [
    "too-many-lines-threshold",
    "cognitive-complexity-threshold",
    "too-many-arguments-threshold",
];

const CONFIG_NAMES: [&str; 2] = ["clippy.toml", ".clippy.toml"];

pub(super) fn check(root: &Path, meta: &super::Metadata) -> Vec<String> {
    let mut problems = Vec::new();
    if std::env::var_os("CLIPPY_CONF_DIR").is_some() {
        problems.push(
            "CLIPPY_CONF_DIR is set, which replaces clippy's config lookup for every crate and hides the files this check reads. Unset it.".to_owned(),
        );
    }
    problems.extend(cargo_env_override(root));
    let root_table = match load(&root.join("clippy.toml")) {
        Ok(Some(table)) => table,
        Ok(None) => {
            problems.push("clippy.toml: missing at the workspace root. It holds the size thresholds every crate's config must repeat.".to_owned());
            return problems;
        }
        Err(e) => {
            problems.push(e);
            return problems;
        }
    };
    for key in THRESHOLD_KEYS {
        if !root_table.contains_key(key) {
            problems.push(format!(
                "clippy.toml is missing `{key}`. The root file is the reference for the size thresholds; add it."
            ));
        }
    }
    for pkg in &meta.packages {
        let dir = super::crate_dir(&pkg.manifest_path);
        problems.extend(check_member(root, &dir, &pkg.name, &root_table));
    }
    problems
}

/// `[env] CLIPPY_CONF_DIR` in cargo's config reaches every rustc invocation, so it repoints
/// clippy's config lookup without touching the process environment the check above reads.
fn cargo_env_override(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for file in [".cargo/config.toml", ".cargo/config"] {
        let Ok(text) = fs::read_to_string(root.join(file)) else {
            continue;
        };
        let Ok(table) = toml::from_str::<toml::Table>(&text) else {
            continue; // an unreadable config is reported by the override check
        };
        if table
            .get("env")
            .and_then(toml::Value::as_table)
            .is_some_and(|env| env.contains_key("CLIPPY_CONF_DIR"))
        {
            problems.push(format!(
                "{file}: [env] sets CLIPPY_CONF_DIR, which replaces clippy's config lookup for every crate and hides the files this check reads. Remove it."
            ));
        }
    }
    problems
}

fn check_member(root: &Path, dir: &Path, name: &str, reference: &toml::Table) -> Vec<String> {
    let found = match effective_config(root, dir) {
        Ok(found) => found,
        Err(problems) => return problems,
    };
    let Some((path, table)) = found else {
        return vec![format!(
            "{name}: no clippy config found between {} and the workspace root.",
            dir.display()
        )];
    };
    let shown = path
        .strip_prefix(root)
        .unwrap_or(&path)
        .display()
        .to_string();
    let mut problems = Vec::new();
    for key in THRESHOLD_KEYS {
        let Some(want) = reference.get(key) else {
            continue;
        };
        match table.get(key) {
            None => problems.push(format!(
                "{shown} is missing `{key} = {want}`; a per-crate clippy config replaces the root file, so copy the key."
            )),
            Some(have) if have != want => problems.push(format!(
                "{shown} sets `{key} = {have}` but the root clippy.toml sets {want}. Make them equal."
            )),
            Some(_) => {}
        }
    }
    problems
}

/// The config clippy would read for the crate at `dir`: the nearest directory, walking up to
/// `root`, that holds `clippy.toml` or `.clippy.toml`. Both in one directory is an error, as
/// clippy would ignore one of them.
fn effective_config(
    root: &Path,
    dir: &Path,
) -> Result<Option<(PathBuf, toml::Table)>, Vec<String>> {
    let mut current = Some(dir);
    while let Some(d) = current {
        let present: Vec<PathBuf> = CONFIG_NAMES
            .iter()
            .map(|n| d.join(n))
            .filter(|p| p.is_file())
            .collect();
        match present.as_slice() {
            [] => {}
            [one] => {
                return load(one)
                    .map(|t| t.map(|t| (one.clone(), t)))
                    .map_err(|e| vec![e]);
            }
            _ => {
                return Err(vec![format!(
                    "{} holds both clippy.toml and .clippy.toml. Keep one; clippy ignores the other.",
                    d.display()
                )]);
            }
        }
        current = if d == root { None } else { d.parent() };
    }
    Ok(None)
}

fn load(path: &Path) -> Result<Option<toml::Table>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "s2w-clippy-config-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(path.join("crates/demo")).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, text: &str) {
            fs::write(self.0.join(name), text).unwrap();
        }

        fn member(&self) -> Vec<String> {
            let root = load(&self.0.join("clippy.toml")).unwrap().unwrap();
            check_member(&self.0, &self.0.join("crates/demo"), "demo", &root)
        }
    }

    const ROOT: &str = "too-many-lines-threshold = 60\ncognitive-complexity-threshold = 15\ntoo-many-arguments-threshold = 5\n";

    #[test]
    fn a_member_inheriting_the_root_file_passes() {
        let s = Scratch::new();
        s.write("clippy.toml", ROOT);
        assert!(s.member().is_empty());
    }

    #[test]
    fn a_per_crate_file_missing_a_key_fails_and_says_to_copy_it() {
        let s = Scratch::new();
        s.write("clippy.toml", ROOT);
        s.write("crates/demo/clippy.toml", "too-many-lines-threshold = 60\n");
        let problems = s.member();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].contains(
                "crates/demo/clippy.toml is missing `cognitive-complexity-threshold = 15`"
            )
        );
        assert!(problems[0].contains("copy the key"));
    }

    #[test]
    fn a_divergent_value_fails() {
        let s = Scratch::new();
        s.write("clippy.toml", ROOT);
        s.write("crates/demo/clippy.toml", &ROOT.replace("= 60", "= 100"));
        let problems = s.member();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0]
                .contains("sets `too-many-lines-threshold = 100` but the root clippy.toml sets 60")
        );
    }

    #[test]
    fn a_hidden_dot_file_is_the_effective_config() {
        let s = Scratch::new();
        s.write("clippy.toml", ROOT);
        s.write("crates/demo/.clippy.toml", "allow-unwrap-in-tests = true\n");
        let problems = s.member();
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].contains(".clippy.toml is missing"));
    }

    #[test]
    fn both_file_names_in_one_directory_fail() {
        let s = Scratch::new();
        s.write("clippy.toml", ROOT);
        s.write("crates/demo/clippy.toml", ROOT);
        s.write("crates/demo/.clippy.toml", ROOT);
        let problems = s.member();
        assert!(problems[0].contains("holds both"), "{problems:?}");
    }

    #[test]
    fn a_cargo_env_entry_for_the_config_dir_fails() {
        let s = Scratch::new();
        fs::create_dir_all(s.0.join(".cargo")).unwrap();
        s.write(".cargo/config.toml", "[env]\nCLIPPY_CONF_DIR = \"/tmp\"\n");
        let problems = cargo_env_override(&s.0);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("[env] sets CLIPPY_CONF_DIR"));
        s.write(".cargo/config.toml", "[env]\nOTHER = \"1\"\n");
        assert!(cargo_env_override(&s.0).is_empty());
    }
}
