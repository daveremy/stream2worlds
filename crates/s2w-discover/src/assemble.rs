//! Stages 5 and 6 of research 0002 §6, H-min subset: per-event value-equality aliases joined
//! with the stage-5b inclusion dependencies (`contain.rs`), attributes by functional dependency,
//! co-occurrence relationships (a leaf type, keyed only by the second entity test, relates only to
//! the type of the path that follows it), and the emitted mapping.

use std::collections::{BTreeMap, BTreeSet};

use s2w_model::{
    AttrRule, EntityRule, FieldPath, MAPPING_VERSION, RelationshipRule, StreamMapping,
};

use crate::flatten::{BOOL, INT, STR, Table, pct};
use crate::roles::{Dependency, Follower, Role, aliased, candidate_dependent, repeat_groups};
use crate::{Config, rule_id, type_labels};

/// An entity type: an alias class, one representative value per event, and the key paths of
/// classes merged into it as 1:1.
struct Type {
    members: Vec<usize>,
    values: Vec<Option<String>>,
    merged: Vec<usize>,
}

/// Builds the mapping from the entity paths in `roles` and the paths in stage 5b's `links`, or
/// says why there is none. `followers` holds, per path, the paths that passed it by the second
/// entity test (none for any other path).
pub(crate) fn assemble(
    table: &Table,
    roles: &[Role],
    followers: &[Vec<Follower>],
    links: &[(usize, usize)],
    cfg: &Config,
) -> Result<StreamMapping, String> {
    let linked: BTreeSet<usize> = links.iter().flat_map(|&(a, b)| [a, b]).collect();
    let keys: Vec<usize> = (0..roles.len())
        .filter(|&p| roles[p] == Role::Entity || linked.contains(&p))
        .collect();
    if keys.is_empty() {
        return Err(
            "no path keys a type: none passed the entity test, or each that did is near-unique (type_uniqueness_pct)"
                .to_owned(),
        );
    }
    let types = merge_one_to_one(table, key_classes(table, &keys, links, cfg), cfg);
    let key_set: BTreeSet<usize> = keys.iter().copied().collect();
    let classes: Vec<Vec<FieldPath>> = types
        .iter()
        .map(|ty| ty.members.iter().map(|&p| table.paths[p].clone()).collect())
        .collect();
    let mut entities = Vec::new();
    for (ty, label) in types.iter().zip(type_labels(&classes)) {
        for &k in &ty.members {
            // Only an entity-test key has the repeat groups the attribute test reads; a key that
            // is unique per event (stage 5b's) has none, so it carries no attributes.
            let mut attrs = if roles[k] == Role::Entity {
                attributes(table, k, &key_set, cfg)
            } else {
                Vec::new()
            };
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
    let mut relationships = relationships(table, &types, followers, &linked, cfg);
    relationships.sort_by(|a, b| (&a.from, &a.to, &a.kind).cmp(&(&b.from, &b.to, &b.kind)));
    let mapping = StreamMapping {
        version: MAPPING_VERSION,
        decode: table.decode.clone(),
        entities,
        relationships,
        links: Vec::new(),
    };
    mapping
        .validate()
        .map_err(|e| format!("emitted mapping is invalid: {e}"))?;
    Ok(mapping)
}

/// Co-occurrence relationships between every two types, except that a leaf relates only to its
/// follower types (`leaf_followers`).
fn relationships(
    table: &Table,
    types: &[Type],
    followers: &[Vec<Follower>],
    linked: &BTreeSet<usize>,
    cfg: &Config,
) -> Vec<RelationshipRule> {
    let leaves: Vec<Option<BTreeSet<usize>>> = (0..types.len())
        .map(|i| leaf_followers(types, i, followers, linked))
        .collect();
    let mut relationships = Vec::new();
    for (i, a) in types.iter().enumerate() {
        for (j, b) in types.iter().enumerate().skip(i + 1) {
            let related = match (&leaves[i], &leaves[j]) {
                (None, None) => true,
                (li, lj) => {
                    li.as_ref().is_some_and(|f| f.contains(&j))
                        || lj.as_ref().is_some_and(|f| f.contains(&i))
                }
            };
            if related {
                relationships.extend(relate(table, a, b, cfg));
            }
        }
    }
    relationships
}

/// Every key path of a type: its alias class and the classes merged into it.
fn key_paths(ty: &Type) -> impl Iterator<Item = usize> + '_ {
    ty.members.iter().chain(&ty.merged).copied()
}

/// For a leaf, the types it relates to; `None` for any other type (s2w#291). A type is a leaf
/// when every key path in it passed only the second entity test and none is a stage-5b link (a
/// link is evidence of identity of its own). A leaf relates only to the type of the path that
/// follows it best: among its key paths' followers that key another type, the highest share,
/// then the most values. A tie keeps every tied type rather than pick by name; no follower that
/// keys a type leaves the leaf with no relationship. The rule caps the co-occurrence fan-out of
/// the types the second test admits, which include counters, sizes and free text that recur
/// (s2w#282).
fn leaf_followers(
    types: &[Type],
    i: usize,
    followers: &[Vec<Follower>],
    linked: &BTreeSet<usize>,
) -> Option<BTreeSet<usize>> {
    if !key_paths(&types[i]).all(|p| !followers[p].is_empty() && !linked.contains(&p)) {
        return None;
    }
    let ranked: Vec<((usize, usize), usize)> = key_paths(&types[i])
        .flat_map(|p| &followers[p])
        .filter_map(|f| {
            let j =
                (0..types.len()).find(|&j| j != i && key_paths(&types[j]).any(|q| q == f.path))?;
            Some(((f.share, f.distinct), j))
        })
        .collect();
    let best = ranked.iter().map(|r| r.0).max();
    Some(
        ranked
            .into_iter()
            .filter(|r| Some(r.0) == best)
            .map(|r| r.1)
            .collect(),
    )
}

fn attr(table: &Table, path: usize) -> AttrRule {
    AttrRule {
        name: rule_id(&table.paths[path]),
        path: table.paths[path].clone(),
    }
}

/// Merges classes that determine each other, transitively (1:1, research 0002 §4: two encodings of one
/// entity). The key is chosen without names: most alias members, then most events, then most
/// distinct values, then integer over string over bool. A tie keeps the classes apart.
fn merge_one_to_one(table: &Table, classes: Vec<Vec<usize>>, cfg: &Config) -> Vec<Type> {
    let values: Vec<Vec<Option<String>>> = classes.iter().map(|c| class_values(table, c)).collect();
    let linked = |i: usize, j: usize| {
        let pairs = co_occurring(&values[i], &values[j]);
        pairs.len() >= cfg.min_support && directions(&pairs, cfg) == (true, true)
    };
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
    for members in components(classes.len(), linked) {
        let mut ranked: Vec<_> = members.iter().map(|&i| (rank(&classes[i]), i)).collect();
        ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
        let unique_top = ranked.len() == 1 || ranked[0].0 != ranked[1].0;
        let winners = if unique_top {
            &ranked[..1]
        } else {
            &ranked[..]
        };
        let merged: Vec<usize> = if unique_top {
            ranked[1..]
                .iter()
                .flat_map(|&(_, i)| classes[i].clone())
                .collect()
        } else {
            Vec::new()
        };
        types.extend(winners.iter().map(|&(_, i)| Type {
            members: classes[i].clone(),
            values: values[i].clone(),
            merged: merged.clone(),
        }));
    }
    types
}

/// Connected components of `0..n` under `linked`, each sorted, ordered by smallest member.
fn components(n: usize, linked: impl Fn(usize, usize) -> bool) -> Vec<Vec<usize>> {
    fn find(parent: &[usize], mut i: usize) -> usize {
        while parent[i] != i {
            i = parent[i];
        }
        i
    }
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            if linked(i, j) {
                let (ri, rj) = (find(&parent, i), find(&parent, j));
                parent[ri.max(rj)] = ri.min(rj);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        groups.entry(find(&parent, i)).or_default().push(i);
    }
    groups.into_values().collect()
}

fn co_occurring<'a>(a: &'a [Option<String>], b: &'a [Option<String>]) -> Vec<(&'a str, &'a str)> {
    a.iter()
        .zip(b)
        .filter_map(|(x, y)| Some((x.as_deref()?, y.as_deref()?)))
        .collect()
}

/// Union of key paths that hold equal values in the same events (stage 5) or share one value
/// domain across events (stage 5b's `links`).
fn key_classes(
    table: &Table,
    keys: &[usize],
    links: &[(usize, usize)],
    cfg: &Config,
) -> Vec<Vec<usize>> {
    components(keys.len(), |i, j| {
        let pair = (keys[i].min(keys[j]), keys[i].max(keys[j]));
        links.binary_search(&pair).is_ok() || aliased(table, keys[i], keys[j], cfg)
    })
    .into_iter()
    .map(|c| c.into_iter().map(|i| keys[i]).collect())
    .collect()
}

/// Non-key paths constant under `k`'s repeated values. `k` passed the entity test, so it has
/// at least `min_groups` groups; unlike that test, a dependent need not be informative here:
/// an attribute with few values (a namespace, a flag-like category) is still an attribute.
fn attributes(table: &Table, k: usize, keys: &BTreeSet<usize>, cfg: &Config) -> Vec<AttrRule> {
    let groups = repeat_groups(&table.columns[k]);
    (0..table.paths.len())
        .filter(|a| !keys.contains(a) && candidate_dependent(&table.columns[*a], cfg))
        .filter(|&a| !aliased(table, k, a, cfg))
        .filter(|&a| Dependency::measure(table, &groups, a).share() >= cfg.fd_accept_pct)
        .map(|a| attr(table, a))
        .collect()
}

/// One representative value per event for an alias class: the value most of its present
/// members hold. Aliases agree in at least `alias_pct` of events; where they do not, a tie
/// yields no value rather than whichever member sorts first by name.
fn class_values(table: &Table, class: &[usize]) -> Vec<Option<String>> {
    (0..table.events)
        .map(|e| {
            let mut votes: BTreeMap<&str, usize> = BTreeMap::new();
            for text in class.iter().filter_map(|&p| table.text(e, p)) {
                *votes.entry(text).or_default() += 1;
            }
            let most = votes.values().copied().max()?;
            let mut top = votes.into_iter().filter(|&(_, n)| n == most);
            match (top.next(), top.next()) {
                (Some((text, _)), None) => Some(text.to_owned()),
                _ => None,
            }
        })
        .collect()
}

/// Co-occurrence relationship between two types, with its cardinality as the kind.
fn relate(table: &Table, a: &Type, b: &Type, cfg: &Config) -> Vec<RelationshipRule> {
    let pairs = co_occurring(&a.values, &b.values);
    if pairs.len() < cfg.min_support {
        return Vec::new();
    }
    let da = pairs.iter().map(|p| p.0).collect::<BTreeSet<_>>().len();
    let db = pairs.iter().map(|p| p.1).collect::<BTreeSet<_>>().len();
    let (from, to, kind) = match directions(&pairs, cfg) {
        (true, false) => (a, b, "n:1"),
        (false, true) => (b, a, "n:1"),
        (true, true) => return Vec::new(),
        (false, false) if da == db => return Vec::new(),
        (false, false) if da < db => (a, b, "n:m"),
        (false, false) => (b, a, "n:m"),
    };
    let mut rules = Vec::new();
    for &f in &endpoints(table, &from.members) {
        for &t in &endpoints(table, &to.members) {
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

/// Whether the left value determines the right one, and the right the left.
fn directions(pairs: &[(&str, &str)], cfg: &Config) -> (bool, bool) {
    let swapped: Vec<(&str, &str)> = pairs.iter().map(|&(x, y)| (y, x)).collect();
    (functional(pairs, cfg), functional(&swapped, cfg))
}

/// Whether the left value determines the right one: constant right values under repeated
/// left values in at least `fd_accept_pct` of at least `min_groups` groups. Unlike the entity
/// test, only events carrying both count: two types are compared where they co-occur.
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
