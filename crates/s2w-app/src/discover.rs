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
    PROPOSAL_DATABASE_FILE, ProposalStore, ReadOnlySqliteProposalStore, SqliteEventLog,
    SqliteProposalStore, StoredProposal, members_at,
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
        bytes.extend_from_slice(&u64::try_from(field.len()).unwrap_or(u64::MAX).to_le_bytes());
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

/// When the producer runs, which only changes what its notes promise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trigger {
    /// Before the registry is built: a mapping filed now routes this start.
    Start,
    /// After a bridge poll (s2w#197 PR 4b): the rows land and the live rebuild routes them.
    InRun,
}

/// What one producer pass did.
#[derive(Debug, Default)]
pub(crate) struct Ran {
    /// Routes should be resolved again: rows were written, or a decision recorded since the
    /// caller's resolution routes a source.
    pub(crate) resolve_again: bool,
    /// Sources whose window was full, whatever the pass then did with them.
    pub(crate) windowed: BTreeSet<SourceId>,
    /// Sources whose rows were not written because another writer held the store.
    pub(crate) locked: BTreeSet<SourceId>,
}

/// Profiles every unrouted member source with a full window and files what the profiler
/// proposes. Never fails: every problem is a `discover:` note and the source keeps its current
/// routes.
pub(crate) fn run(
    log: &SqliteEventLog,
    log_dir: &Path,
    resolution: &Resolution,
    cfg: &DiscoverConfig,
    reporter: &mut dyn Reporter,
) -> Ran {
    run_with(
        REAL,
        (log, log_dir),
        (resolution, None),
        (cfg, Trigger::Start),
        reporter,
    )
}

/// As [`run`], for one source only, after a bridge poll: it reads that source's window and
/// stops, and its notes say the live rebuild applies the mapping. It resolves the routes
/// first, so a source routed since start-up (by another actor) is not read or profiled.
/// Returns whether the rows met a held writer lock, so the caller tries again later.
pub(crate) fn run_one(
    log: &SqliteEventLog,
    log_dir: &Path,
    source: &SourceId,
    cfg: &DiscoverConfig,
    reporter: &mut dyn Reporter,
) -> bool {
    // A source routed since start-up is filtered out before its window is read; an unreadable
    // store falls through to `file`, whose lock-held re-resolution reports it.
    let resolution = routes::load(log_dir).unwrap_or_default();
    run_with(
        REAL,
        (log, log_dir),
        (&resolution, Some(source)),
        (cfg, Trigger::InRun),
        reporter,
    )
    .locked
    .contains(source)
}

fn run_with(
    producer: Producer,
    (log, log_dir): (&SqliteEventLog, &Path),
    (resolution, only): (&Resolution, Option<&SourceId>),
    (cfg, trigger): (&DiscoverConfig, Trigger),
    reporter: &mut dyn Reporter,
) -> Ran {
    let windows = match windows(log, resolution, only, cfg.window) {
        Ok(windows) => windows,
        Err(error) => {
            reporter.note(&format!(
                "discover: reading the log failed: {error}; routes unchanged"
            ));
            return Ran::default();
        }
    };
    let mut ran = Ran::default();
    for (source, window) in windows {
        match window {
            Ok(window) => {
                ran.windowed.insert(source.clone());
                match produce(producer, &window, log_dir, (cfg, trigger), reporter) {
                    Produced::Wrote => ran.resolve_again = true,
                    Produced::Nothing => {}
                    Produced::Locked => {
                        ran.locked.insert(source);
                    }
                }
            }
            Err(count) => reporter.note(&format!(
                "discover: {}: {count} events, below the window of {}; not profiled",
                source.as_str(),
                cfg.window
            )),
        }
    }
    ran
}

/// The first `window` events of each unrouted member source (or of `only`), or its event count
/// when it has fewer. Stops reading once every candidate is full. With no membership rows (a log
/// from before membership, or a test log) every source in the log is a candidate, and the whole
/// log is read unless `only` names the one source wanted.
fn windows(
    log: &SqliteEventLog,
    resolution: &Resolution,
    only: Option<&SourceId>,
    window: usize,
) -> Result<BTreeMap<SourceId, Result<Window, usize>>, LogError> {
    let history = log.membership_history()?;
    let wanted = |source: &SourceId| {
        !resolution.routes.contains_key(source) && only.is_none_or(|only| only == source)
    };
    let members: BTreeSet<SourceId> = members_at(&history, u64::MAX)
        .into_iter()
        .filter(|source| wanted(source))
        .collect();
    if window == 0 || (!history.is_empty() && members.is_empty()) {
        return Ok(BTreeMap::new());
    }
    // Every member starts with an entry, so one with no logged events still gets its note.
    let mut found: BTreeMap<SourceId, Vec<(LogPosition, Vec<u8>)>> = members
        .iter()
        .map(|source| (source.clone(), Vec::new()))
        .collect();
    // With membership rows, or with one named source, the candidates are known up front.
    let known = !history.is_empty() || only.is_some();
    let expected = if history.is_empty() { 1 } else { members.len() };
    for stored in log.read_after(None)? {
        let stored = stored?;
        let source = &stored.event.source;
        if !wanted(source) || (!history.is_empty() && !members.contains(source)) {
            continue;
        }
        let events = found.entry(source.clone()).or_default();
        if events.len() < window {
            events.push((stored.position, stored.event.payload));
        }
        if known && found.len() == expected && found.values().all(|events| events.len() >= window) {
            break;
        }
    }
    Ok(found
        .into_iter()
        .map(|(source, events)| {
            let window = match (events.first(), events.last()) {
                (Some(first), Some(last)) if events.len() >= window => Ok(Window {
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

/// What [`produce`] did with one window.
enum Produced {
    Wrote,
    Nothing,
    Locked,
}

/// Profiles one window and, for a mapping not already proposed, appends the proposal and the
/// policy accept. A window this profiler version already filed and someone already decided is
/// not profiled again (a human reject would otherwise cost one profile per start, and one per
/// poll in-run).
fn produce(
    producer: Producer,
    window: &Window,
    log_dir: &Path,
    (cfg, trigger): (&DiscoverConfig, Trigger),
    reporter: &mut dyn Reporter,
) -> Produced {
    let source = window.source.as_str();
    if producer.lookup
        && let Some(id) = decided_window(log_dir, window)
    {
        reporter.note(&format!(
            "discover: {source}: window {}..{} was filed by this profiler and decided (proposal {id}); not profiled again",
            window.first.as_u64(),
            window.last.as_u64()
        ));
        return Produced::Nothing;
    }
    let payloads: Vec<&[u8]> = window.payloads.iter().map(Vec::as_slice).collect();
    let mapping = match s2w_discover::discover(&payloads, &cfg.profiler).1 {
        Discovery::Mapping(mapping) => mapping,
        Discovery::Abstain(reason) => {
            reporter.note(&format!(
                "discover: {source}: abstained ({reason}) over {} events",
                payloads.len()
            ));
            return Produced::Nothing;
        }
    };
    let (effect, retry) = match trigger {
        Trigger::Start => ("", "retried at the next start".to_owned()),
        Trigger::InRun => (
            "; the live rebuild applies it",
            format!("retried in {} polls", in_run::LOCK_RETRY_POLLS),
        ),
    };
    let (note, produced) = match file(producer, window, &mapping, log_dir) {
        Ok(Filed::Written { id, identity }) => (
            format!(
                "proposed mapping {identity} (proposal {id}), accepted by policy {POLICY}{effect}"
            ),
            Produced::Wrote,
        ),
        Ok(Filed::Completed { id, identity }) => (
            format!(
                "mapping {identity} (proposal {id}) had no decision; accepted by policy {POLICY}{effect}"
            ),
            Produced::Wrote,
        ),
        Ok(Filed::Exists { id, identity }) => (
            format!("mapping {identity} is already proposed (proposal {id}); nothing written"),
            Produced::Nothing,
        ),
        Ok(Filed::Routed) => (
            format!("routed by a decision recorded since start-up; nothing written{effect}"),
            Produced::Wrote,
        ),
        Err(Unfiled::Locked) => (
            format!(
                "store_locked: another writer holds the proposal store; routes unchanged, {retry}"
            ),
            Produced::Locked,
        ),
        Err(Unfiled::Failed(error)) => (format!("{error}; routes unchanged"), Produced::Nothing),
    };
    reporter.note(&format!("discover: {source}: {note}"));
    produced
}

/// The id of a proposal this producer (same actor, so same profiler version) filed for this
/// source and window, when any decision names it. Read without the writer lock; a store that
/// cannot be read here is left to [`file`], which reads it again under the lock.
fn decided_window(log_dir: &Path, window: &Window) -> Option<String> {
    if !log_dir.join(PROPOSAL_DATABASE_FILE).try_exists().ok()? {
        return None;
    }
    let store = ReadOnlySqliteProposalStore::open(log_dir).ok()?;
    let proposals = store.proposals().ok()?;
    let decisions = store.decisions().ok()?;
    // A source routed since start-up is `file`'s `Routed` case: skipping here would hide an
    // accept that landed after `routes::load` and leave the source unrouted until a restart.
    if routes::resolve(&proposals, &decisions)
        .routes
        .contains_key(&window.source)
    {
        return None;
    }
    let decided: BTreeSet<&str> = decisions.iter().map(|d| d.proposal_id.as_str()).collect();
    let actor = actor();
    proposals
        .iter()
        .filter(|p| {
            p.class == STREAM_MAPPING_CLASS
                && p.actor == actor
                && p.snapshot_offset == window.last
                && decided.contains(p.id.as_str())
        })
        .find(|p| {
            routes::decode_envelope(&p.payload).is_ok_and(|(source, ..)| source == window.source)
        })
        .map(|p| p.id.clone())
}

/// What [`file`] did.
enum Filed {
    /// A new proposal and its policy accept.
    Written { id: String, identity: String },
    /// This producer's own proposal from an earlier start whose accept never landed (the
    /// process stopped between the two appends): the accept, now.
    Completed { id: String, identity: String },
    /// A proposal for this (source, identity) exists from some actor and has a decision.
    Exists { id: String, identity: String },
    /// A decision recorded after the start-up resolution routes the source already.
    Routed,
}

/// Why [`file`] wrote nothing.
enum Unfiled {
    /// Another writer holds the proposal store (the CLI or MCP `decision_record`).
    Locked,
    /// Anything else, as a message.
    Failed(String),
}

impl From<LogError> for Unfiled {
    fn from(error: LogError) -> Self {
        match error {
            LogError::Locked => Self::Locked,
            other => Self::Failed(other.to_string()),
        }
    }
}

/// Opens the writer (the lock), re-reads the store under it, and appends what is missing. The
/// check and the write see the same store: no other writer can land between them.
fn file(
    producer: Producer,
    window: &Window,
    mapping: &StreamMapping,
    log_dir: &Path,
) -> Result<Filed, Unfiled> {
    let identity = mapping
        .identity()
        .map_err(|error| Unfiled::Failed(format!("discovered mapping: {error}")))?;
    let mut store = SqliteProposalStore::open(log_dir)?;
    let proposals = store.proposals()?;
    let decisions = store.decisions()?;
    if routes::resolve(&proposals, &decisions)
        .routes
        .contains_key(&window.source)
    {
        return Ok(Filed::Routed);
    }
    let actor = actor();
    let id = (producer.id)(&actor, &window.source, window.first, window.last, &identity);
    let accept = NewDecision {
        proposal_id: id.clone(),
        decider: Decider::Policy,
        outcome: Outcome::Accept,
        basis: basis(window.first, window.last, window.payloads.len(), mapping),
        decided_at_ms: now_ms()?,
    };
    let existing = proposed(&proposals, &window.source, &identity);
    if producer.lookup && !existing.is_empty() {
        // Complete only this producer's own proposal, and only while it is the sole row for the
        // identity and undecided: any other actor's row, or any decision, means someone has
        // already spoken for this mapping.
        let own_undecided = existing
            .iter()
            .all(|p| *p == id && !decisions.iter().any(|d| d.proposal_id == *p));
        if !own_undecided {
            let shown = existing.iter().find(|p| **p != id).unwrap_or(&existing[0]);
            return Ok(Filed::Exists {
                id: shown.clone(),
                identity,
            });
        }
        store.append_decision(&accept)?;
        return Ok(Filed::Completed { id, identity });
    }
    let envelope = MappingEnvelope {
        format: ENVELOPE_FORMAT,
        source: window.source.as_str().to_owned(),
        mapping: mapping.clone(),
    };
    let payload = serde_json::to_vec(&envelope)
        .map_err(|error| Unfiled::Failed(format!("envelope: {error}")))?;
    store.append_proposal(&NewProposal {
        id: id.clone(),
        class: STREAM_MAPPING_CLASS.to_owned(),
        actor,
        snapshot_offset: window.last,
        payload,
        proposed_at_ms: accept.decided_at_ms,
    })?;
    store.append_decision(&accept)?;
    Ok(Filed::Written { id, identity })
}

/// The ids of every `stream-mapping` proposal for (`source`, `identity`), from any actor, in
/// store order.
fn proposed(proposals: &[StoredProposal], source: &SourceId, identity: &str) -> Vec<String> {
    proposals
        .iter()
        .filter(|proposal| proposal.class == STREAM_MAPPING_CLASS)
        .filter(|proposal| {
            routes::decode_envelope(&proposal.payload)
                .is_ok_and(|(s, _, id)| &s == source && id == identity)
        })
        .map(|proposal| proposal.id.clone())
        .collect()
}

fn now_ms() -> Result<i64, Unfiled> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Unfiled::Failed(format!("system clock before epoch: {error}")))?;
    i64::try_from(elapsed.as_millis())
        .map_err(|error| Unfiled::Failed(format!("system clock out of range: {error}")))
}

pub(crate) mod in_run;

#[cfg(test)]
pub(crate) mod tests;
