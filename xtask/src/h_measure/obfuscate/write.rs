//! Writing a run's outputs and metadata: every output is new, and a reused metadata file is
//! replaced through a staged sibling.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::super::pins::DATA;
use super::meta::{Meta, Written};
use super::{Request, check_request};

/// Writes the outputs and the metadata: this run's record alone, or appended to the reused
/// one (which refuses a window it already holds before anything is written).
pub(super) fn save(
    request: &Request<'_>,
    prior: Option<Meta>,
    meta: Meta,
    written: &[(PathBuf, Vec<u8>, Option<usize>)],
) -> Result<Meta, String> {
    let reused = prior.is_some();
    let meta = match prior {
        Some(mut prior) => {
            prior.absorb(meta)?;
            prior
        }
        None => meta,
    };
    for (path, bytes, _) in written {
        write_new(path, bytes)?;
    }
    let text = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())? + "\n";
    if reused {
        replace(request.meta, text.as_bytes())?;
    } else {
        write_new(request.meta, text.as_bytes())?;
    }
    Ok(meta)
}

/// The output paths, each refused if it exists.
pub(super) struct Outputs {
    pub corpora: Vec<PathBuf>,
    pub keys: Vec<PathBuf>,
}

impl Outputs {
    pub(super) fn plan(root: &Path, request: &Request<'_>) -> Result<Self, String> {
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
        let staged = staged(request.meta);
        let mut planned = corpora.iter().chain(&keys).chain(std::iter::once(&staged));
        if let Some(found) = planned.find(|path| path.exists()) {
            return Err(format!(
                "{} exists; an output is never overwritten (a new run is a new replicate name or a deleted file)",
                found.display()
            ));
        }
        Ok(Self { corpora, keys })
    }
}

/// Records each written file's name and sha256 in the metadata.
pub(super) fn record(
    request: &Request<'_>,
    written: &[(PathBuf, Vec<u8>, Option<usize>)],
    meta: &mut Meta,
) {
    let shown = |path: &Path| {
        path.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    let (corpora, keys) = written.split_at(request.corpora.len());
    for (name, (path, bytes, events)) in request.corpora.iter().zip(corpora) {
        let entry = Written {
            file: shown(path),
            events: *events,
            sha256: crate::sha256(bytes),
        };
        meta.outputs.insert(name.clone(), entry);
    }
    for (file, (path, bytes, _)) in request.keys.iter().zip(keys) {
        let entry = Written {
            file: shown(path),
            events: None,
            sha256: crate::sha256(bytes),
        };
        meta.keys.insert(file.clone(), entry);
    }
}

/// Rewrites the reused metadata through a new sibling file and a rename, so a failed write
/// leaves the previous record whole.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let staged = staged(path);
    write_new(&staged, bytes)?;
    fs::rename(&staged, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Where a reused metadata file is staged before its rename; one left by a failed run is
/// refused before anything is written.
fn staged(meta: &Path) -> PathBuf {
    meta.with_extension("json.new")
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
