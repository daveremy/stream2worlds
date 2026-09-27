//! Structural module sizes; report findings separately from the blocking baseline ratchet.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(super) struct Target {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
}
#[derive(Deserialize, Serialize)]
struct Config {
    enforce: bool,
    cap: usize,
    #[serde(default)]
    exempt: Vec<Exempt>,
}
#[derive(Deserialize, Serialize)]
struct Exempt {
    module: String,
    lines: usize,
    reason: String,
    issue: String,
}

mod depinfo;
mod ratchet;
mod walk;

use depinfo::{dep_check, dep_files};
use ratchet::growth;
use walk::Scan;

fn exemptions(config: &Config, scan: &Scan) -> Vec<String> {
    let mut findings = Vec::new();
    let mut seen = BTreeSet::new();
    for e in &config.exempt {
        if !seen.insert(&e.module) || e.reason.trim().is_empty() || e.issue.trim().is_empty() {
            findings.push(format!("{}: exemptions must be unique with non-empty reason and issue fields; fix xtask/module-size.toml", e.module));
        }
        match scan.rows.get(&e.module) {
            None => findings.push(format!("stale exemption for {}: delete it", e.module)),
            Some((_, _, n)) if *n <= config.cap => findings.push(format!("stale exemption for {}: delete it", e.module)),
            Some((_, _, n)) if *n != e.lines => findings.push(format!("{}: exemption lines {} != actual {n}; run cargo xtask check --tighten-baseline (growth requires explicit authorization)", e.module, e.lines)),
            _ => {}
        }
    }
    findings
}
pub(super) fn check(root: &Path, meta: &super::Metadata, tighten: bool) -> Vec<String> {
    let config_path = root.join("xtask/module-size.toml");
    let mut config: Config = match super::read_toml(&config_path) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Always ask Cargo to refresh artifacts: its incremental build reuses an already-built tree.
    let build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--workspace", "--offline"])
        .current_dir(root)
        .status();
    let mut findings = Vec::new();
    if !build.is_ok_and(|s| s.success()) {
        findings.push("module-size dep-info unavailable: cargo build --workspace failed; fix the build and retry".into());
    }
    let mut files = Vec::new();
    if let Err(e) = dep_files(&meta.target_directory, &mut files) {
        findings.push(e);
    }
    let mut scan = Scan::default();
    for pkg in &meta.packages {
        for target in &pkg.targets {
            if !target
                .kind
                .iter()
                .any(|k| matches!(k.as_str(), "lib" | "bin" | "proc-macro"))
            {
                continue;
            }
            let mut target_scan = Scan::default();
            if let Err(e) = target_scan.file(&target.src_path, target.name.replace('-', "_"), true)
            {
                findings.push(e);
            }
            let src = super::crate_dir(&pkg.manifest_path).join("src");
            if let Err(e) = dep_check(root, target, &src, &files, &target_scan.visited) {
                findings.push(e);
            }
            scan.rows.extend(target_scan.rows);
            scan.findings.extend(target_scan.findings);
            scan.incomplete |= target_scan.incomplete;
        }
    }
    // Check growth BEFORE tightening so the repair command cannot conceal a new exemption.
    let mut problems = growth(root, &config);
    if tighten && (scan.incomplete || !findings.is_empty() || !problems.is_empty()) {
        problems.push("cannot tighten an incomplete scan or unauthorized baseline; resolve the findings and retry".into());
    } else if tighten {
        config
            .exempt
            .retain_mut(|e| match scan.rows.get(&e.module) {
                Some((_, _, n)) if *n > config.cap => {
                    e.lines = e.lines.min(*n);
                    true
                }
                _ => false,
            });
        match toml::to_string_pretty(&config)
            .map_err(|e| e.to_string())
            .and_then(|s| fs::write(&config_path, s).map_err(|e| e.to_string()))
        {
            Ok(()) => {}
            Err(e) => problems.push(format!("cannot tighten {}: {e}", config_path.display())),
        }
    }
    findings.extend(exemptions(&config, &scan));
    println!(
        "| Module | wc -l | Test lines (est.) | Non-test | Cap | Exempt |\n|---|---:|---:|---:|---|---|"
    );
    for (key, (wc, tests, n)) in &scan.rows {
        let exempt = config.exempt.iter().any(|e| &e.module == key);
        println!(
            "| {key} | {wc} | {tests} | {n} | {} | {} |",
            if *n > config.cap { "over" } else { "under" },
            if exempt { "Y" } else { "N" }
        );
        if *n > config.cap && !exempt {
            findings.push(format!("{key} is {n} non-test lines (cap {}): split it, or add a [[exempt]] entry with reason and issue.", config.cap));
        }
    }
    findings.extend(scan.findings);
    for finding in findings {
        if config.enforce {
            problems.push(finding);
        } else {
            println!("[report-only] {finding}");
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::depinfo::dep_paths;
    use super::ratchet::{git, trailer};
    use super::walk::test_only;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use syn::Meta;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "s2w-size-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        }
        fn scan(&self, source: &str) -> Scan {
            let path = self.write("src/odd_root.rs", source);
            let mut scan = Scan::default();
            scan.file(&path, "demo".into(), true).unwrap();
            scan
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn structural_counts_and_recursive_test_predicates() {
        let scratch = Scratch::new();
        let scan = scratch.scan("//! docs\n\n// free comment\nfn production() {}\n#[cfg(test)]\nmod tests {\n    fn test_code() {}\n}\nmod inline {\n    #[cfg(all(unix, any(test, all(test, feature = \"a\"))))]\n    fn also_test() {}\n    fn real() {}\n}\n");
        assert_eq!(scan.rows["demo"], (13, 6, 5));
        for (predicate, expected) in [
            ("test", true),
            ("all(unix, test)", true),
            ("any(test, all(test, unix))", true),
            ("any(test, unix)", false),
            ("not(test)", false),
        ] {
            assert_eq!(
                test_only(&syn::parse_str::<Meta>(predicate).unwrap()),
                expected,
                "{predicate}"
            );
        }
        let scan = scratch.scan("#[cfg_attr(test, allow(dead_code))]\nfn kept() {}\n#[cfg(test)]\nimpl T { fn hidden() {} }\nimpl T {\n#[cfg(test)]\nfn hidden() {}\nfn kept() {}\n}\n#[test]\nfn test_fn() {}\n");
        assert_eq!(scan.rows["demo"].2, 5);
    }
    #[test]
    fn standard_layout_arbitrary_roots_and_missing_or_ambiguous_modules() {
        let scratch = Scratch::new();
        scratch.write("src/a/x.rs", "mod deep;");
        scratch.write("src/a/x/deep/mod.rs", "mod leaf;");
        scratch.write("src/a/x/deep/leaf.rs", "fn leaf() {}");
        let scan = scratch.scan("mod a { mod x; }\n#[cfg(test)]\nmod absent;\n");
        assert!(scan.findings.is_empty(), "{:?}", scan.findings);
        assert_eq!(scan.visited.len(), 4);
        assert!(scan.rows.contains_key("demo::a::x::deep::leaf"));
        scratch.write("src/a/x/mod.rs", "");
        let scan = scratch.scan("mod a { mod x; mod missing; }");
        assert!(
            scan.findings
                .iter()
                .any(|s| s.contains("ambiguous module path"))
        );
        assert!(
            scan.findings
                .iter()
                .any(|s| s.contains("module content xtask cannot see"))
        );
    }
    #[test]
    fn refuses_paths_and_includes_in_items_expressions_and_test_items() {
        let scratch = Scratch::new();
        let scan = scratch.scan("#[path = \"elsewhere.rs\"] mod a;\n#[cfg_attr(any(), path = \"elsewhere.rs\")] mod b;\ninclude!(\"opaque.rs\");\nfn f() { let _ = include_str!(\"x\"); let _ = include_bytes!(\"y\"); }\n#[cfg(test)] mod tests { include!(\"z\"); }\n");
        assert_eq!(scan.findings.len(), 6, "{:?}", scan.findings);
        assert!(scan.findings[0].contains("explicit #[path] defeats the size check"));
        assert!(scan.findings[1].contains("cfg_attr path bypass"));
        assert!(
            scan.findings[2..]
                .iter()
                .all(|s| s.contains("include! macros are opaque"))
        );
        println!("{}", scan.findings.join("\n"));
    }
    fn config() -> Config {
        Config {
            enforce: false,
            cap: 400,
            exempt: vec![Exempt {
                module: "demo".into(),
                lines: 501,
                reason: "fixture".into(),
                issue: "s2w#44".into(),
            }],
        }
    }
    #[test]
    fn exemption_equality_staleness_and_required_fields() {
        let mut config = config();
        let mut scan = Scan::default();
        for actual in [500, 502] {
            scan.rows.insert("demo".into(), (600, 100, actual));
            let errors = exemptions(&config, &scan);
            assert!(errors[0].contains("!= actual"));
            println!("{}", errors[0]);
        }
        scan.rows.insert("demo".into(), (600, 99, 501));
        assert!(exemptions(&config, &scan).is_empty());
        config.exempt[0].reason.clear();
        assert!(exemptions(&config, &scan)[0].contains("non-empty reason and issue"));
        config.exempt[0].reason = "fixture".into();
        scan.rows.insert("demo".into(), (600, 200, 400));
        assert!(exemptions(&config, &scan)[0].contains("stale exemption"));
        scan.rows.clear();
        assert!(exemptions(&config, &scan)[0].contains("stale exemption"));
    }
    #[test]
    fn growth_checks_entire_range_including_behind_a_merge_and_fails_closed() {
        let scratch = Scratch::new();
        let root = &scratch.0;
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["commit", "--allow-empty", "-m", "base"],
            vec!["update-ref", "refs/remotes/origin/main", "HEAD"],
        ] {
            git(root, &args).unwrap();
        }
        let mut config = config();
        assert!(!growth(root, &config).is_empty()); // base file absent
        scratch.write("xtask/module-size.toml", &toml::to_string(&config).unwrap());
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-m", "baseline"]).unwrap();
        git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]).unwrap();
        assert!(growth(root, &config).is_empty());
        config.exempt[0].lines += 1;
        assert!(!growth(root, &config).is_empty());
        git(root, &["checkout", "-b", "pr"]).unwrap();
        git(
            root,
            &[
                "commit",
                "--allow-empty",
                "-m",
                "authorized\n\nBaseline-growth: s2w#44",
            ],
        )
        .unwrap();
        git(
            root,
            &[
                "commit",
                "--allow-empty",
                "-m",
                "later commit without trailer",
            ],
        )
        .unwrap();
        git(root, &["checkout", "main"]).unwrap();
        git(
            root,
            &["merge", "--no-ff", "pr", "-m", "synthetic PR merge"],
        )
        .unwrap();
        assert!(growth(root, &config).is_empty());
        config.exempt[0].module = "new_module".into();
        assert!(growth(root, &config).is_empty());
        git(root, &["update-ref", "-d", "refs/remotes/origin/main"]).unwrap();
        assert!(!growth(root, &config).is_empty());
        assert!(!trailer(
            "Baseline-growth: s2w#no\nBaseline-growth: s2w#44 trailing"
        ));
        println!(
            "Growth: absent base/new entry and increased ceiling FAIL; trailer behind merge PASS; missing origin/main FAIL"
        );
    }
    #[test]
    fn dep_info_escaped_paths_continuations_and_compiled_unvisited_files() {
        assert_eq!(
            dep_paths("out: src/a\\ b.rs \\\n src/c.rs\nsrc/c.rs:\n"),
            vec![PathBuf::from("src/a b.rs"), PathBuf::from("src/c.rs")]
        );
        let scratch = Scratch::new();
        let source = scratch.write("src/odd_root.rs", "");
        let hidden = scratch.write("src/hidden.rs", "");
        let dep = scratch.write(
            "target/debug/libdemo.d",
            &format!("out: {} {}\n", source.display(), hidden.display()),
        );
        let target = Target {
            name: "demo".into(),
            kind: vec!["lib".into()],
            src_path: source.clone(),
        };
        let mut visited = BTreeSet::from([source]);
        let err = dep_check(
            &scratch.0,
            &target,
            &scratch.0.join("src"),
            std::slice::from_ref(&dep),
            &visited,
        )
        .unwrap_err();
        assert!(err.contains("which rustc compiled"));
        println!("{err}");
        visited.insert(hidden);
        assert!(
            dep_check(
                &scratch.0,
                &target,
                &scratch.0.join("src"),
                &[dep],
                &visited
            )
            .is_ok()
        );
    }
}
