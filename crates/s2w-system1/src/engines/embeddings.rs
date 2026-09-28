use std::collections::BTreeMap;

use s2w_model::{AttrValue, NaturalKey, RawEvent, WorldEvent};
use serde::Deserialize;
use serde_json::Value;

use crate::embedding::{CURRENT_VERSION, ClassifierError, ClassifyResult, CommentClassifier};
use crate::{AbstainReason, Confidence, Engine, Verdict};

/// The one free-text field on the Wikimedia page-change schema, classified into an edit
/// category by [`CommentClassifier`]. Additive to [`crate::WikimediaPageChangeEngine`]: same
/// source, same entity key, a different attribute.
///
/// Scoped to `enwiki` only (decision 0013): potion-base-8M is an English-only static-embedding
/// model, and `wikipedia.*` routes every language wiki, so every other `wiki_id` abstains
/// `NotMine` — the same reason the schema/canary check already uses.
///
/// Loads the vendored model eagerly at construction (not lazily): there is exactly one `s2w`
/// process per deployment, so "processes that never see a wikipedia event" is not a real case
/// worth optimizing for. A load failure is cached in `Self::classifier` — `should not happen
/// with vendored, golden-tested bytes, but a corrupt build must abstain, not panic or retry.
#[derive(Debug)]
pub struct LocalEmbeddingsEngine {
    classifier: Result<CommentClassifier, ClassifierError>,
    /// `{"model_hash", "config_hash"}` serialized once at construction — `model_hash`/
    /// `config_hash` never change after load, so re-serializing on every `provenance()` call
    /// (the hot path: once per event evaluated by this engine) would be pure waste (refine
    /// pass, s2w#64).
    provenance: Option<Vec<u8>>,
}

impl LocalEmbeddingsEngine {
    /// Loads the vendored model and precomputes prototypes now, once.
    #[must_use]
    pub fn new() -> Self {
        let classifier = CommentClassifier::new();
        let provenance = classifier.as_ref().ok().and_then(|c| {
            serde_json::to_vec(&serde_json::json!({
                "model_hash": c.model_hash(),
                "config_hash": c.config_hash(),
            }))
            .ok()
        });
        Self {
            classifier,
            provenance,
        }
    }
}

impl Default for LocalEmbeddingsEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct Change {
    wiki_id: Option<String>,
    page: Option<Page>,
    revision: Option<Revision>,
}
#[derive(Deserialize)]
struct Page {
    page_id: Option<i64>,
}
#[derive(Deserialize)]
struct Revision {
    rev_id: Option<i64>,
    comment: Option<String>,
}

impl Engine for LocalEmbeddingsEngine {
    fn name(&self) -> &'static str {
        "wikimedia.local_embeddings"
    }
    fn version(&self) -> u32 {
        CURRENT_VERSION
    }
    fn evaluate(&self, event: &RawEvent) -> Verdict {
        match decide(&event.payload, &self.classifier) {
            Ok(verdict) => verdict,
            Err(reason) => Verdict::Abstain { reason },
        }
    }
    fn provenance(&self) -> Option<Vec<u8>> {
        self.provenance.clone()
    }
}

/// Decision order (karpathy ruling on issue #64's plan-gate round 2, folded here rather than
/// re-run as a third plan round; sharpened round 2 of code review after the ordering below
/// was found incomplete): every step, in the order it actually runs —
///
/// 1. JSON parse of the raw payload — `Unparseable` on malformed JSON.
/// 2. Schema check (`$schema` prefix) — `NotMine`.
/// 3. Canary check (`meta.domain == "canary"`) — `NotMine`.
/// 4. Language scope check, read directly off the untyped `Value` (`wiki_id` as a string,
///    compared to `"enwiki"`) — `NotMine`. This runs BEFORE typed deserialization on purpose:
///    a non-`enwiki` event can have any shape in its other fields (it isn't this engine's
///    input), and typed deserialization must never turn "not mine" into `Unparseable` just
///    because some field this engine doesn't care about has an unexpected type.
/// 5. Typed deserialization of the rest of the payload (`page`, `revision`, and re-reading
///    `wiki_id` for the structural check below) — `Unparseable` on a type mismatch. Only
///    reached once step 4 has confirmed this event claims to be `enwiki`.
/// 6. Structural field presence (`wiki_id`, `page.page_id`, `revision`, `revision.rev_id`) —
///    `Insufficient`.
/// 7. Model availability — `Insufficient`.
/// 8. Empty-comment check (inside [`CommentClassifier::classify`]) — `Insufficient`.
/// 9. Scoring / threshold — `BelowThreshold`, `Ambiguous`, or a `Match`.
///
/// So an out-of-scope event — wrong schema, canary, or a non-`enwiki` wiki, however it's
/// otherwise shaped — reports `NotMine`, never `Insufficient` or `Unparseable`: the
/// schema/canary/language checks all mean "not this engine's input," which takes priority over
/// "this engine's input, but incomplete" or "this engine's input, but malformed."
fn decide(
    payload: &[u8],
    classifier: &Result<CommentClassifier, ClassifierError>,
) -> Result<Verdict, AbstainReason> {
    let value: Value =
        serde_json::from_slice(payload).map_err(|e| AbstainReason::Unparseable(e.to_string()))?;
    if !value["$schema"]
        .as_str()
        .is_some_and(|s| s.starts_with("/mediawiki/page/change/"))
        || value["meta"]["domain"] == "canary"
    {
        return Err(AbstainReason::NotMine);
    }
    // Read wiki_id off the untyped Value, before typed deserialization: a non-enwiki event
    // can have any shape elsewhere (it isn't this engine's input), and typed deserialization
    // must never turn "not mine" into Unparseable over a field this engine doesn't care about.
    if let Some(wiki_id) = value["wiki_id"].as_str()
        && wiki_id != "enwiki"
    {
        return Err(AbstainReason::NotMine);
    }
    let change: Change =
        serde_json::from_value(value).map_err(|e| AbstainReason::Unparseable(e.to_string()))?;
    let wiki = change
        .wiki_id
        .ok_or_else(|| AbstainReason::Insufficient("missing wiki_id".into()))?;
    if wiki != "enwiki" {
        return Err(AbstainReason::NotMine);
    }
    let page_id = change
        .page
        .and_then(|p| p.page_id)
        .ok_or_else(|| AbstainReason::Insufficient("missing page.page_id".into()))?;
    let revision = change
        .revision
        .ok_or_else(|| AbstainReason::Insufficient("missing revision".into()))?;
    let rev_id = revision
        .rev_id
        .ok_or_else(|| AbstainReason::Insufficient("missing revision.rev_id".into()))?;

    let classifier = classifier.as_ref().map_err(|error| {
        AbstainReason::Insufficient(format!("embeddings model unavailable: {error}"))
    })?;

    let comment = normalize_comment(revision.comment.as_deref().unwrap_or(""));
    match classifier.classify(&comment) {
        ClassifyResult::Empty => Err(AbstainReason::Insufficient(
            "missing or empty revision.comment".into(),
        )),
        ClassifyResult::Invalid { reason } => Err(AbstainReason::Insufficient(format!(
            "similarity computation produced a non-finite value: {reason}"
        ))),
        ClassifyResult::NoMatch {
            top1_bps,
            top2_bps,
            threshold_bps,
            margin_bps,
        } => Err(if top1_bps < threshold_bps {
            AbstainReason::BelowThreshold {
                score_bps: top1_bps,
                threshold_bps,
            }
        } else {
            AbstainReason::Ambiguous {
                top1_bps,
                top2_bps,
                required_margin_bps: margin_bps,
            }
        }),
        ClassifyResult::Match {
            label, top1_bps, ..
        } => {
            let confidence = Confidence::new(top1_bps)
                .map_err(|error| AbstainReason::Insufficient(error.to_string()))?;
            let page_key = NaturalKey::new(format!("{wiki}:page:{page_id}"));
            let attrs = BTreeMap::from([
                (
                    "last_edit_category".into(),
                    AttrValue::Str(label.to_string()),
                ),
                ("last_edit_category_rev_id".into(), AttrValue::Int(rev_id)),
            ]);
            Ok(Verdict::Propose {
                claims: vec![WorldEvent::EntityObserved {
                    key: page_key,
                    entity_type: "page".into(),
                    attrs,
                }],
                confidence,
            })
        }
    }
}

/// Strips a single leading MediaWiki auto-generated section marker (`/* Section name */`) if
/// present — it is metadata, not the editor's own text, and would let boilerplate dominate
/// short comments. Falls back to the trimmed original when there is no `*/`. Full
/// boilerplate-template stripping is a named follow-up (decision 0013): it needs a real comment
/// corpus to characterize reliably.
fn normalize_comment(comment: &str) -> String {
    let trimmed = comment.trim();
    trimmed
        .strip_prefix("/*")
        .and_then(|rest| rest.split_once("*/"))
        .map_or(trimmed, |(_, tail)| tail.trim())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const ENWIKI_SAMPLE: &[u8] = include_bytes!("../../testdata/page-change-sample-enwiki.json");
    /// The existing golden sample: `wikidatawiki`, now the `NotMine`-by-language path.
    const OTHER_WIKI_SAMPLE: &[u8] = include_bytes!("../../testdata/page-change-sample.json");

    #[test]
    fn normalize_strips_leading_section_marker() {
        assert_eq!(
            normalize_comment("/* History */ added a paragraph"),
            "added a paragraph"
        );
        assert_eq!(normalize_comment("no marker here"), "no marker here");
        assert_eq!(normalize_comment("  /* unterminated"), "/* unterminated");
        assert_eq!(normalize_comment("   "), "");
    }

    #[test]
    fn non_enwiki_wikis_abstain_not_mine() -> TestResult {
        let event = raw(OTHER_WIKI_SAMPLE)?;
        assert_eq!(
            LocalEmbeddingsEngine::new().evaluate(&event),
            Verdict::Abstain {
                reason: AbstainReason::NotMine
            }
        );
        Ok(())
    }

    #[test]
    fn schema_and_canary_checks_match_the_rules_engine() -> TestResult {
        let engine = LocalEmbeddingsEngine::new();
        assert_eq!(
            engine.evaluate(&raw(include_bytes!(
                "../../testdata/page-change-canary.json"
            ))?),
            Verdict::Abstain {
                reason: AbstainReason::NotMine
            }
        );
        for bytes in [b"not json".as_slice(), b""] {
            assert!(matches!(
                engine.evaluate(&raw(bytes)?),
                Verdict::Abstain {
                    reason: AbstainReason::Unparseable(_)
                }
            ));
        }
        Ok(())
    }

    /// Round 2 (opus subagent, correctness finding): the language-scope check must read
    /// `wiki_id` off the untyped `Value` before typed deserialization runs, so a non-`enwiki`
    /// event abstains `NotMine` even when some OTHER field this engine doesn't care about has
    /// an unexpected type — never `Unparseable`, which would wrongly claim the event was ours
    /// but malformed.
    #[test]
    fn a_non_enwiki_event_is_not_mine_even_with_a_malformed_unrelated_field() -> TestResult {
        let engine = LocalEmbeddingsEngine::new();
        let mut value: Value = serde_json::from_slice(ENWIKI_SAMPLE)?;
        value["wiki_id"] = "dewiki".into();
        // A field this engine doesn't scope on, given a type typed deserialization would
        // reject (comment is `Option<String>`, not a number) — must never surface as
        // Unparseable once wiki_id has already ruled the event NotMine.
        value["revision"]["comment"] = 12345.into();
        assert_eq!(
            engine.evaluate(&raw(&serde_json::to_vec(&value)?)?),
            Verdict::Abstain {
                reason: AbstainReason::NotMine
            }
        );
        Ok(())
    }

    #[test]
    fn missing_and_empty_comment_abstain_insufficient() -> TestResult {
        let engine = LocalEmbeddingsEngine::new();
        let mut value: Value = serde_json::from_slice(ENWIKI_SAMPLE)?;
        value["revision"]
            .as_object_mut()
            .ok_or("revision")?
            .remove("comment");
        assert!(matches!(
            engine.evaluate(&raw(&serde_json::to_vec(&value)?)?),
            Verdict::Abstain {
                reason: AbstainReason::Insufficient(_)
            }
        ));
        value["revision"]["comment"] = "   ".into();
        assert!(matches!(
            engine.evaluate(&raw(&serde_json::to_vec(&value)?)?),
            Verdict::Abstain {
                reason: AbstainReason::Insufficient(_)
            }
        ));
        Ok(())
    }

    #[test]
    fn a_confident_comment_proposes_a_labeled_claim() -> TestResult {
        let engine = LocalEmbeddingsEngine::new();
        let verdict = engine.evaluate(&raw(ENWIKI_SAMPLE)?);
        match verdict {
            Verdict::Propose { claims, confidence } => {
                assert_eq!(claims.len(), 1);
                assert!(confidence.basis_points() >= 4_000);
                let WorldEvent::EntityObserved {
                    entity_type, attrs, ..
                } = &claims[0]
                else {
                    return Err("expected an EntityObserved claim".into());
                };
                assert_eq!(entity_type, "page");
                // ENWIKI_SAMPLE's comment ("Reverted edits by ... to last revision by ...") is
                // an unambiguous revert — pin the label, not just its presence (opus review,
                // code review round 1, s2w#64).
                assert_eq!(
                    attrs.get("last_edit_category"),
                    Some(&AttrValue::Str("revert".to_owned()))
                );
                assert!(attrs.contains_key("last_edit_category_rev_id"));
            }
            other => return Err(format!("expected Propose, got {other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn a_low_similarity_comment_abstains_below_threshold() -> TestResult {
        let engine = LocalEmbeddingsEngine::new();
        let mut value: Value = serde_json::from_slice(ENWIKI_SAMPLE)?;
        // Unrelated to every category's example phrases; expected to score below threshold.
        value["revision"]["comment"] =
            "asdkjqwoeiu zzxxccvv qwerty banana platypus xyzzy foobar quux".into();
        let verdict = engine.evaluate(&raw(&serde_json::to_vec(&value)?)?);
        assert!(
            matches!(
                verdict,
                Verdict::Abstain {
                    reason: AbstainReason::BelowThreshold { .. }
                }
            ) || matches!(
                verdict,
                Verdict::Abstain {
                    reason: AbstainReason::Ambiguous { .. }
                }
            ),
            "expected a graded abstain, got {verdict:?}"
        );
        Ok(())
    }

    #[test]
    fn provenance_carries_both_hashes() -> TestResult {
        let engine = LocalEmbeddingsEngine::new();
        let bytes = engine
            .provenance()
            .ok_or("classifier loaded from vendored bytes")?;
        let value: Value = serde_json::from_slice(&bytes)?;
        assert!(value["model_hash"].as_str().is_some_and(|s| s.len() == 16));
        assert!(value["config_hash"].as_str().is_some_and(|s| s.len() == 16));
        Ok(())
    }
}
