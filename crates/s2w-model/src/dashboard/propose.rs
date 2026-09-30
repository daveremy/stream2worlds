//! What a dashboard proposer reads and returns (decision 0029). The DTOs live here because
//! three crates share them: the deterministic proposer in `s2w-discover`, the System 2
//! proposer in `s2w-system2`, and the builder and filer in `s2w-app`. The builder copies the
//! profiler's numbers into [`PathStats`]; no discover type appears here.

use serde::{Deserialize, Serialize};

use super::{AcceptedMapping, DashboardManifest, ManifestContext};
use crate::{FieldPath, StreamMapping};

/// One profiled path's statistics over a source's log tail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathStats {
    /// The path, including any decode prefix.
    pub path: FieldPath,
    /// Tail events that carry it.
    pub count: u64,
    /// Distinct keyable values.
    pub distinct: u64,
    /// Values that were strings.
    pub str_count: u64,
    /// The mean length of those strings in bytes, rounded down; 0 when there are none.
    pub str_len_mean: u64,
    /// Whether the profiler read the path's values as RFC 3339 date-times (decision 0030). A
    /// format-level fact, never a name match: a timestamp is not a label. Left out of the
    /// JSON when false, so an input with no date-time path keeps its hash.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub timestamp: bool,
}

/// One member source with an accepted mapping, profiled over its log tail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    /// The source id.
    pub source: String,
    /// The accepted mapping's identity.
    pub mapping_identity: String,
    /// The accepted mapping.
    pub mapping: StreamMapping,
    /// Events in the profiled tail.
    pub events: u64,
    /// The profiler's event-type path, when it found one.
    pub event_type: Option<FieldPath>,
    /// Every profiled path, sorted by path.
    pub paths: Vec<PathStats>,
    /// The newest decoded events of the tail, oldest first, strings truncated. Untrusted
    /// stream text: a proposer treats it as data only.
    pub sample: Vec<serde_json::Value>,
}

/// Everything a proposer reads, built from the log tail with no world (decision 0029). Its
/// canonical JSON (declaration order) is hashed into the envelope's `input_hash`, so a new
/// mapping identity or a moved tail changes the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestInput {
    /// The world the manifest is for.
    pub world: String,
    /// The member sources that have an accepted mapping, sorted by source id.
    pub sources: Vec<SourceInput>,
}

impl ManifestInput {
    /// The write-path validation context this input implies: its mappings and its paths.
    #[must_use]
    pub fn context(&self) -> ManifestContext {
        ManifestContext {
            mappings: self
                .sources
                .iter()
                .map(|source| AcceptedMapping {
                    source: source.source.clone(),
                    identity: source.mapping_identity.clone(),
                    mapping: source.mapping.clone(),
                })
                .collect(),
            paths: self
                .sources
                .iter()
                .map(|source| {
                    let paths = source.paths.iter().map(|stats| stats.path.clone());
                    (source.source.clone(), paths.collect())
                })
                .collect(),
        }
    }
}

/// Who proposed: recorded as the proposal's `Agent { model, version }` actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposerId {
    /// The model name, as configured (never read from a reply).
    pub model: String,
    /// The model version.
    pub version: String,
}

/// What one proposal attempt cost and returned, for the envelope's provenance. Every field is
/// `None` for a proposer that calls no model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProposerTrace {
    /// Input tokens, summed over the attempt's calls.
    pub input_tokens: Option<u64>,
    /// Output tokens, summed over the attempt's calls.
    pub output_tokens: Option<u64>,
    /// Wall time, summed over the attempt's calls.
    pub latency_ms: Option<u64>,
    /// The last reply's text.
    pub raw: Option<String>,
}

/// A proposer's answer for one attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestOutcome {
    /// A manifest. The filer still validates it before any policy accept.
    Manifest {
        /// The manifest (boxed: it dwarfs the other variants).
        manifest: Box<DashboardManifest>,
        /// Its provenance.
        trace: ProposerTrace,
    },
    /// The attempt failed: a timeout, non-JSON output, an exec failure or a validator refusal.
    /// It is persisted as a null-manifest row.
    Invalid {
        /// Why.
        error: String,
        /// Its provenance.
        trace: ProposerTrace,
    },
    /// Nothing to propose from this input, and why. Nothing is persisted.
    Abstain(String),
}

/// A dashboard manifest proposer (decision 0029).
pub trait ManifestProposer {
    /// Who this proposer is.
    fn id(&self) -> ProposerId;

    /// The hash of the prompt this proposer sends, folded into `input_hash`; `None` for a
    /// proposer with no prompt.
    fn prompt_hash(&self) -> Option<String> {
        None
    }

    /// Proposes a manifest for `input`.
    fn propose(&self, input: &ManifestInput) -> ManifestOutcome;
}
