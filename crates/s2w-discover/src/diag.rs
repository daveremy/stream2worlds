//! A per-key report for tuning the entity tests (decision 0022), read by hand, never by a rule.
//! s2w#291 and s2w#327 each rebuilt it from scratch; `s2w-app`'s ignored `discover_diag` test
//! prints it for a recorded stream.

use std::fmt::Write as _;

use crate::flatten::{Table, pct};
use crate::roles::{self, Follower, Role};
use crate::{Config, rule_id};

/// One line per path that `cfg` makes an entity or near-unique key, or that the second entity
/// test passes with both of its guards off (`churn_pct` and `return_pct` 101): its kinds bits
/// (1 string, 2 integer, 4 bool, 8 other), count, distinct values, uniqueness, role under `cfg`,
/// and which test passed it. Under a second-test key, one line per follower: its constancy share,
/// distinct values, and `k`'s counted changes and superseded share under it (the churn guard's
/// and the integer return floor's input).
#[must_use]
pub fn key_report(payloads: &[&[u8]], cfg: &Config) -> String {
    let table = Table::build(payloads, cfg);
    let unguarded = Config {
        churn_pct: 101,
        return_pct: 101,
        ..cfg.clone()
    };
    let mut out = format!("events={}\n", table.events);
    for k in 0..table.paths.len() {
        let (role, _) = role_of(&table, k, cfg);
        let (_, followers) = role_of(&table, k, &unguarded);
        if followers.is_empty() && !matches!(role, Role::Entity | Role::NearUnique) {
            continue;
        }
        let column = &table.columns[k];
        let _ = writeln!(
            out,
            "{} kinds={} count={} distinct={} uniq%={} role={role:?} test={}",
            rule_id(&table.paths[k]),
            column.kinds,
            column.cells.len(),
            column.texts.len(),
            pct(column.texts.len(), column.cells.len()),
            if followers.is_empty() { 1 } else { 2 },
        );
        for f in &followers {
            let (changes, superseded) = roles::churn(&table, k, f.path);
            let _ = writeln!(
                out,
                "  follower {} share={} distinct={} changes={changes} superseded%={superseded}",
                rule_id(&table.paths[f.path]),
                f.share,
                f.distinct,
            );
        }
    }
    out
}

fn role_of(table: &Table, k: usize, cfg: &Config) -> (Role, Vec<Follower>) {
    match roles::single_column(&table.columns[k], cfg) {
        Some(role) => (role, Vec::new()),
        None => roles::dependency_role(table, k, cfg),
    }
}
