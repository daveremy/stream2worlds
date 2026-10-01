//! Structural module sizes; report findings separately from the blocking baseline ratchet.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(super) struct Target {
    pub(crate) name: String,
    kind: Vec<String>,
    pub(crate) src_path: PathBuf,
}
impl Target {
    /// A library target (any lib crate-type or a proc macro): its `pub` items are public API.
    pub(crate) fn is_lib(&self) -> bool {
        self.kind
            .iter()
            .any(|k| k.ends_with("lib") || k == "proc-macro")
    }
    /// Skip only the kinds the doctrine exempts, so lib crate-types such as cdylib or rlib
    /// (reported as their own kind) are walked rather than silently dropped.
    pub(crate) fn walked(&self) -> bool {
        !self
            .kind
            .iter()
            .any(|k| matches!(k.as_str(), "test" | "bench" | "example" | "custom-build"))
    }
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
pub(crate) mod walk;

#[cfg(test)]
use depinfo::dep_check;
use depinfo::{dep_files, package_dep_check};
use ratchet::growth;
pub(super) use ratchet::{git, trailer};
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
            Some((_, _, n)) if *n < e.lines => findings.push(format!("{}: exemption lines {} > actual {n}; run cargo xtask check --tighten-baseline", e.module, e.lines)),
            Some((_, _, n)) if *n > e.lines => findings.push(format!("{}: actual {n} > exemption lines {}; split the module, or raise lines with a Baseline-growth: s2w#<N> commit trailer", e.module, e.lines)),
            _ => {}
        }
    }
    findings
}
pub(super) fn check(root: &Path, meta: &super::Metadata, tighten: bool) -> Vec<String> {
    let config_path = root.join("xtask/module-size.toml");
    let config: Config = match super::read_toml(&config_path) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Always ask Cargo to refresh artifacts: its incremental build reuses an already-built tree.
    let build = super::cargo()
        .args(["build", "--workspace", "--offline"])
        .current_dir(root)
        .status();
    let mut findings = Vec::new();
    let mut scan = Scan::default();
    // A failed build can leave stale dep-info that would pass the backstop; skip it instead.
    let built = build.is_ok_and(|s| s.success());
    if !built {
        findings.push("module-size dep-info unavailable: cargo build --workspace failed; fix the build and retry".into());
        scan.incomplete = true;
    }
    let mut files = Vec::new();
    if let Err(e) = dep_files(&meta.target_directory.join("debug"), &mut files) {
        findings.push(e);
    }
    for pkg in &meta.packages {
        let mut walked = Vec::new();
        for target in pkg.targets.iter().filter(|t| t.walked()) {
            let mut target_scan = Scan::default();
            if let Err(e) = target_scan.file(&target.src_path, target.name.replace('-', "_"), true)
            {
                findings.push(e);
            }
            walked.push((target, target_scan));
        }
        if built {
            let src = super::crate_dir(&pkg.manifest_path).join("src");
            let visited: Vec<_> = walked.iter().map(|(t, s)| (*t, &s.visited)).collect();
            findings.extend(package_dep_check(root, &src, &files, &visited));
        }
        for (_, target_scan) in walked {
            for (key, row) in target_scan.rows {
                if scan.rows.insert(key.clone(), row).is_some() {
                    findings.push(format!("{key}: two targets resolve to the same module key, so one row would hide the other; rename one target"));
                }
            }
            scan.findings.extend(target_scan.findings);
            scan.incomplete |= target_scan.incomplete;
        }
    }
    // Check growth BEFORE tightening so the repair command cannot conceal a new exemption.
    let problems = growth(root, &config);
    scan.findings.splice(0..0, findings);
    settle(&config_path, config, scan, problems, tighten)
}

/// Tightens `module-size.toml` when allowed, then reports. The refusal to tighten over this
/// ratchet's own findings is itself one of those findings, so it blocks exactly when they do:
/// report-only findings leave the file untouched without failing another ratchet's
/// `--tighten-baseline` run (s2w#192), and growth always blocks.
fn settle(
    config_path: &Path,
    mut config: Config,
    scan: Scan,
    mut problems: Vec<String>,
    tighten: bool,
) -> Vec<String> {
    let mut findings = Vec::new();
    let blocked = scan.incomplete || !scan.findings.is_empty();
    if tighten && (blocked || !problems.is_empty()) {
        findings.push("cannot tighten xtask/module-size.toml over an incomplete scan, module-size findings or unauthorized baseline growth; resolve them and retry".into());
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
            .and_then(|s| fs::write(config_path, s).map_err(|e| e.to_string()))
        {
            Ok(()) => {}
            Err(e) => problems.push(format!(
                "cannot tighten {}: {e}; check the file is writable and retry",
                config_path.display()
            )),
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
        assert_eq!(scan.findings.len(), 4, "{:?}", scan.findings);
        assert!(scan.findings[0].contains("explicit #[path] defeats the size check"));
        assert!(scan.findings[1].contains("cfg_attr path bypass"));
        assert!(
            scan.findings[2..]
                .iter()
                .all(|s| s.contains("include! macros are opaque"))
        );
        println!("{}", scan.findings.join("\n"));
    }
    #[test]
    fn include_str_and_include_bytes_are_expressions_not_module_lines() {
        let scratch = Scratch::new();
        let scan = scratch.scan("const A: &str = include_str!(\"x\");\nfn f() -> &'static [u8] { include_bytes!(\"y\") }\n#[cfg(test)] mod tests { const B: &str = std::include_str!(\"z\"); }\n");
        assert!(scan.findings.is_empty(), "{:?}", scan.findings);
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
        for (actual, next_step) in [(500, "--tighten-baseline"), (502, "Baseline-growth")] {
            scan.rows.insert("demo".into(), (600, 100, actual));
            let errors = exemptions(&config, &scan);
            assert!(errors[0].contains(next_step), "{}", errors[0]);
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
    fn tighten_refusal_blocks_only_when_its_own_findings_do() {
        // s2w#192: report-only walker findings keep module-size.toml untouched without failing
        // the run, so another ratchet's --tighten-baseline (the scale baseline) can exit 0.
        let scratch = Scratch::new();
        let path = scratch.write("module-size.toml", "untouched");
        let opaque = "include!(\"opaque.rs\");\nfn f() {}\n";
        let refused = |p: &[String]| p.iter().any(|s| s.contains("cannot tighten"));
        let settled = |enforce, source, growth: Vec<String>| {
            let config = Config {
                enforce,
                ..config()
            };
            settle(&path, config, scratch.scan(source), growth, true)
        };
        assert!(settled(false, opaque, vec![]).is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "untouched");
        assert!(refused(&settled(true, opaque, vec![])));
        let grown = settled(false, "fn f() {}\n", vec!["growth".into()]);
        assert_eq!(grown, ["growth"]);
        assert_eq!(fs::read_to_string(&path).unwrap(), "untouched");
        // Positive control: a clean scan does rewrite the file (dropping the stale exemption).
        assert!(settled(false, "fn f() {}\n", vec![]).is_empty());
        assert!(!fs::read_to_string(&path).unwrap().contains("untouched"));
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
    fn lib_and_bin_of_one_package_are_checked_against_their_union() {
        let scratch = Scratch::new();
        let lib = scratch.write("src/lib.rs", "mod a;");
        let a = scratch.write("src/a.rs", "");
        let main = scratch.write("src/main.rs", "fn main() {}");
        let hidden = scratch.write("src/hidden.rs", "");
        let lib_dep = scratch.write(
            "target/debug/libdemo.d",
            &format!("out: {} {}\n", lib.display(), a.display()),
        );
        let bin_text = format!(
            "out: {} {} {}\n",
            main.display(),
            lib.display(),
            a.display()
        );
        let bin_dep = scratch.write("target/debug/demo.d", &bin_text);
        let target = |kind: &str, src_path: &PathBuf| Target {
            name: "demo".into(),
            kind: vec![kind.into()],
            src_path: src_path.clone(),
        };
        let (lib_target, bin_target) = (target("lib", &lib), target("bin", &main));
        let lib_visited = BTreeSet::from([lib.clone(), a.clone()]);
        let bin_visited = BTreeSet::from([main.clone()]);
        let files = [lib_dep.clone(), bin_dep.clone()];
        let src = scratch.0.join("src");
        // The bin's dep-info matches the lib target by name and lists main.rs.
        assert!(dep_check(&scratch.0, &lib_target, &src, &files, &lib_visited).is_err());
        let both = [(&lib_target, &lib_visited), (&bin_target, &bin_visited)];
        let found = package_dep_check(&scratch.0, &src, &files, &both);
        assert!(found.is_empty(), "{found:?}");
        // A file compiled into the bin that neither target walked still fails.
        scratch.write(
            "target/debug/demo.d",
            &format!("{} {}\n", bin_text.trim_end(), hidden.display()),
        );
        let found = package_dep_check(&scratch.0, &src, &files, &both);
        assert!(
            !found.is_empty() && found.iter().all(|f| f.contains("did not visit")),
            "{found:?}"
        );
        println!("{}", found.join("\n"));
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
