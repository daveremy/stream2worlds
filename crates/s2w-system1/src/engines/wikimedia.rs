use std::collections::BTreeMap;

use s2w_model::{AttrValue, NaturalKey, RawEvent, WorldEvent};
use serde::Deserialize;

use crate::{AbstainReason, Confidence, Engine, Verdict};

/// Mechanical page-change rules. Accounts on different wikis remain different entities;
/// cross-wiki identity repair belongs to System 2.
#[derive(Debug, Default)]
pub struct WikimediaPageChangeEngine;

#[derive(Deserialize)]
struct Change {
    #[serde(rename = "$schema")]
    schema: Option<String>,
    meta: Option<Meta>,
    wiki_id: Option<String>,
    page_change_kind: Option<String>,
    page: Option<Page>,
    performer: Option<Performer>,
    revision: Option<Revision>,
}
#[derive(Deserialize)]
struct Meta {
    domain: Option<String>,
}
#[derive(Deserialize)]
struct Page {
    page_id: Option<i64>,
    page_title: Option<String>,
    namespace_id: Option<i64>,
    is_redirect: Option<bool>,
}
#[derive(Deserialize)]
struct Performer {
    user_id: Option<i64>,
    user_text: Option<String>,
    is_bot: Option<bool>,
    is_temp: Option<bool>,
    edit_count: Option<i64>,
}
#[derive(Deserialize)]
struct Revision {
    rev_id: Option<i64>,
}

impl Engine for WikimediaPageChangeEngine {
    fn name(&self) -> &'static str {
        "wikimedia.page_change"
    }
    fn version(&self) -> u32 {
        // v2 (s2w#74): `undelete` now clears `deleted`; a persisted v1 verdict never did.
        2
    }
    fn evaluate(&self, event: &RawEvent) -> Verdict {
        match claims(&event.payload) {
            Ok(claims) => Verdict::Propose {
                claims,
                confidence: Confidence::CERTAIN,
            },
            Err(reason) => Verdict::Abstain { reason },
        }
    }
}

fn required<T>(value: Option<T>, field: &str) -> Result<T, AbstainReason> {
    value.ok_or_else(|| AbstainReason::Insufficient(format!("missing {field}")))
}

fn claims(payload: &[u8]) -> Result<Vec<WorldEvent>, AbstainReason> {
    // Deserialized once, straight into `Change` — the schema/canary fields ride along with the
    // rest instead of a separate `Value` pass just to inspect them (s2w#74).
    let change: Change =
        serde_json::from_slice(payload).map_err(|e| AbstainReason::Unparseable(e.to_string()))?;
    if !change
        .schema
        .as_deref()
        .is_some_and(|s| s.starts_with("/mediawiki/page/change/"))
        || change.meta.as_ref().and_then(|m| m.domain.as_deref()) == Some("canary")
    {
        return Err(AbstainReason::NotMine);
    }
    let wiki = required(change.wiki_id, "wiki_id")?;
    let page = required(change.page, "page")?;
    let page_id = required(page.page_id, "page.page_id")?;
    let kind = required(change.page_change_kind, "page_change_kind")?;
    let page_key = NaturalKey::new(format!("{wiki}:page:{page_id}"));
    let mut page_attrs = BTreeMap::from([("wiki_id".into(), AttrValue::Str(wiki.clone()))]);
    if let Some(v) = page.page_title {
        // MediaWiki page titles use `_` for the space in the human-readable title (e.g.
        // `Rust_(programming_language)`); render it back for display. Wikidata's own titles
        // (`Q60988248`, `Lexeme:L123`) have no underscores, so this is a no-op there — a
        // readable Wikidata label needs its own lookup, out of scope here (s2w#91).
        page_attrs.insert("title".into(), AttrValue::Str(v.replace('_', " ")));
    }
    if let Some(v) = page.namespace_id {
        page_attrs.insert("namespace_id".into(), AttrValue::Int(v));
    }
    if let Some(v) = page.is_redirect {
        page_attrs.insert("is_redirect".into(), AttrValue::Bool(v));
    }
    if let Some(v) = change.revision.and_then(|r| r.rev_id) {
        page_attrs.insert("last_rev_id".into(), AttrValue::Int(v));
    }
    if kind == "delete" {
        page_attrs.insert("deleted".into(), AttrValue::Bool(true));
    } else if kind == "undelete" {
        // A restore after a delete: attrs merge by overwrite (`World::observe_entity`), so an
        // explicit `false` is required to clear the `deleted: true` a prior delete event wrote —
        // omitting the key here would leave the stale flag in place (s2w#74).
        page_attrs.insert("deleted".into(), AttrValue::Bool(false));
    }
    let mut claims = Vec::new();
    let user_key = if let Some(user) = change.performer {
        let identity = match user.user_id.filter(|id| *id != 0) {
            Some(id) => id.to_string(),
            None => required(user.user_text.clone(), "performer.user_text")?,
        };
        let key = NaturalKey::new(format!("{wiki}:user:{identity}"));
        let mut attrs = BTreeMap::new();
        if let Some(v) = user.user_text {
            attrs.insert("user_text".into(), AttrValue::Str(v));
        }
        if let Some(v) = user.is_bot {
            attrs.insert("is_bot".into(), AttrValue::Bool(v));
        }
        if let Some(v) = user.is_temp {
            attrs.insert("is_temp".into(), AttrValue::Bool(v));
        }
        if let Some(v) = user.edit_count {
            attrs.insert("edit_count".into(), AttrValue::Int(v));
        }
        claims.push(WorldEvent::EntityObserved {
            key: key.clone(),
            entity_type: "user".into(),
            attrs,
        });
        Some(key)
    } else {
        None
    };
    claims.push(WorldEvent::EntityObserved {
        key: page_key.clone(),
        entity_type: "page".into(),
        attrs: page_attrs,
    });
    if let Some(from) = user_key {
        claims.push(WorldEvent::RelationshipObserved {
            from,
            to: page_key,
            kind,
        });
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw;
    use serde_json::Value;
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    const SAMPLE: &[u8] = include_bytes!("../../testdata/page-change-sample.json");
    const ENWIKI_SAMPLE: &[u8] = include_bytes!("../../testdata/page-change-sample-enwiki.json");

    #[test]
    fn enwiki_title_underscores_become_spaces() -> TestResult {
        let event = raw(ENWIKI_SAMPLE)?;
        let claims = match WikimediaPageChangeEngine.evaluate(&event) {
            Verdict::Propose { claims, .. } => claims,
            other => panic!("expected a proposal, got {other:?}"),
        };
        let page = claims
            .iter()
            .find_map(|claim| match claim {
                WorldEvent::EntityObserved {
                    entity_type, attrs, ..
                } if entity_type == "page" => Some(attrs),
                _ => None,
            })
            .expect("a page claim");
        assert_eq!(
            page.get("title"),
            Some(&AttrValue::Str("Rust (programming language)".into()))
        );
        Ok(())
    }

    #[test]
    fn sample_claims_and_golden_are_exact() -> TestResult {
        let event = raw(SAMPLE)?;
        let verdict = WikimediaPageChangeEngine.evaluate(&event);
        let user = NaturalKey::new("wikidatawiki:user:5401512");
        let page = NaturalKey::new("wikidatawiki:page:60841697");
        assert_eq!(
            verdict,
            Verdict::Propose {
                confidence: Confidence::CERTAIN,
                claims: vec![
                    WorldEvent::EntityObserved {
                        key: user.clone(),
                        entity_type: "user".into(),
                        attrs: BTreeMap::from([
                            ("user_text".into(), AttrValue::Str("Immanuelle".into())),
                            ("is_bot".into(), AttrValue::Bool(false)),
                            ("is_temp".into(), AttrValue::Bool(false)),
                            ("edit_count".into(), AttrValue::Int(716677)),
                        ])
                    },
                    WorldEvent::EntityObserved {
                        key: page.clone(),
                        entity_type: "page".into(),
                        attrs: BTreeMap::from([
                            ("title".into(), AttrValue::Str("Q60988248".into())),
                            ("namespace_id".into(), AttrValue::Int(0)),
                            ("is_redirect".into(), AttrValue::Bool(false)),
                            ("wiki_id".into(), AttrValue::Str("wikidatawiki".into())),
                            ("last_rev_id".into(), AttrValue::Int(2550088644)),
                        ])
                    },
                    WorldEvent::RelationshipObserved {
                        from: user,
                        to: page,
                        kind: "edit".into()
                    },
                ]
            }
        );
        assert_eq!(
            serde_json::to_string(&verdict)?,
            include_str!("../../testdata/page-change-sample.verdict.json").trim()
        );
        assert_eq!(verdict, WikimediaPageChangeEngine.evaluate(&event));
        Ok(())
    }

    #[test]
    fn absent_performer_and_anonymous_identity() -> TestResult {
        let mut value: Value = serde_json::from_slice(SAMPLE)?;
        value.as_object_mut().ok_or("object")?.remove("performer");
        assert!(
            matches!(WikimediaPageChangeEngine.evaluate(&raw(&serde_json::to_vec(&value)?)?), Verdict::Propose { claims, .. } if claims.len() == 1)
        );
        let anon = include_bytes!("../../testdata/page-change-anonymous.json");
        let mut value: Value = serde_json::from_slice(anon)?;
        for id in [None, Some(0)] {
            let performer = value["performer"].as_object_mut().ok_or("performer")?;
            if let Some(id) = id {
                performer.insert("user_id".into(), id.into());
            } else {
                performer.remove("user_id");
            }
            let claims = claims(&serde_json::to_vec(&value)?).map_err(|e| format!("{e:?}"))?;
            assert!(
                matches!(&claims[0], WorldEvent::EntityObserved {key, ..} if key.as_str().ends_with(value["performer"]["user_text"].as_str().ok_or("name")?))
            );
        }
        Ok(())
    }

    #[test]
    fn delete_sets_deleted_and_a_later_undelete_clears_it() -> TestResult {
        let mut deleted: Value = serde_json::from_slice(SAMPLE)?;
        deleted["page_change_kind"] = "delete".into();
        let page_attrs = |claims: &[WorldEvent]| {
            claims
                .iter()
                .find_map(|c| match c {
                    WorldEvent::EntityObserved {
                        entity_type, attrs, ..
                    } if entity_type == "page" => Some(attrs.clone()),
                    _ => None,
                })
                .expect("a page claim")
        };
        let delete_claims = claims(&serde_json::to_vec(&deleted)?).map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            page_attrs(&delete_claims).get("deleted"),
            Some(&AttrValue::Bool(true))
        );

        let mut restored: Value = serde_json::from_slice(SAMPLE)?;
        restored["page_change_kind"] = "undelete".into();
        let undelete_claims =
            claims(&serde_json::to_vec(&restored)?).map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            page_attrs(&undelete_claims).get("deleted"),
            Some(&AttrValue::Bool(false))
        );
        Ok(())
    }

    #[test]
    fn abstentions_are_distinct() -> TestResult {
        for bytes in [b"not json".as_slice(), b""] {
            assert!(matches!(
                WikimediaPageChangeEngine.evaluate(&raw(bytes)?),
                Verdict::Abstain {
                    reason: AbstainReason::Unparseable(_)
                }
            ));
        }
        assert_eq!(
            WikimediaPageChangeEngine.evaluate(&raw(include_bytes!(
                "../../testdata/page-change-canary.json"
            ))?),
            Verdict::Abstain {
                reason: AbstainReason::NotMine
            }
        );
        let mut value: Value = serde_json::from_slice(SAMPLE)?;
        value["$schema"] = "/mediawiki/recentchange/1.0.0".into();
        assert_eq!(
            claims(&serde_json::to_vec(&value)?),
            Err(AbstainReason::NotMine)
        );
        value = serde_json::from_slice(SAMPLE)?;
        value["page"]
            .as_object_mut()
            .ok_or("page")?
            .remove("page_id");
        assert!(matches!(
            claims(&serde_json::to_vec(&value)?),
            Err(AbstainReason::Insufficient(_))
        ));
        value = serde_json::from_slice(SAMPLE)?;
        value["page"]["page_id"] = "wrong type".into();
        assert!(matches!(
            claims(&serde_json::to_vec(&value)?),
            Err(AbstainReason::Unparseable(_))
        ));
        Ok(())
    }
}
