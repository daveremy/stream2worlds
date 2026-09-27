//! Rustc dep-info backstop: every compiled `src/` file must have been walked.
use super::Target;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// Dep-info is Makefile syntax, not Rust source: unfold continuations and decode escaped spaces.
pub(super) fn dep_paths(text: &str) -> Vec<PathBuf> {
    let unfolded = text.replace("\\\r\n", "").replace("\\\n", "");
    let mut paths = Vec::new();
    for line in unfolded.lines() {
        let Some((_, deps)) = line.split_once(": ") else {
            continue;
        };
        let mut word = String::new();
        let mut escaped = false;
        for ch in deps.chars().chain(std::iter::once(' ')) {
            if escaped {
                word.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch.is_whitespace() {
                if !word.is_empty() {
                    paths.push(PathBuf::from(std::mem::take(&mut word)));
                }
            } else {
                word.push(ch);
            }
        }
    }
    paths
}
pub(super) fn dep_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| {
        format!(
            "{}: {e}; build the workspace to produce dep-info",
            dir.display()
        )
    })? {
        let unreadable = |e: std::io::Error| {
            format!("{}: {e}; check target-directory permissions", dir.display())
        };
        let entry = entry.map_err(unreadable)?;
        let path = entry.path();
        let kind = entry.file_type().map_err(unreadable)?;
        if kind.is_dir() {
            dep_files(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "d") {
            files.push(path);
        }
    }
    Ok(())
}
pub(super) fn dep_check(
    root: &Path,
    target: &Target,
    src: &Path,
    files: &[PathBuf],
    visited: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    // Matches Cargo's uplifted, unhashed `target/debug/{lib,}<name>.d`, not `deps/<name>-<hash>.d`.
    let name = target.name.replace('-', "_");
    // Dep-info paths are canonicalized below; compare like with like, or a symlinked root
    // makes every starts_with() false and the backstop passes having checked nothing.
    let src = fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    let src_path = fs::canonicalize(&target.src_path).unwrap_or_else(|_| target.src_path.clone());
    let mut found = false;
    for file in files {
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        if stem != target.name && stem != name && stem != format!("lib{name}") {
            continue;
        }
        let text = fs::read_to_string(file)
            .map_err(|e| format!("{}: {e}; delete it and rebuild", file.display()))?;
        let deps: BTreeSet<_> = dep_paths(&text)
            .iter()
            .map(|p| root.join(p))
            .map(|p| fs::canonicalize(&p).unwrap_or(p))
            .collect();
        if !deps.contains(&src_path) {
            continue;
        }
        found = true;
        for path in deps {
            if path.starts_with(&src)
                && path.extension().is_some_and(|e| e == "rs")
                && !visited.contains(&path)
            {
                return Err(format!(
                    "xtask's module-size walker did not visit {}, which rustc compiled — the resolution algorithm has a bug or a legitimate case it doesn't handle yet; fix the walker",
                    path.display()
                ));
            }
        }
    }
    if found {
        Ok(())
    } else {
        Err(format!(
            "{}: no matching dep-info found; run cargo build --workspace and check the target directory",
            target.name
        ))
    }
}
