//! Per-module collection over syn ASTs, and name resolution that follows re-exports to the
//! module that defines a name.
use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use crate::module_size::walk::excluded;

pub(super) type Mod = Vec<String>;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Res {
    Module(Mod),
    /// An item and the module that defines it.
    Item(Mod),
    /// Outside this crate, a prelude name, a primitive, a generic parameter or `Self`.
    External,
    Unresolved,
}

pub(super) enum Use {
    Named(Vec<String>, Option<String>),
    Glob(Vec<String>),
}

#[derive(Default)]
pub(super) struct Module {
    pub(super) defs: BTreeMap<String, Res>,
    pub(super) uses: Vec<Use>,
    /// Paths named in code, as segments, with at least one resolvable segment.
    pub(super) paths: Vec<Vec<String>>,
    generics: BTreeSet<String>,
    /// Variant names of each enum defined here, for `use path::Enum::*;`.
    variants: BTreeMap<String, BTreeSet<String>>,
}

// Prelude names, primitives and the standard crates: a first segment matching one is not ours.
const OUTSIDE: &str = "std core alloc proc_macro test Option Some None Result Ok Err Vec String Box ToString \
     ToOwned Clone Copy Default Iterator IntoIterator DoubleEndedIterator ExactSizeIterator \
     Extend From Into TryFrom TryInto FromIterator AsRef AsMut Drop Fn FnMut FnOnce Send Sync \
     Sized Unpin PartialEq PartialOrd Eq Ord Debug Hash bool char str u8 u16 u32 u64 u128 \
     usize i8 i16 i32 i64 i128 isize f32 f64";

fn ident(i: &syn::Ident) -> String {
    i.to_string().trim_start_matches("r#").to_owned()
}

/// Collects one module; inline child modules and file modules (from `asts`) recurse.
pub(super) struct Collector<'a> {
    pub(super) asts: &'a BTreeMap<String, syn::File>,
    pub(super) modules: BTreeMap<Mod, Module>,
    here: Mod,
}

impl<'a> Collector<'a> {
    pub(super) fn run(asts: &'a BTreeMap<String, syn::File>, root: &str) -> BTreeMap<Mod, Module> {
        let mut c = Collector {
            asts,
            modules: BTreeMap::new(),
            here: vec![root.to_owned()],
        };
        if let Some(file) = asts.get(root) {
            c.module(&file.items);
        }
        c.modules
    }
    fn module(&mut self, items: &[syn::Item]) {
        self.modules.entry(self.here.clone()).or_default();
        for item in items {
            self.visit_item(item);
        }
    }
    fn this(&mut self) -> &mut Module {
        self.modules.entry(self.here.clone()).or_default()
    }
    fn def(&mut self, name: &syn::Ident) {
        let res = Res::Item(self.here.clone());
        self.this().defs.entry(ident(name)).or_insert(res);
    }
    fn flatten(&mut self, prefix: &mut Vec<String>, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(ident(&p.ident));
                self.flatten(prefix, &p.tree);
                prefix.pop();
            }
            syn::UseTree::Name(n) if n.ident == "self" => {
                let bind = prefix.last().cloned();
                self.this().uses.push(Use::Named(prefix.clone(), bind));
            }
            syn::UseTree::Name(n) => {
                let mut path = prefix.clone();
                path.push(ident(&n.ident));
                self.this()
                    .uses
                    .push(Use::Named(path, Some(ident(&n.ident))));
            }
            syn::UseTree::Rename(r) => {
                let mut path = prefix.clone();
                path.push(ident(&r.ident));
                let bind = (r.rename != "_").then(|| ident(&r.rename));
                self.this().uses.push(Use::Named(path, bind));
            }
            syn::UseTree::Glob(_) => self.this().uses.push(Use::Glob(prefix.clone())),
            syn::UseTree::Group(g) => g.items.iter().for_each(|t| self.flatten(prefix, t)),
        }
    }
}

macro_rules! skip_tests {
    ($($method:ident: $ty:ident),* $(,)?) => {$ (
        fn $method(&mut self, node: &'ast syn::$ty) {
            if !excluded(&node.attrs) { syn::visit::$method(self, node); }
        }
    )*};
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        use syn::Item as I;
        let (attrs, name) = match item {
            I::Const(i) => (&i.attrs, Some(&i.ident)),
            I::Enum(i) => {
                let names = i.variants.iter().map(|v| ident(&v.ident)).collect();
                self.this().variants.insert(ident(&i.ident), names);
                (&i.attrs, Some(&i.ident))
            }
            I::Fn(i) => (&i.attrs, Some(&i.sig.ident)),
            I::Macro(i) => (&i.attrs, i.ident.as_ref()),
            I::Static(i) => (&i.attrs, Some(&i.ident)),
            I::Struct(i) => (&i.attrs, Some(&i.ident)),
            I::Trait(i) => (&i.attrs, Some(&i.ident)),
            I::TraitAlias(i) => (&i.attrs, Some(&i.ident)),
            I::Type(i) => (&i.attrs, Some(&i.ident)),
            I::Union(i) => (&i.attrs, Some(&i.ident)),
            I::Impl(i) => (&i.attrs, None),
            I::ForeignMod(i) => (&i.attrs, None),
            I::ExternCrate(_) => return,
            I::Mod(m) => return self.child(m),
            I::Use(u) => {
                if !excluded(&u.attrs) {
                    let mut prefix = Vec::new();
                    if u.leading_colon.is_some() {
                        prefix.push(String::new());
                    }
                    self.flatten(&mut prefix, &u.tree);
                }
                return;
            }
            _ => return,
        };
        if excluded(attrs) {
            return;
        }
        if let Some(name) = name {
            self.def(name);
            // `#[macro_export]` places the macro at the crate root.
            if attrs.iter().any(|a| a.path().is_ident("macro_export")) {
                let root = Res::Item(self.here.clone());
                let top = self.modules.entry(self.here[..1].to_vec()).or_default();
                top.defs.entry(ident(name)).or_insert(root);
            }
        }
        syn::visit::visit_item(self, item);
    }
    skip_tests!(visit_impl_item_fn: ImplItemFn, visit_impl_item_const: ImplItemConst,
        visit_impl_item_type: ImplItemType, visit_trait_item_fn: TraitItemFn,
        visit_trait_item_const: TraitItemConst, visit_trait_item_type: TraitItemType);
    // Attribute paths (derive, cfg, lint names) are not module dependencies.
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}
    fn visit_generic_param(&mut self, p: &'ast syn::GenericParam) {
        let name = match p {
            syn::GenericParam::Type(t) => ident(&t.ident),
            syn::GenericParam::Const(c) => ident(&c.ident),
            syn::GenericParam::Lifetime(_) => return,
        };
        self.this().generics.insert(name);
        syn::visit::visit_generic_param(self, p);
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let lead = path.leading_colon.map(|_| String::new());
        let segs = lead
            .into_iter()
            .chain(path.segments.iter().map(|s| ident(&s.ident)));
        let segs: Vec<String> = segs.collect();
        self.this().paths.push(segs);
        syn::visit::visit_path(self, path);
    }
    // Best effort: a macro body that parses as comma-separated expressions (format!, vec!,
    // assert_eq!) has its paths visited. Other bodies are opaque: a documented gap.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.visit_path(&mac.path);
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(args) = mac.parse_body_with(parser) {
            args.iter().for_each(|e| self.visit_expr(e));
        }
    }
}

impl Collector<'_> {
    fn child(&mut self, m: &syn::ItemMod) {
        if excluded(&m.attrs) {
            return;
        }
        let name = ident(&m.ident);
        let mut child = self.here.clone();
        child.push(name.clone());
        self.this().defs.insert(name, Res::Module(child.clone()));
        let outer = std::mem::replace(&mut self.here, child);
        match &m.content {
            Some((_, items)) => self.module(items),
            None => {
                if let Some(file) = self.asts.get(&self.here.join("::")) {
                    self.module(&file.items);
                }
            }
        }
        self.here = outer;
    }
}

/// Resolves paths across one target's modules. `outside` holds extern crate names.
pub(super) struct Resolver<'a> {
    pub(super) modules: &'a BTreeMap<Mod, Module>,
    pub(super) outside: &'a BTreeSet<String>,
    /// Lookups in progress, so glob and re-export loops end at once, not at `DEPTH`.
    pub(super) active: std::cell::RefCell<BTreeSet<(Mod, String)>>,
}

const DEPTH: usize = 32;

impl Resolver<'_> {
    pub(super) fn resolve(&self, m: &Mod, segs: &[String]) -> Res {
        self.path(m, segs, 0)
    }
    fn path(&self, m: &Mod, segs: &[String], depth: usize) -> Res {
        let Some((first, rest)) = segs.split_first() else {
            return Res::Unresolved;
        };
        let mut cur = match first.as_str() {
            "crate" => Res::Module(m[..1].to_vec()),
            "self" => Res::Module(m.clone()),
            "super" if m.len() > 1 => Res::Module(m[..m.len() - 1].to_vec()),
            "super" => return Res::Unresolved,
            "Self" | "" => return Res::External,
            name => match self.lookup(m, name, depth) {
                Some(r) => r,
                None if self.outside.contains(name)
                    || OUTSIDE.split_whitespace().any(|n| n == name) =>
                {
                    return Res::External;
                }
                // A generic parameter, or a name from an extern glob this module sees, directly or
                // through a glob of one of our modules (`use crate::prelude::*;`). Only for the
                // first segment of the path being resolved (depth 0): through a re-export
                // (`crate::m::Foo`) a miss stays loud, and `sees_extern_glob` never re-enters
                // itself through `path`, which would fan out once per glob per level.
                None if self
                    .modules
                    .get(m)
                    .is_some_and(|x| x.generics.contains(name))
                    || (depth == 0 && self.sees_extern_glob(m, 0, &mut BTreeSet::new())) =>
                {
                    return Res::External;
                }
                None => return Res::Unresolved,
            },
        };
        for seg in rest {
            cur = match (&cur, seg.as_str()) {
                (Res::Module(t), "super") if t.len() > 1 => Res::Module(t[..t.len() - 1].to_vec()),
                (Res::Module(t), "self") => Res::Module(t.clone()),
                (Res::Module(t), name) => match self.lookup(t, name, depth) {
                    Some(r) => r,
                    None => return Res::Unresolved,
                },
                _ => return cur,
            };
        }
        cur
    }
    /// The target `name` has in module `m`: local items, then `use` bindings (followed to their
    /// origin), then glob imports. `None` when the module does not name it.
    fn lookup(&self, m: &Mod, name: &str, depth: usize) -> Option<Res> {
        let key = (m.clone(), name.to_owned());
        if depth > DEPTH || !self.active.borrow_mut().insert(key.clone()) {
            return None;
        }
        let found = self.find(m, name, depth);
        self.active.borrow_mut().remove(&key);
        found
    }
    fn find(&self, m: &Mod, name: &str, depth: usize) -> Option<Res> {
        let module = self.modules.get(m)?;
        if let Some(r) = module.defs.get(name) {
            return Some(r.clone());
        }
        for u in &module.uses {
            if let Use::Named(path, Some(bind)) = u
                && bind == name
            {
                return Some(self.path(m, path, depth + 1));
            }
        }
        // Globs of modules and of enums (a variant lives in the enum's module). An extern glob's
        // names are unknown, so it matches nothing here; `path` handles it for first segments.
        module.uses.iter().find_map(|u| {
            let Use::Glob(path) = u else { return None };
            match self.path(m, path, depth + 1) {
                Res::Module(g) if &g != m => self.lookup(&g, name, depth + 1),
                Res::Item(g) => {
                    let variants = self.modules.get(&g)?.variants.get(path.last()?)?;
                    variants.contains(name).then_some(Res::Item(g))
                }
                _ => None,
            }
        })
    }
    /// Whether `m` has a glob of an extern crate's module, directly or through globs of ours.
    fn sees_extern_glob(&self, m: &Mod, depth: usize, seen: &mut BTreeSet<Mod>) -> bool {
        if depth > DEPTH || !seen.insert(m.clone()) {
            return false;
        }
        let Some(module) = self.modules.get(m) else {
            return false;
        };
        module.uses.iter().any(|u| match u {
            Use::Glob(path) => match self.path(m, path, depth + 1) {
                Res::External => true,
                Res::Module(g) => self.sees_extern_glob(&g, depth + 1, seen),
                _ => false,
            },
            Use::Named(..) => false,
        })
    }
}
