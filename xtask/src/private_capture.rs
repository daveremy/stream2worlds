//! Check 20: no private capture in the repository (s2w#371).
//!
//! A private capture (`research/h-measure/private/capture.ts`) is written outside every git work
//! tree, and only its pins are committed. This check walks the working tree (everything except
//! `.git`, `target` and `node_modules`) and fails on (a) any `*.sse` or `*.provenance.jsonl`
//! under `research/` other than the synthetic fixture, (b) a synthetic fixture whose first line
//! is not the synthetic header, and (c) any file anywhere with a line that starts with the
//! private capture header. An unreadable directory or file is a failure, not a pass.

use std::fs;
use std::path::Path;

const RESEARCH: &str = "research";
const FIXTURE: &str = "research/h-measure/private/fixture/synthetic-20.sse";
const MARK: &str = ": s2w-private-capture provenance=";
const SKIP: [&str; 3] = [".git", "target", "node_modules"];

pub(super) fn check(root: &Path) -> Vec<String> {
    if !root.join(RESEARCH).is_dir() {
        return vec![format!(
            "{RESEARCH}/ is missing; the private-capture check cannot run without it."
        )];
    }
    let mut problems = Vec::new();
    walk(root, root, &mut problems);
    problems
}

fn walk(root: &Path, dir: &Path, problems: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => return problems.push(format!("{}: {e}", rel(root, dir))),
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(e) => return problems.push(format!("{}: {e}", rel(root, dir))),
        }
    }
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_symlink() || SKIP.contains(&name.as_str()) {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, problems);
        } else {
            match fs::read(&path) {
                Ok(bytes) => problems.extend(file_problems(&rel(root, &path), &bytes)),
                Err(e) => problems.push(format!("{}: {e}", rel(root, &path))),
            }
        }
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The problems one file raises, given its path relative to the repository root.
fn file_problems(path: &str, bytes: &[u8]) -> Vec<String> {
    let mut problems = Vec::new();
    let private = format!("{MARK}private");
    if bytes
        .split(|b| *b == b'\n')
        .any(|line| line.starts_with(private.as_bytes()))
    {
        problems.push(format!(
            "{path}: has a private capture header line. Private captures never enter the repository: delete the file (or the line) and keep the capture under the h-measure --dir, outside any work tree."
        ));
    }
    let captured = path.ends_with(".sse") || path.ends_with(".provenance.jsonl");
    if captured && path.starts_with(&format!("{RESEARCH}/")) {
        if path == FIXTURE {
            if !bytes.starts_with(format!("{MARK}synthetic ").as_bytes()) {
                problems.push(format!(
                    "{path}: the synthetic fixture must start with the synthetic header. Regenerate it with `node --experimental-strip-types research/h-measure/private/fixture.ts --write`."
                ));
            }
        } else {
            problems.push(format!(
                "{path}: capture files do not belong under {RESEARCH}/. Move it to the h-measure --dir and pin its sha256 in research/h-measure/corpora.toml instead."
            ));
        }
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
        let problems = check(&root);
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
    fn a_provenance_sidecar_under_research_fails() {
        let problems = tree("prov", &[("research/a.provenance.jsonl", "{}\n")]);
        assert_eq!(problems.len(), 1, "{problems:?}");
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
    fn skipped_directories_are_not_read() {
        assert!(
            tree(
                "skip",
                &[("research/r.md", "x"), ("research/node_modules/x.sse", "x")]
            )
            .is_empty()
        );
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
