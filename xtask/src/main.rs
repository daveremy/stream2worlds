//! `cargo xtask` — the workspace's fitness functions.
//!
//! `cargo xtask check` runs every check, prints each violation with what to do about it, and
//! fails on enforced violations. Rust source is read only as syn ASTs, never as text.
//!
//! 1. **Dependency allowlist** (`deps.rs`, `xtask/allowlist.toml`): every dependency edge of every
//!    workspace crate is listed, and nothing listed is unused. Internal edges must resolve to the
//!    workspace member of that name; external ones must come from crates.io.
//! 2. **Stack table** (`deps.rs`): every external dependency appears by exact name in the README
//!    "Technical architecture" row it names, and every workspace crate appears by exact name in
//!    the Workspace row.
//! 3. **AGENTS.md** in every crate, `xtask` included.
//! 4. **Lint inheritance** (`lints.rs`): every crate manifest has `[lints] workspace = true`, so the
//!    workspace's forbidden and denied lints apply.
//! 5. **No dependency overrides** (`lints.rs`): no `[patch]` or `[replace]` in the workspace manifest, and no
//!    `[patch]` or `paths` in `.cargo/config.toml` or the older `.cargo/config`. An override would swap a checked crates.io
//!    dependency for another source without changing its declared identity.
//! 6. **Golden replay** (`golden.rs`): the human-owned golden log folds to the committed snapshot,
//!    byte for byte, twice, and to the same bytes when resumed from a serialized prefix at every
//!    split point. The fixture must use every `WorldEvent` variant and trip the hub cap.
//!
//! 7. **Module sizes** (`module_size.rs`): enforced AST spans (cap 400, reasoned `[[exempt]]`
//!    rows) and blocking exemption growth.
//! 9. **Domain vocabulary** (`vocabulary.rs`, terms in `xtask/vocabulary-denylist.txt`): no
//!    crate's `src/` tree — xtask's own included — nor the web view's TypeScript, nor any
//!    committed prompt file under a crate's `prompts/` tree, names the retired domain's terms
//!    (decision 0018: no compiled domain code). The denylist is data read at run time; an entry
//!    matches a contiguous run of tokens, so one term catches every spelling, and test code
//!    plus `// vocabulary: allow` are the only exemptions (the opt-out never reaches prompts).
//! 10. **Obfuscation replay** (`obfuscation.rs`): the golden fixture folds to the same world
//!     whether or not its claim data (identifiers, attribute names, string values) is renamed
//!     and hashed first — a regression guard against code that reads a specific name or value
//!     instead of just shape. Covers `s2w-core`'s fold and, run through `s2w-system1`'s
//!     engines (`JsonClaimsEngine` today), the engine layer; the bridge registry
//!     (`s2w-app::Bridge`/`EngineRegistry`) is not yet covered.
//! 8. **Clippy config consistency** (`clippy_config.rs`, reads TOML only): a per-crate
//!    `clippy.toml` or `.clippy.toml` replaces the root file, so every workspace member's effective
//!    config must carry the root's `too-many-lines`, `cognitive-complexity` and `too-many-arguments`
//!    thresholds with equal values, and `CLIPPY_CONF_DIR` must be unset (process env and cargo `[env]`).
//! 11. **Raw obfuscation replay** (`obfuscation_raw.rs`): `s2w-system1`'s `MappingEngine`,
//!     run over a recorded raw stream and a mapping, builds the same world when every object
//!     key and string value in both (including inside decoded JSON strings) is renamed and
//!     hashed first. Check 10 covers engines that read claims; this one covers the engine that
//!     reads raw payloads through a mapping (decision 0021).
//! 12. **Profiler obfuscation replay** (`discover_replay.rs`): `s2w-discover` proposes the
//!     same mapping for a recorded raw stream when every object key and string value in it
//!     (including inside decoded JSON strings) is renamed and hashed first, up to that renaming
//!     (decision 0022).
//! 13. **Scale memory** (`scale_mem_check.rs`, baseline and judges in `scale.rs`): runs
//!     `s2w-app`'s two ignored `scale_mem` tests, one per event supply (seeded generator,
//!     recorded fixture), as nested `cargo test`s and gates heap bytes per entity against
//!     `[memory]` and `[memory.recorded]` in `xtask/scale-baseline.toml` (+tolerance and a
//!     shared hard budget), plus that file's baseline-growth trailer rule. `cargo xtask scale`
//!     (`scale_run.rs`) gates fold instructions per event for both supplies under Valgrind
//!     (decision 0004, s2w#174).
//! 14. **Decision numbers** (`decision_numbers.rs`): no two files in `docs/decisions/` share a
//!     numeric prefix; the failure names every file holding the number (s2w#181).
//! 15. **Module cycles** (`module_cycles.rs`): no dependency cycle between the modules of one
//!     crate target. Edges run from the naming module to the module that defines the item,
//!     through `use`/`pub use` re-exports and globs; ancestor edges are containment (s2w#67).
//!     Enforced since s2w#240 and s2w#241.
//! 16. **Frozen contract** (`contract_frozen.rs`): everything above `## Dated notes after
//!     sign-off` in `docs/evaluation-contract.md` matches the sha256 of the signed text pinned
//!     in the source; changes go in dated notes below that heading (s2w#59).
//! 17. **README Scale row** (`readme_scale.rs`): every figure in the Technical architecture
//!     Scale row (Ir per event, bytes per entity and their ratios, bytes per relationship, parse
//!     Ir, target and ceiling) equals `xtask/scale-baseline.toml` (s2w#324).
//! 18. **`#[expect]` count** (`expect_count.rs`, baseline `xtask/expect-baseline.toml`): every
//!     `#[expect]` attribute in every workspace package's `*.rs` files, tests and benches included,
//!     counted per lint as syn attributes. Shrink-only: a count above its baseline fails, a count
//!     below it fails until `--tighten-baseline` lowers the file, and raising the file needs a
//!     `Baseline-growth: s2w#<N>` trailer (s2w#156).
//! 19. **Public API** (`public_api.rs`, snapshots in `xtask/public-api/`): every lib crate's
//!     `pub` items match its committed snapshot; `cargo xtask api --update` rewrites them (s2w#68).
//! 20. **No private capture** (`private_capture.rs`): no `*.sse` or `*.provenance.jsonl` under
//!     `research/` except the synthetic fixture (which must carry the synthetic header), and no
//!     file anywhere in the working tree with a line starting with the private capture header
//!     (s2w#371).
//!
//! Escape hatches: the compiler forbids `unwrap`, `expect`, `todo!`,
//! `unimplemented!`, `dbg!`, `unsafe` and unreachable `pub`, and no attribute can override a
//! forbid. Other lints may be relaxed locally only with a reasoned `#[expect]` (the workspace
//! denies `#[allow]`), visible in review and counted by check 18.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::Deserialize;
use sha2::{Digest, Sha256};

mod clippy_config;
mod contract_frozen;
mod decision_numbers;
mod deps;
mod discover_replay;
mod expect_count;
mod golden;
mod h_measure;
mod lints;
mod module_cycles;
mod module_size;
mod obfuscation;
mod obfuscation_raw;
mod private_capture;
mod public_api;
mod readme_scale;
mod scale;
mod scale_mem_check;
mod scale_run;
mod vocabulary;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["scale"] {
        return scale_run::run(&workspace_root());
    }
    if args.first().is_some_and(|a| a == "h-measure") {
        return h_measure::run(&workspace_root(), &args[1..]);
    }
    if args.first().is_some_and(|a| a == "api") {
        return public_api::run(&workspace_root(), &args[1..]);
    }
    let tighten = args == ["check", "--tighten-baseline"];
    if args != ["check"] && !tighten {
        eprintln!(
            "usage: cargo xtask check [--tighten-baseline] | cargo xtask api [--update] | cargo xtask scale | cargo xtask h-measure selftest|freeze|score"
        );
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

/// Lower-case hex sha256 of `bytes`.
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
    rename: Option<String>,
    source: Option<String>,
    path: Option<PathBuf>,
}

/// The `cargo` that is running xtask (`$CARGO`), or `cargo` from PATH.
fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn metadata(root: &Path) -> Result<Metadata, Vec<String>> {
    let out = cargo()
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

#[expect(
    clippy::too_many_lines,
    reason = "the check orchestrator runs every check in sequence; each check lives in its own module"
)]
fn check(root: &Path, tighten: bool) -> Result<String, Vec<String>> {
    let meta = metadata(root)?;
    let allow: deps::Allowlist = read_toml(&root.join("xtask/allowlist.toml"))?;
    let readme =
        fs::read_to_string(root.join("README.md")).map_err(|e| vec![format!("README.md: {e}")])?;
    let table = deps::stack_table(&readme);

    let members: BTreeMap<&str, PathBuf> = meta
        .packages
        .iter()
        .map(|p| (p.name.as_str(), crate_dir(&p.manifest_path)))
        .collect();
    let mut problems = Vec::new();
    let mut used_external = BTreeSet::new();

    for pkg in &meta.packages {
        let dir = crate_dir(&pkg.manifest_path);
        deps::check_edges(pkg, &allow, &members, &mut used_external, &mut problems);

        if !dir.join("AGENTS.md").is_file() {
            problems.push(format!(
                "{}: missing AGENTS.md. Every crate states its allowed dependencies and invariants there.",
                pkg.name
            ));
        }
        match read_toml::<lints::Manifest>(&pkg.manifest_path) {
            Ok(m) if m.lints.as_ref().and_then(|l| l.workspace) == Some(true) => {}
            Ok(_) => problems.push(format!(
                "{}: Cargo.toml lacks `[lints] workspace = true`, so the workspace's denied lints do not apply to it.",
                pkg.name
            )),
            Err(e) => problems.extend(e),
        }
        if pkg.name != "xtask" && !deps::row_has_token(&table, "Workspace", &pkg.name) {
            problems.push(format!(
                "{}: not named as `{}` in the README Technical architecture Workspace row.",
                pkg.name, pkg.name
            ));
        }
    }
    problems.extend(lints::overrides(root));
    problems.extend(golden::check(root));
    problems.extend(module_size::check(root, &meta, tighten));
    problems.extend(module_cycles::check(&meta));
    problems.extend(vocabulary::check(root));
    problems.extend(obfuscation::check(root));
    problems.extend(obfuscation_raw::check(root));
    problems.extend(discover_replay::check(root));
    problems.extend(clippy_config::check(root, &meta));
    problems.extend(scale_mem_check::check(root, tighten));
    problems.extend(decision_numbers::check(root));
    problems.extend(contract_frozen::check(root));
    problems.extend(readme_scale::check(root, &table));
    problems.extend(expect_count::check(root, &meta, tighten));
    problems.extend(public_api::check(root, &meta));
    problems.extend(private_capture::check(root));
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
            Some(ext) if !deps::row_has_token(&table, &ext.readme_row, dep) => problems.push(format!(
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
            "✓ dependency allowlist, stack table, AGENTS.md, lint inheritance, no overrides, golden replay, module sizes, domain vocabulary, obfuscation replay, raw obfuscation replay, profiler obfuscation replay, clippy config, scale memory, decision numbers, frozen contract, expect count, public API, no private capture: {} crates, {} external dependencies",
            meta.packages.len(),
            used_external.len()
        ))
    } else {
        Err(problems)
    }
}

fn crate_dir(manifest: &Path) -> PathBuf {
    manifest.parent().map(Path::to_path_buf).unwrap_or_default()
}
