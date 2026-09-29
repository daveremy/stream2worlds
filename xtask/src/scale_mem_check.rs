//! Check 13, heap bytes per entity (s2w#32, decision 0004): runs `s2w-app`'s ignored
//! `scale_mem` test (a nested `cargo test`), reads the last stdout line that parses as JSON, and
//! judges it against `[memory]` in `xtask/scale-baseline.toml`, plus the baseline-growth check.
//! No JSON line, a failed test or a zero measurement is a failure, never a pass. Fold
//! instructions per event need Valgrind, so they live in `cargo xtask scale` instead.
use std::path::Path;

use crate::scale::{self, BASELINE, MemMeasurement};

/// The one test that measures; `--exact` so a rename matches nothing and fails below.
const TEST_NAME: &str = "tests::bytes_per_entity_and_relationship";

/// Runs the memory check; `tighten` also lowers `[memory]` values to the measurement.
pub(super) fn check(root: &Path, tighten: bool) -> Vec<String> {
    println!("scale: run 'cargo xtask scale' (Linux + Valgrind; CI job 'scale')");
    let baseline = match scale::read(root) {
        Ok(b) => b,
        Err(e) => return vec![e],
    };
    let mut problems = scale::growth(root, &baseline);
    let measured = match measure(root) {
        Ok(m) => m,
        Err(e) => {
            problems.push(e);
            return problems;
        }
    };
    let (report, judged) = scale::judge_memory(&baseline, &measured);
    for line in report {
        println!("scale: {line}");
    }
    problems.extend(judged);
    if tighten && !problems.is_empty() {
        problems.push("cannot tighten the scale baseline while its check fails; resolve the findings and retry".into());
    } else if tighten {
        problems.extend(tighten_file(root, &measured));
    }
    problems
}

fn measure(root: &Path) -> Result<MemMeasurement, String> {
    let out = crate::cargo()
        .args([
            "test",
            "--package",
            "s2w-app",
            "--test",
            "scale_mem",
            "--offline",
            "--quiet",
            "--",
            "--ignored",
            "--exact",
            TEST_NAME,
            "--nocapture",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("bytes/entity: UNKNOWN, could not run cargo test: {e}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let tail: Vec<&str> = stderr.lines().rev().take(20).collect();
        return Err(format!(
            "bytes/entity: UNKNOWN, the memory test failed ({}); fix it and retry. Last stderr lines:\n{}",
            out.status,
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        ));
    }
    scale::last_json_line(&String::from_utf8_lossy(&out.stdout)).map_err(|e| {
        format!(
            "bytes/entity: UNKNOWN, `cargo test -p s2w-app --test scale_mem -- --ignored --exact {TEST_NAME}` left {e}; make sure that test exists under that exact name and prints its JSON line last"
        )
    })
}

fn tighten_file(root: &Path, measured: &MemMeasurement) -> Vec<String> {
    let path = root.join(BASELINE);
    let result = std::fs::read_to_string(&path).and_then(|text| {
        match scale::tighten_text(&text, measured) {
            Some(lowered) => std::fs::write(&path, lowered).map(|()| true),
            None => Ok(false),
        }
    });
    match result {
        Ok(true) => {
            println!("scale: lowered [memory] in {BASELINE} to the measurement");
            Vec::new()
        }
        Ok(false) => Vec::new(),
        Err(e) => vec![format!(
            "cannot tighten {}: {e}; check the file is writable and retry",
            path.display()
        )],
    }
}
