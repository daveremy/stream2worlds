//! AST walker: counts non-test lines per module and refuses opaque layout.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::Span;
use syn::{Attribute, Meta, Token, punctuated::Punctuated, spanned::Spanned, visit::Visit};

#[derive(Default)]
pub(crate) struct Scan {
    pub(super) rows: BTreeMap<String, (usize, usize, usize)>, // wc -l, excluded test lines, non-test
    pub(super) visited: BTreeSet<PathBuf>,
    pub(crate) findings: Vec<String>,
    pub(super) incomplete: bool,
    /// Each file module's parsed AST under its module key (read by `module_cycles`).
    pub(crate) asts: BTreeMap<String, syn::File>,
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
pub(super) fn test_only(meta: &Meta) -> bool {
    meta.path().is_ident("test")
        || (meta.path().is_ident("all") && arms(meta).iter().any(test_only))
        || (meta.path().is_ident("any") && arms(meta).iter().all(test_only))
}
pub(crate) fn excluded(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg") && a.parse_args::<Meta>().is_ok_and(|m| test_only(&m)))
    })
}
fn path_attr(meta: &Meta) -> bool {
    meta.path().is_ident("path")
        || (meta.path().is_ident("cfg_attr") && arms(meta).iter().skip(1).any(path_attr))
}
pub(super) fn lines(span: Span) -> std::ops::RangeInclusive<usize> {
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
            self.findings.push(format!(
                "{}: {class} defeats the size check; remove it and use standard module layout",
                self.key
            ));
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
    #[expect(
        clippy::too_many_lines,
        reason = "one file scan: parse, cfg-test exclusion and module descent"
    )]
    pub(crate) fn file(&mut self, path: &Path, key: String, root: bool) -> Result<(), String> {
        let canonical = fs::canonicalize(path)
            .map_err(|e| format!("{}: {e}; restore the module file", path.display()))?;
        // Resolve each module identity, even when multiple targets share the same source file.
        self.visited.insert(canonical);
        let source = fs::read_to_string(path).map_err(|e| {
            format!(
                "{}: {e}; make the module file readable UTF-8",
                path.display()
            )
        })?;
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
        self.asts.insert(key.clone(), ast);
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
