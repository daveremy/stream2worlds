//! Renders one crate's public items as sorted, body-free `<module>: <tokens>` lines.
use std::collections::{BTreeMap, BTreeSet};

use quote::ToTokens;
use syn::{
    Attribute, Expr, Fields, ForeignItem, ImplItem, Item, TraitItem, Type, Visibility, parse_quote,
};

use crate::module_size::walk::excluded;

/// Every public item of the crate whose file modules are `asts` (keyed by module path).
pub(super) fn crate_lines(asts: &BTreeMap<String, syn::File>) -> BTreeSet<String> {
    let mut names = Names::default();
    for file in asts.values() {
        names.collect(&file.items);
    }
    let private: BTreeSet<String> = names.private.difference(&names.public).cloned().collect();
    let mut out = BTreeSet::new();
    for (module, file) in asts {
        if !excluded(&file.attrs) {
            items(module, &file.items, &private, &mut out);
        }
    }
    out
}

/// Type names defined in the crate, split by visibility, for the trait-impl filter.
#[derive(Default)]
struct Names {
    public: BTreeSet<String>,
    private: BTreeSet<String>,
}
impl Names {
    fn collect(&mut self, items: &[Item]) {
        for item in items {
            let (attrs, vis, ident) = match item {
                Item::Struct(i) => (&i.attrs, &i.vis, &i.ident),
                Item::Enum(i) => (&i.attrs, &i.vis, &i.ident),
                Item::Union(i) => (&i.attrs, &i.vis, &i.ident),
                Item::Type(i) => (&i.attrs, &i.vis, &i.ident),
                Item::Mod(m) if !excluded(&m.attrs) => {
                    if let Some((_, inner)) = &m.content {
                        self.collect(inner);
                    }
                    continue;
                }
                _ => continue,
            };
            if excluded(attrs) {
                continue;
            }
            let set = if public(vis) {
                &mut self.public
            } else {
                &mut self.private
            };
            set.insert(ident.to_string());
        }
    }
}

fn public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

fn tok(node: &impl ToTokens) -> String {
    node.to_token_stream().to_string()
}

/// Keeps only the attributes that change what a caller can do; docs and lint levels churn.
fn keep_attrs(attrs: &mut Vec<Attribute>) {
    attrs.retain(|a| {
        let p = a.path();
        [
            "derive",
            "non_exhaustive",
            "repr",
            "cfg",
            "cfg_attr",
            "must_use",
        ]
        .iter()
        .any(|k| p.is_ident(k))
            || (p.is_ident("doc")
                && a.parse_args::<syn::Path>()
                    .is_ok_and(|x| x.is_ident("hidden")))
    });
}

fn attrs_text(attrs: &[Attribute]) -> String {
    let mut kept = attrs.to_vec();
    keep_attrs(&mut kept);
    kept.iter().map(|a| format!("{} ", tok(a))).collect()
}

/// The value placeholder: a const's value is behaviour, not signature.
fn hole() -> Expr {
    parse_quote!(_)
}

/// Strips private fields (named: dropped plus a `..` marker; tuple: `_` by position).
fn fields(f: &mut Fields) -> bool {
    let mut hidden = false;
    match f {
        Fields::Named(named) => {
            let kept = named
                .named
                .iter()
                .filter(|x| public(&x.vis) && !excluded(&x.attrs))
                .cloned()
                .collect::<Vec<_>>();
            hidden = kept.len() != named.named.len();
            named.named = kept.into_iter().collect();
        }
        Fields::Unnamed(unnamed) => {
            for x in &mut unnamed.unnamed {
                if !public(&x.vis) {
                    x.ty = Type::Infer(parse_quote!(_));
                    x.attrs.clear();
                }
            }
        }
        Fields::Unit => {}
    }
    for x in f.iter_mut() {
        keep_attrs(&mut x.attrs);
    }
    hidden
}

fn items(module: &str, list: &[Item], private: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    for item in list {
        if let Some(line) = item_line(module, item, private, out) {
            out.insert(format!("{module}: {line}"));
        }
    }
}

fn item_attrs(item: &mut Item) -> Option<&mut Vec<Attribute>> {
    Some(match item {
        Item::Const(i) => &mut i.attrs,
        Item::Enum(i) => &mut i.attrs,
        Item::ExternCrate(i) => &mut i.attrs,
        Item::Fn(i) => &mut i.attrs,
        Item::ForeignMod(i) => &mut i.attrs,
        Item::Impl(i) => &mut i.attrs,
        Item::Macro(i) => &mut i.attrs,
        Item::Mod(i) => &mut i.attrs,
        Item::Static(i) => &mut i.attrs,
        Item::Struct(i) => &mut i.attrs,
        Item::Trait(i) => &mut i.attrs,
        Item::TraitAlias(i) => &mut i.attrs,
        Item::Type(i) => &mut i.attrs,
        Item::Union(i) => &mut i.attrs,
        Item::Use(i) => &mut i.attrs,
        _ => return None,
    })
}

/// The line for one item, if it is public API; recurses into inline modules and impl blocks.
fn item_line(
    module: &str,
    item: &Item,
    private: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) -> Option<String> {
    let mut item = item.clone();
    let attrs = item_attrs(&mut item)?;
    if excluded(attrs) {
        return None;
    }
    let exported = attrs.iter().any(|a| a.path().is_ident("macro_export"));
    keep_attrs(attrs);
    match item {
        Item::Mod(m) => mod_line(module, &m, private, out),
        Item::ForeignMod(f) => {
            foreign_lines(module, f, out);
            None
        }
        Item::Impl(i) => {
            impl_lines(module, i, private, out);
            None
        }
        Item::Macro(m) if exported => m.ident.map(|i| format!("macro {i}!")),
        Item::Type(t) if !public(&t.vis) => Some(format!("(private alias) {}", tok(&t))),
        other => public_item(other),
    }
}

/// A `pub` item that is not a container: its tokens with bodies and values stripped.
fn public_item(item: Item) -> Option<String> {
    match item {
        Item::Fn(f) if public(&f.vis) => {
            Some(format!("{}pub {}", attrs_text(&f.attrs), tok(&f.sig)))
        }
        Item::Struct(mut s) if public(&s.vis) => {
            let hidden = fields(&mut s.fields);
            Some(format!("{}{}", tok(&s), if hidden { " .." } else { "" }))
        }
        Item::Union(u) if public(&u.vis) => Some(union_line(u)),
        Item::Enum(e) if public(&e.vis) => Some(enum_line(e)),
        Item::Trait(t) if public(&t.vis) => Some(trait_line(t)),
        Item::TraitAlias(t) if public(&t.vis) => Some(tok(&t)),
        Item::Type(t) if public(&t.vis) => Some(tok(&t)),
        Item::Const(mut c) if public(&c.vis) => {
            *c.expr = hole();
            Some(tok(&c))
        }
        Item::Static(mut s) if public(&s.vis) => {
            *s.expr = hole();
            Some(tok(&s))
        }
        Item::Use(u) if public(&u.vis) => Some(tok(&u)),
        Item::ExternCrate(e) if public(&e.vis) => Some(tok(&e)),
        _ => None,
    }
}

fn union_line(mut u: syn::ItemUnion) -> String {
    let mut f = Fields::Named(u.fields.clone());
    let hidden = fields(&mut f);
    if let Fields::Named(n) = f {
        u.fields = n;
    }
    format!("{}{}", tok(&u), if hidden { " .." } else { "" })
}

fn enum_line(mut e: syn::ItemEnum) -> String {
    e.variants = e
        .variants
        .into_iter()
        .filter(|v| !excluded(&v.attrs))
        .collect();
    for v in &mut e.variants {
        keep_attrs(&mut v.attrs);
        for x in v.fields.iter_mut() {
            keep_attrs(&mut x.attrs);
        }
    }
    tok(&e)
}

fn trait_line(mut t: syn::ItemTrait) -> String {
    t.items.retain(|i| !excluded(trait_item_attrs(i)));
    for i in &mut t.items {
        match i {
            TraitItem::Fn(f) => {
                f.default = None;
                f.semi_token = Some(parse_quote!(;));
                keep_attrs(&mut f.attrs);
            }
            TraitItem::Const(c) => {
                c.default = None;
                keep_attrs(&mut c.attrs);
            }
            TraitItem::Type(ty) => keep_attrs(&mut ty.attrs),
            _ => {}
        }
    }
    tok(&t)
}

fn mod_line(
    module: &str,
    m: &syn::ItemMod,
    private: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) -> Option<String> {
    if let Some((_, inner)) = &m.content {
        items(&format!("{module}::{}", m.ident), inner, private, out);
    }
    public(&m.vis).then(|| format!("{}pub mod {}", attrs_text(&m.attrs), m.ident))
}

/// `pub fn` and `pub static` declared in an `extern` block, one line each.
fn foreign_lines(module: &str, f: syn::ItemForeignMod, out: &mut BTreeSet<String>) {
    for fi in f.items {
        let line = match fi {
            ForeignItem::Fn(x) if public(&x.vis) && !excluded(&x.attrs) => {
                format!("{}pub {}", attrs_text(&x.attrs), tok(&x.sig))
            }
            ForeignItem::Static(mut x) if public(&x.vis) && !excluded(&x.attrs) => {
                keep_attrs(&mut x.attrs);
                tok(&x)
            }
            _ => continue,
        };
        out.insert(format!("{module}: extern {} {{ {line} }}", tok(&f.abi)));
    }
}

fn trait_item_attrs(i: &TraitItem) -> &[Attribute] {
    match i {
        TraitItem::Fn(x) => &x.attrs,
        TraitItem::Const(x) => &x.attrs,
        TraitItem::Type(x) => &x.attrs,
        TraitItem::Macro(x) => &x.attrs,
        _ => &[],
    }
}

/// Inherent impls: each `pub` associated item. Trait impls: the header, plus associated types
/// and consts (fixed by the trait otherwise), unless the self type is a private local type.
fn impl_lines(
    module: &str,
    mut i: syn::ItemImpl,
    private: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    let trait_impl = i.trait_.is_some();
    if trait_impl && private_self(&i.self_ty, private) {
        return;
    }
    let body = std::mem::take(&mut i.items);
    let header = tok(&i);
    let header = header.trim_end_matches("{ }").trim_end();
    if trait_impl {
        out.insert(format!("{module}: {header}"));
    }
    for mut item in body {
        let line = match &mut item {
            ImplItem::Fn(f) if !trait_impl && public(&f.vis) && !excluded(&f.attrs) => {
                format!("{}pub {}", attrs_text(&f.attrs), tok(&f.sig))
            }
            ImplItem::Const(c) if (trait_impl || public(&c.vis)) && !excluded(&c.attrs) => {
                keep_attrs(&mut c.attrs);
                c.expr = hole();
                tok(c)
            }
            ImplItem::Type(t) if (trait_impl || public(&t.vis)) && !excluded(&t.attrs) => {
                keep_attrs(&mut t.attrs);
                tok(t)
            }
            _ => continue,
        };
        out.insert(format!("{module}: {header} {{ {line} }}"));
    }
}

/// A bare single-segment path naming a type that is only ever defined non-`pub` here.
fn private_self(ty: &Type, private: &BTreeSet<String>) -> bool {
    match ty {
        Type::Path(p) if p.qself.is_none() => p
            .path
            .get_ident()
            .is_some_and(|id| private.contains(&id.to_string())),
        _ => false,
    }
}
