//! Exemption-growth ratchet against `origin/main`; blocks regardless of `enforce`.
use std::path::Path;
use std::process::Command;

use super::Config;

pub(super) fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
pub(super) fn trailer(messages: &str) -> bool {
    messages.lines().any(|line| {
        line.strip_prefix("Baseline-growth: s2w#")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}
pub(super) fn growth(root: &Path, config: &Config) -> Vec<String> {
    let base = git(root, &["show", "origin/main:xtask/module-size.toml"])
        .and_then(|s| toml::from_str::<Config>(&s).map_err(|e| e.to_string()));
    let base = match base {
        Ok(c) => c.exempt,
        Err(e) => {
            println!(
                "[baseline] base read failed: {e}; no exemption growth allowed without Baseline-growth authorization"
            );
            Vec::new()
        }
    };
    let grew = config.exempt.iter().any(|e| {
        !base
            .iter()
            .any(|b| b.module == e.module && e.lines <= b.lines)
    });
    if !grew {
        return Vec::new();
    }
    // CI requires checkout fetch-depth: 0: origin/main must be a real reachable ref.
    // Scan the entire PR range, including commits behind a synthetic merge commit.
    match git(root, &["log", "origin/main..HEAD", "--pretty=%B"]) {
        Ok(messages) if trailer(&messages) => Vec::new(),
        result => vec![format!(
            "exemption baseline grew: add a Baseline-growth: s2w#<N> trailer to a commit in origin/main..HEAD or remove the growth{}",
            result
                .err()
                .map(|e| format!("; cannot read commit range: {e}"))
                .unwrap_or_default()
        )],
    }
}
