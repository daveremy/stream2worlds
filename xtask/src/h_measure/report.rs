//! `cargo xtask h-measure score`: grades a frozen mapping against pinned keys on a pinned
//! corpus and prints the report (markdown; `--json` also writes every number).
//!
//! It refuses before reading the scored corpus when: a pin this score uses changed since the
//! mapping was frozen (the freeze corpus, the scored corpus if the freeze recorded it, every
//! key, which the freeze must have recorded); the mapping was not frozen on the development
//! corpus; the corpus is reserved; a key is not pinned or does not hash to its pin; or the
//! file is not what `freeze` writes for its recorded corpus and window (s2w#238), which
//! re-reads the development corpus. The scored corpus is then checked against its own pin.

use std::fs;
use std::path::Path;

use s2w_discover::{Config, PROFILER_VERSION};
use s2w_model::{MAPPING_VERSION, StreamMapping};
use serde::Serialize;

use super::freeze::{Frozen, derive};
use super::grade::{Grade, grade};
use super::pins::{Pins, Role, sha256};
use super::score::{Bcubed, Score, shown};

/// The report's first paragraph: the mapping format's alias limit (ruling on s2w#56,
/// 2026-09-29), stated before any number.
const ALIAS_LIMIT: &str = "A v0 stream mapping cannot say that different values name one entity. A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key) and against the canonical-mention key (alias paths unscored), not against 1.0.";

/// What a score run reads, from the command line.
pub(crate) struct Request<'a> {
    pub frozen: &'a Path,
    pub corpus: &'a str,
    pub keys: &'a [String],
    pub dir: &'a Path,
    pub json: Option<&'a Path>,
}

#[derive(Serialize)]
struct KeyReport {
    file: String,
    variant: String,
    sha256: String,
    grade: Grade,
}

#[derive(Serialize)]
struct Report<'a> {
    frozen: &'a Frozen,
    frozen_sha256: String,
    corpus: &'a str,
    corpus_sha256: &'a str,
    records: usize,
    keys: Vec<KeyReport>,
}

/// Runs `score`, returning the markdown report.
pub(crate) fn run(root: &Path, request: &Request<'_>) -> Result<String, String> {
    let pins = Pins::load(root)?;
    let bytes =
        fs::read(request.frozen).map_err(|e| format!("{}: {e}", request.frozen.display()))?;
    let frozen: Frozen =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", request.frozen.display()))?;
    let scored = pins.corpus(request.corpus)?;
    admissible(&pins, &frozen, scored.role, request)?;
    let specs = request
        .keys
        .iter()
        .map(|file| pins.key(root, file))
        .collect::<Result<Vec<_>, _>>()?;
    pins_used(&pins, &frozen, request)?;
    reproduced(&pins, &frozen, request)?;
    let payloads = pins.payloads(request.dir, request.corpus)?;
    // An abstention is contract B3's degenerate output: it is graded as the empty prediction.
    let mapping = frozen.mapping.clone().unwrap_or(StreamMapping {
        version: MAPPING_VERSION,
        decode: Vec::new(),
        entities: Vec::new(),
        relationships: Vec::new(),
        links: Vec::new(),
    });
    let mut keys = Vec::new();
    for (pin, spec) in specs {
        keys.push(KeyReport {
            grade: grade(&spec, &mapping, &payloads)?,
            file: pin.file,
            variant: pin.variant,
            sha256: pin.sha256,
        });
    }
    let report = Report {
        frozen: &frozen,
        frozen_sha256: sha256(&bytes),
        corpus: request.corpus,
        corpus_sha256: &scored.sha256,
        records: payloads.len(),
        keys,
    };
    if let Some(path) = request.json {
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n";
        fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(markdown(&report, request))
}

/// The refusals that need no corpus bytes.
fn admissible(
    pins: &Pins,
    frozen: &Frozen,
    scored: Role,
    request: &Request<'_>,
) -> Result<(), String> {
    let origin = pins.corpus(&frozen.corpus)?;
    if origin.role != Role::Development || origin.sha256 != frozen.corpus_sha256 {
        return Err(format!(
            "{} was not frozen on the pinned development corpus",
            request.frozen.display()
        ));
    }
    if scored == Role::Reserved {
        return Err(format!(
            "{} is reserved for a later change and is never scored here",
            request.corpus
        ));
    }
    if request.keys.is_empty() {
        return Err("score needs at least one --key".to_owned());
    }
    let mut seen = std::collections::BTreeSet::new();
    if let Some(twice) = request.keys.iter().find(|file| !seen.insert(file.as_str())) {
        return Err(format!("--key {twice} given twice"));
    }
    Ok(())
}

/// Compares, recorded against current, only the pins this score depends on: the freeze
/// corpus, the scored corpus when the freeze recorded it (a row added later is fine), and each
/// `--key`, which the freeze must have recorded (a key pinned later could be fitted to the
/// mapping). Any other row may be added, removed or changed (s2w#238). The recorded pins are
/// taken as written: code cannot prove when a pin existed, the commit history does.
///
/// A corpus row may differ in one way: a span recorded `reserved` that is `heldout` now, with
/// the same file, event count and sha256. That is the span being opened after the freeze, and
/// it needs no re-freeze (s2w#277). Whether a span may be scored now is `admissible`'s check on
/// the current manifest. Any other role change still refuses, so a span that was `development`
/// when the mapping was frozen never scores as held out.
fn pins_used(pins: &Pins, frozen: &Frozen, request: &Request<'_>) -> Result<(), String> {
    let current = pins.all();
    let file = request.frozen.display();
    let corpora = [
        (format!("corpus {}", frozen.corpus), true),
        (format!("corpus {}", request.corpus), false),
    ];
    let keys = request.keys.iter().map(|key| (format!("key {key}"), true));
    for (name, required) in corpora.into_iter().chain(keys) {
        match (frozen.pins.get(&name), current.get(&name)) {
            (None, _) if required => {
                return Err(format!(
                    "{name} was not pinned when {file} was frozen; a scored key and the freeze corpus are pinned before the freeze"
                ));
            }
            (Some(then), Some(now)) if then != now && opened(&name, then, now) => {}
            (Some(then), now) if Some(then) != now => {
                return Err(format!(
                    "{name}: its row in keys.toml or corpora.toml changed or was removed since {file} was frozen; freeze again under the current pins"
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// True when a corpus pin (`"{role:?} {file} {events} {sha256}"`, as `Pins::all` writes it)
/// differs from its recorded value only by the role going from `Reserved` to `Heldout`.
fn opened(name: &str, then: &str, now: &str) -> bool {
    if !name.starts_with("corpus ") {
        return false;
    }
    match (then.split_once(' '), now.split_once(' ')) {
        (Some((was, same)), Some((is, now))) => {
            same == now
                && was == format!("{:?}", Role::Reserved)
                && is == format!("{:?}", Role::Heldout)
        }
        _ => false,
    }
}

/// Re-runs the freeze on the file's recorded corpus and window and refuses unless the file
/// holds exactly what that run writes (pins aside, which `pins_used` compares), so a
/// hand-written or edited mapping never scores (s2w#238).
fn reproduced(pins: &Pins, frozen: &Frozen, request: &Request<'_>) -> Result<(), String> {
    let file = request.frozen.display();
    let config = format!("{:?}", Config::default());
    if (PROFILER_VERSION, config.as_str())
        != (frozen.profiler_version.as_str(), frozen.config.as_str())
    {
        return Err(format!(
            "{file} was frozen by profiler {} with {}; this build has profiler {PROFILER_VERSION} with {config}: score with the build that froze it",
            frozen.profiler_version, frozen.config
        ));
    }
    let mut derived = derive(pins, request.dir, &frozen.corpus, frozen.window)
        .map_err(|e| format!("re-running the freeze recorded in {file}: {e}"))?;
    derived.pins.clone_from(&frozen.pins);
    if derived != *frozen {
        return Err(format!(
            "{file} is not what freeze writes for its recorded inputs (the first {} events of {}): it was edited, not written by freeze, or the profiler changed without a version bump",
            frozen.window, frozen.corpus
        ));
    }
    Ok(())
}

fn table(header: &[&str], rows: impl IntoIterator<Item = Vec<String>>) -> String {
    let line = |cells: &[String]| format!("| {} |\n", cells.join(" | "));
    let head: Vec<String> = header.iter().map(|h| (*h).to_owned()).collect();
    let rule: Vec<String> = header.iter().map(|_| "---".to_owned()).collect();
    let mut out = line(&head) + &line(&rule);
    for row in rows {
        out.push_str(&line(&row));
    }
    out
}

fn prf(b: &Bcubed) -> [String; 3] {
    [shown(b.precision), shown(b.recall), shown(b.f1)]
}

fn headline(name: &str, score: &Score) -> Vec<Vec<String>> {
    let singletons: Vec<&str> = score.singleton_types.iter().map(String::as_str).collect();
    let [p, r, f] = prf(&score.micro);
    let [wp, wr, wf] = prf(&score.without_singleton_types);
    vec![
        vec![
            name.to_owned(),
            p,
            r,
            f,
            shown(score.false_merge),
            shown(score.recovery),
        ],
        vec![
            format!("{name}, without singleton-only types {singletons:?}"),
            wp,
            wr,
            wf,
            String::new(),
            String::new(),
        ],
    ]
}

const HEADLINE: [&str; 6] = [
    "row",
    "P",
    "R",
    "F1",
    "false-merge (mention-weighted)",
    "recovery",
];
const PER_TYPE: [&str; 7] = [
    "type",
    "P",
    "R",
    "F1",
    "ceiling P",
    "ceiling R",
    "ceiling F1",
];
const PER_PATH: [&str; 5] = [
    "key path",
    "key mentions",
    "predicted",
    "recall",
    "ceiling recall",
];
const CONTEXT: [&str; 10] = [
    "type @ context",
    "groups",
    "entities",
    "mentions",
    "P",
    "R",
    "F1",
    "ceiling P",
    "ceiling R",
    "ceiling F1",
];

fn key_section(key: &KeyReport) -> String {
    let g = &key.grade;
    let mut rows = headline("mapping", &g.mapping);
    rows.extend(headline("ceiling", &g.ceiling));
    let types = g.mapping.per_type.iter().map(|(t, b)| {
        let c = g.ceiling.per_type.get(t).copied().unwrap_or_default();
        [vec![t.clone()], prf(b).to_vec(), prf(&c).to_vec()].concat()
    });
    let paths = g.mapping.per_path.iter().map(|(p, row)| {
        let ceiling = g.ceiling.per_path.get(p).and_then(|c| c.recall);
        let counts = [row.key.to_string(), row.predicted.to_string()];
        [
            vec![p.clone()],
            counts.to_vec(),
            vec![shown(row.recall), shown(ceiling)],
        ]
        .concat()
    });
    let contexts = g.contexts.iter().map(|(name, c)| {
        let counts = [c.groups, c.entities, c.mentions].map(|n| n.to_string());
        let scores = [prf(&c.mapping), prf(&c.ceiling)].concat();
        [vec![name.clone()], counts.to_vec(), scores].concat()
    });
    format!(
        "## Key {} ({}, sha256 {})\n\n{}\n{}\n{}\nContext collisions (the composite-key sub-metric, unfloored):\n\n{}{}",
        key.file,
        key.variant,
        key.sha256,
        table(&HEADLINE, rows),
        table(&PER_TYPE, types),
        table(&PER_PATH, paths),
        table(&CONTEXT, contexts),
        counts_line(g)
    )
}

fn counts_line(g: &Grade) -> String {
    let spurious: usize = g.mapping.spurious.values().sum();
    let mut top: Vec<(&String, &usize)> = g.mapping.spurious.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    top.truncate(10);
    format!(
        "\nSpurious predicted mentions: {spurious} at {} paths (most: {top:?}). Key abstained: {:?}. Excluded (no_identity): {:?}. Undecodable records: {}.\n\n",
        g.mapping.spurious.len(),
        g.abstained,
        g.excluded,
        g.undecodable
    )
}

fn markdown(report: &Report<'_>, request: &Request<'_>) -> String {
    let f = report.frozen;
    let outcome = f.abstain.as_ref().map_or_else(
        || "a mapping".to_owned(),
        |reason| format!("no mapping (abstained: {reason}; graded as the empty prediction)"),
    );
    // Compared by bytes as well as name: two corpora.toml names may pin one file.
    let in_sample = if f.corpus == report.corpus || f.corpus_sha256 == *report.corpus_sha256 {
        " **In sample**: the mapping was frozen on this corpus."
    } else {
        ""
    };
    let mut out = format!(
        "# h-measure score\n\n{ALIAS_LIMIT}\n\nMapping {} (sha256 {}): profiler {} proposed {outcome} from the first {} events of {} (sha256 {}); it abstained on {} paths. Scored on {} (sha256 {}), {} records.{in_sample}\n\n",
        request.frozen.display(),
        report.frozen_sha256,
        f.profiler_version,
        f.window,
        f.corpus,
        f.corpus_sha256,
        f.profile.abstained.len(),
        report.corpus,
        report.corpus_sha256,
        report.records,
    );
    for key in &report.keys {
        out += &key_section(key);
    }
    out
}
