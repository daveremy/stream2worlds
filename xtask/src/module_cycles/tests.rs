//! Prove-it-fires: scratch crates with and without module cycles.
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::target_findings;
use crate::module_size::walk::Scan;

/// Writes `files` (path under `src/`, source) and returns the check's findings for `lib.rs`.
fn findings(files: &[(&str, &str)]) -> Vec<String> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "s2w-cycles-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    for (name, text) in files {
        let path = dir.join("src").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
    }
    let mut scan = Scan::default();
    scan.file(&dir.join("src/lib.rs"), "demo".into(), true)
        .unwrap();
    let _ = fs::remove_dir_all(&dir);
    let outside = BTreeSet::from(["serde".to_owned()]);
    target_findings(&scan.asts, "demo", &outside)
}

#[test]
fn fires_on_a_direct_file_cycle() {
    let f = findings(&[
        ("lib.rs", "mod a;\nmod b;\n"),
        ("a.rs", "pub struct A;\nfn f(_: crate::b::B) {}\n"),
        ("b.rs", "use crate::a::A;\npub struct B;\n"),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("module cycle in demo::a, demo::b"), "{f:?}");
    assert!(
        f[0].contains("demo::a uses `crate::b::B`, demo::b uses `crate::a::A`"),
        "{f:?}"
    );
}

#[test]
fn fires_through_a_mod_rs_re_export() {
    // Both children name only their parent `sub`; the re-exports make them siblings that
    // depend on each other. Stopping at the ancestor would drop both edges.
    let f = findings(&[
        ("lib.rs", "mod sub;\n"),
        (
            "sub/mod.rs",
            "mod a;\nmod b;\npub use a::A;\npub use self::b::*;\n",
        ),
        ("sub/a.rs", "pub struct A;\nfn f(_: super::B) {}\n"),
        ("sub/b.rs", "pub struct B;\nuse super::A;\n"),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("demo::sub::a, demo::sub::b"), "{f:?}");
}

#[test]
fn fires_across_subtrees_and_through_a_crate_root_re_export() {
    let f = findings(&[
        (
            "lib.rs",
            "mod a { pub mod x; }\nmod b { pub mod y { pub struct Y; } }\npub use a::x::X;\n",
        ),
        ("a/x.rs", "pub struct X;\nfn f(_: crate::b::y::Y) {}\n"),
        ("b.rs", "unused"),
    ]);
    assert!(f.is_empty(), "one-way is not a cycle: {f:?}");
    let f = findings(&[
        (
            "lib.rs",
            "mod a { pub mod x; }\nmod b { pub mod y { fn g(_: crate::X) {} pub struct Y; } }\npub use a::x::X;\n",
        ),
        ("a/x.rs", "pub struct X;\nfn f(_: crate::b::y::Y) {}\n"),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("demo::a::x, demo::b::y"), "{f:?}");
}

#[test]
fn containment_externals_tests_and_generics_are_not_findings() {
    let f = findings(&[
        (
            "lib.rs",
            "mod err;\npub use err::Error;\nmod child;\nfn top(_: child::C) {}\n",
        ),
        ("err.rs", "pub struct Error;\n"),
        (
            "child.rs",
            "use std::fmt;\nuse serde::Serialize;\nuse super::Error;\nuse self::Kind::*;\n\
             pub struct C;\nenum Kind { K }\n\
             fn f<T: Default>(_: Option<u64>) -> T { let _ = Kind::K; let _ = K; let _ = u64::MAX; T::default() }\n\
             impl C { fn g() -> Self { Self::h(); C } fn h() {} }\n\
             fn m() -> String { format!(\"{}\", crate::err::Error::X) }\n\
             #[cfg(test)]\nmod tests { use crate::nope::Missing; }\n",
        ),
    ]);
    assert!(f.is_empty(), "{f:?}");
    // `child` -> `err` is a sibling edge and one way; `lib` -> `child` is containment.
}

#[test]
fn test_only_cycles_are_ignored_and_unresolvable_paths_are_findings() {
    let f = findings(&[
        ("lib.rs", "mod a;\n#[cfg(test)]\nmod b;\n"),
        (
            "a.rs",
            "#[cfg(test)]\nuse crate::b::B;\nfn f(_: crate::nope::X) {}\n",
        ),
        ("b.rs", "use crate::a::A;\n"),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(
        f[0].contains("demo::a: xtask cannot resolve `crate::nope::X`"),
        "{f:?}"
    );
}

#[test]
fn a_cycle_inside_a_macro_argument_fires() {
    let f = findings(&[
        ("lib.rs", "mod a;\nmod b;\n"),
        ("a.rs", "pub fn a() { println!(\"{}\", crate::b::b()); }\n"),
        ("b.rs", "pub fn b() -> u8 { crate::a::a(); 1 }\n"),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
}

#[test]
fn fires_when_the_cycle_closes_through_a_mod_rs_item() {
    // `x` uses `Foo` from `y/mod.rs`; `y::child` uses `Bar` from `x/mod.rs`. The leaf graph has
    // `x -> y` and `y::child -> x`, no leaf cycle; the subtrees `x` and `y` still depend on
    // each other.
    let f = findings(&[
        ("lib.rs", "mod x;\nmod y;\n"),
        ("x/mod.rs", "pub struct Bar;\nfn f(_: crate::y::Foo) {}\n"),
        ("y/mod.rs", "mod child;\npub struct Foo;\n"),
        ("y/child.rs", "use crate::x::Bar;\n"),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("module cycle in demo::x, demo::y"), "{f:?}");
}

#[test]
fn leading_colons_extern_globs_and_exported_macros_resolve() {
    let f = findings(&[
        ("lib.rs", "mod log;\nmod m;\n"),
        (
            "log.rs",
            "#[macro_export]\nmacro_rules! note { () => {} }\n",
        ),
        (
            "m.rs",
            "use serde::prelude::*;\nuse ::std::fmt;\nfn f() { let _ = ::log::Level::Info; let _ = Tr::x(); crate::note!(); }\n",
        ),
    ]);
    assert!(f.is_empty(), "{f:?}");
}
