//! Grading a mapping against a key on one corpus: the identity score (`score`) for the mapping and
//! its oracle ceiling, plus the context-collision rows (`context`). Kept apart from both so
//! neither imports the other.

use std::collections::{BTreeMap, BTreeSet};

use s2w_model::StreamMapping;
use serde::Serialize;
use serde_json::Value;

use super::context::{ContextRow, rows};
use super::key::KeySpec;
use super::mentions::{Decoded, Mention, Partition, key_mentions, mapping_mentions};
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
    let predicted = without(
        mapping_mentions(mapping, mapping_corpus)?.partition,
        &gold.excluded,
    );
    let oracle = without(
        mapping_mentions(&spec.oracle()?, &corpus)?.partition,
        &gold.excluded,
    );
    let linked = without(
        mapping_mentions(&spec.oracle_with_links()?, &corpus)?.partition,
        &gold.excluded,
    );
    Ok(Grade {
        mapping: score(&gold.partition, &predicted, &unscored),
        ceiling: score(&gold.partition, &oracle, &unscored),
        ceiling_links: score(&gold.partition, &linked, &unscored),
        contexts: rows(spec, &gold.partition, &predicted, &oracle)?,
        excluded: gold.excluded_per_path(),
        abstained: gold.abstained,
        undecodable: corpus.undecodable(),
    })
}

/// A prediction with the key's excluded mentions dropped. `no_identity` means the key has no
/// mention at that record and path, and the v0 mapping format cannot exclude a value, so every
/// prediction (graded mapping and oracle alike) is filtered the same way; filtering only the
/// oracle would make the ceiling unreachable by any expressible mapping.
fn without(mut predicted: Partition, excluded: &BTreeSet<Mention>) -> Partition {
    predicted
        .cluster
        .retain(|mention, _| !excluded.contains(mention));
    predicted
}
