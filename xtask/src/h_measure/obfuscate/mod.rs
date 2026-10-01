//! `cargo xtask h-measure obfuscate` (s2w#370, contract B2.2): turns pinned corpora into one
//! replicate's obfuscated corpora, given a replicate key, and the same key and inputs always give
//! byte-identical output. Field names become `f1…fN` in an order drawn from the key; identifier
//! values become keyed hashes of (domain, value); identifiers inside URLs are read out by the
//! rules file's canonicalization and hashed the same way; other strings are hashed as text;
//! date-times shift by one constant; other numbers and the event's structure are kept.
//!
//! Everything about a stream (domains, URL shapes, unix-time paths, unobservable relationships)
//! is data in the rules file (decision 0018): this module knows JSON, SSE framing, RFC 3339 and
//! percent-encoding, never what a stream is about. It writes, never overwrites: one obfuscated
//! corpus per input, one renamed answer key per plain key, and the replicate's private metadata.
//! Not check 11 (`obfuscation_raw.rs`), which is an unkeyed, single-domain replay that proves an
//! engine reads no names.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::pins::{DATA, Pins};
use super::{Flags, one};
use hash::Keyed;
use meta::{META_FORMAT, Meta, Written};
use rules::Rules;
use transform::{FieldTable, Transformer, collect_paths};

mod canon;
mod hash;
mod meta;
mod rekey;
mod rules;
mod transform;

#[cfg(test)]
mod tests;

/// The flags `obfuscate` takes.
const FLAGS: [&str; 7] = ["rules", "key-file", "replicate", "corpus", "key", "meta", "dir"];

/// One `obfuscate` run.
pub(super) struct Request<'a> {
    /// The stream's rules file.
    pub rules: &'a Path,
    /// The replicate key: 64 hex digits, in a file outside the repository.
    pub key_file: &'a Path,
    /// The replicate's name, used in every output file name.
    pub replicate: &'a str,
    /// Pinned corpora (names in `corpora.toml`), transformed with one shared field table.
    pub corpora: &'a [String],
    /// Pinned answer keys (files in `keys.toml`) to rename for the obfuscated corpora.
    pub keys: &'a [String],
    /// The metadata file: written when absent; when present, its field table is reused and a
    /// field it does not hold is refused.
    pub meta: &'a Path,
    /// Where the corpora are read and the obfuscated corpora written.
    pub dir: &'a Path,
}

/// `cargo xtask h-measure obfuscate` from parsed flags.
pub(super) fn command(root: &Path, flags: &Flags, dir: &Path) -> Result<String, String> {
    if let Some(name) = flags.keys().find(|n| !FLAGS.contains(&n.as_str())) {
        return Err(format!("obfuscate takes no --{name}; usage: {}", super::USAGE));
    }
    let replicate = one(flags, "replicate")?;
    let meta = match flags.get("meta") {
        Some(_) => PathBuf::from(one(flags, "meta")?),
        None => root
            .join(DATA)
            .join("obfuscation")
            .join(format!("{replicate}.meta.json")),
    };
    let request = Request {
        rules: Path::new(one(flags, "rules")?),
        key_file: Path::new(one(flags, "key-file")?),
        replicate,
        corpora: flags.get("corpus").map_or(&[], Vec::as_slice),
        keys: flags.get("key").map_or(&[], Vec::as_slice),
        meta: &meta,
        dir,
    };
    obfuscate(root, &request)
}

/// Runs one replicate's transformation and writes its outputs. Nothing is written unless every
/// input transforms.
pub(super) fn obfuscate(root: &Path, request: &Request<'_>) -> Result<String, String> {
    let outputs = Outputs::plan(root, request)?;
    let rules = Rules::load(request.rules)?;
    let keyed = Keyed::new(read_key(root, request.key_file)?);
    run(root, request, &rules, keyed, &outputs)
}

/// The run after its inputs are read; the collision test enters here with a narrow hasher.
fn run(
    root: &Path,
    request: &Request<'_>,
    rules: &Rules,
    keyed: Keyed,
    outputs: &Outputs,
) -> Result<String, String> {
    let pins = Pins::load(root)?;
    let prior = prior(request, rules, &keyed)?;
    let (paths, inputs) = scan(&pins, request)?;
    let table = match &prior {
        Some(meta) => reuse(meta, &paths)?,
        None => FieldTable(keyed.field_names(&paths)),
    };
    let mut transformer = Transformer::new(rules, keyed, table);
    let mut written = Vec::new();
    for (name, out) in request.corpora.iter().zip(&outputs.corpora) {
        let (bytes, events) = transform_corpus(&pins, request.dir, name, &mut transformer)?;
        written.push((out.clone(), bytes, Some(events)));
    }
    for (file, out) in request.keys.iter().zip(&outputs.keys) {
        let (_, spec) = pins.key(root, file)?;
        let renamed = rekey::rekey(&spec, &mut transformer, &rules.unobservable)
            .map_err(|e| format!("{file}: {e}"))?;
        let text = serde_json::to_string_pretty(&renamed).map_err(|e| e.to_string())? + "\n";
        written.push((out.clone(), text.into_bytes(), None));
    }
    let mut meta = Meta {
        format: META_FORMAT,
        replicate: request.replicate.to_owned(),
        key_sha256: transformer.fingerprint(),
        rules_sha256: rules.sha256.clone(),
        shift_seconds: transformer.shift(),
        fields: Vec::new(),
        treatments: Vec::new(),
        undeclared_numbers: transformer.stats.undeclared_numbers.iter().cloned().collect(),
        fallbacks: Vec::new(),
        unused_rules: rules.by_path.keys().filter(|p| !paths.contains(*p)).cloned().collect(),
        unobservable: rules.unobservable.clone(),
        inputs,
        outputs: BTreeMap::new(),
        keys: BTreeMap::new(),
    };
    (meta.fields, meta.treatments, meta.fallbacks) = meta::rows(transformer.table(), &transformer.stats);
    record(request, &written, &mut meta);
    for (path, bytes, _) in &written {
        write_new(path, bytes)?;
    }
    if prior.is_none() {
        let text = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())? + "\n";
        write_new(request.meta, text.as_bytes())?;
    }
    Ok(report(request, &meta, prior.is_some()))
}

/// The output paths, each refused if it exists.
struct Outputs {
    corpora: Vec<PathBuf>,
    keys: Vec<PathBuf>,
}

impl Outputs {
    fn plan(root: &Path, request: &Request<'_>) -> Result<Self, String> {
        check_request(request)?;
        let replicate = request.replicate;
        let corpora: Vec<PathBuf> = request
            .corpora
            .iter()
            .map(|name| request.dir.join(format!("{name}.obf-{replicate}.raw.sse")))
            .collect();
        let keys: Vec<PathBuf> = request
            .keys
            .iter()
            .map(|file| {
                let stem = file.strip_suffix(".json").unwrap_or(file);
                root.join(DATA).join(format!("{stem}.obf-{replicate}.json"))
            })
            .collect();
        if let Some(found) = corpora.iter().chain(&keys).find(|path| path.exists()) {
            return Err(format!(
                "{} exists; an output is never overwritten (a new run is a new replicate name or a deleted file)",
                found.display()
            ));
        }
        Ok(Self { corpora, keys })
    }
}

fn check_request(request: &Request<'_>) -> Result<(), String> {
    let name = request.replicate;
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "--replicate {name:?}: use letters, digits, `-` and `_` (it names output files)"
        ));
    }
    if request.corpora.is_empty() {
        return Err(format!("--corpus is required; usage: {}", super::USAGE));
    }
    for (what, list) in [("corpus", request.corpora), ("key", request.keys)] {
        let distinct: BTreeSet<&String> = list.iter().collect();
        if distinct.len() != list.len() {
            return Err(format!("a --{what} is given twice"));
        }
    }
    Ok(())
}

/// The key bytes. A key file inside the repository is refused: the key never enters git.
fn read_key(root: &Path, path: &Path) -> Result<[u8; 32], String> {
    let shown = path.display();
    let real = fs::canonicalize(path).map_err(|e| format!("--key-file {shown}: {e}"))?;
    let repo = fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if real.starts_with(&repo) {
        return Err(format!(
            "--key-file {shown} is inside the repository; keep replicate keys outside it (~/.local/share/stream2worlds/h-measure/replicates/)"
        ));
    }
    let text = fs::read_to_string(&real).map_err(|e| format!("--key-file {shown}: {e}"))?;
    hash::parse_key(&text).map_err(|e| format!("--key-file {shown}: {e}"))
}

/// The existing metadata, if any, checked against this run's replicate, key, shift and rules.
fn prior(request: &Request<'_>, rules: &Rules, keyed: &Keyed) -> Result<Option<Meta>, String> {
    let path = request.meta;
    if !path.exists() {
        return Ok(None);
    }
    let shown = path.display();
    let text = fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
    let meta: Meta = serde_json::from_str(&text).map_err(|e| format!("{shown}: {e}"))?;
    let checks = [
        ("format", meta.format.to_string(), META_FORMAT.to_string()),
        ("replicate", meta.replicate.clone(), request.replicate.to_owned()),
        ("key", meta.key_sha256.clone(), keyed.fingerprint()),
        ("shift", meta.shift_seconds.to_string(), keyed.shift().to_string()),
        ("rules", meta.rules_sha256.clone(), rules.sha256.clone()),
    ];
    for (what, recorded, now) in checks {
        if recorded != now {
            return Err(format!(
                "{shown} records a different {what} ({recorded} there, {now} now); a replicate's windows share one key, one rules file and one field table"
            ));
        }
    }
    Ok(Some(meta))
}

/// The recorded field table, refusing any path the inputs hold that it does not.
fn reuse(meta: &Meta, paths: &BTreeSet<Vec<String>>) -> Result<FieldTable, String> {
    let table = meta.table()?;
    for path in paths {
        table.name(path)?;
    }
    Ok(table)
}

/// Every field path in the inputs, and each input's pinned sha256.
fn scan(
    pins: &Pins,
    request: &Request<'_>,
) -> Result<(BTreeSet<Vec<String>>, BTreeMap<String, String>), String> {
    let mut paths = BTreeSet::new();
    let mut inputs = BTreeMap::new();
    for name in request.corpora {
        for (_, data) in frames(&pins.payloads(request.dir, name)?, name)? {
            let event: Value = serde_json::from_str(&data)
                .map_err(|e| format!("corpus {name}: a data: line is not JSON: {e}"))?;
            collect_paths(&event, &mut Vec::new(), &mut paths);
        }
        inputs.insert(name.clone(), pins.corpus(name)?.sha256.clone());
    }
    Ok((paths, inputs))
}

/// `(id, data)` of each stored envelope.
fn frames(envelopes: &[Value], name: &str) -> Result<Vec<(String, String)>, String> {
    envelopes
        .iter()
        .map(|envelope| {
            let text = |field: &str| envelope.get(field).and_then(Value::as_str).map(str::to_owned);
            text("id")
                .zip(text("data"))
                .ok_or_else(|| format!("corpus {name}: a frame lacks an id: or data: line"))
        })
        .collect()
}

fn transform_corpus(
    pins: &Pins,
    dir: &Path,
    name: &str,
    transformer: &mut Transformer<'_>,
) -> Result<(Vec<u8>, usize), String> {
    let frames = frames(&pins.payloads(dir, name)?, name)?;
    let mut out = String::new();
    for (index, (id, data)) in frames.iter().enumerate() {
        let frame = transformer
            .frame(id, data)
            .map_err(|e| format!("corpus {name}, frame {}: {e}", index + 1))?;
        out.push_str(&frame);
    }
    Ok((out.into_bytes(), frames.len()))
}

/// Records each written file's name and sha256 in the metadata.
fn record(request: &Request<'_>, written: &[(PathBuf, Vec<u8>, Option<usize>)], meta: &mut Meta) {
    let shown = |path: &Path| path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let (corpora, keys) = written.split_at(request.corpora.len());
    for (name, (path, bytes, events)) in request.corpora.iter().zip(corpora) {
        let entry = Written { file: shown(path), events: *events, sha256: crate::sha256(bytes) };
        meta.outputs.insert(name.clone(), entry);
    }
    for (file, (path, bytes, _)) in request.keys.iter().zip(keys) {
        let entry = Written { file: shown(path), events: None, sha256: crate::sha256(bytes) };
        meta.keys.insert(file.clone(), entry);
    }
}

/// Writes a new file, creating its directory; an existing file is refused.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let shown = path.display();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|e| format!("{shown}: {e}; an output is never overwritten"))
}

fn report(request: &Request<'_>, meta: &Meta, reused: bool) -> String {
    let mut lines = vec![format!(
        "obfuscated replicate {} (key sha256 {}, {} fields)",
        meta.replicate,
        meta.key_sha256,
        meta.fields.len()
    )];
    for (name, out) in &meta.outputs {
        let events = out.events.unwrap_or_default();
        lines.push(format!("  corpus {name} -> {}: {events} events, sha256 {}", out.file, out.sha256));
    }
    for (file, out) in &meta.keys {
        lines.push(format!("  key {file} -> {DATA}/{}: sha256 {}", out.file, out.sha256));
    }
    let fallbacks: usize = meta.fallbacks.iter().map(|f| f.count).sum();
    lines.push(format!(
        "  undeclared number paths kept: {}; URL values hashed whole: {fallbacks}; unused rules: {:?}",
        meta.undeclared_numbers.len(),
        meta.unused_rules
    ));
    let how = if reused { "reused, not rewritten" } else { "written" };
    lines.push(format!("  metadata {} ({how})", request.meta.display()));
    lines.push("Pin each output in corpora.toml / keys.toml before freezing or scoring with it.".to_owned());
    lines.join("\n")
}
