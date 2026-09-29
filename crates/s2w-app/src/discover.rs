//! The learned-mapping producer (decision 0025, s2w#197 PR 4a). At `serve` start, for each
//! member source with no effective mapping and at least a window of logged events, it profiles
//! the first window with `s2w-discover`, files the mapping as a `stream-mapping` proposal and
//! accepts it with the `policy` decider. `serve` then resolves routes again (decision 0023).
//!
//! Rules, each pinned by a test: it never re-profiles a routed source; it writes nothing when a
//! proposal with the same (source, identity) exists from any actor, so a restart and a human
//! reject are both stable; the proposal id is a hash of the actor, source, window and identity;
//! it opens the proposal writer per run and drops it; its failures are notes, never fatal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use s2w_discover::{Discovery, PROFILER_VERSION};
use s2w_log::{
    Actor, Decider, LogError, LogPosition, LogReader, NewDecision, NewProposal, Outcome,
    ProposalStore, ReadOnlySqliteProposalStore, SqliteEventLog, SqliteProposalStore,
};
use s2w_model::{SourceId, StreamMapping, fnv1a64_hex};

use crate::Reporter;
use crate::routes::{self, ENVELOPE_FORMAT, MappingEnvelope, Resolution, STREAM_MAPPING_CLASS};

/// Events profiled per source: the first this many of the source, by log position.
pub const DISCOVER_WINDOW: usize = 10_000;

/// The model name on the producer's proposals (decision 0022's heuristic profiler).
pub const PROFILER_MODEL: &str = "h-lite";

/// The policy that accepts the producer's proposals, as written in each decision's basis.
pub const POLICY: &str = "learned-mapping-auto-apply/1";

/// The window and the profiler's thresholds. Production uses [`Default`]; tests shrink both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoverConfig {
    /// Events profiled per source; a source with fewer is not profiled.
    pub window: usize,
    /// The profiler's thresholds.
    pub profiler: s2w_discover::Config,
}

impl Default for DiscoverConfig {
    fn default() -> Self {
        Self {
            window: DISCOVER_WINDOW,
            profiler: s2w_discover::Config::default(),
        }
    }
}

/// The first window of one source's logged events.
struct Window {
    source: SourceId,
    first: LogPosition,
    last: LogPosition,
    payloads: Vec<Vec<u8>>,
}

/// The id and lookup rule of one producer. [`REAL`] is the only one outside tests; the mutant
/// in `tests.rs` shows what each rule prevents.
#[derive(Clone, Copy)]
struct Producer {
    id: fn(&Actor, &SourceId, LogPosition, LogPosition, &str) -> String,
    lookup: bool,
}

const REAL: Producer = Producer {
    id: proposal_id,
    lookup: true,
};

/// The actor on every proposal this producer files.
#[must_use]
pub fn actor() -> Actor {
    Actor::Agent {
        model: PROFILER_MODEL.to_owned(),
        version: PROFILER_VERSION.to_owned(),
    }
}

/// `fnv1a64_hex` over the actor, source, window bounds and mapping identity, each length-
/// prefixed so no two tuples share an encoding. The same log gives the same id; a moved window
/// gives another.
#[must_use]
pub fn proposal_id(
    actor: &Actor,
    source: &SourceId,
    first: LogPosition,
    last: LogPosition,
    identity: &str,
) -> String {
    let actor = match actor {
        Actor::Human { id } => format!("human:{id}"),
        Actor::Agent { model, version } => format!("agent:{model}/{version}"),
    };
    let first = first.as_u64().to_string();
    let last = last.as_u64().to_string();
    let mut bytes = Vec::new();
    for field in [actor.as_str(), source.as_str(), &first, &last, identity] {
        bytes.extend_from_slice(&field.len().to_le_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
    fnv1a64_hex(&bytes)
}

/// The policy decision's basis: the policy, the profiler, the window and the mapping's size.
#[must_use]
pub fn basis(
    first: LogPosition,
    last: LogPosition,
    events: usize,
    mapping: &StreamMapping,
) -> String {
    let types: BTreeSet<&str> = mapping
        .entities
        .iter()
        .map(|rule| rule.type_label.as_str())
        .collect();
    format!(
        "policy={POLICY} profiler={PROFILER_MODEL}/{PROFILER_VERSION} window={}..{} events={events} types={} entity_rules={} relationship_rules={}",
        first.as_u64(),
        last.as_u64(),
        types.len(),
        mapping.entities.len(),
        mapping.relationships.len(),
    )
}

/// Profiles every unrouted member source with a full window and files what the profiler
/// proposes. Returns whether any row was written, so the caller knows to resolve again.
/// Never fails: every problem is a `discover:` note and the source keeps its current routes.
pub(crate) fn run(
    log: &SqliteEventLog,
    log_dir: &Path,
    resolution: &Resolution,
    cfg: &DiscoverConfig,
    reporter: &mut dyn Reporter,
) -> bool {
    run_with(REAL, log, log_dir, resolution, cfg, reporter)
}

fn run_with(
    producer: Producer,
    log: &SqliteEventLog,
    log_dir: &Path,
    resolution: &Resolution,
    cfg: &DiscoverConfig,
    reporter: &mut dyn Reporter,
) -> bool {
    let windows = match windows(log, resolution, cfg.window) {
        Ok(windows) => windows,
        Err(error) => {
            reporter.note(&format!(
                "discover: reading the log failed: {error}; routes unchanged"
            ));
            return false;
        }
    };
    let mut wrote = false;
    for (source, window) in windows {
        match window {
            Ok(window) => wrote |= produce(producer, &window, log_dir, cfg, reporter),
            Err(count) => reporter.note(&format!(
                "discover: {}: {count} events, below the window of {}; not profiled",
                source.as_str(),
                cfg.window
            )),
        }
    }
    wrote
}

/// The first `window` events of each unrouted member source, or its event count when it has
/// fewer. Stops reading once every candidate is full. With no membership rows (a log from
/// before membership, or a test log) every source in the log is a candidate.
fn windows(
    log: &SqliteEventLog,
    resolution: &Resolution,
    window: usize,
) -> Result<BTreeMap<SourceId, Result<Window, usize>>, LogError> {
    let history = log.membership_history()?;
    let mut members = BTreeSet::new();
    for row in &history {
        if !resolution.routes.contains_key(&row.source) && log.is_source_member(&row.source)? {
            members.insert(row.source.clone());
        }
    }
    if !history.is_empty() && members.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut found: BTreeMap<SourceId, Vec<(LogPosition, Vec<u8>)>> = BTreeMap::new();
    for stored in log.read_after(None)? {
        let stored = stored?;
        let source = &stored.event.source;
        if resolution.routes.contains_key(source)
            || (!history.is_empty() && !members.contains(source))
        {
            continue;
        }
        let events = found.entry(source.clone()).or_default();
        if events.len() < window {
            events.push((stored.position, stored.event.payload));
        }
        if !history.is_empty()
            && found.len() == members.len()
            && found.values().all(|events| events.len() >= window)
        {
            break;
        }
    }
    Ok(found
        .into_iter()
        .map(|(source, events)| {
            let window = match (events.first(), events.last()) {
                (Some(first), Some(last)) if events.len() >= window && window > 0 => Ok(Window {
                    source: source.clone(),
                    first: first.0,
                    last: last.0,
                    payloads: events.into_iter().map(|(_, payload)| payload).collect(),
                }),
                _ => Err(events.len()),
            };
            (source, window)
        })
        .collect())
}

/// Profiles one window and, for a mapping not already proposed, appends the proposal and the
/// policy accept. Returns whether rows were written.
fn produce(
    producer: Producer,
    window: &Window,
    log_dir: &Path,
    cfg: &DiscoverConfig,
    reporter: &mut dyn Reporter,
) -> bool {
    let source = &window.source;
    let payloads: Vec<&[u8]> = window.payloads.iter().map(Vec::as_slice).collect();
    let mapping = match s2w_discover::discover(&payloads, &cfg.profiler).1 {
        Discovery::Mapping(mapping) => mapping,
        Discovery::Abstain(reason) => {
            reporter.note(&format!(
                "discover: {}: abstained ({reason}) over {} events",
                source.as_str(),
                payloads.len()
            ));
            return false;
        }
    };
    match file(producer, window, &mapping, log_dir) {
        Ok(Filed::Written { id, identity }) => {
            reporter.note(&format!(
                "discover: {}: proposed mapping {identity} (proposal {id}), accepted by policy {POLICY}",
                source.as_str()
            ));
            true
        }
        Ok(Filed::Exists { id, identity }) => {
            reporter.note(&format!(
                "discover: {}: mapping {identity} is already proposed (proposal {id}); nothing written",
                source.as_str()
            ));
            false
        }
        Err(LogError::Locked) => {
            reporter.note(&format!(
                "discover: {}: store_locked: another writer holds the proposal store; routes unchanged, retried at the next start",
                source.as_str()
            ));
            false
        }
        Err(error) => {
            reporter.note(&format!(
                "discover: {}: {error}; routes unchanged",
                source.as_str()
            ));
            false
        }
    }
}

enum Filed {
    Written { id: String, identity: String },
    Exists { id: String, identity: String },
}

/// Opens the writer (the lock), looks the identity up, appends, and drops the writer.
fn file(
    producer: Producer,
    window: &Window,
    mapping: &StreamMapping,
    log_dir: &Path,
) -> Result<Filed, LogError> {
    let identity = mapping
        .identity()
        .map_err(|error| LogError::Corrupt(format!("discovered mapping: {error}")))?;
    let mut store = SqliteProposalStore::open(log_dir)?;
    if producer.lookup
        && let Some(id) = proposed(log_dir, &window.source, &identity)?
    {
        return Ok(Filed::Exists { id, identity });
    }
    let actor = actor();
    let id = (producer.id)(&actor, &window.source, window.first, window.last, &identity);
    let envelope = MappingEnvelope {
        format: ENVELOPE_FORMAT,
        source: window.source.as_str().to_owned(),
        mapping: mapping.clone(),
    };
    let payload = serde_json::to_vec(&envelope)
        .map_err(|error| LogError::Corrupt(format!("envelope: {error}")))?;
    let now = now_ms()?;
    store.append_proposal(&NewProposal {
        id: id.clone(),
        class: STREAM_MAPPING_CLASS.to_owned(),
        actor,
        snapshot_offset: window.last,
        payload,
        proposed_at_ms: now,
    })?;
    store.append_decision(&NewDecision {
        proposal_id: id.clone(),
        decider: Decider::Policy,
        outcome: Outcome::Accept,
        basis: basis(window.first, window.last, window.payloads.len(), mapping),
        decided_at_ms: now,
    })?;
    drop(store);
    Ok(Filed::Written { id, identity })
}

/// The id of a `stream-mapping` proposal for (`source`, `identity`) from any actor, if one exists.
fn proposed(log_dir: &Path, source: &SourceId, identity: &str) -> Result<Option<String>, LogError> {
    let store = ReadOnlySqliteProposalStore::open(log_dir)?;
    Ok(store
        .proposals()?
        .into_iter()
        .filter(|proposal| proposal.class == STREAM_MAPPING_CLASS)
        .find(|proposal| {
            routes::decode_envelope(&proposal.payload)
                .is_ok_and(|(s, _, id)| &s == source && id == identity)
        })
        .map(|proposal| proposal.id))
}

fn now_ms() -> Result<i64, LogError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| LogError::Corrupt(format!("system clock before epoch: {error}")))?;
    i64::try_from(elapsed.as_millis())
        .map_err(|error| LogError::Corrupt(format!("system clock out of range: {error}")))
}

#[cfg(test)]
pub(crate) mod tests;
