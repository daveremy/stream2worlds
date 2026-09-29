//! `cargo xtask h-measure`: grades a stream mapping against an answer key (s2w#56, contract B3).
//! Everything here is generic over streams: the key spec and the mapping are data, read by
//! [`key`] and [`mentions`] (decision 0018).
//!
//! `selftest` runs the mapping executor over the committed 20-event sample and checks it against
//! `MappingEngine`, the executor `serve` runs: every predicted cluster must be an entity the
//! engine proposes for that record, and every entity it proposes must be a cluster. It then reads
//! the mapping as a key spec ([`key::KeySpec::from_mapping`]) and checks the key executor
//! places the same mentions in the same clusters, grades the mapping against that key with
//! [`score`] (it and the oracle ceiling must score 1.0), and prints the contract's frozen
//! fixtures (B3: the 4/9 case and an all-singletons prediction).

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
pub(crate) mod score;

/// Runs `cargo xtask h-measure <args>`.
pub(crate) fn run(root: &Path, args: &[String]) -> ExitCode {
    let result = match args {
        [one] if one == "selftest" => selftest(root),
        _ => {
            eprintln!("usage: cargo xtask h-measure selftest");
            return ExitCode::from(2);
        }
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
    let corpus = mentions::Decoded::new(&payloads, &mapping.decode);
    let predicted = mentions::mapping_mentions(&mapping, &corpus)?;
    let proposed = engine_entities(&mapping, &payloads)?;
    let executed: BTreeSet<(usize, String)> = predicted
        .cluster
        .iter()
        .map(|((record, _), cluster)| (*record, cluster.clone()))
        .collect();
    if predicted.cluster.is_empty() {
        return Err(format!(
            "executor parity: {SAMPLE} gave no mentions, so the check is vacuous"
        ));
    }
    if executed != proposed {
        let missing: Vec<_> = proposed.difference(&executed).collect();
        let extra: Vec<_> = executed.difference(&proposed).collect();
        return Err(format!(
            "executor parity: the mention executor and MappingEngine disagree on {SAMPLE} ({} engine entities without a mention, first {:?}; {} mentions the engine does not propose, first {:?})",
            missing.len(),
            missing.first(),
            extra.len(),
            extra.first()
        ));
    }
    let own_key = key::KeySpec::from_mapping(&mapping)?;
    let graded = mentions::key_mentions(&own_key, &corpus)?.partition;
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
    grades_itself_perfectly(&own_key, &mapping, &payloads)?;
    Ok(format!(
        "h-measure selftest: executor parity with MappingEngine and with the mapping's own key on {SAMPLE}: {} records, {} mentions, {} clusters; graded against its own key: F1 1, recovery 1, ceiling 1\n{}",
        payloads.len(),
        predicted.cluster.len(),
        clusters.len(),
        reference_fixtures()?
    ))
}

/// The mapping graded against its own key, and the key's oracle, both score 1.0.
fn grades_itself_perfectly(
    own_key: &key::KeySpec,
    mapping: &StreamMapping,
    payloads: &[Value],
) -> Result<(), String> {
    let own = score::grade(own_key, mapping, payloads)?;
    for (row, graded) in [("mapping", &own.mapping), ("ceiling", &own.ceiling)] {
        if graded.micro.f1 != Some(1.0) || graded.recovery != Some(1.0) {
            return Err(format!(
                "scorer: {SAMPLE_MAPPING} graded against its own key does not score 1.0 ({row}: {graded:?})"
            ));
        }
    }
    Ok(())
}

/// The contract's frozen fixtures (B3), scored and checked, as a printed table: the 4/9 case
/// (key `{a, b, c}`, prediction `{a, b, d}`) and an all-singletons prediction.
fn reference_fixtures() -> Result<String, String> {
    let partition = |pairs: &[(&str, &str)]| mentions::Partition {
        cluster: pairs
            .iter()
            .map(|(path, cluster)| ((0, (*path).to_owned()), (*cluster).to_owned()))
            .collect(),
    };
    let key = partition(&[("a", "E"), ("b", "E"), ("c", "E")]);
    let four_ninths = score::score(
        &key,
        &partition(&[("a", "X"), ("b", "X"), ("d", "X")]),
        &BTreeSet::new(),
    );
    let singletons = score::score(
        &key,
        &partition(&[("a", "1"), ("b", "2"), ("c", "3")]),
        &BTreeSet::new(),
    );
    let exact = |got: Option<f64>| got.is_some_and(|x| (9.0 * x - 4.0).abs() < 1e-12);
    let m = four_ninths.micro;
    if !(exact(m.precision) && exact(m.recall) && exact(m.f1)) {
        return Err(format!("scorer: the 4/9 fixture scores {m:?}"));
    }
    if singletons.recovery != Some(0.0) {
        return Err(format!(
            "scorer: the all-singletons fixture recovers {:?}",
            singletons.recovery
        ));
    }
    let mut table = "fixture         P       R       F1      false-merge  recovery".to_owned();
    for (name, row) in [("4/9", &four_ninths), ("all-singletons", &singletons)] {
        let b = row.micro;
        table.push_str(&format!(
            "\n{name:<15} {:<7} {:<7} {:<7} {:<12} {}",
            shown(b.precision),
            shown(b.recall),
            shown(b.f1),
            shown(row.false_merge),
            shown(row.recovery)
        ));
    }
    Ok(table)
}

/// A metric as printed: four decimals, or "undefined" for a zero denominator.
fn shown(metric: Option<f64>) -> String {
    metric.map_or_else(|| "undefined".to_owned(), |x| format!("{x:.4}"))
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
