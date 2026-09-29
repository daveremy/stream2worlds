//! Stages 5 and 6 of research 0002 §6, H-lite subset: per-event value-equality aliases,
//! attributes by functional dependency, co-occurrence relationships, and the emitted mapping.

use std::collections::{BTreeMap, BTreeSet};

use s2w_model::{
    AttrRule, EntityRule, FieldPath, MAPPING_VERSION, RelationshipRule, StreamMapping,
};

use crate::flatten::{BOOL, INT, STR, Table, pct};
use crate::roles::{Dependency, Role, aliased, candidate_dependent, repeat_groups};
use crate::{Config, rule_id, type_labels};

/// An entity type: an alias class, plus the key paths of classes merged into it as 1:1.
struct Type {
    members: Vec<usize>,
    merged: Vec<usize>,
}

/// Builds the mapping from the entity paths in `roles`, or says why there is none.
pub(crate) fn assemble(
    table: &Table,
    roles: &[Role],
    cfg: &Config,
) -> Result<StreamMapping, String> {
    let keys: Vec<usize> = (0..roles.len())
        .filter(|&p| roles[p] == Role::Entity)
        .collect();
    if keys.is_empty() {
        return Err("no path passed the entity test".to_owned());
    }
    let types = merge_one_to_one(table, alias_classes(table, &keys, cfg), cfg);
    let key_set: BTreeSet<usize> = keys.iter().copied().collect();
    let classes: Vec<Vec<FieldPath>> = types
        .iter()
        .map(|ty| ty.members.iter().map(|&p| table.paths[p].clone()).collect())
        .collect();
    let mut entities = Vec::new();
    for (ty, label) in types.iter().zip(type_labels(&classes)) {
        for &k in &ty.members {
            let mut attrs = attributes(table, k, &key_set, cfg);
            attrs.extend(ty.merged.iter().map(|&m| attr(table, m)));
            attrs.sort_by(|a, b| a.name.cmp(&b.name));
            entities.push(EntityRule {
                id: rule_id(&table.paths[k]),
                type_label: label.clone(),
                key: vec![table.paths[k].clone()],
                attrs,
            });
        }
    }
    entities.sort_by(|a, b| a.id.cmp(&b.id));
    let mut relationships = Vec::new();
    for (i, a) in types.iter().enumerate() {
        for b in &types[i + 1..] {
            relationships.extend(relate(table, &a.members, &b.members, cfg));
        }
    }
    relationships.sort_by(|a, b| (&a.from, &a.to, &a.kind).cmp(&(&b.from, &b.to, &b.kind)));
    let mapping = StreamMapping {
        version: MAPPING_VERSION,
        decode: table.decode.clone(),
        entities,
        relationships,
    };
    mapping
        .validate()
        .map_err(|e| format!("emitted mapping is invalid: {e}"))?;
    Ok(mapping)
}

fn attr(table: &Table, path: usize) -> AttrRule {
    AttrRule {
        name: rule_id(&table.paths[path]),
        path: table.paths[path].clone(),
    }
}

/// Merges classes that determine each other (1:1, research 0002 §4: two encodings of one
/// entity). The key is chosen without names: most alias members, then most events, then most
/// distinct values, then integer over string over bool. A tie keeps the classes apart.
fn merge_one_to_one(table: &Table, classes: Vec<Vec<usize>>, cfg: &Config) -> Vec<Type> {
    let values: Vec<Vec<Option<String>>> = classes.iter().map(|c| class_values(table, c)).collect();
    let mut parent: Vec<usize> = (0..classes.len()).collect();
    for i in 0..classes.len() {
        for j in i + 1..classes.len() {
            let pairs = co_occurring(&values[i], &values[j]);
            let swapped: Vec<(&str, &str)> = pairs.iter().map(|&(x, y)| (y, x)).collect();
            if pairs.len() >= cfg.min_support
                && functional(&pairs, cfg)
                && functional(&swapped, cfg)
            {
                let (ri, rj) = (find(&parent, i), find(&parent, j));
                parent[ri.max(rj)] = ri.min(rj);
            }
        }
    }
    let mut components: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..classes.len() {
        components.entry(find(&parent, i)).or_default().push(i);
    }
    let rank = |c: &Vec<usize>| {
        let col = |p: usize| &table.columns[p];
        let kind = c.iter().map(|&p| col(p).kinds).max().unwrap_or(0);
        let kind = [BOOL, STR, INT]
            .iter()
            .position(|k| *k == kind)
            .unwrap_or(0);
        let count = c.iter().map(|&p| col(p).cells.len()).max().unwrap_or(0);
        let distinct = c.iter().map(|&p| col(p).texts.len()).max().unwrap_or(0);
        (c.len(), count, distinct, kind)
    };
    let mut types = Vec::new();
    for members in components.into_values() {
        let mut ranked: Vec<_> = members.iter().map(|&i| (rank(&classes[i]), i)).collect();
        ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
        let unique_top = ranked.len() == 1 || ranked[0].0 != ranked[1].0;
        if unique_top {
            let merged = ranked[1..]
                .iter()
                .flat_map(|&(_, i)| classes[i].clone())
                .collect();
            types.push(Type {
                members: classes[ranked[0].1].clone(),
                merged,
            });
        } else {
            types.extend(members.iter().map(|&i| Type {
                members: classes[i].clone(),
                merged: Vec::new(),
            }));
        }
    }
    types
}

fn find(parent: &[usize], mut i: usize) -> usize {
    while parent[i] != i {
        i = parent[i];
    }
    i
}

fn co_occurring<'a>(a: &'a [Option<String>], b: &'a [Option<String>]) -> Vec<(&'a str, &'a str)> {
    a.iter()
        .zip(b)
        .filter_map(|(x, y)| Some((x.as_deref()?, y.as_deref()?)))
        .collect()
}

/// Union of entity paths that hold equal values in the same events.
fn alias_classes(table: &Table, keys: &[usize], cfg: &Config) -> Vec<Vec<usize>> {
    let mut parent: BTreeMap<usize, usize> = keys.iter().map(|&k| (k, k)).collect();
    fn root(parent: &BTreeMap<usize, usize>, mut k: usize) -> usize {
        while parent[&k] != k {
            k = parent[&k];
        }
        k
    }
    for (i, &a) in keys.iter().enumerate() {
        for &b in &keys[i + 1..] {
            if aliased(table, a, b, cfg) {
                let (ra, rb) = (root(&parent, a), root(&parent, b));
                parent.insert(ra.max(rb), ra.min(rb));
            }
        }
    }
    let mut classes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &k in keys {
        classes.entry(root(&parent, k)).or_default().push(k);
    }
    classes.into_values().collect()
}

/// Non-key paths constant under `k`'s repeated values.
fn attributes(table: &Table, k: usize, keys: &BTreeSet<usize>, cfg: &Config) -> Vec<AttrRule> {
    let groups = repeat_groups(&table.columns[k]);
    let mut attrs: Vec<AttrRule> = (0..table.paths.len())
        .filter(|a| !keys.contains(a) && candidate_dependent(&table.columns[*a], cfg))
        .filter(|&a| !aliased(table, k, a, cfg))
        .filter(|&a| {
            let d = Dependency::measure(table, &groups, a);
            d.considered >= cfg.min_groups && d.share() >= cfg.fd_accept_pct
        })
        .map(|a| AttrRule {
            name: rule_id(&table.paths[a]),
            path: table.paths[a].clone(),
        })
        .collect();
    attrs.sort_by(|a, b| a.name.cmp(&b.name));
    attrs
}

/// One representative value per event for an alias class: its members hold equal values, so
/// any present member will do.
fn class_values(table: &Table, class: &[usize]) -> Vec<Option<String>> {
    (0..table.events)
        .map(|e| {
            class
                .iter()
                .find_map(|&p| table.text(e, p))
                .map(str::to_owned)
        })
        .collect()
}

/// Co-occurrence relationship between two types, with its cardinality as the kind.
fn relate(table: &Table, a: &[usize], b: &[usize], cfg: &Config) -> Vec<RelationshipRule> {
    let (va, vb) = (class_values(table, a), class_values(table, b));
    let pairs = co_occurring(&va, &vb);
    if pairs.len() < cfg.min_support {
        return Vec::new();
    }
    let forward = functional(&pairs, cfg);
    let swapped: Vec<(&str, &str)> = pairs.iter().map(|&(x, y)| (y, x)).collect();
    let backward = functional(&swapped, cfg);
    let da = pairs.iter().map(|p| p.0).collect::<BTreeSet<_>>().len();
    let db = pairs.iter().map(|p| p.1).collect::<BTreeSet<_>>().len();
    let (from, to, kind) = match (forward, backward) {
        (true, false) => (a, b, "n:1"),
        (false, true) => (b, a, "n:1"),
        (true, true) => return Vec::new(),
        (false, false) if da == db => return Vec::new(),
        (false, false) if da < db => (a, b, "n:m"),
        (false, false) => (b, a, "n:m"),
    };
    let mut rules = Vec::new();
    for &f in &endpoints(table, from) {
        for &t in &endpoints(table, to) {
            rules.push(RelationshipRule {
                from: rule_id(&table.paths[f]),
                to: rule_id(&table.paths[t]),
                kind: kind.to_owned(),
            });
        }
    }
    rules
}

/// The members of a class that relationships name: those carried by the most events. Aliases
/// hold equal values, so one would do; a tie keeps every tied member rather than pick by name.
fn endpoints(table: &Table, class: &[usize]) -> Vec<usize> {
    let most = class
        .iter()
        .map(|&p| table.columns[p].cells.len())
        .max()
        .unwrap_or(0);
    class
        .iter()
        .copied()
        .filter(|&p| table.columns[p].cells.len() == most)
        .collect()
}

/// Whether the left value determines the right one: constant right values under repeated
/// left values in at least `fd_accept_pct` of at least `min_groups` groups.
fn functional(pairs: &[(&str, &str)], cfg: &Config) -> bool {
    let mut groups: BTreeMap<&str, (usize, BTreeSet<&str>)> = BTreeMap::new();
    for &(left, right) in pairs {
        let entry = groups.entry(left).or_default();
        entry.0 += 1;
        entry.1.insert(right);
    }
    let repeated: Vec<_> = groups.values().filter(|(n, _)| *n >= 2).collect();
    let constant = repeated.iter().filter(|(_, set)| set.len() == 1).count();
    repeated.len() >= cfg.min_groups && pct(constant, repeated.len()) >= cfg.fd_accept_pct
}
