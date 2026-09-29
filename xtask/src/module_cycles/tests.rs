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

#[test]
fn a_deeper_cycle_does_not_hide_a_wider_shallow_one() {
    // `a::x <-> b::y` at depth 3 and `a <-> c` at depth 2; at depth 2 `a`, `b`, `c` form one
    // component, which must be reported, not dropped as covered by the deeper cycle.
    let f = findings(&[
        (
            "lib.rs",
            "mod a { pub mod x; pub struct A; fn g(_: crate::c::C) {} }\nmod b { pub mod y; }\nmod c;\n",
        ),
        ("a/x.rs", "pub struct X;\nfn f(_: crate::b::y::Y) {}\n"),
        ("b/y.rs", "pub struct Y;\nfn f(_: crate::a::x::X) {}\n"),
        ("c.rs", "pub struct C;\nfn f(_: crate::a::A) {}\n"),
    ]);
    assert!(
        f.iter()
            .any(|l| l.contains("module cycle in demo::a, demo::b, demo::c:")),
        "{f:?}"
    );
    assert!(
        f.iter()
            .any(|l| l.contains("module cycle in demo::a::x, demo::b::y:")),
        "{f:?}"
    );
}

#[test]
fn an_enum_variant_glob_re_export_resolves_to_the_enum_module() {
    // `pub use self::Kind::*;` flattens variants; `use crate::k::K` must resolve (not be an
    // unresolvable-path finding) and carry the `m -> k` edge that closes this cycle.
    let f = findings(&[
        ("lib.rs", "mod k;\nmod m;\n"),
        (
            "k.rs",
            "pub enum Kind { K }\npub use self::Kind::*;\nfn f(_: crate::m::M) {}\n",
        ),
        (
            "m.rs",
            "use crate::k::K;\npub struct M;\nfn g() { let _ = K; }\n",
        ),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("module cycle in demo::k, demo::m"), "{f:?}");
}

#[test]
fn an_extern_glob_through_an_internal_prelude_is_not_a_finding() {
    let f = findings(&[
        ("lib.rs", "mod prelude;\nmod m;\n"),
        ("prelude.rs", "pub use std::fmt::*;\n"),
        (
            "m.rs",
            "use crate::prelude::*;\nfn f(x: &u8, out: &mut Formatter) { let _ = Display::fmt(x, out); }\n",
        ),
    ]);
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn an_enum_glob_matches_only_its_variants() {
    // `m` re-exports `Kind`'s variants: `crate::m::K` resolves, `crate::m::Nope` does not. An
    // enum glob that matched any name would silently resolve `Nope` to `other`.
    let f = findings(&[
        (
            "lib.rs",
            "mod other;
mod m;
mod z;
",
        ),
        (
            "other.rs",
            "pub enum Kind { K }
",
        ),
        (
            "m.rs",
            "pub use crate::other::Kind::*;
",
        ),
        (
            "z.rs",
            "fn g() { let _ = crate::m::K; let _ = crate::m::Nope; }
",
        ),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(
        f[0].contains("demo::z: xtask cannot resolve `crate::m::Nope`"),
        "{f:?}"
    );
}

#[test]
fn mutual_globs_are_a_cycle() {
    let f = findings(&[
        ("lib.rs", "mod a;\nmod b;\n"),
        ("a.rs", "pub use crate::b::*;\npub struct A;\n"),
        ("b.rs", "pub use crate::a::*;\npub struct B;\n"),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("module cycle in demo::a, demo::b"), "{f:?}");
}

#[test]
fn a_miss_through_a_module_with_an_extern_glob_stays_loud() {
    // `Foo` is macro-generated, so invisible; `m`'s `std::fmt::*` glob must not absorb it.
    let f = findings(&[
        ("lib.rs", "mod m;\nmod n;\n"),
        (
            "m.rs",
            "use std::fmt::*;\nmacro_rules! def { () => { pub struct Foo; } }\ndef!();\n",
        ),
        ("n.rs", "use crate::m::Foo;\n"),
    ]);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(
        f[0].contains("demo::n: xtask cannot resolve `crate::m::Foo`"),
        "{f:?}"
    );
}
