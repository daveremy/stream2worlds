use std::collections::{BTreeMap, BTreeSet};

use super::{compare, render::crate_lines};

fn api(src: &str) -> BTreeSet<String> {
    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("sample does not parse: {e}"));
    crate_lines(&BTreeMap::from([("k".to_owned(), file)]))
}

fn differs(a: &str, b: &str) -> bool {
    api(a) != api(b)
}

#[test]
fn a_pub_fn_signature_change_fires() {
    assert!(differs(
        "pub fn f(x: u32) -> u32 { x }",
        "pub fn f(x: u64) -> u32 { 0 }"
    ));
    assert_eq!(
        api("pub fn f(x: u32) -> u32 { x }"),
        BTreeSet::from(["k: pub fn f (x : u32) -> u32".to_owned()])
    );
}

#[test]
fn restricted_private_and_test_items_are_not_recorded() {
    let src = "pub(crate) fn a() {} fn b() {} #[cfg(test)] mod tests { pub fn c() {} }
        mod inner { pub(super) fn d() {} #[test] pub fn e() {} }";
    assert!(api(src).is_empty(), "{:?}", api(src));
}

#[test]
fn bodies_docs_and_order_do_not_churn() {
    let base = "/// one\npub fn f() -> u8 { 1 }\npub struct S;";
    assert!(!differs(
        base,
        "/// two\npub fn f() -> u8 { 2 }\npub struct S;"
    ));
    assert!(!differs(base, "pub struct S;\npub fn f() -> u8 { 3 }"));
}

#[test]
fn struct_fields_and_derives_are_api() {
    let base = "#[derive(Clone)] pub struct S { pub a: u8 }";
    assert!(differs(
        base,
        "#[derive(Clone)] pub struct S { pub a: u8, b: u8 }"
    ));
    assert!(differs(
        base,
        "#[derive(Clone)] pub struct S { pub a: u8, pub b: u8 }"
    ));
    assert!(differs(base, "pub struct S { pub a: u8 }"));
    assert!(
        api("pub struct S { pub a: u8, b: u8 }")
            .iter()
            .any(|l| l.ends_with(" .."))
    );
}

#[test]
fn trait_impls_on_foreign_or_public_types_are_recorded() {
    let src = "pub struct P; struct Q;
        impl From<P> for String { fn from(_: P) -> String { String::new() } }
        impl Clone for Q { fn clone(&self) -> Q { Q } }";
    let lines = api(src);
    assert!(lines.contains("k: impl From < P > for String"), "{lines:?}");
    assert!(!lines.iter().any(|l| l.contains("for Q")), "{lines:?}");
    assert!(differs(src, "pub struct P; struct Q;"));
}

#[test]
fn trait_impl_associated_types_are_recorded() {
    let a = "pub struct P; impl TryFrom<u8> for P { type Error = u8; fn try_from(v: u8) -> Result<P, u8> { Err(v) } }";
    assert!(differs(
        a,
        &a.replace("type Error = u8", "type Error = u16")
    ));
}

#[test]
fn inherent_methods_and_private_aliases_are_recorded() {
    assert!(differs(
        "pub struct P; impl P { pub fn m(&self) {} }",
        "pub struct P; impl P { fn m(&self) {} }"
    ));
    assert!(differs(
        "type A = u32; pub fn f(_: A) {}",
        "type A = u64; pub fn f(_: A) {}"
    ));
}

#[test]
fn compare_names_the_next_action() {
    let snap = |s: &str| BTreeMap::from([("c".to_owned(), s.to_owned())]);
    assert!(compare(&snap("x\n"), &snap("x\n")).is_empty());
    let missing = compare(&BTreeMap::new(), &snap("x\n"));
    assert!(missing[0].contains("no public API snapshot") && missing[0].contains("--update"));
    let changed = compare(&snap("x\n"), &snap("y\n"));
    assert!(
        changed[0].contains("+ y") && changed[0].contains("- x") && changed[0].contains("--update")
    );
    let stale = compare(
        &BTreeMap::from([("gone".to_owned(), String::new())]),
        &snap("x\n"),
    );
    assert!(
        stale
            .iter()
            .any(|p| p.contains("gone.txt") && p.contains("delete"))
    );
}

#[test]
fn trait_default_methods_and_deprecation_are_api() {
    assert!(differs(
        "pub trait T { fn f(&self) {} }",
        "pub trait T { fn f(&self); }"
    ));
    assert!(!differs(
        "pub trait T { fn f(&self) { let _ = 1; } }",
        "pub trait T { fn f(&self) {} }"
    ));
    assert!(differs("pub fn f() {}", "#[deprecated] pub fn f() {}"));
}
