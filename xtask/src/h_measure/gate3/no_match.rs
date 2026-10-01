//! The no-match check both gate-3 arms share (s2w#409, decision 0032 dated note 2026-10-01): a
//! mapping is a no-match on a sample when `MappingEngine` claims nothing from any of the
//! sampled records as stored, the records the executor reads. h-s2's sample is the window slice
//! its shown sample is built from, before decode and truncation; b3's is each fit's envelopes as
//! sent. The engine's verdict is the definition, so the check moves with the executor.

use s2w_model::StreamMapping;
use s2w_system1::{Engine, MappingEngine, Verdict};
use s2w_system2::MappingCheck;
use serde_json::Value;

use super::super::raw_event;

/// An arm's sampled records as stored: the executor's view.
pub(crate) struct Sample {
    payloads: Vec<Vec<u8>>,
}

impl Sample {
    /// h-s2: the window events its sample is built from, serialized as `b3::raw_events` does.
    ///
    /// # Errors
    ///
    /// A record does not serialize.
    pub(crate) fn of_values(records: &[Value]) -> Result<Self, String> {
        let payloads = records
            .iter()
            .enumerate()
            .map(|(n, record)| {
                serde_json::to_vec(record).map_err(|e| format!("sampled record {n}: {e}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { payloads })
    }

    /// b3: a fit's envelopes, byte for byte as sent.
    pub(crate) fn of_events(events: &[String]) -> Self {
        Self {
            payloads: events.iter().map(|e| e.as_bytes().to_vec()).collect(),
        }
    }
}

impl MappingCheck for Sample {
    fn no_match(&self, mapping: &StreamMapping) -> Result<bool, String> {
        no_match(mapping, &self.payloads)
    }
}

/// True when `MappingEngine` claims nothing from every payload: each one abstains
/// (`Unparseable` when a decode step meets a non-string or bad JSON, `Insufficient` when no
/// entity rule had every key path hold a scalar). An empty sample is not a no-match.
///
/// # Errors
///
/// The engine cannot be built from `mapping`, or a payload's record number does not fit a cursor.
pub(crate) fn no_match(mapping: &StreamMapping, payloads: &[Vec<u8>]) -> Result<bool, String> {
    let engine = MappingEngine::new(mapping.clone()).map_err(|e| e.to_string())?;
    for (record, payload) in payloads.iter().enumerate() {
        if let Verdict::Propose { claims, .. } =
            engine.evaluate(&raw_event(record, payload.clone())?)
            && !claims.is_empty()
        {
            return Ok(false);
        }
    }
    Ok(!payloads.is_empty())
}
