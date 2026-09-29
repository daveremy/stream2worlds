//! The producer against real stores in a temporary directory. Every stream is synthetic with
//! one-letter names (decision 0018).

use std::sync::atomic::{AtomicU64, Ordering};

use s2w_log::{EventLog, StoredDecision, StoredProposal};
use s2w_model::{Cursor, RawEvent, Timestamp};
use serde_json::json;

use super::*;
use crate::tests::TestDirectory;

/// Notes only; the producer reports nothing else.
#[derive(Default)]
pub(crate) struct Notes(pub(crate) Vec<String>);

impl Reporter for Notes {
    fn flushed(&mut self, _: u64, _: u64, _: u64, _: Option<&str>) {}
    fn duplicate(&mut self, _: u64) {}
    fn note(&mut self, message: &str) {
        self.0.push(message.to_owned());
    }
    fn source_error(&mut self, _: &str, _: bool) {}
    fn wants_ticker(&self) -> bool {
        false
    }
}

impl Notes {
    fn has(&self, needle: &str) -> bool {
        self.0.iter().any(|note| note.contains(needle))
    }
}

/// Deterministic pseudo-random values (a 64-bit LCG), so no randomness crate is needed.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// `n` payloads the profiler maps: `e` names the event, `t` is a sequence, `a` is an entity
/// whose attribute is `an`, `b` is an entity determined by `a`, and `r` repeats with nothing
/// depending on it.
pub(crate) fn stream(n: u64) -> Vec<Vec<u8>> {
    let mut rng = Lcg(7);
    (0..n)
        .map(|i| {
            let a = rng.below(40);
            json!({
                "e": format!("e{i}"),
                "t": 1_000 + i / 3,
                "a": format!("a{a}"),
                "an": format!("n{}", a / 2),
                "b": format!("b{}", a % 10),
                "r": rng.below(300),
            })
            .to_string()
            .into_bytes()
        })
        .collect()
}

/// `n` payloads with nothing to map: every value is unique.
fn structureless(n: u64) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            json!({"e": format!("e{i}"), "u": i * 7})
                .to_string()
                .into_bytes()
        })
        .collect()
}

pub(crate) const SOURCE: &str = "test.learned";

/// A window and profiler small enough for a test log.
pub(crate) fn small() -> DiscoverConfig {
    DiscoverConfig {
        window: 300,
        profiler: s2w_discover::Config {
            min_events: 300,
            ..s2w_discover::Config::default()
        },
    }
}

/// Appends `payloads` to `dir`'s log as events of `source`.
pub(crate) fn append(dir: &Path, source: &str, payloads: Vec<Vec<u8>>) {
    let mut log = SqliteEventLog::open(dir).expect("log");
    let events = payloads
        .into_iter()
        .zip(0_u64..)
        .map(|(payload, i)| RawEvent {
            source: SourceId::new(source).expect("source"),
            cursor: Cursor::new(format!("{source}-{i}").into_bytes()).expect("cursor"),
            received_at: Timestamp::from_millis(1_000 + i64::try_from(i).expect("small")),
            payload,
        })
        .collect();
    log.append_batch(events).expect("append");
}

fn rows(dir: &Path) -> (Vec<StoredProposal>, Vec<StoredDecision>) {
    if !dir.join(s2w_log::PROPOSAL_DATABASE_FILE).exists() {
        return (Vec::new(), Vec::new());
    }
    let store = ReadOnlySqliteProposalStore::open(dir).expect("store");
    (
        store.proposals().expect("proposals"),
        store.decisions().expect("decisions"),
    )
}

/// One producer start: resolve, run, as `serve` does.
fn start(producer: Producer, dir: &Path, cfg: &DiscoverConfig) -> (bool, Notes) {
    let log = SqliteEventLog::open(dir).expect("log");
    let resolution = routes::load(dir).expect("routes");
    let mut notes = Notes::default();
    let wrote = run_with(producer, &log, dir, &resolution, cfg, &mut notes);
    (wrote, notes)
}

fn source() -> SourceId {
    SourceId::new(SOURCE).expect("source")
}

fn position(n: u64) -> LogPosition {
    LogPosition::from_u64(n).expect("position")
}

/// Appends a human reject on `id`, as `s2w proposals decide --outcome reject` would.
pub(crate) fn reject(dir: &Path, id: &str) {
    let mut store = SqliteProposalStore::open(dir).expect("writer");
    store
        .append_decision(&NewDecision {
            proposal_id: id.to_owned(),
            decider: Decider::Human,
            outcome: Outcome::Reject,
            basis: "reviewer=h; revoked".to_owned(),
            decided_at_ms: 0,
        })
        .expect("reject");
}

#[test]
fn the_proposal_id_is_deterministic_and_moves_with_every_field() {
    let id = |actor: &Actor, source: &str, first, last, identity| {
        proposal_id(
            actor,
            &SourceId::new(source).expect("source"),
            position(first),
            position(last),
            identity,
        )
    };
    let base = id(&actor(), SOURCE, 1, 300, "m1");
    assert_eq!(base, id(&actor(), SOURCE, 1, 300, "m1"));
    assert_eq!(base.len(), 16);
    let human = Actor::Human { id: "h".to_owned() };
    for other in [
        id(&human, SOURCE, 1, 300, "m1"),
        id(&actor(), "test.other", 1, 300, "m1"),
        id(&actor(), SOURCE, 2, 300, "m1"),
        id(&actor(), SOURCE, 1, 301, "m1"),
        id(&actor(), SOURCE, 1, 300, "m2"),
    ] {
        assert_ne!(base, other);
    }
}

#[test]
fn the_basis_names_the_policy_profiler_window_and_mapping_size() {
    let payloads = stream(300);
    let refs: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
    let Discovery::Mapping(mapping) = s2w_discover::discover(&refs, &small().profiler).1 else {
        panic!("the synthetic stream maps");
    };
    let types: BTreeSet<&str> = mapping
        .entities
        .iter()
        .map(|r| r.type_label.as_str())
        .collect();
    assert_eq!(
        basis(position(1), position(300), 300, &mapping),
        format!(
            "policy=learned-mapping-auto-apply/1 profiler=h-lite/{PROFILER_VERSION} window=1..300 events=300 types={} entity_rules={} relationship_rules={}",
            types.len(),
            mapping.entities.len(),
            mapping.relationships.len()
        )
    );
}

#[test]
fn a_full_window_files_one_proposal_and_one_policy_accept_that_routes_the_source() {
    let dir = TestDirectory::new("discover-files");
    append(dir.path(), SOURCE, stream(400));
    let (wrote, notes) = start(REAL, dir.path(), &small());
    assert!(wrote, "{:?}", notes.0);
    assert!(
        notes.has("accepted by policy learned-mapping-auto-apply/1"),
        "{:?}",
        notes.0
    );
    let (proposals, decisions) = rows(dir.path());
    assert_eq!(proposals.len(), 1);
    assert_eq!(decisions.len(), 1);
    let proposal = &proposals[0];
    assert_eq!(proposal.class, STREAM_MAPPING_CLASS);
    assert_eq!(proposal.actor, actor());
    assert_eq!(
        proposal.snapshot_offset,
        position(300),
        "the window's last position"
    );
    let (source, _, identity) = routes::decode_envelope(&proposal.payload).expect("envelope");
    assert_eq!(source, self::source());
    assert_eq!(
        proposal.id,
        proposal_id(&actor(), &source, position(1), position(300), &identity)
    );
    assert_eq!(decisions[0].decider, Decider::Policy);
    assert_eq!(decisions[0].outcome, Outcome::Accept);
    assert!(decisions[0].basis.contains("window=1..300 events=300"));
    let resolution = routes::load(dir.path()).expect("routes");
    assert_eq!(resolution.routes[&source].proposal_id, proposal.id);
}

#[test]
fn a_routed_source_is_never_profiled_again() {
    let dir = TestDirectory::new("discover-routed");
    append(dir.path(), SOURCE, stream(400));
    assert!(start(REAL, dir.path(), &small()).0);
    let (wrote, notes) = start(REAL, dir.path(), &small());
    assert!(!wrote);
    assert!(
        !notes.has("discover:"),
        "a routed source is skipped silently: {:?}",
        notes.0
    );
    assert_eq!(rows(dir.path()).0.len(), 1);
}

#[test]
fn below_the_window_nothing_is_profiled_and_no_store_is_created() {
    let dir = TestDirectory::new("discover-below");
    append(dir.path(), SOURCE, stream(299));
    let (wrote, notes) = start(REAL, dir.path(), &small());
    assert!(!wrote);
    assert!(
        notes.has("test.learned: 299 events, below the window of 300"),
        "{:?}",
        notes.0
    );
    assert!(!dir.path().join(s2w_log::PROPOSAL_DATABASE_FILE).exists());
}

#[test]
fn an_abstaining_profiler_writes_no_rows_and_says_why() {
    let dir = TestDirectory::new("discover-abstain");
    append(dir.path(), SOURCE, structureless(300));
    let (wrote, notes) = start(REAL, dir.path(), &small());
    assert!(!wrote);
    assert!(notes.has("test.learned: abstained ("), "{:?}", notes.0);
    assert!(notes.has(") over 300 events"), "{:?}", notes.0);
    assert_eq!(rows(dir.path()).0.len(), 0);
}

#[test]
fn a_held_writer_lock_is_a_store_locked_note_and_no_rows() {
    let dir = TestDirectory::new("discover-locked");
    append(dir.path(), SOURCE, stream(300));
    let held = SqliteProposalStore::open(dir.path()).expect("writer");
    let (wrote, notes) = start(REAL, dir.path(), &small());
    assert!(!wrote);
    assert!(notes.has("test.learned: store_locked"), "{:?}", notes.0);
    drop(held);
    assert_eq!(rows(dir.path()).0.len(), 0);
    assert!(
        start(REAL, dir.path(), &small()).0,
        "released: the next start files it"
    );
}

static MINTED: AtomicU64 = AtomicU64::new(0);

fn fresh_id(_: &Actor, _: &SourceId, _: LogPosition, _: LogPosition, _: &str) -> String {
    format!("fresh-{}", MINTED.fetch_add(1, Ordering::Relaxed))
}

/// The producer decision 0025 rules out: a fresh id per run and no lookup.
const MUTANT: Producer = Producer {
    id: fresh_id,
    lookup: false,
};

/// Files, revokes by human reject, then starts three more times. Returns the proposal count.
fn revoke_then_restart_three_times(producer: Producer, name: &str) -> usize {
    let dir = TestDirectory::new(name);
    append(dir.path(), SOURCE, stream(300));
    assert!(start(producer, dir.path(), &small()).0);
    let id = rows(dir.path()).0[0].id.clone();
    reject(dir.path(), &id);
    for _ in 0..3 {
        start(producer, dir.path(), &small());
        let resolution = routes::load(dir.path()).expect("routes");
        assert!(
            !resolution.routes.contains_key(&source()),
            "a human reject binds the identity, whoever files it again"
        );
    }
    rows(dir.path()).0.len()
}

#[test]
fn a_human_reject_is_never_refiled_and_the_mutant_producer_shows_why() {
    assert_eq!(
        revoke_then_restart_three_times(REAL, "discover-revoke-real"),
        1
    );
    assert_eq!(
        revoke_then_restart_three_times(MUTANT, "discover-revoke-mutant"),
        4,
        "without the lookup and the deterministic id every start files again"
    );
}

#[test]
fn an_identity_filed_by_another_actor_is_not_filed_again() {
    let dir = TestDirectory::new("discover-other-actor");
    append(dir.path(), SOURCE, stream(300));
    let payloads = stream(300);
    let refs: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
    let Discovery::Mapping(mapping) = s2w_discover::discover(&refs, &small().profiler).1 else {
        panic!("the synthetic stream maps");
    };
    let envelope = MappingEnvelope {
        format: ENVELOPE_FORMAT,
        source: SOURCE.to_owned(),
        mapping,
    };
    let mut store = SqliteProposalStore::open(dir.path()).expect("writer");
    store
        .append_proposal(&NewProposal {
            id: "by-hand".to_owned(),
            class: STREAM_MAPPING_CLASS.to_owned(),
            actor: Actor::Human { id: "h".to_owned() },
            snapshot_offset: position(1),
            payload: serde_json::to_vec(&envelope).expect("envelope"),
            proposed_at_ms: 0,
        })
        .expect("proposal");
    drop(store);
    let (wrote, notes) = start(REAL, dir.path(), &small());
    assert!(!wrote);
    assert!(
        notes.has("is already proposed (proposal by-hand)"),
        "{:?}",
        notes.0
    );
    assert_eq!(rows(dir.path()).0.len(), 1);
}
