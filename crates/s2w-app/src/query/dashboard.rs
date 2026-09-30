//! The dashboard read (decision 0029): per world, the effective `dashboard-manifest` proposal,
//! resolved by decision 0023's rule, and whether the current mappings still carry everything it
//! names.

use std::path::Path;

use s2w_log::{StoredDecision, StoredProposal};
use s2w_model::{AcceptedMapping, DashboardManifest};
use serde::{Deserialize, Deserializer, Serialize};

use super::QueryError;
use super::proposal_store::open_proposal_reader;
use super::proposals::ActorDto;
use super::resolve::resolve_class;
use super::stream_mapping::{STREAM_MAPPING_CLASS, decode_envelope as decode_mapping};

/// The proposal class whose payloads are [`DashboardEnvelope`]s.
pub const DASHBOARD_MANIFEST_CLASS: &str = "dashboard-manifest";

/// The one dashboard envelope format this build reads.
pub const DASHBOARD_ENVELOPE_FORMAT: u32 = 1;

/// The most attempts per (world, input hash, actor).
pub const MAX_ATTEMPTS: u32 = 3;

/// The largest `provenance.raw`, in bytes.
pub const MAX_RAW_BYTES: usize = 1 << 20;

/// A `dashboard-manifest` proposal's payload (decision 0029). Every key is required, `null`
/// where the value is absent, and unknown keys are refused at every level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardEnvelope {
    /// Always [`DASHBOARD_ENVELOPE_FORMAT`].
    pub format: u32,
    /// The world the manifest presents.
    pub world: String,
    /// The proposer's input hash: 16 lowercase hex digits.
    pub input_hash: String,
    /// 1 to [`MAX_ATTEMPTS`].
    pub attempt: u32,
    /// The manifest, or `null` exactly when [`Provenance::error`] is set.
    #[serde(deserialize_with = "required")]
    pub manifest: Option<DashboardManifest>,
    /// How the manifest was produced. Never part of its identity.
    pub provenance: Provenance,
}

/// How a manifest was produced: context for review and grading, outside the identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// The prompt's hash, 16 lowercase hex digits; `null` for a proposer with no prompt.
    #[serde(deserialize_with = "required")]
    pub prompt_hash: Option<String>,
    /// Input tokens over the attempt's calls.
    #[serde(deserialize_with = "required")]
    pub input_tokens: Option<u64>,
    /// Output tokens over the attempt's calls.
    #[serde(deserialize_with = "required")]
    pub output_tokens: Option<u64>,
    /// Latency over the attempt's calls, in milliseconds.
    #[serde(deserialize_with = "required")]
    pub latency_ms: Option<u64>,
    /// The last reply, at most [`MAX_RAW_BYTES`].
    #[serde(deserialize_with = "required")]
    pub raw: Option<String>,
    /// Why the attempt produced no manifest.
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
}

/// A key that must be present, `null` or not: serde treats a missing `Option` as `None`
/// unless the field names a deserializer.
fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

fn is_hex16(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Parses a `dashboard-manifest` payload and checks every envelope rule of decision 0029.
///
/// # Errors
/// A message naming the failure: not JSON of the envelope's shape, another format, an empty
/// world, a malformed hash, an attempt outside 1 to [`MAX_ATTEMPTS`], an oversize `raw`, or a
/// `manifest` that is not null exactly when `error` is set.
pub fn parse_envelope(payload: &[u8]) -> Result<DashboardEnvelope, String> {
    let envelope: DashboardEnvelope =
        serde_json::from_slice(payload).map_err(|error| format!("payload: {error}"))?;
    if envelope.format != DASHBOARD_ENVELOPE_FORMAT {
        return Err(format!(
            "envelope format {} is not supported; this build reads format \
             {DASHBOARD_ENVELOPE_FORMAT}",
            envelope.format
        ));
    }
    if envelope.world.is_empty() {
        return Err("world: must not be empty".to_owned());
    }
    if !is_hex16(&envelope.input_hash) {
        return Err("input_hash: must be 16 lowercase hex digits".to_owned());
    }
    if !(1..=MAX_ATTEMPTS).contains(&envelope.attempt) {
        return Err(format!(
            "attempt: {} is outside 1 to {MAX_ATTEMPTS}",
            envelope.attempt
        ));
    }
    let provenance = &envelope.provenance;
    if provenance
        .prompt_hash
        .as_deref()
        .is_some_and(|h| !is_hex16(h))
    {
        return Err("provenance.prompt_hash: must be 16 lowercase hex digits".to_owned());
    }
    if provenance
        .raw
        .as_ref()
        .is_some_and(|raw| raw.len() > MAX_RAW_BYTES)
    {
        return Err(format!("provenance.raw: longer than {MAX_RAW_BYTES} bytes"));
    }
    match (&envelope.manifest, &provenance.error) {
        (Some(_), Some(_)) => Err("manifest is set, so provenance.error must be null".to_owned()),
        (None, None) => Err("manifest is null, so provenance.error must be set".to_owned()),
        _ => Ok(envelope),
    }
}

/// Decodes a `dashboard-manifest` payload into its world, manifest and identity. A row whose
/// manifest is null has no identity and is refused here, so resolution excludes it.
///
/// # Errors
/// Every [`parse_envelope`] failure, a null manifest (with its recorded error), or a manifest
/// that fails [`DashboardManifest::validate_shape`].
pub fn decode_envelope(payload: &[u8]) -> Result<(String, DashboardManifest, String), String> {
    let envelope = parse_envelope(payload)?;
    let Some(manifest) = envelope.manifest else {
        let error = envelope.provenance.error.unwrap_or_default();
        return Err(format!("manifest is null: {error}"));
    };
    manifest
        .validate_shape()
        .map_err(|error| format!("manifest: {error}"))?;
    let identity = manifest
        .identity()
        .map_err(|error| format!("manifest: {error}"))?;
    Ok((envelope.world, manifest, identity))
}

/// A `dashboard-manifest` row resolution left out, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExcludedDto {
    /// The proposal id.
    pub proposal_id: String,
    /// The decode, envelope or validation failure.
    pub reason: String,
}

/// A world's dashboard, as `GET /worlds/{w}/dashboard`, MCP `dashboard` and `s2w dashboard
/// show` serve it. Every field but `excluded` is `null`/empty when no manifest is in effect.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DashboardView {
    /// The effective manifest.
    pub manifest: Option<DashboardManifest>,
    /// The earliest accepted proposal carrying the manifest's identity.
    pub proposal_id: Option<String>,
    /// That proposal's author.
    pub actor: Option<ActorDto>,
    /// [`DashboardManifest::identity`].
    pub identity: Option<String>,
    /// Whether the current mappings lack anything the manifest names.
    pub stale: bool,
    /// Each entry the current mappings lack; the viewer falls back for these.
    pub stale_entries: Vec<String>,
    /// This world's rows resolution left out (null manifests included), in proposal order,
    /// plus rows whose world cannot be read at all.
    pub excluded: Vec<ExcludedDto>,
}

/// The world a payload names, if it is JSON with a string `world` at all.
fn payload_world(payload: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    value.get("world")?.as_str().map(str::to_owned)
}

/// The dashboard of `world` from these rows. Pure: the same rows always give the same view.
/// The stale check runs against the mappings the same rows resolve now (decision 0023).
#[must_use]
pub fn dashboard_view(
    world: &str,
    proposals: &[StoredProposal],
    decisions: &[StoredDecision],
) -> DashboardView {
    let resolution = resolve_class(
        DASHBOARD_MANIFEST_CLASS,
        decode_envelope,
        proposals,
        decisions,
    );
    let payload_of = |id: &str| proposals.iter().find(|p| p.id == id);
    let excluded = resolution
        .excluded
        .into_iter()
        .filter(|row| {
            payload_of(&row.proposal_id)
                .and_then(|p| payload_world(&p.payload))
                .is_none_or(|w| w == world)
        })
        .map(|row| ExcludedDto {
            proposal_id: row.proposal_id,
            reason: row.reason,
        })
        .collect();
    let Some(winner) = resolution.winners.get(world) else {
        return DashboardView {
            excluded,
            ..DashboardView::default()
        };
    };
    let current: Vec<AcceptedMapping> =
        resolve_class(STREAM_MAPPING_CLASS, decode_mapping, proposals, decisions)
            .winners
            .into_iter()
            .map(|(source, mapping)| AcceptedMapping {
                source: source.as_str().to_owned(),
                identity: mapping.identity,
                mapping: mapping.value,
            })
            .collect();
    let stale_entries = winner.value.stale_entries(&current);
    DashboardView {
        manifest: Some(winner.value.clone()),
        proposal_id: Some(winner.proposal_id.clone()),
        actor: payload_of(&winner.proposal_id).map(|p| ActorDto::from(&p.actor)),
        identity: Some(winner.identity.clone()),
        stale: !stale_entries.is_empty(),
        stale_entries,
        excluded,
    }
}

/// The dashboard of `world` in `log_dir`'s proposal store. A missing store is an empty view;
/// the read never creates it.
///
/// # Errors
/// [`QueryError::Storage`] if the store exists but cannot be opened or read.
pub fn read_dashboard(log_dir: &Path, world: &str) -> Result<DashboardView, QueryError> {
    let Some(reader) = open_proposal_reader(log_dir)? else {
        return Ok(DashboardView::default());
    };
    Ok(dashboard_view(
        world,
        &reader.proposals()?,
        &reader.decisions()?,
    ))
}

#[cfg(test)]
mod tests;
