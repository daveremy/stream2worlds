//! Enforces that `VERSION_HISTORY` in `crates/s2w-system1/src/embedding.rs` is genuinely
//! append-only: no row already committed on `origin/main` may be edited, reordered, or removed.
//!
//! Complements `embedding::tests::version_history_is_append_only_and_matches_the_current_build`,
//! which only proves the array is internally self-consistent *as it currently reads* — a
//! compiled unit test has no view of git history, so it cannot tell "a new row was appended"
//! from "an old row was edited in place and happens to still be internally consistent" (code
//! review round 1, s2w#64: editing `THRESHOLD_BPS` and the existing row's `config_hash` together
//! passes that test with no version bump). Proving append-only-ness needs a comparison against a
//! real prior state, so it lives here — the same shape as the exemption-growth ratchet, which
//! solves the identical "did the tracked value only grow" problem for module-size exemptions
//! (`module_size::ratchet`).
use std::path::Path;
use std::process::Command;

const REL_PATH: &str = "crates/s2w-system1/src/embedding.rs";

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git: {e}; install git and put it on PATH"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Extracts `VERSION_HISTORY`'s literal rows via `syn`, never text matching (workspace
/// convention: Rust source is read only as syn ASTs).
fn rows(source: &str) -> Result<Vec<(u32, String, String)>, String> {
    let file = syn::parse_file(source).map_err(|e| format!("parse: {e}"))?;
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(c) if c.ident == "VERSION_HISTORY" => Some(c),
            _ => None,
        })
        .ok_or("VERSION_HISTORY const not found")?;
    extract(&item.expr)
}

fn extract(expr: &syn::Expr) -> Result<Vec<(u32, String, String)>, String> {
    let inner = match expr {
        syn::Expr::Reference(r) => r.expr.as_ref(),
        other => other,
    };
    let syn::Expr::Array(arr) = inner else {
        return Err("VERSION_HISTORY is not an array literal".to_owned());
    };
    arr.elems.iter().map(row).collect()
}

fn row(expr: &syn::Expr) -> Result<(u32, String, String), String> {
    let syn::Expr::Tuple(t) = expr else {
        return Err("row is not a tuple literal".to_owned());
    };
    let elems: Vec<&syn::Expr> = t.elems.iter().collect();
    let [version_expr, model_expr, config_expr] = elems[..] else {
        return Err("row does not have exactly 3 elements".to_owned());
    };
    let version = match version_expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(i),
            ..
        }) => i.base10_parse::<u32>().map_err(|e| e.to_string())?,
        _ => return Err("row's version is not an integer literal".to_owned()),
    };
    Ok((version, str_lit(model_expr)?, str_lit(config_expr)?))
}

fn str_lit(expr: &syn::Expr) -> Result<String, String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => Ok(s.value()),
        _ => Err("expected a string literal".to_owned()),
    }
}

pub(super) fn check(root: &Path) -> Vec<String> {
    let path = root.join(REL_PATH);
    let current = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        // Nothing to check yet if the crate doesn't exist on this branch.
        Err(_) => return Vec::new(),
    };
    let current_rows = match rows(&current) {
        Ok(r) => r,
        Err(e) => return vec![format!("{REL_PATH}: {e}")],
    };
    let base_ref = format!("origin/main:{REL_PATH}");
    let base_rows = match git(root, &["show", &base_ref]) {
        Ok(s) => match rows(&s) {
            Ok(r) => r,
            Err(e) => return vec![format!("origin/main {REL_PATH}: {e}")],
        },
        // File doesn't exist on origin/main yet (e.g. this PR is the one introducing it) —
        // nothing to compare against, so nothing to enforce yet. The next PR to touch this
        // file gets a real baseline.
        Err(_) => return Vec::new(),
    };
    if current_rows.len() < base_rows.len() || current_rows[..base_rows.len()] != base_rows[..] {
        return vec![format!(
            "{REL_PATH}: VERSION_HISTORY must be append-only — every row already on origin/main \
             must still appear, unchanged and in the same order, in this branch. Append a new \
             (version, model_hash, config_hash) row instead of editing an existing one."
        )];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rows_from_a_real_array_literal() {
        let src = r#"
            pub(crate) const VERSION_HISTORY: &[(u32, &str, &str)] =
                &[(1, "abc123", "def456"), (2, "aaa111", "bbb222")];
        "#;
        assert_eq!(
            rows(src).unwrap(),
            vec![
                (1, "abc123".to_owned(), "def456".to_owned()),
                (2, "aaa111".to_owned(), "bbb222".to_owned()),
            ]
        );
    }

    #[test]
    fn missing_const_is_an_error() {
        assert!(rows("const X: u32 = 1;").is_err());
    }
}
