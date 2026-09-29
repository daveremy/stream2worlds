//! Check 15: no dependency cycle between modules of one crate (s2w#67, plan #44 §1c).
//!
//! Edges are leaf to leaf: from the module that names a path to the module that DEFINES the
//! named item, following `use`/`pub use` re-exports and globs to the origin, so a cycle through
//! a `mod.rs` or `lib.rs` re-export is seen. An edge between a module and its own ancestor is
//! containment and is dropped; siblings under a shared ancestor are real edges. Tarjan's SCCs
//! with more than one module are violations. Test-only code is skipped, like the size walker.
//! Gap, accepted: paths that exist only after macro expansion, and macro bodies that do not
//! parse as comma-separated expressions, are invisible to this check.
use std::collections::{BTreeMap, BTreeSet};

mod resolve;
#[cfg(test)]
mod tests;

use crate::module_size::walk::Scan;
use resolve::{Collector, Mod, Res, Resolver, Use};

/// Report-only while false: findings print with `[report-only]` and do not fail the check.
/// Flip to true once s2w#240 breaks the `s2w_app` cycle; there is no exemption file (#44 §4).
const ENFORCE: bool = false;

pub(super) fn check(meta: &super::Metadata) -> Vec<String> {
    let mut findings = Vec::new();
    for pkg in &meta.packages {
        let mut outside: BTreeSet<String> = pkg
            .dependencies
            .iter()
            .map(|d| d.rename.as_ref().unwrap_or(&d.name).replace('-', "_"))
            .collect();
        outside.extend(pkg.targets.iter().map(|t| t.name.replace('-', "_")));
        for target in &pkg.targets {
            if target
                .kind
                .iter()
                .any(|k| matches!(k.as_str(), "test" | "bench" | "example" | "custom-build"))
            {
                continue;
            }
            let key = target.name.replace('-', "_");
            let mut scan = Scan::default();
            // An unreadable module is the size check's finding; it is reported there.
            let _ = scan.file(&target.src_path, key.clone(), true);
            findings.extend(target_findings(&scan.asts, &key, &outside));
        }
    }
    if ENFORCE {
        return findings;
    }
    for f in &findings {
        println!("[report-only] {f}");
    }
    Vec::new()
}

type Edges = BTreeMap<Mod, BTreeMap<Mod, String>>;

/// Findings for one crate target: unresolvable paths, then one line per cycle.
pub(super) fn target_findings(
    asts: &BTreeMap<String, syn::File>,
    root: &str,
    outside: &BTreeSet<String>,
) -> Vec<String> {
    let modules = Collector::run(asts, root);
    let resolver = Resolver {
        modules: &modules,
        outside,
    };
    let mut findings = Vec::new();
    let mut edges = Edges::new();
    for (m, module) in &modules {
        let named = module.uses.iter().map(|u| match u {
            Use::Named(p, _) | Use::Glob(p) => (p, true),
        });
        for (path, strict) in named.chain(module.paths.iter().map(|p| (p, p.len() > 1))) {
            let target = match resolver.resolve(m, path) {
                Res::Module(t) | Res::Item(t) => t,
                Res::External => continue,
                Res::Unresolved if strict => {
                    findings.push(format!(
                        "{}: xtask cannot resolve `{}` to a module of this crate — a glob of a macro-generated item, or a macro-generated path? Name it with a plain `use`, or report the gap in xtask/src/module_cycles.rs",
                        m.join("::"),
                        path.join("::")
                    ));
                    continue;
                }
                Res::Unresolved => continue,
            };
            if !target.starts_with(m) && !m.starts_with(&target) {
                edges
                    .entry(m.clone())
                    .or_default()
                    .entry(target)
                    .or_insert_with(|| path.join("::"));
            }
        }
    }
    findings.extend(cycles(&edges));
    findings
}

/// One finding per module cycle, at the deepest module depth that shows it.
fn cycles(edges: &Edges) -> Vec<String> {
    let mut findings = Vec::new();
    // Leaf edges alone miss a cycle that closes through an item defined in a `mod.rs`
    // (`x` uses `y::Foo` from `y/mod.rs`, `y::child` uses `x::Bar`): project every edge onto
    // each depth's ancestors, deepest first, and report a component unless a deeper one
    // already reported projects into it.
    let deepest = edges.keys().map(Vec::len).max().unwrap_or(0);
    let mut reported: Vec<Vec<Mod>> = Vec::new();
    for depth in (2..=deepest).rev() {
        let cut = |m: &Mod| m[..m.len().min(depth)].to_vec();
        let mut projected = Edges::new();
        for (from, tos) in edges {
            for (to, path) in tos {
                let (f, t) = (cut(from), cut(to));
                if !t.starts_with(&f) && !f.starts_with(&t) {
                    projected
                        .entry(f)
                        .or_default()
                        .entry(t)
                        .or_insert_with(|| path.clone());
                }
            }
        }
        for scc in sccs(&projected) {
            let covered = reported.iter().any(|r| {
                let mut seen: Vec<Mod> = r.iter().map(cut).collect();
                seen.dedup();
                seen.len() > 1 && seen.iter().all(|m| scc.contains(m))
            });
            if !covered {
                let names: Vec<String> = scc.iter().map(|m| m.join("::")).collect();
                findings.push(format!(
                    "module cycle in {}: {}. Move the items they share into a module both depend on, or merge them",
                    names.join(", "),
                    witness(&projected, &scc)
                ));
                reported.push(scc);
            }
        }
    }
    findings
}

/// Tarjan's strongly connected components with more than one module, each sorted.
fn sccs(edges: &Edges) -> Vec<Vec<Mod>> {
    struct T<'a> {
        edges: &'a Edges,
        index: BTreeMap<&'a Mod, (usize, usize)>,
        stack: Vec<&'a Mod>,
        on: BTreeSet<&'a Mod>,
        out: Vec<Vec<Mod>>,
    }
    impl<'a> T<'a> {
        fn visit(&mut self, v: &'a Mod) {
            let n = self.index.len();
            self.index.insert(v, (n, n));
            self.stack.push(v);
            self.on.insert(v);
            for w in self.edges.get(v).into_iter().flat_map(BTreeMap::keys) {
                if !self.index.contains_key(w) {
                    self.visit(w);
                    let low = self.index[w].1;
                    self.index.entry(v).and_modify(|e| e.1 = e.1.min(low));
                } else if self.on.contains(w) {
                    let low = self.index[w].0;
                    self.index.entry(v).and_modify(|e| e.1 = e.1.min(low));
                }
            }
            if self.index[v].0 == self.index[v].1 {
                let mut scc = Vec::new();
                while let Some(w) = self.stack.pop() {
                    self.on.remove(w);
                    scc.push(w.clone());
                    if w == v {
                        break;
                    }
                }
                if scc.len() > 1 {
                    scc.sort();
                    self.out.push(scc);
                }
            }
        }
    }
    let mut t = T {
        edges,
        index: BTreeMap::new(),
        stack: Vec::new(),
        on: BTreeSet::new(),
        out: Vec::new(),
    };
    for v in edges.keys() {
        if !t.index.contains_key(v) {
            t.visit(v);
        }
    }
    t.out
}

/// One concrete cycle through the SCC's first module, each hop with the path that makes it.
fn witness(edges: &Edges, scc: &[Mod]) -> String {
    let inside: BTreeSet<&Mod> = scc.iter().collect();
    let start = &scc[0];
    // Breadth-first from start back to start, staying inside the component.
    let mut prev: BTreeMap<&Mod, &Mod> = BTreeMap::new();
    let mut queue = std::collections::VecDeque::from([start]);
    let mut last = None;
    while let Some(v) = queue.pop_front() {
        for w in edges.get(v).into_iter().flat_map(BTreeMap::keys) {
            if w == start {
                last = Some(v);
                queue.clear();
                break;
            }
            if inside.contains(w) && !prev.contains_key(w) {
                prev.insert(w, v);
                queue.push_back(w);
            }
        }
    }
    let mut hops = vec![start];
    let mut cur = last;
    while let Some(v) = cur.filter(|v| *v != start) {
        hops.push(v);
        cur = prev.get(v).copied();
    }
    hops.push(start);
    let n = hops.len();
    hops[1..n - 1].reverse();
    hops.windows(2)
        .map(|w| format!("{} uses `{}`", w[0].join("::"), edges[w[0]][w[1]]))
        .collect::<Vec<_>>()
        .join(", ")
}
