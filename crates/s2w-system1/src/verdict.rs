use s2w_model::WorldEvent;
use serde::{Deserialize, Serialize};

/// An engine's complete answer, including an explicit abstention.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Recognized input; an empty claims list means understood, nothing to say.
    Propose {
        /// Claims in deterministic emission order.
        claims: Vec<WorldEvent>,
        /// Confidence shared by these claims.
        confidence: Confidence,
    },
    /// The engine could not map this input.
    Abstain {
        /// Why the engine abstained.
        reason: AbstainReason,
    },
}

/// Distinguishes routing misses, malformed inputs, missing data and engine defects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbstainReason {
    /// A foreign schema or canary.
    NotMine,
    /// Invalid JSON or field shape; the text is diagnostic.
    Unparseable(String),
    /// A required value is absent or outside its range.
    Insufficient(String),
    /// The bridge caught an engine panic; engines must not return this themselves.
    Panicked(String),
    /// A graded engine's top score did not clear its threshold.
    BelowThreshold {
        /// The top score, in basis points.
        score_bps: u16,
        /// The threshold it did not clear.
        threshold_bps: u16,
    },
    /// A graded engine's top score cleared its threshold but not its margin over the
    /// runner-up: too close a tie to call confidently. Never reused as [`Self::BelowThreshold`]
    /// — that would persist a self-contradictory verdict (decision 0012: verdicts are
    /// append-only), since the top score genuinely was above threshold.
    Ambiguous {
        /// The top score, in basis points.
        top1_bps: u16,
        /// The runner-up score, in basis points.
        top2_bps: u16,
        /// The margin `top1_bps - top2_bps` needed to clear.
        required_margin_bps: u16,
    },
}

/// Basis points in 0..=10_000. Deserialization enforces the same range as construction.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16")]
pub struct Confidence(u16);

impl Confidence {
    /// Maximum confidence.
    pub const CERTAIN: Self = Self(10_000);

    /// Validates a basis-point value.
    pub fn new(bp: u16) -> Result<Self, ConfidenceError> {
        if bp <= 10_000 {
            Ok(Self(bp))
        } else {
            Err(ConfidenceError::OutOfRange(bp))
        }
    }

    /// The integer basis-point value.
    #[must_use]
    pub const fn basis_points(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for Confidence {
    type Error = ConfidenceError;
    fn try_from(bp: u16) -> Result<Self, Self::Error> {
        Self::new(bp)
    }
}

/// Invalid confidence supplied by an engine or serialized input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfidenceError {
    /// Confidence exceeds certainty.
    #[error("confidence {0} exceeds 10_000 basis points")]
    OutOfRange(u16),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_validates_and_verdicts_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        assert!(Confidence::new(10_001).is_err());
        assert!(serde_json::from_str::<Confidence>("20000").is_err());
        assert_eq!(Confidence::CERTAIN.basis_points(), 10_000);
        assert_eq!(serde_json::to_string(&Confidence::CERTAIN)?, "10000");
        let mut verdicts = vec![Verdict::Propose {
            claims: vec![],
            confidence: Confidence::new(8000)?,
        }];
        for reason in [
            AbstainReason::NotMine,
            AbstainReason::Unparseable("bad".into()),
            AbstainReason::Insufficient("missing".into()),
            AbstainReason::Panicked("panic".into()),
            AbstainReason::BelowThreshold {
                score_bps: 3_000,
                threshold_bps: 4_000,
            },
            AbstainReason::Ambiguous {
                top1_bps: 6_000,
                top2_bps: 5_600,
                required_margin_bps: 500,
            },
        ] {
            verdicts.push(Verdict::Abstain { reason });
        }
        for verdict in verdicts {
            assert_eq!(
                serde_json::from_str::<Verdict>(&serde_json::to_string(&verdict)?)?,
                verdict
            );
        }
        Ok(())
    }
}
