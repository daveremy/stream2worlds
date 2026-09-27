//! Structural module sizes; report findings separately from the blocking baseline ratchet.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use proc_macro2::Span;
use serde::{Deserialize, Serialize};
use syn::{Attribute, Meta, Token, punctuated::Punctuated, spanned::Spanned, visit::Visit};

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
#[derive(Default)]
struct Scan {
    rows: BTreeMap<String, (usize, usize, usize)>, // wc -l, excluded test lines, non-test
    visited: BTreeSet<PathBuf>,
    findings: Vec<String>,
    incomplete: bool,
}
fn arms(meta: &Meta) -> Vec<Meta> {
    match meta {
        Meta::List(list) => list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .map(|args| args.into_iter().collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
fn test_only(meta: &Meta) -> bool {
    meta.path().is_ident("test")
        || (meta.path().is_ident("all") && arms(meta).iter().any(test_only))
        || (meta.path().is_ident("any") && arms(meta).iter().all(test_only))
}
fn excluded(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg") && a.parse_args::<Meta>().is_ok_and(|m| test_only(&m)))
    })
}
fn path_attr(meta: &Meta) -> bool {
    meta.path().is_ident("path")
        || (meta.path().is_ident("cfg_attr") && arms(meta).iter().skip(1).any(path_attr))
}
fn lines(span: Span) -> std::ops::RangeInclusive<usize> {
    span.start().line..=span.end().line
}
struct Walker {
    dir: PathBuf,
    key: String,
    hidden: bool,
    counted: BTreeSet<usize>,
    tests: BTreeSet<usize>,
    children: Vec<(PathBuf, String)>,
    findings: Vec<String>,
}
// Visit every item kind (including associated/foreign items and items inside blocks).
// Union spans within each top-level item, then subtract test spans: no nesting double count.
macro_rules! visit_items {
    ($($method:ident: $ty:ident),* $(,)?) => {$ (
        fn $method(&mut self, node: &'ast syn::$ty) {
            let hidden = self.hidden;
            self.hidden |= excluded(&node.attrs);
            if self.hidden { self.tests.extend(lines(node.span())); }
            else { self.counted.extend(lines(node.span())); }
            syn::visit::$method(self, node);
            self.hidden = hidden;
        }
    )*};
}
impl<'ast> Visit<'ast> for Walker {
    visit_items!(visit_item_const: ItemConst, visit_item_enum: ItemEnum,
        visit_item_extern_crate: ItemExternCrate, visit_item_fn: ItemFn,
        visit_item_foreign_mod: ItemForeignMod, visit_item_impl: ItemImpl,
        visit_item_macro: ItemMacro, visit_item_static: ItemStatic,
        visit_item_struct: ItemStruct, visit_item_trait: ItemTrait,
        visit_item_trait_alias: ItemTraitAlias, visit_item_type: ItemType,
        visit_item_union: ItemUnion, visit_item_use: ItemUse,
        visit_impl_item_const: ImplItemConst, visit_impl_item_fn: ImplItemFn,
        visit_impl_item_type: ImplItemType, visit_impl_item_macro: ImplItemMacro,
        visit_trait_item_const: TraitItemConst, visit_trait_item_fn: TraitItemFn,
        visit_trait_item_type: TraitItemType, visit_trait_item_macro: TraitItemMacro,
        visit_foreign_item_fn: ForeignItemFn, visit_foreign_item_static: ForeignItemStatic,
        visit_foreign_item_type: ForeignItemType, visit_foreign_item_macro: ForeignItemMacro);
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let hidden = self.hidden;
        self.hidden |= excluded(&node.attrs);
        if self.hidden {
            self.tests.extend(lines(node.span()));
        } else {
            self.counted.extend(lines(node.span()));
        }
        let name = node.ident.to_string().trim_start_matches("r#").to_owned();
        let dir = self.dir.clone();
        let key = self.key.clone();
        self.key = format!("{key}::{name}");
        if node.content.is_some() {
            self.dir.push(&name);
        } else if !self.hidden && !node.attrs.iter().any(|a| path_attr(&a.meta)) {
            let flat = dir.join(format!("{name}.rs"));
            let nested = dir.join(&name).join("mod.rs");
            match (flat.is_file(), nested.is_file()) {
                (true, false) => self.children.push((flat, self.key.clone())),
                (false, true) => self.children.push((nested, self.key.clone())),
                (true, true) => self.findings.push(format!(
                    "{}: ambiguous module path — resolves to two files; keep only one",
                    self.key
                )),
                _ => self.findings.push(format!(
                    "{}: module content xtask cannot see — move it into a normal module file",
                    self.key
                )),
            }
        }
        syn::visit::visit_item_mod(self, node);
        self.dir = dir;
        self.key = key;
        self.hidden = hidden;
    }
    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        if path_attr(&attr.meta) {
            let class = if attr.path().is_ident("cfg_attr") {
                "cfg_attr path bypass"
            } else {
                "explicit #[path]"
            };
            self.findings.push(format!("{}: {class} defeats the size check; remove it and use standard module layout, or if the file genuinely needs an unusual location, list it in xtask/module-size.toml with a reason", self.key));
        }
        syn::visit::visit_attribute(self, attr);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.segments.last().is_some_and(|s| {
            matches!(
                s.ident.to_string().as_str(),
                "include" | "include_str" | "include_bytes"
            )
        }) {
            self.findings.push(format!("{}: include! macros are opaque to the size check; inline the content as real module structure", self.key));
        }
        syn::visit::visit_macro(self, mac);
    }
}
impl Scan {
    fn file(&mut self, path: &Path, key: String, root: bool) -> Result<(), String> {
        let canonical = fs::canonicalize(path)
            .map_err(|e| format!("{}: {e}; restore the module file", path.display()))?;
        // Resolve each module identity, even when multiple targets share the same source file.
        self.visited.insert(canonical);
        let source = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let ast = syn::parse_file(&source).map_err(|e| {
            format!(
                "{}: {e}; use parseable Rust module structure",
                path.display()
            )
        })?;
        let mut dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        if !root && path.file_name().is_some_and(|n| n != "mod.rs") {
            dir.push(path.file_stem().unwrap_or_default());
        }
        let mut walk = Walker {
            dir,
            key: key.clone(),
            hidden: excluded(&ast.attrs),
            counted: BTreeSet::new(),
            tests: BTreeSet::new(),
            children: Vec::new(),
            findings: Vec::new(),
        };
        let mut non_test = 0;
        let mut tests = 0;
        for attr in &ast.attrs {
            if walk.hidden {
                tests += lines(attr.span()).count();
            } else {
                non_test += lines(attr.span()).count();
            }
            walk.visit_attribute(attr);
        }
        for item in &ast.items {
            walk.counted.clear();
            walk.tests.clear();
            walk.visit_item(item);
            non_test += walk.counted.difference(&walk.tests).count();
            tests += walk.tests.len();
        }
        self.rows.insert(
            key,
            (
                source.bytes().filter(|b| *b == b'\n').count(),
                tests,
                non_test,
            ),
        );
        self.findings.extend(walk.findings);
        for (child, name) in walk.children {
            if let Err(e) = self.file(&child, name, false) {
                self.findings.push(e);
                self.incomplete = true;
            }
        }
        Ok(())
    }
}
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
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
fn trailer(messages: &str) -> bool {
    messages.lines().any(|line| {
        line.strip_prefix("Baseline-growth: s2w#")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}
fn growth(root: &Path, config: &Config) -> Vec<String> {
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
// Make dep-info is text, not Rust source: unfold continuations and decode escaped spaces.
fn dep_paths(text: &str) -> Vec<PathBuf> {
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
fn dep_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| {
        format!(
            "{}: {e}; build the workspace to produce dep-info",
            dir.display()
        )
    })? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            dep_files(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "d") {
            files.push(path);
        }
    }
    Ok(())
}
fn dep_check(
    root: &Path,
    target: &Target,
    src: &Path,
    files: &[PathBuf],
    visited: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let name = target.name.replace('-', "_");
    let mut found = false;
    for file in files {
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        if stem != target.name && stem != name && stem != format!("lib{name}") {
            continue;
        }
        let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let deps: BTreeSet<_> = dep_paths(&text)
            .iter()
            .map(|p| root.join(p))
            .map(|p| fs::canonicalize(&p).unwrap_or(p))
            .collect();
        if !deps.contains(&target.src_path) {
            continue;
        }
        found = true;
        for path in deps {
            if path.starts_with(src)
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
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
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
