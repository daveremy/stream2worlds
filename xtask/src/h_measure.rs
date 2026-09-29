//! `cargo xtask h-measure`: grades a stream mapping against an answer key (s2w#56, contract B3).
//! Everything here is generic over streams: the key spec and the mapping are data, read by
//! [`key`] and [`mentions`] (decision 0018).
//!
//! `selftest` runs the mapping executor over the committed 20-event sample and checks it against
//! `MappingEngine`, the executor `serve` runs: every predicted cluster must be an entity the
//! engine proposes for that record, and every entity it proposes must be a cluster. It then reads
//! the mapping as a key spec ([`key::KeySpec::from_mapping`]) and checks the key executor
//! places the same mentions in the same clusters.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use s2w_model::{Cursor, RawEvent, SourceId, StreamMapping, Timestamp, WorldEvent};
use s2w_system1::{Engine, MappingEngine, Verdict};

// The committed sample the self-test grades: 20 stored envelopes and a hand-written mapping
// with a composite key (decision 0021), the same pair check 11 replays.
use crate::obfuscation_raw::{MAPPING as SAMPLE_MAPPING, RAW as SAMPLE};
use serde_json::Value;

pub(crate) mod key;
pub(crate) mod mentions;

/// Runs `cargo xtask h-measure <args>`.
pub(crate) fn run(root: &Path, args: &[String]) -> ExitCode {
    let result = match args {
        [one] if one == "selftest" => selftest(root),
        _ => Err("usage: cargo xtask h-measure selftest".to_owned()),
    };
    match result {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("✗ h-measure: {problem}");
            ExitCode::FAILURE
        }
    }
}

fn selftest(root: &Path) -> Result<String, String> {
    let payloads = jsonl(&read(root, SAMPLE)?)?;
    let mapping: StreamMapping = serde_json::from_str(&read(root, SAMPLE_MAPPING)?)
        .map_err(|e| format!("{SAMPLE_MAPPING}: {e}"))?;
    let predicted = mentions::mapping_mentions(&mapping, &payloads)?;
    let proposed = engine_entities(&mapping, &payloads)?;
    let executed: BTreeSet<(usize, String)> = predicted
        .cluster
        .iter()
        .map(|((record, _), cluster)| (*record, cluster.clone()))
        .collect();
    if executed != proposed {
        let missing = proposed.difference(&executed).count();
        let extra = executed.difference(&proposed).count();
        let first_missing = proposed.difference(&executed).next();
        let first_extra = executed.difference(&proposed).next();
        return Err(format!(
            "executor parity: the mention executor and MappingEngine disagree on {SAMPLE} ({missing} engine entities without a mention, first {first_missing:?}; {extra} mentions the engine does not propose, first {first_extra:?})"
        ));
    }
    if predicted.cluster.is_empty() {
        return Err(format!(
            "executor parity: {SAMPLE} gave no mentions, so the check is vacuous"
        ));
    }
    let own_key = key::KeySpec::from_mapping(&mapping)?;
    let graded = mentions::key_mentions(&own_key, &payloads)?;
    if graded != predicted {
        let first = graded
            .cluster
            .iter()
            .find(|(mention, cluster)| predicted.cluster.get(*mention) != Some(*cluster))
            .map(|(mention, _)| mention)
            .or_else(|| {
                predicted
                    .cluster
                    .keys()
                    .find(|mention| !graded.cluster.contains_key(*mention))
            });
        return Err(format!(
            "executor parity: {SAMPLE_MAPPING} read as a key spec does not reproduce its own mentions ({} key mentions, {} mapping mentions; first difference at {first:?})",
            graded.cluster.len(),
            predicted.cluster.len()
        ));
    }
    let clusters: BTreeSet<&String> = predicted.cluster.values().collect();
    Ok(format!(
        "h-measure selftest: executor parity with MappingEngine and with the mapping's own key on {SAMPLE}: {} records, {} mentions, {} clusters",
        payloads.len(),
        predicted.cluster.len(),
        clusters.len()
    ))
}

/// `(record, natural key)` for every entity `MappingEngine` proposes.
fn engine_entities(
    mapping: &StreamMapping,
    payloads: &[Value],
) -> Result<BTreeSet<(usize, String)>, String> {
    let engine = MappingEngine::new(mapping.clone()).map_err(|e| e.to_string())?;
    let source = SourceId::new("fixture").map_err(|e| e.to_string())?;
    let mut entities = BTreeSet::new();
    for (record, payload) in payloads.iter().enumerate() {
        let position = u64::try_from(record).map_err(|e| e.to_string())?;
        let event = RawEvent {
            source: source.clone(),
            cursor: Cursor::new(position.to_be_bytes().to_vec()).map_err(|e| e.to_string())?,
            received_at: Timestamp::from_millis(0),
            payload: serde_json::to_vec(payload).map_err(|e| e.to_string())?,
        };
        if let Verdict::Propose { claims, .. } = engine.evaluate(&event) {
            for claim in claims {
                if let WorldEvent::EntityObserved { key, .. } = claim {
                    entities.insert((record, key.as_str().to_owned()));
                }
            }
        }
    }
    Ok(entities)
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

/// One JSON value per non-empty line.
fn jsonl(text: &str) -> Result<Vec<Value>, String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| e.to_string()))
        .collect()
}

#[cfg(test)]
mod tests;
