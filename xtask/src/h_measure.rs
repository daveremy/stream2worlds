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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use s2w_model::{Cursor, RawEvent, SourceId, StreamMapping, Timestamp, WorldEvent};
use s2w_system1::{Engine, MappingEngine, Verdict};

// The committed sample the self-test grades: 20 stored envelopes and a hand-written mapping
// with a composite key (decision 0021), the same pair check 11 replays.
use crate::obfuscation_raw::{MAPPING as SAMPLE_MAPPING, RAW as SAMPLE};
use key::Unscored;
use serde_json::Value;

mod context;
mod freeze;
mod grade;
pub(crate) mod key;
pub(crate) mod mentions;
mod pins;
mod report;
pub(crate) mod score;

/// The command's usage line.
pub(crate) const USAGE: &str = "cargo xtask h-measure selftest | freeze --corpus NAME --window N --out FILE [--dir DIR] | profile --corpus NAME --window N [--dir DIR] | score --mapping FILE --corpus NAME --key FILE [--key FILE ...] [--json FILE] [--dir DIR]";

/// Where the corpora live when `--dir` is not given, under `$HOME`.
const CORPUS_DIR: &str = ".local/share/stream2worlds/h-measure";

/// Runs `cargo xtask h-measure <args>`.
pub(crate) fn run(root: &Path, args: &[String]) -> ExitCode {
    let result = match args.split_first() {
        Some((one, [])) if one == "selftest" => selftest(root),
        Some((verb, rest)) if ["freeze", "profile", "score"].contains(&verb.as_str()) => {
            flags(rest).and_then(|f| subcommand(root, verb, &f))
        }
        _ => {
            eprintln!("usage: {USAGE}");
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

type Flags = BTreeMap<String, Vec<String>>;

/// `--name value` pairs; a name may repeat.
fn flags(args: &[String]) -> Result<Flags, String> {
    let mut found = Flags::new();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let name = flag
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected argument {flag:?}; usage: {USAGE}"))?;
        let value = args
            .next()
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| format!("--{name} needs a value"))?;
        found
            .entry(name.to_owned())
            .or_default()
            .push(value.clone());
    }
    Ok(found)
}

/// The one value of a required flag.
fn one<'a>(flags: &'a Flags, name: &str) -> Result<&'a str, String> {
    match flags.get(name).map(Vec::as_slice) {
        Some([value]) => Ok(value),
        Some(_) => Err(format!("--{name} given more than once")),
        None => Err(format!("--{name} is required; usage: {USAGE}")),
    }
}

fn subcommand(root: &Path, verb: &str, flags: &Flags) -> Result<String, String> {
    let known: &[&str] = if verb == "freeze" {
        &["corpus", "window", "out", "dir"]
    } else if verb == "profile" {
        &["corpus", "window", "dir"]
    } else {
        &["mapping", "corpus", "key", "json", "dir"]
    };
    if let Some(name) = flags.keys().find(|n| !known.contains(&n.as_str())) {
        return Err(format!("{verb} takes no --{name}; usage: {USAGE}"));
    }
    let dir = match flags.get("dir") {
        Some(_) => PathBuf::from(one(flags, "dir")?),
        None => PathBuf::from(std::env::var("HOME").map_err(|e| format!("$HOME: {e}"))?)
            .join(CORPUS_DIR),
    };
    let corpus = one(flags, "corpus")?;
    if verb == "freeze" || verb == "profile" {
        let raw = one(flags, "window")?;
        let window = raw.parse().map_err(|e| format!("--window {raw:?}: {e}"))?;
        if verb == "profile" {
            return freeze::profile(root, &dir, corpus, window);
        }
        return freeze::freeze(root, &dir, corpus, window, Path::new(one(flags, "out")?));
    }
    let json = flags
        .get("json")
        .map(|_| one(flags, "json").map(Path::new))
        .transpose()?;
    let request = report::Request {
        frozen: Path::new(one(flags, "mapping")?),
        corpus,
        keys: flags.get("key").map_or(&[], Vec::as_slice),
        dir: &dir,
        json,
    };
    report::run(root, &request)
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
    let own = grade::grade(own_key, mapping, payloads)?;
    for (row, graded) in [("mapping", &own.mapping), ("ceiling", &own.ceiling)] {
        if graded.micro.f1 != Some(1.0) || graded.recovery != Some(1.0) {
            return Err(format!(
                "scorer: {SAMPLE_MAPPING} graded against its own key does not score 1.0 ({row}: {graded:?})"
            ));
        }
    }
    Ok(())
}

/// The contract's frozen fixtures ([`score::frozen_fixtures`]), scored and checked, as a
/// printed table.
fn reference_fixtures() -> Result<String, String> {
    let ninths = |got: Option<f64>, n: f64| got.is_some_and(|x| (9.0 * x - n).abs() < 1e-12);
    let exact = |got: Option<f64>| ninths(got, 4.0);
    let mut table = "fixture         P       R       F1      false-merge  recovery".to_owned();
    for (name, key, prediction) in score::frozen_fixtures() {
        let row = score::score(&key, &prediction, &Unscored::default());
        let b = row.micro;
        let holds = match name {
            "4/9" => {
                exact(b.precision) && exact(b.recall) && exact(b.f1) && ninths(row.false_merge, 5.0)
            }
            _ => row.recovery == Some(0.0),
        };
        if !holds {
            return Err(format!("scorer: the {name} fixture scores {row:?}"));
        }
        table.push_str(&format!(
            "\n{name:<15} {:<7} {:<7} {:<7} {:<12} {}",
            score::shown(b.precision),
            score::shown(b.recall),
            score::shown(b.f1),
            score::shown(row.false_merge),
            score::shown(row.recovery)
        ));
    }
    Ok(table)
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
mod freeze_tests;
#[cfg(test)]
mod report_tests;
#[cfg(test)]
mod tests;
