//! Check 9, domain vocabulary: no shipped source names the retired domain (decision 0018, no
//! compiled domain code — domain knowledge is data in the log, and code knows protocols and
//! formats, never what a stream is about).
//!
//! The denylist is data, `xtask/vocabulary-denylist.txt` read at run time, never a `const` in
//! this file: the check scans its own source like any other, and a baked-in list would match
//! itself. Rust is read as `syn` ASTs, never as text; the web view's TypeScript is read a line
//! at a time, because no TypeScript parser is a dependency here. Committed prompt files (every
//! file under a crate's `prompts/` tree) are read the same way, but the per-line opt-out below
//! does not reach them: every word in a prompt is sent to a model.
//!
//! An entry matches when its tokens appear as a contiguous run of the candidate's tokens, so an
//! entry written `x_y` catches `xY`, `x-y`, `x.y` and the literal `"x_y"`, but not `x_other_y`.
//! Test code is exempt (`#[test]`, `#[cfg(test)]`, and files that exist only as a test-gated
//! `mod x;`), and a line carrying `// vocabulary: allow` is exempt whole — preset data, or a
//! word in its ordinary engineering sense, gets a per-line opt-out rather than a rename.
//!
//! A known, accepted gap: tokenizing splits only on non-alphanumeric characters and a
//! lowercase→uppercase boundary, so an all-lowercase compound with no separator (a made-up
//! two-letter country prefix glued directly onto the retired encyclopedia's name, no
//! underscore or case change) never tokenizes down to a standalone entry — the same design
//! that keeps a near-miss like `x_other_y` from false-positiving on `x_y` (see the contiguity
//! test below) also means a fused compound needs its own denylist entry to be caught. This
//! check trades recall for precision on that one shape; it is not a vocabulary-scan bypass an
//! attacker gains anything from, since the obfuscation replay (`obfuscation.rs`) covers this
//! case for `s2w-core`'s fold and, since s2w#131, `s2w-system1`'s claim-reading engines
//! (`JsonClaimsEngine`), and the raw obfuscation replay (`obfuscation_raw.rs`, check 11)
//! covers the mapping engine (`MappingEngine`) over a recorded raw stream: they fail on any
//! fold or engine that keys on a specific string, spelled however. That coverage does not
//! extend to the bridge registry or `s2w-app` — a fused compound there needs its own denylist
//! entry to be caught.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{Attribute, Item, Meta, Token};

/// Where the denylist lives: a data file, so this crate's own source carries no terms.
const DENYLIST: &str = "xtask/vocabulary-denylist.txt";
/// The per-line opt-out, in the shape of the repo's `secret-scan: ignore` convention.
const ALLOW: &str = "vocabulary: allow";
/// Every violation says what to do next (xtask/AGENTS.md).
const NEXT_STEP: &str = ". Rename it to the protocol or format's own terms, or end that line with '// vocabulary: allow' if it is preset data or ordinary engineering usage";

pub(crate) fn check(root: &Path) -> Vec<String> {
    let entries = match fs::read_to_string(root.join(DENYLIST)) {
        Ok(text) => entries(&text),
        Err(e) => return vec![format!("{DENYLIST}: {e}; restore the denylist data file")],
    };
    // Fail closed: a hollow or unparsable denylist would pass everything.
    let mut problems: Vec<String> = entries
        .iter()
        .filter(|e| e.tokens.is_empty())
        .map(|e| {
            format!(
                "{DENYLIST}: entry '{}' has no words in it; write one term per line",
                e.term
            )
        })
        .collect();
    if entries.is_empty() {
        problems.push(format!(
            "{DENYLIST}: no entries. An empty denylist passes everything; list the retired domain's terms, one per line."
        ));
        return problems;
    }
    problems.extend(rust_problems(root, &entries));
    problems.extend(ts_problems(root, &entries));
    problems.extend(prompt_problems(root, &entries));
    problems
}

// ---------- the denylist, as data ----------

/// One denylist row: the term exactly as the data file writes it, and its tokens.
struct Entry {
    term: String,
    tokens: Vec<String>,
}

/// The denylist: one term per line, blank lines skipped.
fn entries(text: &str) -> Vec<Entry> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|term| Entry {
            term: term.to_owned(),
            tokens: tokenize(term),
        })
        .collect()
}

// ---------- tokens and matching ----------

/// Splits on every non-alphanumeric and on each lowercase→uppercase boundary, lowercased:
/// `x_y`, `xY` and `x-y` all tokenize to `["x", "y"]`, so one entry covers every spelling.
fn tokenize(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut lower = false;
    for ch in s.chars() {
        if !ch.is_alphanumeric() {
            finish(&mut tokens, &mut current);
            lower = false;
            continue;
        }
        if lower && ch.is_uppercase() {
            finish(&mut tokens, &mut current);
        }
        current.push(ch);
        lower = ch.is_lowercase();
    }
    finish(&mut tokens, &mut current);
    tokens
}

fn finish(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
        current.clear();
    }
}

/// Whether `needle`'s tokens run contiguously through `haystack`'s: `["x", "y"]` matches
/// `["x", "y"]` and `["a", "x", "y"]`, but not `["x", "other", "y"]`.
fn contiguous(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The problem for one match: the entry as the data file writes it, plus enough context to act.
fn problem(rel: &Path, line: usize, entry: &Entry, context: &str) -> String {
    format!(
        "{}:{line}: matched denylist entry '{}' in '{context}'{NEXT_STEP}",
        rel.display(),
        entry.term
    )
}

/// The path as the repo sees it, for problem strings.
fn rel<'a>(root: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

// ---------- Rust ----------

/// Every Rust file in scope, parsed once, minus the ones that only test code can reach.
fn rust_problems(root: &Path, denylist: &[Entry]) -> Vec<String> {
    let mut sources = Vec::new();
    let mut problems = Vec::new();
    for file in rust_files(root) {
        match read_parse(root, &file) {
            Ok(parsed) => sources.push(parsed),
            Err(e) => problems.push(e),
        }
    }
    let mut edges = Vec::new();
    for parsed in &sources {
        edges.extend(mod_edges(&parsed.path, &parsed.ast));
    }
    let test_only = test_only_files(&sources, &edges);
    for parsed in &sources {
        if test_only.contains(&parsed.path) {
            continue;
        }
        problems.extend(scan_rust(
            &parsed.text,
            &parsed.ast,
            rel(root, &parsed.path),
            denylist,
        ));
    }
    problems
}

/// One in-scope file, read and parsed once.
struct Parsed {
    path: PathBuf,
    text: String,
    ast: syn::File,
}

fn read_parse(root: &Path, path: &Path) -> Result<Parsed, String> {
    let text = fs::read_to_string(path).map_err(|e| {
        format!(
            "{}: {e}; make the module file readable UTF-8",
            rel(root, path).display()
        )
    })?;
    let ast = syn::parse_file(&text).map_err(|e| {
        format!(
            "{}: {e}; use parseable Rust module structure",
            rel(root, path).display()
        )
    })?;
    Ok(Parsed {
        path: path.to_path_buf(),
        text,
        ast,
    })
}

/// One Rust file's problems, from its AST: every identifier, every string literal (doc comments
/// arrive as `#[doc = "..."]` literals, so they are already covered) and every macro body,
/// which `Visit` does not descend into on its own.
fn scan_rust<'a>(
    source: &'a str,
    ast: &'a syn::File,
    rel: &'a Path,
    denylist: &'a [Entry],
) -> Vec<String> {
    let mut walker = Walker {
        denylist,
        rel,
        lines: source.lines().collect(),
        hidden: excluded(&ast.attrs),
        problems: Vec::new(),
    };
    for attr in &ast.attrs {
        walker.visit_attribute(attr);
    }
    for item in &ast.items {
        walker.visit_item(item);
    }
    walker.problems
}

/// The AST walker: records every identifier and string literal that matches an entry, unless
/// test code or an allowed line hides it.
struct Walker<'a> {
    denylist: &'a [Entry],
    rel: &'a Path,
    lines: Vec<&'a str>,
    hidden: bool,
    problems: Vec<String>,
}

// Enter every item kind (associated, foreign and block-nested items included) so a test-gated
// item hides everything nested inside it. Same shape as module_size/walk.rs.
macro_rules! visit_items {
    ($($method:ident: $ty:ident),* $(,)?) => {$ (
        fn $method(&mut self, node: &'ast syn::$ty) {
            let hidden = self.hidden;
            self.hidden |= excluded(&node.attrs);
            syn::visit::$method(self, node);
            self.hidden = hidden;
        }
    )*};
}

impl<'ast, 'a> Visit<'ast> for Walker<'a> {
    visit_items!(
        visit_item_const: ItemConst, visit_item_enum: ItemEnum,
        visit_item_extern_crate: ItemExternCrate, visit_item_fn: ItemFn,
        visit_item_foreign_mod: ItemForeignMod, visit_item_impl: ItemImpl,
        visit_item_macro: ItemMacro, visit_item_mod: ItemMod, visit_item_static: ItemStatic,
        visit_item_struct: ItemStruct, visit_item_trait: ItemTrait,
        visit_item_trait_alias: ItemTraitAlias, visit_item_type: ItemType,
        visit_item_union: ItemUnion, visit_item_use: ItemUse,
        visit_impl_item_const: ImplItemConst, visit_impl_item_fn: ImplItemFn,
        visit_impl_item_macro: ImplItemMacro, visit_impl_item_type: ImplItemType,
        visit_trait_item_const: TraitItemConst, visit_trait_item_fn: TraitItemFn,
        visit_trait_item_macro: TraitItemMacro, visit_trait_item_type: TraitItemType,
        visit_foreign_item_fn: ForeignItemFn, visit_foreign_item_macro: ForeignItemMacro,
        visit_foreign_item_static: ForeignItemStatic, visit_foreign_item_type: ForeignItemType,
    );

    fn visit_ident(&mut self, ident: &'ast proc_macro2::Ident) {
        if !self.hidden {
            self.note(&ident.to_string(), ident.span());
        }
    }

    /// Doc comments reach this as `#[doc = "..."]` string literals; nothing is special-cased.
    fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
        if !self.hidden {
            self.note(&lit.value(), lit.span());
        }
    }

    /// A byte-string literal is a distinct `syn::Lit` variant from `LitStr` — engines take
    /// `&[u8]` payloads, so this is a real position to smuggle a domain term through, not just
    /// a theoretical gap.
    fn visit_lit_byte_str(&mut self, lit: &'ast syn::LitByteStr) {
        if !self.hidden {
            self.note(&String::from_utf8_lossy(&lit.value()), lit.span());
        }
    }

    /// A C-string literal, the same gap as the byte-string case above.
    fn visit_lit_cstr(&mut self, lit: &'ast syn::LitCStr) {
        if !self.hidden {
            self.note(&lit.value().to_string_lossy(), lit.span());
        }
    }

    /// `syn`'s default `visit_token_stream` is a no-op, so any opaque token stream — a macro
    /// body (`some_macro!({"x_y": 1})`) and, just as much, an attribute's argument list
    /// (`#[serde(rename = "x_y")]`, which `syn` parses as `Meta::List` and never descends
    /// into) — would otherwise pass the scan untouched. Overriding this one method covers
    /// both: `syn::visit::visit_macro` and `syn::visit::visit_meta_list` each already call
    /// `visit_token_stream` on their tokens: see `syn`'s generated `gen/visit.rs`.
    fn visit_token_stream(&mut self, stream: &'ast TokenStream) {
        if !self.hidden {
            self.scan_tokens(stream.clone());
        }
    }
}

impl<'a> Walker<'a> {
    /// Records every entry whose tokens run contiguously through `text`'s, unless the hit's
    /// line carries the escape hatch.
    fn note(&mut self, text: &str, span: Span) {
        let line = span.start().line;
        if line == 0 || self.allows(line) {
            return;
        }
        let tokens = tokenize(text);
        for entry in self.denylist {
            if contiguous(&tokens, &entry.tokens) {
                self.problems
                    .push(problem(self.rel, line, entry, text.trim()));
            }
        }
    }

    /// The leaves of a macro body: identifiers as themselves, literals with one layer of
    /// quotes stripped.
    fn scan_tokens(&mut self, stream: TokenStream) {
        for tree in stream {
            match tree {
                TokenTree::Ident(ident) => self.note(&ident.to_string(), ident.span()),
                TokenTree::Literal(literal) => {
                    let text = literal.to_string();
                    let text = text
                        .strip_prefix('"')
                        .and_then(|t| t.strip_suffix('"'))
                        .unwrap_or(&text);
                    self.note(text, literal.span());
                }
                TokenTree::Group(group) => self.scan_tokens(group.stream()),
                TokenTree::Punct(_) => {}
            }
        }
    }

    /// The per-line opt-out: whatever else the line holds, `vocabulary: allow` exempts it.
    fn allows(&self, line: usize) -> bool {
        line.checked_sub(1)
            .and_then(|index| self.lines.get(index))
            .is_some_and(|line| line.contains(ALLOW))
    }
}

// ---------- test-only files ----------

/// One `mod x;` declaration: the file that declares it, the files it can resolve to, and
/// whether test code is the only way to reach them.
struct ModEdge {
    from: PathBuf,
    children: Vec<PathBuf>,
    gated: bool,
}

/// Files that exist only as test-gated modules (`#[cfg(test)] mod x;`) are test fixtures, so
/// the whole file is exempt — the file-shaped twin of the inline `#[cfg(test)] mod` case.
/// module_size/walk.rs never descends into a test-gated declaration either.
fn test_only_files(sources: &[Parsed], edges: &[ModEdge]) -> BTreeSet<PathBuf> {
    let known: BTreeSet<&Path> = sources.iter().map(|s| s.path.as_path()).collect();
    let mut test_only = BTreeSet::new();
    loop {
        let before = test_only.len();
        for edge in edges {
            if !edge.gated && !test_only.contains(&edge.from) {
                continue;
            }
            for child in &edge.children {
                if known.contains(child.as_path()) {
                    test_only.insert(child.clone());
                }
            }
        }
        if test_only.len() == before {
            return test_only;
        }
    }
}

/// Every `mod x;` a file declares, top level and inside inline modules.
fn mod_edges(file: &Path, ast: &syn::File) -> Vec<ModEdge> {
    let mut edges = Vec::new();
    mod_tree(&module_dir(file), &ast.items, false, file, &mut edges);
    edges
}

/// Where a `mod x;` written in `file` looks for `x.rs`: next to `file`, or in a directory named
/// after it — the resolution module_size/walk.rs follows. Crate roots are the exception.
fn module_dir(file: &Path) -> PathBuf {
    let mut dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    if !crate_root(file) && file.file_name().is_some_and(|n| n != "mod.rs") {
        dir.push(file.file_stem().unwrap_or_default());
    }
    dir
}

fn crate_root(file: &Path) -> bool {
    matches!(
        file.file_name().and_then(|n| n.to_str()),
        Some("lib.rs" | "main.rs")
    ) || file
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "bin")
}

/// Inline modules shift the directory their children resolve in. A `#[path]` declaration is
/// opaque to this walk, so it stays scanned rather than guessed at.
fn mod_tree(dir: &Path, items: &[Item], hidden: bool, from: &Path, edges: &mut Vec<ModEdge>) {
    for item in items {
        let Item::Mod(module) = item else {
            continue;
        };
        let hidden = hidden || excluded(&module.attrs);
        let name = module.ident.to_string();
        match &module.content {
            Some((_, nested)) => mod_tree(&dir.join(&name), nested, hidden, from, edges),
            None if module.attrs.iter().any(|a| path_attr(&a.meta)) => {}
            None => edges.push(ModEdge {
                from: from.to_path_buf(),
                children: vec![
                    dir.join(format!("{name}.rs")),
                    dir.join(&name).join("mod.rs"),
                ],
                gated: hidden,
            }),
        }
    }
}

// ---------- cfg predicates, as module_size/walk.rs reads them ----------

fn arms(meta: &Meta) -> Vec<Meta> {
    match meta {
        Meta::List(list) => list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .map(|args| args.into_iter().collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Whether a cfg predicate means test code, `all(...)`/`any(...)` included.
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

// ---------- files in scope ----------

/// Every crate directory under `crates/`, sorted so problems come out in a stable order.
fn crate_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(root.join("crates")) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = read
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// The Rust in scope: each crate's `src/` tree, and xtask's own — a check the checks must
/// survive too.
fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in crate_dirs(root) {
        collect_files(&dir.join("src"), &["rs"], &mut files);
    }
    collect_files(&root.join("xtask/src"), &["rs"], &mut files);
    files
}

/// The web view's TypeScript: every crate's `web/src` tree. A tree with no TypeScript in it
/// fails closed rather than passing silently.
fn ts_files(root: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut problems = Vec::new();
    for dir in crate_dirs(root) {
        let src = dir.join("web/src");
        if !src.is_dir() {
            continue;
        }
        let before = files.len();
        collect_files(&src, &["ts", "tsx"], &mut files);
        if files.len() == before {
            problems.push(format!(
                "{}: no TypeScript under it. If the web view moved, point this check at its new tree.",
                src.display()
            ));
        }
    }
    (files, problems)
}

/// Every committed prompt file: each crate's `prompts/` tree, whatever the extension. A prompt
/// is data a model reads, so it ships domain words as surely as source does. A crate with no
/// `prompts/` directory has none to scan; that is not a failure.
fn prompt_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in crate_dirs(root) {
        collect_matching(&dir.join("prompts"), &|_| true, &mut files);
    }
    files
}

/// `exts` files under `dir`, depth-first and sorted. `tests/`, `benches/` and `examples/` are
/// not shipped source; `target/`, `node_modules/` and `dist/` are build output.
fn collect_files(dir: &Path, exts: &[&str], files: &mut Vec<PathBuf>) {
    collect_matching(dir, &|path| has_ext(path, exts), files);
}

/// The files under `dir` that `keep` accepts, depth-first and sorted, skipping excluded dirs.
fn collect_matching(dir: &Path, keep: &dyn Fn(&Path) -> bool, files: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = read.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            if !excluded_dir(&path) {
                collect_matching(&path, keep, files);
            }
        } else if keep(&path) {
            files.push(path);
        }
    }
}

fn excluded_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                "tests" | "benches" | "examples" | "target" | "node_modules" | "dist"
            )
        })
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| exts.contains(&ext))
}

// ---------- TypeScript ----------

/// TypeScript is read as text, a line at a time, with the same tokenizer: no TypeScript parser
/// is a dependency, and line-by-line keeps `file:line` reportable.
fn ts_problems(root: &Path, denylist: &[Entry]) -> Vec<String> {
    let (files, mut problems) = ts_files(root);
    problems.extend(text_problems(root, &files, denylist, OptOut::Honored));
    problems
}

/// Prompt files are read the same way, but the per-line opt-out does not apply: every word in
/// a prompt reaches the model, so a comment marker is just more prompt text.
fn prompt_problems(root: &Path, denylist: &[Entry]) -> Vec<String> {
    text_problems(root, &prompt_files(root), denylist, OptOut::Ignored)
}

/// Whether a line carrying the `vocabulary: allow` marker is exempt.
#[derive(Clone, Copy)]
enum OptOut {
    Honored,
    Ignored,
}

fn text_problems(root: &Path, files: &[PathBuf], denylist: &[Entry], opt: OptOut) -> Vec<String> {
    let mut problems = Vec::new();
    for file in files {
        match fs::read_to_string(file) {
            Ok(text) => problems.extend(scan_text(&text, rel(root, file), denylist, opt)),
            Err(e) => problems.push(format!(
                "{}: {e}; make it readable UTF-8",
                rel(root, file).display()
            )),
        }
    }
    problems
}

fn scan_text(text: &str, rel: &Path, denylist: &[Entry], opt: OptOut) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if matches!(opt, OptOut::Honored) && line.contains(ALLOW) {
            continue;
        }
        let tokens = tokenize(line);
        for entry in denylist {
            if contiguous(&tokens, &entry.tokens) {
                problems.push(problem(rel, index + 1, entry, line.trim()));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// One entry on purpose, not the real data file: with the whole denylist loaded, a matcher
    /// that ignored contiguity would still pass these fixtures through the bare single-word
    /// entry, and the matching core is testable without touching the filesystem.
    fn denylist() -> Vec<Entry> {
        entries("wiki_id\n")
    }

    fn rust(source: &str) -> Vec<String> {
        scan_rust(
            source,
            &syn::parse_file(source).unwrap(),
            Path::new("crates/demo/src/lib.rs"),
            &denylist(),
        )
    }

    #[test]
    fn a_snake_case_field_hits_and_names_the_entry_it_matched() {
        let problems = rust("struct Foo { wiki_id: String }\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("crates/demo/src/lib.rs:1"),
            "{}",
            problems[0]
        );
        assert!(
            problems[0].contains("matched denylist entry 'wiki_id'"),
            "{}",
            problems[0]
        );
        assert!(problems[0].contains("in 'wiki_id'"), "{}", problems[0]);
    }

    #[test]
    fn a_camel_case_identifier_hits_through_the_case_boundary_split() {
        let problems = rust("fn f(wikiId: &str) {}\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("matched denylist entry 'wiki_id'"),
            "{}",
            problems[0]
        );
        assert!(problems[0].contains("in 'wikiId'"), "{}", problems[0]);
    }

    #[test]
    fn tokens_present_but_not_contiguous_do_not_hit() {
        // The proof of contiguity: both words appear, never as a run.
        assert!(rust("fn f() { let wiki_other_id = 1; }\n").is_empty());
    }

    #[test]
    fn a_string_inside_a_macro_body_hits_through_the_token_walk() {
        // `Visit` does not descend into a macro's token stream on its own, so this one hit can
        // only come from the hand walk in `visit_token_stream`; `json!` need not exist for
        // `syn` to parse it.
        let problems = rust("fn f() { json!({ \"wiki_id\": 1 }); }\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("matched denylist entry 'wiki_id'"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn a_serde_rename_argument_hits_through_the_attribute_token_walk() {
        // `Meta::List` (`serde(...)`) is an opaque token stream to `syn::visit` too, exactly
        // like a macro body — this is the position the retired Wikimedia engine itself used to
        // carry the domain schema under a neutrally-named field. Prove the field name alone
        // (`site`) does not trip the scan, so the hit below can only come from the rename arg.
        assert!(rust("struct Foo { site: String }\n").is_empty());
        let problems =
            rust("struct Foo {\n    #[serde(rename = \"wiki_id\")]\n    site: String,\n}\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("matched denylist entry 'wiki_id'"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn byte_string_and_c_string_literals_hit_like_any_other_string() {
        // `syn::Lit::ByteStr`/`::CStr` are distinct variants from `LitStr`, each with their own
        // no-op default `Visit` method — a bare `visit_lit_str` override alone misses both.
        // Engines take `&[u8]` payloads, so `b"..."` is a real smuggling position, not a
        // theoretical one.
        let problems = rust("fn f() { let _ = b\"wiki_id\"; }\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("matched denylist entry 'wiki_id'"),
            "{}",
            problems[0]
        );

        let problems = rust("fn f() { let _ = c\"wiki_id\"; }\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("matched denylist entry 'wiki_id'"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn an_allowed_line_is_exempt_whatever_else_it_holds() {
        assert!(rust("fn f() { let wiki_id = 1; } // vocabulary: allow\n").is_empty());
    }

    #[test]
    fn test_gated_items_are_exempt_including_everything_nested_in_them() {
        assert!(rust("#[cfg(test)]\nmod tests {\n    struct WikiId;\n}\n").is_empty());
        assert!(
            rust("#[cfg(all(test, feature = \"demo\"))]\nfn f() { let wiki_id = 1; }\n").is_empty()
        );
    }

    #[test]
    fn doc_comments_count_and_report_their_own_line() {
        let problems = rust("/// The preset table names `wiki_id`.\nfn f() {}\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("crates/demo/src/lib.rs:1"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn typescript_is_scanned_line_by_line_with_the_same_rules() {
        let text = "const a = 1;\nconst wikiId = 2; // vocabulary: allow\nconst wiki_id = 3;\n";
        let problems = scan_text(
            text,
            Path::new("crates/demo/web/src/app.ts"),
            &denylist(),
            OptOut::Honored,
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("crates/demo/web/src/app.ts:3"),
            "{}",
            problems[0]
        );
        assert!(
            problems[0].contains("matched denylist entry 'wiki_id'"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn one_entry_covers_every_spelling_and_only_contiguous_runs() {
        let entry = &denylist()[0];
        for spelling in [
            "wiki_id",
            "wikiId",
            "wiki-id",
            "wiki.id",
            "WIKI_ID",
            "the wiki id",
        ] {
            assert!(contiguous(&tokenize(spelling), &entry.tokens), "{spelling}");
        }
        for near_miss in ["wiki", "wiki_other_id", "id_wiki", "wikiword"] {
            assert!(
                !contiguous(&tokenize(near_miss), &entry.tokens),
                "{near_miss}"
            );
        }
        assert_eq!(tokenize("EnWiki"), ["en", "wiki"]);
        assert_eq!(
            tokenize("crates/s2w-app/web/src"),
            ["crates", "s2w", "app", "web", "src"]
        );
    }

    /// A throwaway workspace root, so the file-reading paths run for real.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "s2w-vocabulary-{}-{}",
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
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_check_reads_its_data_file_and_skips_test_only_files() {
        let scratch = Scratch::new();
        scratch.write("xtask/vocabulary-denylist.txt", "wiki_id\n");
        scratch.write(
            "crates/demo/src/lib.rs",
            "struct Foo { wiki_id: String }\n#[cfg(test)]\nmod tests;\n",
        );
        // Reached only through the test-gated declaration above: a fixture file, exempt whole.
        scratch.write("crates/demo/src/tests.rs", "struct WikiId;\n");
        scratch.write("crates/demo/web/src/app.ts", "const wikiId = 1;\n");
        // xtask's own source is in scope, like any crate's.
        scratch.write("xtask/src/main.rs", "fn f(wiki_id: u8) {}\n");
        let problems = check(&scratch.0);
        assert_eq!(problems.len(), 3, "{problems:?}");
        for (hit, line) in [
            ("crates/demo/src/lib.rs", 1),
            ("crates/demo/web/src/app.ts", 1),
            ("xtask/src/main.rs", 1),
        ] {
            assert!(
                problems.iter().any(|p| p.contains(&format!("{hit}:{line}"))
                    && p.contains("matched denylist entry 'wiki_id'")),
                "{hit}:{line} missing from {problems:?}"
            );
        }
    }

    #[test]
    fn prompt_files_are_scanned_and_the_opt_out_does_not_reach_them() {
        let scratch = Scratch::new();
        scratch.write("xtask/vocabulary-denylist.txt", "wiki_id\n");
        scratch.write("crates/demo/prompts/clean.txt", "Propose a manifest.\n");
        assert!(check(&scratch.0).is_empty());
        scratch.write(
            "crates/demo/prompts/p.txt",
            "Line one.\nKey rows by wiki_id.\nUse wikiId. vocabulary: allow\n",
        );
        let problems = check(&scratch.0);
        assert_eq!(problems.len(), 2, "{problems:?}");
        for line in [2, 3] {
            assert!(
                problems
                    .iter()
                    .any(|p| p.contains(&format!("crates/demo/prompts/p.txt:{line}"))),
                "line {line} missing from {problems:?}"
            );
        }
    }

    #[test]
    fn a_prompt_file_that_is_not_utf8_fails() {
        let scratch = Scratch::new();
        scratch.write("xtask/vocabulary-denylist.txt", "wiki_id\n");
        let path = scratch.write("crates/demo/prompts/bin.txt", "");
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let problems = check(&scratch.0);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("crates/demo/prompts/bin.txt") && problems[0].contains("UTF-8"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn an_unreadable_or_hollow_denylist_fails_closed() {
        let scratch = Scratch::new();
        let problems = check(&scratch.0);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("vocabulary-denylist.txt"),
            "{}",
            problems[0]
        );
        scratch.write("xtask/vocabulary-denylist.txt", "\n\n");
        let problems = check(&scratch.0);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("no entries"), "{}", problems[0]);
        // A web tree with no TypeScript in it does not pass silently either.
        scratch.write("xtask/vocabulary-denylist.txt", "wiki_id\n");
        scratch.write("crates/demo/web/src/placeholder.css", "");
        let problems = check(&scratch.0);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("no TypeScript"), "{}", problems[0]);
    }
}
