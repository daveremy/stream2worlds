//! Check 16: the signed evaluation contract only grows by dated notes.
//!
//! `docs/evaluation-contract.md` says: "Once signed, this file is frozen. A change after sign-off
//! is a new dated section with its reason, never an edit in place." Review alone did not hold that
//! rule (s2w#59: two reviewers approved an in-place edit to B1). This check hashes every byte
//! above the `## Dated notes after sign-off` heading and compares it with the sha256 of the signed
//! text, pinned here. Anything at or below the heading may change.
//!
//! The pin lives in the source, not in a tagged commit, so the check needs no git history and
//! runs the same on a shallow clone. A missing file, a missing heading or a different hash fails.
//! `.gitattributes` pins the file to LF line endings, so a CRLF checkout cannot change the bytes.

use std::fs;
use std::path::Path;

const PATH: &str = "docs/evaluation-contract.md";
const HEADING: &str = "## Dated notes after sign-off";

/// Lower-case hex sha256 of the bytes above [`HEADING`] in the signed contract (v4.1, signed
/// 2026-09-27; unchanged since the heading was added in adbd78e). Change it only when Dave
/// re-signs the contract, in the same PR as the re-signed text.
const SIGNED_SHA256: &str = "2525746abbb5b6b3820da78faf829e945d394c2b9126a87b647bd925a6fe4326";

pub(super) fn check(root: &Path) -> Vec<String> {
    match fs::read(root.join(PATH)) {
        Ok(bytes) => judge(&bytes, SIGNED_SHA256),
        Err(e) => vec![format!(
            "{PATH}: {e}. The signed evaluation contract lives there; the frozen-text check cannot run without it."
        )],
    }
}

/// Compares the sha256 of everything above the first line that is exactly [`HEADING`] with
/// `signed`.
fn judge(bytes: &[u8], signed: &str) -> Vec<String> {
    let Some(end) = heading_start(bytes) else {
        return vec![format!(
            "{PATH}: no line reads exactly `{HEADING}`. That heading separates the signed text from the dated notes; restore it, and add changes as dated notes below it."
        )];
    };
    let actual = crate::sha256(&bytes[..end]);
    if actual == signed {
        return Vec::new();
    }
    vec![format!(
        "{PATH}: the text above `{HEADING}` differs from the signed version (sha256 {actual}, signed {signed}). The signed contract is frozen: undo the edit in place (`git diff origin/main -- {PATH}` shows it) and record the change as a new dated note below that heading, with its reason. Only if Dave has re-signed the contract, update SIGNED_SHA256 in xtask/src/contract_frozen.rs in the same PR."
    )]
}

/// Byte offset of the first line that is exactly [`HEADING`], if any.
fn heading_start(bytes: &[u8]) -> Option<usize> {
    let mut start = 0;
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let text = line.strip_suffix(b"\n").unwrap_or(line);
        if text == HEADING.as_bytes() {
            return Some(start);
        }
        start += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn committed() -> String {
        fs::read_to_string(crate::workspace_root().join(PATH)).unwrap()
    }

    #[test]
    fn the_committed_tree_passes() {
        assert_eq!(check(&crate::workspace_root()), Vec::<String>::new());
    }

    #[test]
    fn an_in_place_edit_to_b1_fails() {
        let text = committed();
        let original = "- **H:** heuristics alone (System 1 rules and local embeddings).";
        assert!(text.contains(original));
        let edited = text.replacen(
            original,
            "- **H:** heuristics alone (System 1 rules, local embeddings and Rebmann 2022).",
            1,
        );
        let problems = judge(edited.as_bytes(), SIGNED_SHA256);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("differs from the signed version"),
            "{problems:?}"
        );
        assert!(problems[0].contains("new dated note"), "{problems:?}");
    }

    #[test]
    fn an_appended_dated_note_passes() {
        let mut text = committed();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(
            "\n### 2026-09-29: a test note (no change in meaning)\n\nReason: s2w#59 fixture.\n",
        );
        assert_eq!(judge(text.as_bytes(), SIGNED_SHA256), Vec::<String>::new());
    }

    #[test]
    fn an_edit_to_an_existing_dated_note_passes() {
        let text = committed();
        let start = heading_start(text.as_bytes()).unwrap();
        let (signed, notes) = text.split_at(start);
        let edited = format!("{signed}{}", notes.replacen("pointer", "Pointer", 1));
        assert_ne!(edited, text);
        assert_eq!(
            judge(edited.as_bytes(), SIGNED_SHA256),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_missing_or_renamed_heading_fails() {
        let text = committed().replace(HEADING, "## Notes after sign-off");
        let problems = judge(text.as_bytes(), SIGNED_SHA256);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("no line reads exactly"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_second_heading_above_b1_fails() {
        let text = committed().replacen("### B1. Arms", &format!("{HEADING}\n\n### B1. Arms"), 1);
        assert_eq!(judge(text.as_bytes(), SIGNED_SHA256).len(), 1);
    }

    #[test]
    fn a_heading_on_the_first_line_hashes_nothing_and_fails() {
        let text = format!("{HEADING}\n{}", committed());
        assert_eq!(judge(text.as_bytes(), SIGNED_SHA256).len(), 1);
    }

    #[test]
    fn the_committed_text_fails_against_another_pin() {
        let other = crate::sha256(b"");
        assert_eq!(judge(committed().as_bytes(), &other).len(), 1);
    }

    #[test]
    fn a_missing_file_fails() {
        let root = std::env::temp_dir().join(format!("s2w-contract-frozen-{}", std::process::id()));
        assert_eq!(check(&root).len(), 1);
    }
}
