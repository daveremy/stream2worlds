//! The sha256 pins `freeze` and `score` check: `research/h-measure/keys.toml` (every answer
//! key) and `corpora.toml` (every corpus, with its role). A key or corpus whose bytes do not
//! hash to its pin is refused, and so is one that is not pinned at all.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use super::key::KeySpec;
use crate::discover_replay::envelopes;

/// Where the measurement's data lives, relative to the workspace root.
pub(crate) const DATA: &str = "research/h-measure";

/// One `[[key]]` row of `keys.toml`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct KeyPin {
    /// The key file, relative to [`DATA`].
    pub file: String,
    /// Which reading of the key it is (`base`, or a sensitivity variant's name).
    pub variant: String,
    /// Lower-case hex sha256 of the file's bytes.
    pub sha256: String,
}

/// What a corpus is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    /// The development window: the only corpus a mapping is frozen on.
    Development,
    /// A held-out span: scored, never used to build, tune or freeze.
    Heldout,
    /// Kept for a later change; `score` refuses it.
    Reserved,
}

/// One `[corpus.<name>]` table of `corpora.toml`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CorpusPin {
    pub role: Role,
    /// The corpus file, relative to the corpus directory.
    pub file: String,
    /// SSE frames in the file.
    pub events: usize,
    /// Lower-case hex sha256 of the file's bytes.
    pub sha256: String,
}

#[derive(Deserialize)]
struct KeyRows {
    key: Vec<KeyPin>,
}

#[derive(Deserialize)]
struct CorpusRows {
    corpus: BTreeMap<String, CorpusPin>,
}

/// Both manifests.
pub(crate) struct Pins {
    keys: BTreeMap<String, KeyPin>,
    corpora: BTreeMap<String, CorpusPin>,
}

pub(crate) use crate::sha256;

pub(super) fn toml_file<T: for<'de> Deserialize<'de>>(
    root: &Path,
    name: &str,
) -> Result<T, String> {
    let rel = format!("{DATA}/{name}");
    let text = fs::read_to_string(root.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
    toml::from_str(&text).map_err(|e| format!("{rel}: {e}"))
}

impl Pins {
    /// Reads `keys.toml` and `corpora.toml`. A key file pinned twice is refused.
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let mut keys = BTreeMap::new();
        for pin in toml_file::<KeyRows>(root, "keys.toml")?.key {
            if let Some(old) = keys.insert(pin.file.clone(), pin) {
                return Err(format!("keys.toml pins {} twice", old.file));
            }
        }
        let corpora = toml_file::<CorpusRows>(root, "corpora.toml")?.corpus;
        Ok(Self { keys, corpora })
    }

    /// Every pin, as `key <file>` to its sha256 and `corpus <name>` to its role, file, event
    /// count and sha256: what `freeze` records and `score` compares against. A role change after
    /// a freeze counts as a changed pin, except `reserved` to `heldout` (opening the span,
    /// s2w#277): see [`opened`].
    pub(crate) fn all(&self) -> BTreeMap<String, String> {
        let keys = self
            .keys
            .values()
            .map(|pin| (format!("key {}", pin.file), pin.sha256.clone()));
        let corpora = self.corpora.iter().map(|(name, pin)| {
            let value = format!("{:?} {} {} {}", pin.role, pin.file, pin.events, pin.sha256);
            (format!("corpus {name}"), value)
        });
        keys.chain(corpora).collect()
    }

    /// Reads a pinned key file, checks its hash, then parses and validates it.
    pub(crate) fn key(&self, root: &Path, file: &str) -> Result<(KeyPin, KeySpec), String> {
        let pin = self.keys.get(file).ok_or_else(|| {
            format!("{file} is not pinned in {DATA}/keys.toml; pin a key before scoring with it")
        })?;
        let rel = format!("{DATA}/{file}");
        let bytes = fs::read(root.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
        matches(&rel, &bytes, &pin.sha256)?;
        let spec: KeySpec = serde_json::from_slice(&bytes).map_err(|e| format!("{rel}: {e}"))?;
        spec.validate().map_err(|e| format!("{rel}: {e}"))?;
        Ok((pin.clone(), spec))
    }

    /// Checks every pinned key file against its pin.
    pub(crate) fn verify_keys(&self, root: &Path) -> Result<(), String> {
        self.keys
            .keys()
            .try_for_each(|file| self.key(root, file).map(|_| ()))
    }

    pub(crate) fn corpus(&self, name: &str) -> Result<&CorpusPin, String> {
        self.corpora
            .get(name)
            .ok_or_else(|| format!("no corpus {name:?} in {DATA}/corpora.toml"))
    }

    /// Reads a pinned corpus from `dir`, checks its hash, and returns its events as stored
    /// envelopes; a count other than the pinned one is refused.
    pub(crate) fn payloads(&self, dir: &Path, name: &str) -> Result<Vec<Value>, String> {
        let pin = self.corpus(name)?;
        let path = dir.join(&pin.file);
        let shown = path.display().to_string();
        let bytes = fs::read(&path).map_err(|e| format!("{shown}: {e}"))?;
        matches(&shown, &bytes, &pin.sha256)?;
        let text = String::from_utf8(bytes).map_err(|e| format!("{shown}: {e}"))?;
        let payloads = envelopes(&text).map_err(|e| format!("{shown}: {e}"))?;
        if payloads.len() == pin.events {
            Ok(payloads)
        } else {
            Err(format!(
                "{shown}: {} events, the pin says {}",
                payloads.len(),
                pin.events
            ))
        }
    }
}

fn matches(what: &str, bytes: &[u8], pinned: &str) -> Result<(), String> {
    let got = sha256(bytes);
    if got == pinned {
        Ok(())
    } else {
        Err(format!(
            "{what}: sha256 {got} does not match its pin {pinned}; a pinned file never changes (a new key is a new file and a new pin, a re-captured corpus a new manifest entry)"
        ))
    }
}

/// True when a corpus pin, as [`Pins::all`] writes it (`"{role:?} {file} {events} {sha256}"`),
/// differs from its recorded value only by the role going from `Reserved` to `Heldout`: the
/// span was opened after the freeze (s2w#277).
pub(crate) fn opened(name: &str, then: &str, now: &str) -> bool {
    let reserved = format!("{:?} ", Role::Reserved);
    let heldout = format!("{:?} ", Role::Heldout);
    name.starts_with("corpus ")
        && matches!(
            (then.strip_prefix(&reserved), now.strip_prefix(&heldout)),
            (Some(rest_then), Some(rest_now)) if rest_then == rest_now
        )
}
