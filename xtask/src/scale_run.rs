//! `cargo xtask scale` (s2w#32, decision 0004): fold instructions per event under Valgrind for
//! both event supplies (s2w#174), judged against `[ir]` and `[ir.recorded]` in
//! `xtask/scale-baseline.toml`, then the reported-only numbers. The recorded fixture's pin
//! (`[recorded]`, `[ir.recorded] events`) is checked first, so a changed recording fails before
//! the Valgrind run, in xtask's own terms.
//!
//! Linux only, and needs `valgrind` plus `gungraun-runner` at the version `s2w-app` pins for
//! `gungraun`; a missing tool is a failure with the install command, never a skip. The
//! benchmark's output directory is deleted first, so a summary that exists was written by this
//! run and a stale file can never pass. The `scale_wall` append benchmark must run: a failed run
//! or an unreadable JSON line is a failure, but its value is reported, not judged. Exit 0 only
//! when every gated number was measured and passed and `scale_wall` ran.
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::Deserialize;

use crate::scale::{self, Baseline, Supply};

/// The benchmark's output directory under the target directory.
const OUTPUT: &str = "gungraun/s2w-app/scale_ir";

/// The generator whose `IR_EVENTS` the benchmark folds.
const GENERATOR: &str = "crates/s2w-app/tests/support/scale_generator.rs";
/// Decision 0004's ingest target, events per second.
const INGEST_TARGET: f64 = 1000.0;

/// The wall-clock benchmark's JSON line.
#[derive(Deserialize)]
struct Wall {
    append_events_per_s_tx1: f64,
    filesystem: String,
    /// The bench's own label for a tmpfs number; present exactly when `filesystem` is tmpfs.
    warning: Option<String>,
}

/// Runs the scale fitness function and prints its report.
pub(super) fn run(root: &Path) -> ExitCode {
    let mut problems = Vec::new();
    match gated(root) {
        Ok(lines) => lines.iter().for_each(|line| println!("✓ {line}")),
        Err(e) => problems.extend(e),
    }
    match wall(root) {
        Ok(line) => println!("{line}"),
        Err(e) => problems.push(e),
    }
    println!("parse: not yet measurable (follow-up)");
    println!("fork: not yet measurable (blocked on s2w#14)");
    if problems.is_empty() {
        return ExitCode::SUCCESS;
    }
    for p in &problems {
        eprintln!("✗ {p}");
    }
    eprintln!("xtask scale: {} problem(s)", problems.len());
    ExitCode::FAILURE
}

/// Preflight, the fixture pin, the benchmarks, and both `[ir]` judgments.
fn gated(root: &Path) -> Result<Vec<String>, Vec<String>> {
    let baseline = scale::read(root).map_err(|e| vec![e])?;
    preflight(root)?;
    let events = ir_events(root).map_err(|e| vec![e])?;
    if events != baseline.ir.events {
        return Err(vec![format!(
            "{GENERATOR} folds IR_EVENTS = {events} but [ir] events = {}; re-measure and update [ir] in {}",
            baseline.ir.events,
            scale::BASELINE
        )]);
    }
    let fixture = std::fs::read(root.join(scale::FIXTURE)).map_err(|e| {
        vec![format!(
            "recorded fixture: UNKNOWN, {}: {e}",
            scale::FIXTURE
        )]
    })?;
    scale::judge_fixture(&baseline, &fixture).map_err(|e| vec![e])?;
    let target = crate::metadata(root)?.target_directory;
    let output = bench(root, &target).map_err(|e| vec![e])?;
    stamp(&baseline);
    let (mut lines, mut problems) = (Vec::new(), Vec::new());
    for supply in Supply::ALL {
        match summary_total(&output, supply).and_then(|t| scale::judge_ir(&baseline, supply, t)) {
            Ok(line) => lines.push(line),
            Err(e) => problems.push(e),
        }
    }
    if problems.is_empty() {
        Ok(lines)
    } else {
        Err(problems)
    }
}

/// Prints what the gated number was measured with, and notes a compiler that moved.
fn stamp(baseline: &Baseline) {
    let rustc = Command::new("rustc")
        .arg("-V")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "rustc: UNKNOWN".to_owned());
    println!(
        "scale: {rustc}, profile {}; baseline set by {} with {} on CI image {} (Ir is owned by that image; a local number may differ)",
        baseline.ir.profile, baseline.set_by, baseline.ir.rustc, baseline.ir.ci_image
    );
    if baseline.ir.rustc != "unset" && rustc != baseline.ir.rustc {
        println!(
            "scale: the compiler differs from the baseline's; instruction counts move with it, so a toolchain bump re-sets [ir] from the CI job"
        );
    }
}

/// `valgrind` and `gungraun-runner` on PATH, the runner at the crate's pinned version.
fn preflight(root: &Path) -> Result<(), Vec<String>> {
    let pin = gungraun_pin(root).map_err(|e| vec![e])?;
    let install = format!("cargo install gungraun-runner --version {pin} --locked");
    let mut problems = Vec::new();
    if on_path("valgrind").is_none() {
        problems.push(
            "valgrind not found on PATH: install it (Debian/Ubuntu: sudo apt-get install valgrind)"
                .to_owned(),
        );
    }
    match on_path("gungraun-runner") {
        None => problems.push(format!("gungraun-runner not found on PATH: {install}")),
        Some(runner) => {
            let version = Command::new(runner)
                .arg("--version")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_default();
            if version.rsplit(' ').next() != Some(pin.as_str()) {
                problems.push(format!(
                    "gungraun-runner reports '{version}' but s2w-app pins gungraun ={pin}; the two must match: {install} --force"
                ));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn on_path(tool: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(tool))
        .find(|p| p.is_file())
}

/// The `=x.y.z` pin on `gungraun` in `s2w-app`'s dev-dependencies, without the `=`.
fn gungraun_pin(root: &Path) -> Result<String, String> {
    let manifest: toml::Table =
        crate::read_toml(&root.join("crates/s2w-app/Cargo.toml")).map_err(|e| e.join("; "))?;
    let dep = manifest
        .get("dev-dependencies")
        .and_then(|d| d.get("gungraun"));
    dep.and_then(|d| d.as_str().or_else(|| d.get("version")?.as_str()))
        .and_then(|v| v.strip_prefix('='))
        .map(str::to_owned)
        .ok_or_else(|| "crates/s2w-app/Cargo.toml: gungraun must be an exact `=x.y.z` dev-dependency so gungraun-runner can match it".to_owned())
}

/// `IR_EVENTS` from the generator, read as a syn AST.
fn ir_events(root: &Path) -> Result<u64, String> {
    let path = root.join(GENERATOR);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    file.items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(c) if c.ident == "IR_EVENTS" => match &*c.expr {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(n),
                    ..
                }) => n.base10_parse().ok(),
                _ => None,
            },
            _ => None,
        })
        .ok_or_else(|| format!("{GENERATOR}: no integer-literal `const IR_EVENTS`; the benchmark's event count must stay a literal xtask can read"))
}

/// Runs both benchmarks fresh and returns their output directory.
fn bench(root: &Path, target: &Path) -> Result<PathBuf, String> {
    let output = target.join(OUTPUT);
    if output.exists() {
        std::fs::remove_dir_all(&output)
            .map_err(|e| format!("cannot delete {}: {e}", output.display()))?;
    }
    let mut args = vec![
        "bench".to_owned(),
        "--package".to_owned(),
        "s2w-app".to_owned(),
        "--bench".to_owned(),
        "scale_ir".to_owned(),
        "--".to_owned(),
        "--save-summary=json".to_owned(),
    ];
    // gungraun clears the environment before starting valgrind; a userland valgrind needs this.
    if std::env::var_os("VALGRIND_LIB").is_some() {
        args.push("--envs=VALGRIND_LIB".to_owned());
    }
    let status = crate::cargo()
        .args(&args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("fold Ir: UNKNOWN, could not run cargo bench: {e}"))?;
    if !status.success() {
        return Err(format!(
            "fold Ir: UNKNOWN, `cargo {}` failed ({status}); fix the benchmark and retry",
            args.join(" ")
        ));
    }
    Ok(output)
}

/// One supply's total Callgrind `Ir`. [`bench()`] deleted the output directory first, so a
/// summary that exists is this run's.
fn summary_total(output: &Path, supply: Supply) -> Result<u64, String> {
    let (summary, name) = (output.join(supply.ir_summary()), supply.name("fold Ir"));
    let text = std::fs::read_to_string(&summary).map_err(|e| {
        format!(
            "{name}: UNKNOWN, no summary written by this run at {} ({e}); the benchmark or gungraun's output layout changed",
            summary.display()
        )
    })?;
    scale::summary_ir(&text).map_err(|e| format!("{name}: UNKNOWN, {e}"))
}

/// The append rate: the benchmark must run and print its JSON line; the value is reported, not
/// judged.
fn wall(root: &Path) -> Result<String, String> {
    let out = crate::cargo()
        .args(["bench", "--package", "s2w-app", "--bench", "scale_wall"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("append: could not run cargo bench: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "append: the scale_wall benchmark failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let w: Wall = scale::last_json_line(&String::from_utf8_lossy(&out.stdout))
        .map_err(|e| format!("append: the scale_wall benchmark left {e}"))?;
    let rate = w.append_events_per_s_tx1;
    Ok(match (w.filesystem.as_str(), w.warning) {
        ("disk", _) => format!(
            "append (1 event per transaction, reported, not gated): {rate:.0} events/s = {:.2}x decision 0004's 1,000/s target, on disk",
            rate / INGEST_TARGET
        ),
        ("tmpfs", Some(warning)) => format!(
            "append (1 event per transaction): {rate:.0} events/s, {warning}; excluded from comparison"
        ),
        ("tmpfs", None) => {
            return Err(
                "append: the scale_wall benchmark measured on tmpfs but printed no `warning` field; restore it in crates/s2w-app/benches/scale_wall.rs".to_owned(),
            );
        }
        (other, _) => format!(
            "append (1 event per transaction): {rate:.0} events/s on filesystem '{other}'; not a known disk, excluded from comparison"
        ),
    })
}
