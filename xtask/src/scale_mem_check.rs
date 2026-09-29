//! Check 13, heap bytes per entity (s2w#32, decision 0004): runs `s2w-app`'s ignored
//! `scale_mem` tests, one per event supply (s2w#174), each as a nested `cargo test`, reads the
//! last stdout line that parses as JSON, and judges it against that supply's table (`[memory]`
//! or `[memory.recorded]`) in `xtask/scale-baseline.toml`, plus the baseline-growth check. The
//! recorded test's loader refuses a changed fixture itself, so this check does not re-read it.
//! No JSON line, a failed test or a zero measurement is a failure, never a pass. Fold
//! instructions per event need Valgrind, so they live in `cargo xtask scale` instead.
use std::path::Path;

use crate::scale::{self, BASELINE, MemMeasurement, Supply};

/// Runs the memory check; `tighten` also lowers `[memory]` values to the measurement.
pub(super) fn check(root: &Path, tighten: bool) -> Vec<String> {
    println!("scale: run 'cargo xtask scale' (Linux + Valgrind; CI job 'scale')");
    let baseline = match scale::read(root) {
        Ok(b) => b,
        Err(e) => return vec![e],
    };
    let mut problems = scale::growth(root, &baseline);
    let mut measured = Vec::new();
    for supply in Supply::ALL {
        match measure(root, supply) {
            Ok(m) => {
                let (report, judged) = scale::judge_memory(&baseline, supply, &m);
                for line in report {
                    println!("scale: {line}");
                }
                problems.extend(judged);
                measured.push((supply, m));
            }
            Err(e) => problems.push(e),
        }
    }
    if tighten && !problems.is_empty() {
        problems.push("cannot tighten the scale baseline while its check fails; resolve the findings and retry".into());
    } else if tighten {
        for (supply, m) in &measured {
            problems.extend(tighten_file(root, *supply, m));
        }
    }
    problems
}

fn measure(root: &Path, supply: Supply) -> Result<MemMeasurement, String> {
    let (test, name) = (supply.mem_test(), supply.name("bytes/entity"));
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
            test,
            "--nocapture",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("{name}: UNKNOWN, could not run cargo test: {e}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let tail: Vec<&str> = stderr.lines().rev().take(20).collect();
        return Err(format!(
            "{name}: UNKNOWN, the memory test failed ({}); fix it and retry. Last stderr lines:\n{}",
            out.status,
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        ));
    }
    scale::last_json_line(&String::from_utf8_lossy(&out.stdout)).map_err(|e| {
        format!(
            "{name}: UNKNOWN, `cargo test -p s2w-app --test scale_mem -- --ignored --exact {test}` left {e}; make sure that test exists under that exact name and prints its JSON line last"
        )
    })
}

fn tighten_file(root: &Path, supply: Supply, measured: &MemMeasurement) -> Vec<String> {
    let path = root.join(BASELINE);
    let result = std::fs::read_to_string(&path).and_then(|text| {
        match scale::tighten_text(&text, supply, measured) {
            Some(lowered) => std::fs::write(&path, lowered).map(|()| true),
            None => Ok(false),
        }
    });
    match result {
        Ok(true) => {
            println!(
                "scale: lowered {} in {BASELINE} to the measurement",
                supply.memory()
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
