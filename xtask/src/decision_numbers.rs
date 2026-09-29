//! Check 14: decision-record numbers are unique.
//!
//! Every file in `docs/decisions/` whose name starts with digits carries that number as its
//! identity; references across the repo say "decision 0021". Two parallel legs can each pick the
//! next free number and land records that share it (s2w#181), and nothing else notices. This
//! check groups the files by numeric prefix and fails on any number held by more than one file,
//! naming every file that holds it. A missing or unreadable directory is a failure, not a pass.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const DIR: &str = "docs/decisions";

pub(super) fn check(root: &Path) -> Vec<String> {
    let entries = match fs::read_dir(root.join(DIR)) {
        Ok(entries) => entries,
        Err(e) => {
            return vec![format!(
                "{DIR}: {e}. The decision records live there; the duplicate-number check cannot run without them."
            )];
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Err(e) => return vec![format!("{DIR}: {e}")],
        }
    }
    duplicates(&names)
}

/// Groups file names by numeric prefix (`0021-x.md` and `21-y.md` share 21) and reports every
/// number held by more than one file. Names without a leading digit are not decision records.
fn duplicates(names: &[String]) -> Vec<String> {
    let mut by_number: BTreeMap<u64, Vec<&str>> = BTreeMap::new();
    for name in names {
        let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(number) = digits.parse::<u64>() {
            by_number.entry(number).or_default().push(name);
        }
    }
    let next_free = by_number.keys().next_back().map_or(1, |n| n + 1);
    by_number
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(number, mut files)| {
            files.sort_unstable();
            format!(
                "{DIR}: decision number {number:04} is used by {} files: {}. Renumber all but one to the next free number ({next_free:04}) and update every reference to it.",
                files.len(),
                files.join(", ")
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_duplicate_number_fails_and_names_both_files() {
        let root = std::env::temp_dir().join(format!("s2w-decision-dup-{}", std::process::id()));
        let dir = root.join(DIR);
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "0020-a.md",
            "0021-stream-mapping-v0.md",
            "0021-snapshots.md",
            "0022-b.md",
        ] {
            fs::write(dir.join(name), "# fixture\n").unwrap();
        }
        let problems = check(&root);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("0021-snapshots.md"), "{problems:?}");
        assert!(
            problems[0].contains("0021-stream-mapping-v0.md"),
            "{problems:?}"
        );
        assert!(problems[0].contains("(0023)"), "{problems:?}");
    }

    #[test]
    fn the_same_number_with_different_padding_is_a_duplicate() {
        assert_eq!(duplicates(&names(&["0007-a.md", "7-b.md"])).len(), 1);
    }

    #[test]
    fn unique_numbers_and_unnumbered_files_pass() {
        assert!(duplicates(&names(&["0001-a.md", "0002-b.md", "README.md"])).is_empty());
    }

    #[test]
    fn the_committed_tree_passes() {
        let root = crate::workspace_root();
        assert!(root.join(DIR).is_dir());
        assert_eq!(check(&root), Vec::<String>::new());
    }

    #[test]
    fn a_missing_directory_fails() {
        let root =
            std::env::temp_dir().join(format!("s2w-decision-numbers-{}", std::process::id()));
        assert_eq!(check(&root).len(), 1);
    }
}
