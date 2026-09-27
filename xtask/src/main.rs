//! `cargo xtask` — the workspace's fitness functions.
//!
//! `cargo xtask check` runs every check and fails on the first class of violation it finds,
//! printing each violation with what to do about it. The checks:
//!
//! 1. **Dependency allowlist** (`xtask/allowlist.toml`): every dependency edge of every
//!    workspace crate is listed, and nothing listed is unused. An allowlist, not a denylist.
//! 2. **Stack table**: every external dependency names the README "Technical architecture" row
//!    that explains it, that row exists, and every workspace crate is named in that section.
//! 3. **AGENTS.md**: every crate has one.
//! 4. **Escape-hatch ratchet** (`xtask/ratchet.toml`): per-crate counts of `allow` attributes,
//!    `unwrap`/`expect`, `todo!`/`unimplemented!` and `pub` items may only fall. Raising one needs
//!    a decision record: `cargo xtask ratchet --raise docs/decisions/NNNN-*.md`.
//!
//! Replay determinism (the third fitness function in the design) arrives with the fold.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::{Deserialize, Serialize};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["check"] => check(&root),
        ["ratchet", "--tighten"] => ratchet_write(&root, None),
        ["ratchet", "--raise", record] => ratchet_write(&root, Some(record)),
        _ => Err(vec![
            "usage: cargo xtask check | ratchet --tighten | ratchet --raise docs/decisions/NNNN-<slug>.md"
                .to_owned(),
        ]),
    };
    match result {
        Ok(notes) => {
            for n in notes {
                println!("{n}");
            }
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

type Outcome = Result<Vec<String>, Vec<String>>;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

// ---------- cargo metadata ----------

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
    path: Option<PathBuf>,
}

fn metadata(root: &Path) -> Result<Metadata, Vec<String>> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
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

// ---------- allowlist ----------

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

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Vec<String>> {
    let text = fs::read_to_string(path).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    toml::from_str(&text).map_err(|e| vec![format!("{}: {e}", path.display())])
}

fn check(root: &Path) -> Outcome {
    let meta = metadata(root)?;
    let allow: Allowlist = read_toml(&root.join("xtask/allowlist.toml"))?;
    let readme =
        fs::read_to_string(root.join("README.md")).map_err(|e| vec![format!("README.md: {e}")])?;
    let stack = stack_section(&readme);
    let rows = stack_rows(&stack);

    let mut problems = Vec::new();
    let members: BTreeSet<&str> = meta.packages.iter().map(|p| p.name.as_str()).collect();
    let mut used_external = BTreeSet::new();

    for pkg in &meta.packages {
        let Some(listed) = allow.crates.get(&pkg.name) else {
            problems.push(format!(
                "{}: not in xtask/allowlist.toml. Add a [crates.{}] table listing its dependencies.",
                pkg.name, pkg.name
            ));
            continue;
        };
        let mut actual: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for dep in &pkg.dependencies {
            let kind = dep.kind.as_deref().unwrap_or("normal");
            actual.entry(kind).or_default().insert(dep.name.clone());
            if dep.path.is_none() {
                used_external.insert(dep.name.clone());
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
                    "{}: {kind} dependency '{extra}' is not allowed. If it belongs, add it to [crates.{}].{kind} in xtask/allowlist.toml (and a README stack row if external); if not, remove it. Layer rules: crate AGENTS.md.",
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
        let crate_dir = pkg.manifest_path.parent().unwrap_or(root);
        if !crate_dir.join("AGENTS.md").is_file() && pkg.name != "xtask" {
            problems.push(format!(
                "{}: missing AGENTS.md. Every crate states its allowed dependencies and invariants there.",
                pkg.name
            ));
        }
        if pkg.name != "xtask" && !stack.contains(pkg.name.as_str()) {
            problems.push(format!(
                "{}: not named in the README's Technical architecture section. Add it to the Workspace row.",
                pkg.name
            ));
        }
    }
    for listed in allow.crates.keys() {
        if !members.contains(listed.as_str()) {
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
            Some(ext) if !rows.contains(&ext.readme_row) => problems.push(format!(
                "external dependency '{dep}' names README row '{}', which is not in the Technical architecture table. Rows: {}.",
                ext.readme_row,
                rows.iter().cloned().collect::<Vec<_>>().join(", ")
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

    let (ratchet_problems, ratchet_notes) = ratchet_check(root, &meta)?;
    problems.extend(ratchet_problems);

    if problems.is_empty() {
        let mut notes = vec![format!(
            "✓ dependency allowlist, stack table, AGENTS.md, ratchet: {} crates, {} external dependencies",
            meta.packages.len(),
            used_external.len()
        )];
        notes.extend(ratchet_notes);
        Ok(notes)
    } else {
        Err(problems)
    }
}

/// The README's "Technical architecture" section, up to the next level-2 heading.
fn stack_section(readme: &str) -> String {
    let Some(start) = readme.find("\n## Technical architecture") else {
        return String::new();
    };
    let rest = &readme[start + 1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |i| i + 3);
    rest[..end].to_owned()
}

/// First-column labels of the stack table, with Markdown emphasis removed.
fn stack_rows(section: &str) -> BTreeSet<String> {
    section
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Part") && !l.starts_with("|---"))
        .filter_map(|l| l.split('|').nth(1))
        .map(|c| c.trim().trim_matches('*').to_owned())
        .collect()
}

// ---------- ratchet ----------

#[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    allow: u32,
    unwrap: u32,
    todo: u32,
    pub_items: u32,
}

#[derive(Serialize, Deserialize, Default)]
struct Ratchet {
    #[serde(default)]
    crates: BTreeMap<String, Counts>,
}

/// Counts escape hatches in a crate's `src/`, ignoring everything after a `#[cfg(test)]` line:
/// test modules sit at the end of a file by convention, and tests may unwrap.
fn count(dir: &Path) -> Result<Counts, Vec<String>> {
    let mut c = Counts::default();
    let mut stack = vec![dir.join("src")];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                let text =
                    fs::read_to_string(&p).map_err(|e| vec![format!("{}: {e}", p.display())])?;
                for line in text.lines() {
                    let t = line.trim_start();
                    if t.starts_with("#[cfg(test)]") {
                        break;
                    }
                    if t.starts_with("//") {
                        continue;
                    }
                    let n = |pat: &str| u32::try_from(t.matches(pat).count()).unwrap_or(u32::MAX);
                    c.allow += n("#[allow(") + n("#![allow(") + n("#[expect(") + n("#![expect(");
                    c.unwrap += n(".unwrap()") + n(".expect(");
                    c.todo += n("todo!(") + n("unimplemented!(");
                    if t.starts_with("pub ")
                        && !t.starts_with("pub use ")
                        && !t.starts_with("pub mod ")
                    {
                        c.pub_items += 1;
                    }
                }
            }
        }
    }
    Ok(c)
}

fn current_counts(meta: &Metadata) -> Result<BTreeMap<String, Counts>, Vec<String>> {
    let mut out = BTreeMap::new();
    for pkg in &meta.packages {
        if pkg.name == "xtask" {
            continue;
        }
        let dir = pkg
            .manifest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        out.insert(pkg.name.clone(), count(&dir)?);
    }
    Ok(out)
}

fn ratchet_check(root: &Path, meta: &Metadata) -> Result<(Vec<String>, Vec<String>), Vec<String>> {
    let recorded: Ratchet = read_toml(&root.join("xtask/ratchet.toml"))?;
    let now = current_counts(meta)?;
    let mut problems = Vec::new();
    let mut notes = Vec::new();
    for (name, c) in &now {
        let r = recorded.crates.get(name).copied().unwrap_or_default();
        for (metric, have, cap) in [
            ("allow attributes", c.allow, r.allow),
            ("unwrap/expect", c.unwrap, r.unwrap),
            ("todo!/unimplemented!", c.todo, r.todo),
            ("pub items", c.pub_items, r.pub_items),
        ] {
            if have > cap {
                problems.push(format!(
                    "{name}: {metric} rose from {cap} to {have}. Remove them, or write a decision record and run `cargo xtask ratchet --raise docs/decisions/NNNN-<slug>.md`."
                ));
            } else if have < cap {
                notes.push(format!("ratchet can tighten: {name} {metric} {cap} → {have}. Run `cargo xtask ratchet --tighten`."));
            }
        }
    }
    Ok((problems, notes))
}

fn ratchet_write(root: &Path, record: Option<&str>) -> Outcome {
    let meta = metadata(root)?;
    let now = current_counts(&meta)?;
    let path = root.join("xtask/ratchet.toml");
    let recorded: Ratchet = read_toml(&path).unwrap_or_default();
    let raised: Vec<&String> = now
        .iter()
        .filter(|(n, c)| {
            let r = recorded.crates.get(*n).copied().unwrap_or_default();
            c.allow > r.allow || c.unwrap > r.unwrap || c.todo > r.todo || c.pub_items > r.pub_items
        })
        .map(|(n, _)| n)
        .collect();
    match record {
        None if !raised.is_empty() => {
            return Err(vec![format!(
                "counts rose for {raised:?}; --tighten only lowers. Use --raise with a decision record."
            )]);
        }
        Some(r) if !root.join(r).is_file() => {
            return Err(vec![format!(
                "decision record {r} does not exist. Write it first."
            )]);
        }
        _ => {}
    }
    let header = match record {
        Some(r) => format!("# Last raised under {r}.\n"),
        None => String::new(),
    };
    let body = toml::to_string(&Ratchet { crates: now })
        .map_err(|e| vec![format!("serialize ratchet: {e}")])?;
    fs::write(
        &path,
        format!(
            "# Escape-hatch counts per crate. May only fall; see xtask/src/main.rs.\n{header}{body}"
        ),
    )
    .map_err(|e| vec![format!("{}: {e}", path.display())])?;
    Ok(vec![format!("wrote {}", path.display())])
}
