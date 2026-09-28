//! `cargo xtask` — the workspace's fitness functions.
//!
//! `cargo xtask check` runs every check, prints each violation with what to do about it, and
//! fails on enforced violations. Rust source is read only as syn ASTs, never as text.
//!
//! 1. **Dependency allowlist** (`xtask/allowlist.toml`): every dependency edge of every
//!    workspace crate is listed, and nothing listed is unused. Internal edges must resolve to the
//!    workspace member of that name; external ones must come from crates.io.
//! 2. **Stack table:** every external dependency appears by exact name in the README
//!    "Technical architecture" row it names, and every workspace crate appears by exact name in
//!    the Workspace row.
//! 3. **AGENTS.md** in every crate, `xtask` included.
//! 4. **Lint inheritance:** every crate manifest has `[lints] workspace = true`, so the
//!    workspace's forbidden and denied lints apply.
//! 5. **No dependency overrides:** no `[patch]` or `[replace]` in the workspace manifest, and no
//!    `[patch]` or `paths` in `.cargo/config.toml` or the older `.cargo/config`. An override would swap a checked crates.io
//!    dependency for another source without changing its declared identity.
//! 6. **Golden replay** (`golden.rs`): the human-owned golden log folds to the committed snapshot,
//!    byte for byte, twice, and to the same bytes when resumed from a serialized prefix at every
//!    split point. The fixture must use every `WorldEvent` variant and trip the hub cap.
//!
//! 7. **Module sizes** (`module_size.rs`): report-only AST spans and blocking exemption growth.
//! 8. **Version-history append-only** (`version_history.rs`): `VERSION_HISTORY` in
//!    `s2w-system1/src/embedding.rs` may only grow — no row already on `origin/main` may be
//!    edited, reordered, or removed.
//! 9. **Domain vocabulary** (`vocabulary.rs`, terms in `xtask/vocabulary-denylist.txt`): no
//!    crate's `src/` tree — xtask's own included — nor the web view's TypeScript names the
//!    retired domain's terms (decision 0018: no compiled domain code). The denylist is data read
//!    at run time; an entry matches a contiguous run of tokens, so one term catches every
//!    spelling, and test code plus `// vocabulary: allow` are the only exemptions.
//! 10. **Obfuscation replay** (`obfuscation.rs`): the golden fixture folds to the same world
//!     whether or not its claim data (identifiers, attribute names, string values) is renamed
//!     and hashed first — a regression guard on the one layer (`s2w-core`'s fold) known clean
//!     today against code that reads a specific name or value instead of just shape.
//!
//! Escape hatches are not counted here: the compiler forbids `unwrap`, `expect`, `todo!`,
//! `unimplemented!`, `dbg!`, `unsafe` and unreachable `pub`, and no attribute can override a
//! forbid. Other lints may be relaxed locally only with a reason, visible in review.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::Deserialize;

mod golden;
mod module_size;
mod obfuscation;
mod version_history;
mod vocabulary;

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tighten = args == ["check", "--tighten-baseline"];
    if args != ["check"] && !tighten {
        eprintln!("usage: cargo xtask check [--tighten-baseline]");
        return ExitCode::from(2);
    }
    match check(&workspace_root(), tighten) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            for p in &problems {
                eprintln!("✗ {p}");
            }
            eprintln!("xtask: {} problem(s)", problems.len());
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

// ---------- inputs ----------

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    target_directory: PathBuf,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
    targets: Vec<module_size::Target>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
    source: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Deserialize, Default)]
struct Allowlist {
    #[serde(default)]
    crates: BTreeMap<String, CrateDeps>,
    #[serde(default)]
    external: BTreeMap<String, External>,
}

#[derive(Deserialize, Default)]
struct CrateDeps {
    #[serde(default)]
    normal: BTreeSet<String>,
    #[serde(default)]
    dev: BTreeSet<String>,
    #[serde(default)]
    build: BTreeSet<String>,
}

#[derive(Deserialize)]
struct External {
    readme_row: String,
}

#[derive(Deserialize)]
struct Manifest {
    lints: Option<LintsTable>,
}

#[derive(Deserialize)]
struct LintsTable {
    workspace: Option<bool>,
}

fn metadata(root: &Path) -> Result<Metadata, Vec<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| vec![format!("could not run cargo metadata: {e}")])?;
    if !out.status.success() {
        return Err(vec![format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )]);
    }
    serde_json::from_slice(&out.stdout).map_err(|e| vec![format!("unreadable cargo metadata: {e}")])
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Vec<String>> {
    let text = fs::read_to_string(path).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    toml::from_str(&text).map_err(|e| vec![format!("{}: {e}", path.display())])
}

// ---------- the check ----------

fn check(root: &Path, tighten: bool) -> Result<String, Vec<String>> {
    let meta = metadata(root)?;
    let allow: Allowlist = read_toml(&root.join("xtask/allowlist.toml"))?;
    let readme =
        fs::read_to_string(root.join("README.md")).map_err(|e| vec![format!("README.md: {e}")])?;
    let table = stack_table(&readme);

    let members: BTreeMap<&str, PathBuf> = meta
        .packages
        .iter()
        .map(|p| (p.name.as_str(), crate_dir(&p.manifest_path)))
        .collect();
    let mut problems = Vec::new();
    let mut used_external = BTreeSet::new();

    for pkg in &meta.packages {
        let dir = crate_dir(&pkg.manifest_path);
        check_edges(pkg, &allow, &members, &mut used_external, &mut problems);

        if !dir.join("AGENTS.md").is_file() {
            problems.push(format!(
                "{}: missing AGENTS.md. Every crate states its allowed dependencies and invariants there.",
                pkg.name
            ));
        }
        match read_toml::<Manifest>(&pkg.manifest_path) {
            Ok(m) if m.lints.as_ref().and_then(|l| l.workspace) == Some(true) => {}
            Ok(_) => problems.push(format!(
                "{}: Cargo.toml lacks `[lints] workspace = true`, so the workspace's denied lints do not apply to it.",
                pkg.name
            )),
            Err(e) => problems.extend(e),
        }
        if pkg.name != "xtask" && !row_has_token(&table, "Workspace", &pkg.name) {
            problems.push(format!(
                "{}: not named as `{}` in the README Technical architecture Workspace row.",
                pkg.name, pkg.name
            ));
        }
    }
    problems.extend(overrides(root));
    problems.extend(golden::check(root));
    problems.extend(module_size::check(root, &meta, tighten));
    problems.extend(version_history::check(root));
    problems.extend(vocabulary::check(root));
    problems.extend(obfuscation::check(root));
    for listed in allow.crates.keys() {
        if !members.contains_key(listed.as_str()) {
            problems.push(format!(
                "allowlist has [crates.{listed}] but no such workspace crate exists. Remove it."
            ));
        }
    }
    for dep in &used_external {
        match allow.external.get(dep) {
            None => problems.push(format!(
                "external dependency '{dep}' has no [external.{dep}] entry in xtask/allowlist.toml naming its README stack row."
            )),
            Some(ext) if !table.contains_key(&ext.readme_row) => problems.push(format!(
                "external dependency '{dep}' names README row '{}', which is not in the Technical architecture table. Rows: {}.",
                ext.readme_row,
                table.keys().cloned().collect::<Vec<_>>().join(", ")
            )),
            Some(ext) if !row_has_token(&table, &ext.readme_row, dep) => problems.push(format!(
                "external dependency '{dep}' is not named as `{dep}` in the README row '{}'.",
                ext.readme_row
            )),
            Some(_) => {}
        }
    }
    for listed in allow.external.keys() {
        if !used_external.contains(listed) {
            problems.push(format!(
                "allowlist has [external.{listed}] but no crate uses it. Remove it."
            ));
        }
    }

    if problems.is_empty() {
        Ok(format!(
            "✓ dependency allowlist, stack table, AGENTS.md, lint inheritance, no overrides, golden replay, module sizes, domain vocabulary, obfuscation replay: {} crates, {} external dependencies",
            meta.packages.len(),
            used_external.len()
        ))
    } else {
        Err(problems)
    }
}

/// Dependency overrides that would change a dependency's resolved source without changing its
/// declared identity.
fn overrides(root: &Path) -> Vec<String> {
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

fn crate_dir(manifest: &Path) -> PathBuf {
    manifest.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// Compares one crate's declared dependency edges with the allowlist, and checks each edge's
/// identity: internal edges must resolve to the workspace member of that name, external edges
/// must come from crates.io.
fn check_edges(
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
fn stack_table(readme: &str) -> BTreeMap<String, String> {
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
fn row_has_token(table: &BTreeMap<String, String>, row: &str, name: &str) -> bool {
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
