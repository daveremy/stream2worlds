//! `cargo xtask discover-volume [--tighten-baseline]` (s2w#392; decisions 0022, 0025): the
//! world heap a discovered mapping costs. Runs `s2w-app`'s `discover_volume_heap` test's `fresh`
//! fold child as a nested release `cargo test` (dhat counts the world's heap bytes exactly),
//! reads the last stdout line that parses as JSON and judges it against `[discover_volume]` in
//! `xtask/scale-baseline.toml`. No JSON line, a failed test or a zero is a failure, never a pass.
//!
//! Too slow for `cargo xtask check` (a release build plus ~5 minutes under dhat), so it is its
//! own CI job, `discover-volume`. The baseline-growth trailer rule for the table runs in `check`
//! with the rest of the file. `--tighten-baseline` lowers `heap_bytes` to the measurement when
//! the check passes; it never raises anything.
use std::path::Path;
use std::process::{ExitCode, Stdio};

use crate::scale::{self, BASELINE, VolumeMeasurement};

/// The fold child, by its path in the heap target; run with `--exact`, so a rename matches
/// nothing and fails for want of a JSON line.
const CHILD: &str = "discover_volume::volume::fold_child";

/// Runs the gate and prints its report.
pub(super) fn run(root: &Path, args: &[String]) -> ExitCode {
    let tighten = match args {
        [] => false,
        [flag] if flag == "--tighten-baseline" => true,
        _ => {
            eprintln!("usage: cargo xtask discover-volume [--tighten-baseline]");
            return ExitCode::from(2);
        }
    };
    let problems = gate(root, tighten);
    if problems.is_empty() {
        return ExitCode::SUCCESS;
    }
    for p in &problems {
        eprintln!("✗ {p}");
    }
    eprintln!("xtask discover-volume: {} problem(s)", problems.len());
    ExitCode::FAILURE
}

fn gate(root: &Path, tighten: bool) -> Vec<String> {
    let baseline = match scale::read(root) {
        Ok(b) => b,
        Err(e) => return vec![e],
    };
    let m = match measure(root) {
        Ok(m) => m,
        Err(e) => return vec![e],
    };
    let (report, mut problems) = scale::discover_volume::judge(&baseline, &m);
    for line in report {
        println!("✓ {line}");
    }
    if tighten && !problems.is_empty() {
        problems.push("cannot tighten [discover_volume] while its check fails; resolve the findings and retry".into());
    } else if tighten {
        problems.extend(tighten_file(root, &m));
    }
    problems
}

fn measure(root: &Path) -> Result<VolumeMeasurement, String> {
    let args = [
        "test",
        "--release",
        "--package",
        "s2w-app",
        "--test",
        "discover_volume_heap",
        "--",
        "--ignored",
        "--exact",
        CHILD,
        "--nocapture",
    ];
    let name = "discover_volume heap";
    // stderr streams through (build progress, the child's human-readable line); stdout carries
    // the JSON line.
    let out = crate::cargo()
        .args(args)
        .env("S2W_DISCOVER_VOLUME_VARIANT", "fresh")
        .env_remove("S2W_DISCOVER_VOLUME_LINKS")
        .current_dir(root)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{name}: UNKNOWN, could not run cargo test: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{name}: UNKNOWN, `cargo {}` failed ({}); see its output above, fix it and retry",
            args.join(" "),
            out.status
        ));
    }
    scale::last_json_line(&String::from_utf8_lossy(&out.stdout)).map_err(|e| {
        format!(
            "{name}: UNKNOWN, `cargo {}` left {e}; make sure {CHILD} exists under that exact name and prints its JSON line last",
            args.join(" ")
        )
    })
}

fn tighten_file(root: &Path, m: &VolumeMeasurement) -> Vec<String> {
    let path = root.join(BASELINE);
    let result =
        std::fs::read_to_string(&path).and_then(|text| match scale::discover_volume::tighten_text(
            &text, m,
        ) {
            Some(lowered) => std::fs::write(&path, lowered).map(|()| true),
            None => Ok(false),
        });
    match result {
        Ok(true) => {
            println!(
                "discover-volume: lowered [discover_volume] heap_bytes in {BASELINE} to the measurement"
            );
            Vec::new()
        }
        Ok(false) => Vec::new(),
        Err(e) => vec![format!(
            "cannot tighten {}: {e}; check the file is writable and retry",
            path.display()
        )],
    }
}
