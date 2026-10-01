//! The dashboard proposer's filer (decision 0029, s2w#301). It reads each member source's log
//! tail, builds the proposer's input, asks a [`ManifestProposer`] for a manifest, and files the
//! answer as a `dashboard-manifest` proposal with a `policy` decision.
//!
//! Rules, each pinned by a test: the same log gives the same input hash, and a (world, input
//! hash, actor) that already has a manifest row is never filed again, so a re-run is a no-op;
//! a failed attempt is a null-manifest row with a policy reject, and at most [`MAX_ATTEMPTS`]
//! are filed per input; the proposal id is a hash of the proposer, world, input hash and
//! attempt; the proposer runs outside the writer lock, and the store is read again under it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use s2w_log::{
    Actor, Decider, LogError, LogPosition, LogReader, NewDecision, NewProposal, Outcome,
    ProposalStore, ReadOnlySqliteEventLog, SqliteProposalStore, StoredDecision, StoredEvent,
    StoredProposal, members_at,
};
use s2w_model::{
    DashboardManifest, Fnv64, ManifestInput, ManifestOutcome, ManifestProposer, ProposerId,
    ProposerTrace, SourceId, SourceInput, StreamMapping,
};
use s2w_system2::{ExecProvider, ExecSetupError, System2Proposer};
use serde::Serialize;

use crate::proposals::check_identity;
use crate::query::{
    DASHBOARD_ENVELOPE_FORMAT, DASHBOARD_MANIFEST_CLASS, DashboardEnvelope, MAX_ATTEMPTS,
    MAX_RAW_BYTES, Provenance, QueryError, STREAM_MAPPING_CLASS, decode_envelope as decode_mapping,
    open_proposal_reader, parse_dashboard_envelope, resolve_class,
};

/// The policy that decides the filer's proposals, as written in each decision's basis.
pub const POLICY: &str = "dashboard-auto-apply/1";

/// Events per source the input is built from: the newest this many of the source.
pub const TAIL_EVENTS: usize = 2_000;

/// Events per source the input carries verbatim: the newest this many of the tail.
pub const SAMPLE_EVENTS: usize = 40;

/// The longest string in a sampled event, in characters.
pub const SAMPLE_STRING_CHARS: usize = 200;

/// The newest `k` events of each source in `targets`, oldest first. It reads a window of the
/// log's end, `2k` positions at first, and doubles the window until every target has `k` events
/// or the window covers the whole log. Events appended after the first head read are ignored,
/// so every pass reads the same log.
///
/// # Errors
/// Any log read failure.
pub fn read_tail(
    log: &impl LogReader,
    targets: &BTreeSet<SourceId>,
    k: usize,
) -> Result<BTreeMap<SourceId, VecDeque<StoredEvent>>, LogError> {
    let Some(head) = log.read_head()? else {
        return Ok(BTreeMap::new());
    };
    if targets.is_empty() || k == 0 {
        return Ok(BTreeMap::new());
    }
    let head = head.as_u64();
    let mut window = u64::try_from(k).unwrap_or(u64::MAX).saturating_mul(2);
    loop {
        let start = if head <= window {
            None
        } else {
            LogPosition::from_u64(head - window)
        };
        let mut tails: BTreeMap<SourceId, VecDeque<StoredEvent>> = BTreeMap::new();
        for stored in log.read_after(start)? {
            let stored = stored?;
            if stored.position.as_u64() > head {
                break;
            }
            if !targets.contains(&stored.event.source) {
                continue;
            }
            let tail = tails.entry(stored.event.source.clone()).or_default();
            tail.push_back(stored);
            if tail.len() > k {
                tail.pop_front();
            }
        }
        let full = tails.len() == targets.len() && tails.values().all(|tail| tail.len() >= k);
        if start.is_none() || full {
            return Ok(tails);
        }
        window = window.saturating_mul(2);
    }
}

/// Every source with an accepted `stream-mapping`, and its mapping identity and mapping, among
/// the log's members. With no membership rows every mapped source is a member.
fn mapped_members(
    log: &ReadOnlySqliteEventLog,
    proposals: &[StoredProposal],
    decisions: &[StoredDecision],
) -> Result<BTreeMap<SourceId, (String, StreamMapping)>, LogError> {
    let history = log.membership_history()?;
    let members: BTreeSet<SourceId> = members_at(&history, u64::MAX).into_iter().collect();
    Ok(
        resolve_class(STREAM_MAPPING_CLASS, decode_mapping, proposals, decisions)
            .winners
            .into_iter()
            .filter(|(source, _)| history.is_empty() || members.contains(source))
            .map(|(source, winner)| (source, (winner.identity, winner.value)))
            .collect(),
    )
}

/// The proposer's input for `world` from each mapped source's tail, and the newest position
/// the tails read (the proposal's snapshot offset). A mapped source with no logged events is
/// left out. Pure: the same tails always give the same input.
#[must_use]
pub fn build_input(
    world: &str,
    tails: &BTreeMap<SourceId, VecDeque<StoredEvent>>,
    mapped: &BTreeMap<SourceId, (String, StreamMapping)>,
) -> (ManifestInput, Option<LogPosition>) {
    let mut snapshot: Option<LogPosition> = None;
    let mut sources = Vec::new();
    for (source, (identity, mapping)) in mapped {
        let Some(tail) = tails.get(source).filter(|tail| !tail.is_empty()) else {
            continue;
        };
        if let Some(last) = tail.back() {
            snapshot = Some(snapshot.map_or(last.position, |s| s.max(last.position)));
        }
        let payloads: Vec<&[u8]> = tail.iter().map(|s| s.event.payload.as_slice()).collect();
        let profile = s2w_discover::discover(&payloads, &s2w_discover::Config::default()).0;
        let skip = payloads.len().saturating_sub(SAMPLE_EVENTS);
        let sample = payloads[skip..]
            .iter()
            .filter_map(|payload| {
                s2w_discover::manifest::sample_event(payload, &profile.decode, SAMPLE_STRING_CHARS)
            })
            .collect();
        sources.push(SourceInput {
            source: source.as_str().to_owned(),
            mapping_identity: identity.clone(),
            mapping: mapping.clone(),
            events: u64::try_from(tail.len()).unwrap_or(u64::MAX),
            event_type: profile.event_type.clone(),
            paths: s2w_discover::manifest::path_stats(&profile),
            sample,
        });
    }
    let input = ManifestInput {
        world: world.to_owned(),
        sources,
    };
    (input, snapshot)
}

/// `fnv1a64_hex` over length-prefixed fields, so no two tuples share an encoding.
fn hash_fields(fields: &[&[u8]]) -> String {
    let mut hasher = Fnv64::new();
    for field in fields {
        hasher.write_field(field);
    }
    format!("{:016x}", hasher.finish())
}

/// The input hash: the input's canonical JSON and the proposer's prompt hash (empty for none).
///
/// # Errors
/// The input does not serialize (it always does; the error is kept, not unwrapped).
pub fn input_hash(input: &ManifestInput, prompt_hash: Option<&str>) -> Result<String, String> {
    let json = serde_json::to_vec(input).map_err(|error| format!("input: {error}"))?;
    Ok(hash_fields(&[&json, prompt_hash.unwrap_or("").as_bytes()]))
}

/// The proposal id: the proposer's model and version, the world, the input hash and the
/// attempt. A new attempt, input or proposer gives another id.
#[must_use]
pub fn proposal_id(proposer: &ProposerId, world: &str, input_hash: &str, attempt: u32) -> String {
    let attempt = attempt.to_string();
    hash_fields(&[
        proposer.model.as_bytes(),
        proposer.version.as_bytes(),
        world.as_bytes(),
        input_hash.as_bytes(),
        attempt.as_bytes(),
    ])
}

/// The accept's basis: the policy, the proposer, the world, what the manifest is built on and
/// its size.
#[must_use]
pub fn accept_basis(proposer: &ProposerId, world: &str, manifest: &DashboardManifest) -> String {
    let built_on: Vec<String> = manifest
        .built_on
        .iter()
        .map(|b| format!("{}:{}", b.source, b.mapping))
        .collect();
    format!(
        "policy={POLICY} proposer={}/{} world={world} built_on={} types={} events={} roles={}",
        proposer.model,
        proposer.version,
        built_on.join(","),
        manifest.types.len(),
        manifest.events.as_ref().map_or(0, Vec::len),
        manifest.roles.len(),
    )
}

/// The reject's basis for a null-manifest row.
#[must_use]
pub fn reject_basis(error: &str) -> String {
    format!("invalid: {error}")
}

/// What the filer does next for one (world, input hash, proposer).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Plan {
    /// This proposer's row has no decision (the process stopped between the two appends):
    /// decide it now.
    Complete {
        id: String,
        attempt: u32,
        decision: NewDecision,
    },
    /// Nothing to do, and why.
    Skip(String),
    /// Ask the proposer, and file its answer as this attempt.
    Propose { attempt: u32 },
}

/// The next step, from the store's rows. Only this proposer's rows for this world and input
/// hash count; a row whose envelope does not parse is not this filer's.
fn plan(
    proposals: &[StoredProposal],
    decisions: &[StoredDecision],
    proposer: &ProposerId,
    world: &str,
    input_hash: &str,
) -> Result<Plan, String> {
    let actor = actor(proposer);
    let ours: Vec<(&StoredProposal, DashboardEnvelope)> = proposals
        .iter()
        .filter(|p| p.class == DASHBOARD_MANIFEST_CLASS && p.actor == actor)
        .filter_map(|p| parse_dashboard_envelope(&p.payload).ok().map(|e| (p, e)))
        .filter(|(_, e)| e.world == world && e.input_hash == input_hash)
        .collect();
    let decided: BTreeSet<&str> = decisions.iter().map(|d| d.proposal_id.as_str()).collect();
    if let Some((row, envelope)) = ours.iter().find(|(p, _)| !decided.contains(p.id.as_str())) {
        let (outcome, basis) = match &envelope.manifest {
            Some(manifest) => (Outcome::Accept, accept_basis(proposer, world, manifest)),
            None => (
                Outcome::Reject,
                reject_basis(envelope.provenance.error.as_deref().unwrap_or("")),
            ),
        };
        return Ok(Plan::Complete {
            id: row.id.clone(),
            attempt: envelope.attempt,
            decision: NewDecision {
                proposal_id: row.id.clone(),
                decider: Decider::Policy,
                outcome,
                basis,
                decided_at_ms: now_ms()?,
            },
        });
    }
    if let Some((row, _)) = ours.iter().find(|(_, e)| e.manifest.is_some()) {
        return Ok(Plan::Skip(format!(
            "this input already has a manifest from this proposer (proposal {})",
            row.id
        )));
    }
    let attempts = u32::try_from(ours.len()).unwrap_or(u32::MAX);
    if attempts >= MAX_ATTEMPTS {
        return Ok(Plan::Skip(format!(
            "{attempts} attempts on this input failed; the most is {MAX_ATTEMPTS}"
        )));
    }
    Ok(Plan::Propose {
        attempt: attempts + 1,
    })
}

fn actor(proposer: &ProposerId) -> Actor {
    Actor::Agent {
        model: proposer.model.clone(),
        version: proposer.version.clone(),
    }
}

/// What [`propose`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// A proposal and its policy decision were appended.
    Filed,
    /// An undecided proposal from an earlier run got its policy decision.
    Completed,
    /// Nothing was written; see `reason`.
    Skipped,
    /// The proposer had nothing to propose; see `reason`.
    Abstained,
    /// `--dry-run`: what a run would file; nothing was written.
    DryRun,
}

/// What one [`propose`] run did, for the CLI's output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposeReport {
    /// The world.
    pub world: String,
    /// The proposer, `model/version`.
    pub actor: String,
    /// The input hash.
    pub input_hash: String,
    /// What happened.
    pub action: Action,
    /// The proposal filed, completed or (dry run) that would be.
    pub proposal_id: Option<String>,
    /// Its attempt, 1 to [`MAX_ATTEMPTS`].
    pub attempt: Option<u32>,
    /// `accept` or `reject`.
    pub decision: Option<String>,
    /// The decision's basis.
    pub basis: Option<String>,
    /// Why nothing was written, for `skipped` and `abstained`.
    pub reason: Option<String>,
    /// The envelope a dry run would file.
    pub envelope: Option<DashboardEnvelope>,
}

/// Builds the input for `world` from `log_dir`, asks `proposer`, and files its answer. With
/// `dry_run` it writes nothing and reports the envelope it would file.
///
/// # Errors
/// [`QueryError::StoreLocked`] when another writer holds the proposal store;
/// [`QueryError::Storage`] when the log or store cannot be read or written;
/// [`QueryError::BadParameter`] for an envelope this filer built that does not parse (a bug).
pub fn propose(
    log_dir: &Path,
    world: &str,
    proposer: &dyn ManifestProposer,
    dry_run: bool,
) -> Result<ProposeReport, QueryError> {
    let log = ReadOnlySqliteEventLog::open(log_dir)?;
    let (proposals, decisions) = match open_proposal_reader(log_dir)? {
        Some(reader) => (reader.proposals()?, reader.decisions()?),
        None => (Vec::new(), Vec::new()),
    };
    let mapped = mapped_members(&log, &proposals, &decisions)?;
    let targets: BTreeSet<SourceId> = mapped.keys().cloned().collect();
    let tails = read_tail(&log, &targets, TAIL_EVENTS)?;
    let (input, snapshot) = build_input(world, &tails, &mapped);
    let id = proposer.id();
    let prompt_hash = proposer.prompt_hash();
    let hash = input_hash(&input, prompt_hash.as_deref()).map_err(QueryError::Storage)?;
    let mut report = ProposeReport::new(world, &id, &hash);
    let Some(snapshot) = snapshot else {
        report.action = Action::Abstained;
        report.reason = Some("no member source with an accepted mapping has logged events".into());
        return Ok(report);
    };
    let attempt =
        match plan(&proposals, &decisions, &id, world, &hash).map_err(QueryError::Storage)? {
            Plan::Skip(reason) => return Ok(report.skipped(reason)),
            Plan::Complete { .. } if dry_run => {
                report.action = Action::DryRun;
                report.reason =
                    Some("an undecided proposal from an earlier run would be decided".into());
                return Ok(report);
            }
            Plan::Complete { .. } => None,
            Plan::Propose { attempt } => Some(attempt),
        };
    // The proposer may be slow (a model call): it runs before the writer lock is taken.
    let filing = match attempt.map(|_| proposer.propose(&input)) {
        Some(ManifestOutcome::Abstain(reason)) => {
            report.action = Action::Abstained;
            report.reason = Some(reason);
            return Ok(report);
        }
        Some(outcome) => Some(filing(&input, prompt_hash, outcome)),
        None => None,
    };
    if !dry_run {
        return write(log_dir, (world, &id, &hash), snapshot, filing, report);
    }
    // A dry run reaches here only from `Plan::Propose`, so both are set.
    if let (Some(attempt), Some(filing)) = (attempt, filing) {
        let envelope = filing.envelope(world, &hash, attempt);
        report.decided(
            Action::DryRun,
            proposal_id(&id, world, &hash, attempt),
            attempt,
            (filing.outcome, filing.basis(&id, world)),
        );
        report.envelope = Some(envelope);
    }
    Ok(report)
}

/// Takes the writer lock, plans again from the store it guards, and appends what the plan
/// says: the decision an undecided row lacks, or the new proposal and its decision.
fn write(
    log_dir: &Path,
    (world, id, hash): (&str, &ProposerId, &str),
    snapshot: LogPosition,
    filing: Option<Filing>,
    mut report: ProposeReport,
) -> Result<ProposeReport, QueryError> {
    let mut store = match SqliteProposalStore::open(log_dir) {
        Ok(store) => store,
        Err(LogError::Locked) => return Err(QueryError::StoreLocked),
        Err(error) => return Err(error.into()),
    };
    let proposals = store.proposals()?;
    let decisions = store.decisions()?;
    match plan(&proposals, &decisions, id, world, hash).map_err(QueryError::Storage)? {
        Plan::Skip(reason) => Ok(report.skipped(reason)),
        Plan::Complete {
            id: proposal,
            attempt,
            decision,
        } => {
            store.append_decision(&decision)?;
            report.decided(
                Action::Completed,
                proposal,
                attempt,
                (decision.outcome, decision.basis),
            );
            Ok(report)
        }
        Plan::Propose { attempt } => {
            // Only a concurrent run gets here with no answer: another filer decided the row
            // this run meant to. Asking now would hold the lock over a model call.
            let Some(filing) = filing else {
                return Ok(report.skipped("the store changed during the run; run again".into()));
            };
            let payload = serde_json::to_vec(&filing.envelope(world, hash, attempt))
                .map_err(|error| QueryError::Storage(format!("envelope: {error}")))?;
            parse_dashboard_envelope(&payload).map_err(|reason| QueryError::BadParameter {
                name: "envelope",
                reason,
            })?;
            let decision = NewDecision {
                proposal_id: proposal_id(id, world, hash, attempt),
                decider: Decider::Policy,
                outcome: filing.outcome,
                basis: filing.basis(id, world),
                decided_at_ms: now_ms().map_err(QueryError::Storage)?,
            };
            store.append_proposal(&NewProposal {
                id: decision.proposal_id.clone(),
                class: DASHBOARD_MANIFEST_CLASS.to_owned(),
                actor: actor(id),
                snapshot_offset: snapshot,
                payload,
                proposed_at_ms: decision.decided_at_ms,
            })?;
            store.append_decision(&decision)?;
            report.decided(
                Action::Filed,
                decision.proposal_id,
                attempt,
                (decision.outcome, decision.basis),
            );
            Ok(report)
        }
    }
}

impl ProposeReport {
    fn new(world: &str, id: &ProposerId, input_hash: &str) -> Self {
        Self {
            world: world.to_owned(),
            actor: format!("{}/{}", id.model, id.version),
            input_hash: input_hash.to_owned(),
            action: Action::Skipped,
            proposal_id: None,
            attempt: None,
            decision: None,
            basis: None,
            reason: None,
            envelope: None,
        }
    }

    fn skipped(mut self, reason: String) -> Self {
        self.action = Action::Skipped;
        self.reason = Some(reason);
        self
    }

    fn decided(
        &mut self,
        action: Action,
        proposal_id: String,
        attempt: u32,
        (outcome, basis): (Outcome, String),
    ) {
        self.action = action;
        self.proposal_id = Some(proposal_id);
        self.attempt = Some(attempt);
        self.decision = Some(outcome_name(outcome).to_owned());
        self.basis = Some(basis);
    }
}

/// [`propose`] with the deterministic `FallbackProposer`, so the CLI files without depending on
/// `s2w-discover` itself.
///
/// # Errors
/// As [`propose`].
pub fn propose_fallback(
    log_dir: &Path,
    world: &str,
    dry_run: bool,
) -> Result<ProposeReport, QueryError> {
    propose(
        log_dir,
        world,
        &s2w_discover::manifest::FallbackProposer,
        dry_run,
    )
}

/// The model command `propose_system2` runs (decision 0029, s2w#311): the argv, which
/// variables it may see, and the (model, version) every row it files is recorded under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct System2Command {
    /// The command and its arguments, run without a shell; `argv[0]` is an absolute path or
    /// found through a `PATH` passed in `env`.
    pub argv: Vec<String>,
    /// The model name, recorded as given (a CLI may take an alias).
    pub model: String,
    /// The model version.
    pub version: String,
    /// Names of the variables passed through from this process; nothing else is.
    pub env: Vec<String>,
}

/// [`propose`] with a System 2 proposer that runs `command` once per model call (at most two
/// per attempt: the first reply and one repair). A dry run that would file still runs the
/// command: it writes nothing, but it shows what this model would file.
///
/// # Errors
/// [`QueryError::BadParameter`] (`system2-model`, `system2-cmd` or `system2-env`) for a model
/// or version holding whitespace, a control character or `;`, an empty command, a bad
/// variable name, or a named variable that is not set here, before the log is opened;
/// otherwise as [`propose`].
pub fn propose_system2(
    log_dir: &Path,
    world: &str,
    command: &System2Command,
    dry_run: bool,
) -> Result<ProposeReport, QueryError> {
    check_identity("system2-model", &command.model)?;
    check_identity("system2-model", &command.version)?;
    let provider = ExecProvider::inherit(command.argv.clone(), &command.env).map_err(|error| {
        let name = match error {
            ExecSetupError::EmptyCommand => "system2-cmd",
            ExecSetupError::BadName(_) | ExecSetupError::Unset(_) => "system2-env",
        };
        QueryError::BadParameter {
            name,
            reason: error.to_string(),
        }
    })?;
    let id = ProposerId {
        model: command.model.clone(),
        version: command.version.clone(),
    };
    propose(log_dir, world, &System2Proposer::new(provider, id), dry_run)
}

/// A proposer's answer, checked and ready to become an envelope.
struct Filing {
    manifest: Option<DashboardManifest>,
    provenance: Provenance,
    outcome: Outcome,
}

impl Filing {
    fn envelope(&self, world: &str, input_hash: &str, attempt: u32) -> DashboardEnvelope {
        DashboardEnvelope {
            format: DASHBOARD_ENVELOPE_FORMAT,
            world: world.to_owned(),
            input_hash: input_hash.to_owned(),
            attempt,
            manifest: self.manifest.clone(),
            provenance: self.provenance.clone(),
        }
    }

    fn basis(&self, proposer: &ProposerId, world: &str) -> String {
        match &self.manifest {
            Some(manifest) => accept_basis(proposer, world, manifest),
            None => reject_basis(self.provenance.error.as_deref().unwrap_or("")),
        }
    }
}

/// Validates a manifest against the input it was built from; a refusal, like an `Invalid`
/// answer, becomes a null-manifest row that keeps the reply.
fn filing(input: &ManifestInput, prompt_hash: Option<String>, outcome: ManifestOutcome) -> Filing {
    let (manifest, error, trace) = match outcome {
        ManifestOutcome::Manifest { manifest, trace } => {
            match manifest.validate(&input.context()) {
                Ok(()) => (Some(*manifest), None, trace),
                Err(error) => {
                    let raw = trace
                        .raw
                        .clone()
                        .or_else(|| serde_json::to_string(&manifest).ok());
                    (
                        None,
                        Some(format!("validator: {error}")),
                        ProposerTrace { raw, ..trace },
                    )
                }
            }
        }
        ManifestOutcome::Invalid { error, trace } => (None, Some(error), trace),
        ManifestOutcome::Abstain(reason) => (None, Some(reason), ProposerTrace::default()),
    };
    let outcome = if manifest.is_some() {
        Outcome::Accept
    } else {
        Outcome::Reject
    };
    Filing {
        manifest,
        provenance: Provenance {
            prompt_hash,
            input_tokens: trace.input_tokens,
            output_tokens: trace.output_tokens,
            latency_ms: trace.latency_ms,
            raw: trace.raw.map(cap_raw),
            error,
        },
        outcome,
    }
}

/// `raw` cut to [`MAX_RAW_BYTES`] at a character boundary.
fn cap_raw(mut raw: String) -> String {
    if raw.len() > MAX_RAW_BYTES {
        let mut cut = MAX_RAW_BYTES;
        while !raw.is_char_boundary(cut) {
            cut -= 1;
        }
        raw.truncate(cut);
    }
    raw
}

const fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Accept => "accept",
        Outcome::Reject => "reject",
    }
}

fn now_ms() -> Result<i64, String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock before epoch: {error}"))?;
    i64::try_from(elapsed.as_millis())
        .map_err(|error| format!("system clock out of range: {error}"))
}

#[cfg(test)]
mod tests;
