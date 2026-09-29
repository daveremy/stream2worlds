//! An engine for a payload that already is one claim, for hand-written worlds.

use s2w_model::{RawEvent, WorldEvent};
use serde::Deserialize;
use serde_json::Value;

use crate::{AbstainReason, Confidence, Engine, Verdict};

/// Proposes exactly the claim a payload carries, so `stdin` can feed the query API directly.
///
/// Two accepted shapes, tried in order: an envelope `{"confidence": <basis points>,
/// "event": <WorldEvent>}` (a missing `confidence` means certain), then a bare externally
/// tagged [`WorldEvent`] such as `{"EntityObserved": …}`, proposed as certain. Anything else
/// abstains. This is the one engine that proposes at a graded confidence, so the non-certain
/// half of [`Confidence`] is exercised by this engine.
#[derive(Debug, Default)]
pub struct JsonClaimsEngine;

/// The envelope shape: one claim plus the confidence to propose it at.
#[derive(Deserialize)]
struct Envelope {
    confidence: Option<u16>,
    event: WorldEvent,
}

impl Engine for JsonClaimsEngine {
    fn name(&self) -> &str {
        "json_claims"
    }
    fn version(&self) -> u32 {
        // v2 (s2w#74): confidence outside 0-65535 now abstains `Insufficient`, not `NotMine`.
        2
    }
    fn evaluate(&self, event: &RawEvent) -> Verdict {
        let value: Value = match serde_json::from_slice(&event.payload) {
            Ok(value) => value,
            Err(error) => {
                return Verdict::Abstain {
                    reason: AbstainReason::Unparseable(error.to_string()),
                };
            }
        };
        match Envelope::deserialize(&value) {
            Ok(envelope) => {
                let confidence = match envelope.confidence {
                    None | Some(10_000) => Ok(Confidence::CERTAIN),
                    Some(bp) => Confidence::try_from(bp),
                };
                return match confidence {
                    Ok(confidence) => Verdict::Propose {
                        claims: vec![envelope.event],
                        confidence,
                    },
                    Err(error) => Verdict::Abstain {
                        reason: AbstainReason::Insufficient(error.to_string()),
                    },
                };
            }
            // A `confidence` outside `u16` (0-65535) fails `Envelope::deserialize` outright
            // rather than reaching `Confidence::try_from`'s own 0-10_000 basis-point check — an
            // envelope shape ("has an `event` field") with a malformed `confidence` is still
            // this engine's input, just incomplete, so it must abstain `Insufficient`, not fall
            // through to the bare-`WorldEvent` path and report `NotMine` (s2w#74).
            Err(_) if value.get("event").is_some() => {
                return Verdict::Abstain {
                    reason: AbstainReason::Insufficient(
                        "confidence must be a basis-point value between 0 and 65535".into(),
                    ),
                };
            }
            Err(_) => {}
        }
        match serde_json::from_value::<WorldEvent>(value) {
            Ok(event) => Verdict::Propose {
                claims: vec![event],
                confidence: Confidence::CERTAIN,
            },
            Err(_) => Verdict::Abstain {
                reason: AbstainReason::NotMine,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw;
    use s2w_model::NaturalKey;

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    const MERGE: &str = r#"{"EntitiesMerged":{"survivor":"a:page:1","absorbed":"a:page:2"}}"#;

    #[test]
    fn a_bare_world_event_proposes_itself_as_certain() -> TestResult {
        let verdict = JsonClaimsEngine.evaluate(&raw(MERGE.as_bytes())?);
        assert_eq!(
            verdict,
            Verdict::Propose {
                claims: vec![WorldEvent::EntitiesMerged {
                    survivor: NaturalKey::new("a:page:1"),
                    absorbed: NaturalKey::new("a:page:2"),
                }],
                confidence: Confidence::CERTAIN,
            }
        );
        Ok(())
    }

    #[test]
    fn an_envelope_carries_its_confidence() -> TestResult {
        let claim = WorldEvent::EntitiesMerged {
            survivor: NaturalKey::new("a:page:1"),
            absorbed: NaturalKey::new("a:page:2"),
        };
        let enveloped = raw(format!(r#"{{"confidence":8000,"event":{MERGE}}}"#).as_bytes())?;
        assert_eq!(
            JsonClaimsEngine.evaluate(&enveloped),
            Verdict::Propose {
                claims: vec![claim.clone()],
                confidence: Confidence::new(8000)?,
            }
        );
        // A missing confidence defaults to certain.
        let defaulted = raw(format!(r#"{{"event":{MERGE}}}"#).as_bytes())?;
        assert_eq!(
            JsonClaimsEngine.evaluate(&defaulted),
            Verdict::Propose {
                claims: vec![claim],
                confidence: Confidence::CERTAIN,
            }
        );
        Ok(())
    }

    #[test]
    fn foreign_json_abstains_as_not_mine_and_bad_json_as_unparseable() -> TestResult {
        assert_eq!(
            JsonClaimsEngine.evaluate(&raw(b"{\"hello\":1}")?),
            Verdict::Abstain {
                reason: AbstainReason::NotMine
            }
        );
        assert!(matches!(
            JsonClaimsEngine.evaluate(&raw(b"{")?),
            Verdict::Abstain {
                reason: AbstainReason::Unparseable(_)
            }
        ));
        Ok(())
    }

    #[test]
    fn an_out_of_range_envelope_confidence_is_insufficient() -> TestResult {
        let over = raw(format!(r#"{{"confidence":20000,"event":{MERGE}}}"#).as_bytes())?;
        assert!(matches!(
            JsonClaimsEngine.evaluate(&over),
            Verdict::Abstain {
                reason: AbstainReason::Insufficient(_)
            }
        ));
        Ok(())
    }

    /// A `confidence` that doesn't even fit in `u16` (0-65535) fails `Envelope::deserialize`
    /// outright, before `Confidence::try_from`'s 0-10_000 basis-point check ever runs. An
    /// envelope shape (an `event` field is present) with that malformed confidence is still
    /// this engine's input, just incomplete — it must abstain `Insufficient`, not fall through
    /// to the bare-`WorldEvent` path and report `NotMine` (s2w#74).
    #[test]
    fn confidence_outside_u16_is_insufficient_not_not_mine() -> TestResult {
        let too_big = raw(format!(r#"{{"confidence":70000,"event":{MERGE}}}"#).as_bytes())?;
        assert!(matches!(
            JsonClaimsEngine.evaluate(&too_big),
            Verdict::Abstain {
                reason: AbstainReason::Insufficient(_)
            }
        ));
        let negative = raw(format!(r#"{{"confidence":-1,"event":{MERGE}}}"#).as_bytes())?;
        assert!(matches!(
            JsonClaimsEngine.evaluate(&negative),
            Verdict::Abstain {
                reason: AbstainReason::Insufficient(_)
            }
        ));
        Ok(())
    }

    /// The implementation remains usable behind the object-safe engine seam.
    #[test]
    fn json_claims_works_behind_the_engine_trait() -> TestResult {
        let engines: Vec<Box<dyn Engine>> = vec![Box::new(JsonClaimsEngine)];
        let event = raw(MERGE.as_bytes())?;
        let proposing = engines
            .iter()
            .filter(|engine| matches!(engine.evaluate(&event), Verdict::Propose { .. }))
            .map(|engine| engine.name())
            .collect::<Vec<_>>();
        assert_eq!(proposing, ["json_claims"]);
        Ok(())
    }
}
