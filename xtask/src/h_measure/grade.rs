//! Grading a mapping against a key on one corpus: the identity score (`score`) for the mapping and
//! its oracle ceilings, the edge score (`edges`) when the key declares relationships, plus the
//! context-collision rows (`context`). Kept apart from all three so none imports the others.

use std::collections::{BTreeMap, BTreeSet};

use s2w_model::StreamMapping;
use serde::Serialize;
use serde_json::Value;

use super::context::{ContextRow, rows};
use super::edges::{EdgeScore, GoldEdges, score_edges};
use super::key::KeySpec;
use super::mentions::{Decoded, MappingMentions, Mention, key_mentions, mapping_mentions};
use super::score::{Score, score};

/// A mapping graded against a key on one corpus.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct Grade {
    /// The mapping's score.
    pub mapping: Score,
    /// The oracle v0 mapping's score ([`KeySpec::oracle`]): the ceiling to read `mapping`
    /// against, since a v0 mapping cannot join aliases with different values.
    pub ceiling: Score,
    /// The oracle with links' score ([`KeySpec::oracle_with_links`]): the ceiling for a mapping
    /// that may state links (decision 0027), which can join aliases with different values.
    pub ceiling_links: Score,
    /// Abstained paths from the key executor, per mention path id.
    pub abstained: BTreeMap<String, usize>,
    /// Excluded mentions from the key executor (`no_identity`), per mention path id. The key
    /// has no mention there, so each is dropped from every prediction (the mapping's and the
    /// oracle's alike) before scoring: neither spurious nor abstained, like an unscored path.
    pub excluded: BTreeMap<String, usize>,
    /// Records the key's decode steps could not decode.
    pub undecodable: usize,
    /// The unfloored composite-key sub-metric, one row per key type and context path
    /// ([`super::context`]).
    pub contexts: BTreeMap<String, ContextRow>,
    /// The mapping's edge score (contract B3 "Relationships"); `None` when the key declares no
    /// relationships (format 2 or earlier).
    pub edges: Option<EdgeScore>,
    /// The oracle's edge score: the edge ceiling.
    pub ceiling_edges: Option<EdgeScore>,
    /// The oracle with links' edge score.
    pub ceiling_links_edges: Option<EdgeScore>,
}

/// Grades `mapping` against `spec` on `payloads`. The payloads are decoded once for the key and
/// its oracle, and again for the mapping only when its decode steps differ.
pub(crate) fn grade(
    spec: &KeySpec,
    mapping: &StreamMapping,
    payloads: &[Value],
) -> Result<Grade, String> {
    let corpus = Decoded::new(payloads, &spec.decode);
    let gold = key_mentions(spec, &corpus)?;
    let unscored = spec.unscored();
    let other;
    let mapping_corpus = if mapping.decode == corpus.steps() {
        &corpus
    } else {
        other = Decoded::new(payloads, &mapping.decode);
        &other
    };
    // A mention's record is its index in `payloads` whichever decode steps built the corpus, so
    // the key's excluded set applies to a prediction made on the mapping's own decoding.
    let predicted = without(mapping_mentions(mapping, mapping_corpus)?, &gold.excluded);
    let oracle = without(mapping_mentions(&spec.oracle()?, &corpus)?, &gold.excluded);
    let linked = without(
        mapping_mentions(&spec.oracle_with_links()?, &corpus)?,
        &gold.excluded,
    );
    let edges = GoldEdges::of(spec, &gold);
    let edge = |found: &MappingMentions| {
        edges
            .as_ref()
            .map(|g| score_edges(g, &found.partition, &found.edges, &unscored))
    };
    Ok(Grade {
        mapping: score(&gold.partition, &predicted.partition, &unscored),
        ceiling: score(&gold.partition, &oracle.partition, &unscored),
        ceiling_links: score(&gold.partition, &linked.partition, &unscored),
        contexts: rows(
            spec,
            &gold.partition,
            &predicted.partition,
            &oracle.partition,
        )?,
        edges: edge(&predicted),
        ceiling_edges: edge(&oracle),
        ceiling_links_edges: edge(&linked),
        excluded: gold.excluded_per_path(),
        abstained: gold.abstained,
        undecodable: corpus.undecodable(),
    })
}

/// A prediction with the key's excluded mentions dropped. `no_identity` means the key has no
/// mention at that record and path, and the v0 mapping format cannot exclude a value, so every
/// prediction (graded mapping and oracle alike) is filtered the same way; filtering only the
/// oracle would make the ceiling unreachable by any expressible mapping. Edges keep their
/// clusters: an edge whose endpoint cluster loses every mention here is dropped by the edge
/// scorer (no scored mention), so an oracle edge on an excluded value does not count as false.
/// One exception: when the same excluded value is also a scored mention of that type at another
/// path, the cluster keeps that mention and the edge maps to its entity (edges carry no record).
fn without(mut predicted: MappingMentions, excluded: &BTreeSet<Mention>) -> MappingMentions {
    predicted
        .partition
        .cluster
        .retain(|mention, _| !excluded.contains(mention));
    predicted
}
