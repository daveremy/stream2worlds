//! Check 20: no private capture in the repository (s2w#371).
//!
//! A private capture (`research/h-measure/private/capture.ts`) is written outside every git work
//! tree, and only its pins are committed. This check reads every file `git ls-files -co
//! --exclude-standard` lists (tracked plus untracked-but-not-ignored, so a capture dropped into
//! the tree is caught before it is committed) and fails on (a) any `*.sse` under `research/`
//! other than the synthetic fixture, (b) any `*.provenance.jsonl` anywhere (the sidecar has no
//! header line, so its name is the only mark), (c) a synthetic fixture whose first line is not
//! the synthetic header, and (d) any non-Rust file with a line that starts with the private
//! capture header (Rust source is read only as `syn` ASTs, xtask/AGENTS.md). A git failure, a
//! missing `research/` or an unreadable file is a failure, not a pass.

use std::fs;
use std::path::Path;

const RESEARCH: &str = "research";
const FIXTURE: &str = "research/h-measure/private/fixture/synthetic-20.sse";
const MARK: &str = ": s2w-private-capture provenance=";
const REGENERATE: &str =
    "node --experimental-strip-types research/h-measure/private/fixture.ts --write";

pub(super) fn check(root: &Path) -> Vec<String> {
    match crate::module_size::git(root, &["ls-files", "-co", "--exclude-standard", "-z"]) {
        Ok(listing) => {
            let paths: Vec<&str> = listing.split('\0').filter(|p| !p.is_empty()).collect();
            check_paths(root, &paths)
        }
        Err(e) => vec![format!(
            "git ls-files: {e}. Check 20 lists the work tree with git; run it from a git checkout with git on PATH."
        )],
    }
}

/// Reads each listed path (relative to `root`) and collects the problems it raises.
fn check_paths(root: &Path, paths: &[&str]) -> Vec<String> {
    if !root.join(RESEARCH).is_dir() {
        return vec![format!(
            "{RESEARCH}/ is missing; check 20 cannot run without it. Run `cargo xtask check` from the repository root."
        )];
    }
    let mut problems = Vec::new();
    for path in paths {
        let full = root.join(path);
        if full.is_symlink() || !full.is_file() {
            continue; // a listed path deleted in the work tree, or a link: nothing to read
        }
        match fs::read(&full) {
            Ok(bytes) => problems.extend(file_problems(path, &bytes)),
            Err(e) => problems.push(format!(
                "{path}: {e}. Check 20 must read every file; fix its permissions or remove it, then rerun `cargo xtask check`."
            )),
        }
    }
    problems
}

/// The problems one file raises, given its path relative to the repository root.
fn file_problems(path: &str, bytes: &[u8]) -> Vec<String> {
    let mut problems = Vec::new();
    let private = format!("{MARK}private");
    let rust = Path::new(path).extension().is_some_and(|e| e == "rs");
    if !rust
        && bytes
            .split(|b| *b == b'\n')
            .any(|line| line.starts_with(private.as_bytes()))
    {
        problems.push(format!(
            "{path}: has a private capture header line. Private captures never enter the repository: delete the file (or the line) and keep the capture under the h-measure --dir, outside any work tree."
        ));
    }
    if path.ends_with(".provenance.jsonl") {
        problems.push(format!(
            "{path}: a capture provenance sidecar never enters the repository. Move it next to its capture under the h-measure --dir."
        ));
    } else if path == FIXTURE {
        if !bytes.starts_with(format!("{MARK}synthetic ").as_bytes()) {
            problems.push(format!(
                "{path}: the synthetic fixture must start with the synthetic header. Regenerate it with `{REGENERATE}`."
            ));
        }
    } else if path.starts_with(&format!("{RESEARCH}/"))
        && Path::new(path).extension().is_some_and(|e| e == "sse")
    {
        problems.push(format!(
            "{path}: capture files do not belong under {RESEARCH}/. Move it to the h-measure --dir and pin its sha256 in research/h-measure/corpora.toml instead."
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str, files: &[(&str, &str)]) -> Vec<String> {
        let root = std::env::temp_dir().join(format!("s2w-private-{name}-{}", std::process::id()));
        for (path, body) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        let paths: Vec<&str> = files.iter().map(|(p, _)| *p).collect();
        let problems = check_paths(&root, &paths);
        fs::remove_dir_all(&root).unwrap();
        problems
    }

    fn synthetic() -> String {
        format!("{MARK}synthetic generated\n\nevent: message\nid: 1\ndata: {{}}\n\n")
    }

    #[test]
    fn a_capture_file_under_research_fails() {
        let problems = tree("sse", &[("research/x/dev.raw.sse", "id: 1\ndata: {}\n\n")]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].starts_with("research/x/dev.raw.sse:"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_provenance_sidecar_anywhere_fails() {
        let problems = tree(
            "prov",
            &[
                ("research/a.provenance.jsonl", "{}\n"),
                ("data/b.provenance.jsonl", "{}\n"),
            ],
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
    }

    #[test]
    fn a_capture_file_outside_research_and_rust_source_are_not_this_checks_business() {
        let body = format!("{MARK}private\n");
        let files = [
            ("research/r.md", "x"),
            ("crates/a/testdata/x.sse", "x"),
            ("src/a.rs", body.as_str()),
        ];
        assert!(tree("outside", &files).is_empty());
    }

    #[test]
    fn a_private_header_line_anywhere_fails() {
        let body = format!("notes\n{MARK}private captured x\n");
        let problems = tree(
            "header",
            &[("research/README.md", "x"), ("docs/notes.md", &body)],
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("docs/notes.md:"), "{problems:?}");
    }

    #[test]
    fn a_private_header_inside_a_line_passes() {
        let body = format!("const HEADER = \"{MARK}private\";\n");
        assert!(tree("inline", &[("research/a.ts", &body)]).is_empty());
    }

    #[test]
    fn the_fixture_passes_only_with_the_synthetic_header() {
        assert!(tree("fixture-ok", &[(FIXTURE, &synthetic())]).is_empty());
        let relabelled = synthetic().replace("synthetic", "private");
        let problems = tree("fixture-private", &[(FIXTURE, &relabelled)]);
        assert_eq!(problems.len(), 2, "{problems:?}");
    }

    #[test]
    fn a_missing_research_directory_fails() {
        assert_eq!(tree("missing", &[("docs/a.md", "x")]).len(), 1);
    }

    #[test]
    fn the_committed_fixture_replays_as_twenty_frames() {
        let root = crate::workspace_root();
        let sse = fs::read_to_string(root.join(FIXTURE)).unwrap();
        let envelopes = crate::discover_replay::envelopes(&sse).unwrap();
        assert_eq!(envelopes.len(), 20);
    }

    #[test]
    fn the_committed_tree_passes() {
        assert_eq!(check(&crate::workspace_root()), Vec::<String>::new());
    }
}
