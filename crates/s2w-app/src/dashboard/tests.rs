use std::cell::Cell;

use s2w_discover::manifest::{FallbackProposer, sample_event};
use s2w_log::{EventLog, ReadOnlySqliteProposalStore, SqliteEventLog};
use s2w_model::{Cursor, FieldPath, RawEvent, Segment, Timestamp};
use s2w_system2::{ReplayProvider, Reply};

use super::*;
use crate::discover::tests::{Notes, SOURCE, append, small, stream};
use crate::query::read_dashboard;
use crate::tests::TestDirectory;

const WORLD: &str = "default";

/// A log with one source whose learned mapping is filed and accepted.
fn mapped_log(name: &str) -> TestDirectory {
    let dir = TestDirectory::new(name);
    append(dir.path(), SOURCE, stream(300));
    let log = SqliteEventLog::open(dir.path()).expect("log");
    let resolution = crate::routes::load(dir.path()).expect("routes");
    let ran = crate::discover::run(
        &log,
        dir.path(),
        &resolution,
        &small(),
        &mut Notes::default(),
    );
    assert!(ran.resolve_again, "the mapping was filed");
    dir
}

/// The proposer input `propose` builds from `dir`.
fn built_input(dir: &Path) -> ManifestInput {
    let log = ReadOnlySqliteEventLog::open(dir).expect("log");
    let store = ReadOnlySqliteProposalStore::open(dir).expect("store");
    let proposals = store.proposals().expect("proposals");
    let decisions = store.decisions().expect("decisions");
    let mapped = mapped_members(&log, &proposals, &decisions).expect("mapped");
    let tails = read_tail(&log, &mapped.keys().cloned().collect(), TAIL_EVENTS).expect("tail");
    build_input(WORLD, &tails, &mapped).0
}

/// Every `dashboard-manifest` proposal and every decision naming one.
fn dashboard_rows(dir: &Path) -> (Vec<StoredProposal>, Vec<StoredDecision>) {
    let store = ReadOnlySqliteProposalStore::open(dir).expect("store");
    let proposals: Vec<StoredProposal> = store
        .proposals()
        .expect("proposals")
        .into_iter()
        .filter(|p| p.class == DASHBOARD_MANIFEST_CLASS)
        .collect();
    let ids: BTreeSet<String> = proposals.iter().map(|p| p.id.clone()).collect();
    let decisions = store
        .decisions()
        .expect("decisions")
        .into_iter()
        .filter(|d| ids.contains(&d.proposal_id))
        .collect();
    (proposals, decisions)
}

/// A proposer that answers with a fixed outcome and counts its calls.
struct Fixed {
    outcome: ManifestOutcome,
    calls: Cell<u32>,
}

impl Fixed {
    fn new(outcome: ManifestOutcome) -> Self {
        Self {
            outcome,
            calls: Cell::new(0),
        }
    }
}

impl ManifestProposer for Fixed {
    fn id(&self) -> ProposerId {
        ProposerId {
            model: "fixed".to_owned(),
            version: "1".to_owned(),
        }
    }

    fn propose(&self, _: &ManifestInput) -> ManifestOutcome {
        self.calls.set(self.calls.get() + 1);
        self.outcome.clone()
    }
}

fn invalid(error: &str) -> ManifestOutcome {
    ManifestOutcome::Invalid {
        error: error.to_owned(),
        trace: ProposerTrace {
            raw: Some("not json".to_owned()),
            ..ProposerTrace::default()
        },
    }
}

#[test]
fn the_fallback_files_an_accepted_manifest_and_a_second_run_writes_nothing() {
    let dir = mapped_log("dashboard-fallback");
    let report = propose_fallback(dir.path(), WORLD, false).expect("propose");
    assert_eq!(report.action, Action::Filed);
    assert_eq!(report.attempt, Some(1));
    assert_eq!(report.decision.as_deref(), Some("accept"));
    let basis = report.basis.clone().expect("basis");
    assert!(
        basis.starts_with(&format!(
            "policy={POLICY} proposer=dashboard-fallback/3 world={WORLD} built_on={SOURCE}:"
        )),
        "{basis}"
    );
    assert!(basis.ends_with(" events=1 roles=1"), "{basis}");

    let view = read_dashboard(dir.path(), WORLD).expect("view");
    assert_eq!(view.proposal_id, report.proposal_id);
    assert!(!view.stale, "{:?}", view.stale_entries);

    let before = dashboard_rows(dir.path());
    let again = propose_fallback(dir.path(), WORLD, false).expect("again");
    assert_eq!(again.action, Action::Skipped);
    assert_eq!(again.input_hash, report.input_hash);
    assert_eq!(dashboard_rows(dir.path()), before);
}

#[test]
fn a_failed_attempt_is_a_null_row_with_a_reject_and_the_fourth_run_is_skipped() {
    let dir = mapped_log("dashboard-invalid");
    let proposer = Fixed::new(invalid("timeout after 60s"));
    for attempt in 1..=MAX_ATTEMPTS {
        let report = propose(dir.path(), WORLD, &proposer, false).expect("propose");
        assert_eq!(report.action, Action::Filed);
        assert_eq!(report.attempt, Some(attempt));
        assert_eq!(report.decision.as_deref(), Some("reject"));
        assert_eq!(report.basis.as_deref(), Some("invalid: timeout after 60s"));
    }
    let report = propose(dir.path(), WORLD, &proposer, false).expect("fourth");
    assert_eq!(report.action, Action::Skipped);
    assert_eq!(
        proposer.calls.get(),
        MAX_ATTEMPTS,
        "no call once the attempts are spent"
    );

    let (proposals, decisions) = dashboard_rows(dir.path());
    assert_eq!(proposals.len(), 3);
    assert_eq!(decisions.len(), 3);
    for proposal in &proposals {
        let envelope = parse_dashboard_envelope(&proposal.payload).expect("envelope");
        assert_eq!(envelope.manifest, None);
        assert_eq!(envelope.provenance.raw.as_deref(), Some("not json"));
    }
    let view = read_dashboard(dir.path(), WORLD).expect("view");
    assert_eq!(view.manifest, None);
    assert_eq!(view.excluded.len(), 3);
}

#[test]
fn a_manifest_the_validator_refuses_is_filed_as_a_null_row_that_keeps_it() {
    let dir = mapped_log("dashboard-validator");
    let input = built_input(dir.path());
    let ManifestOutcome::Manifest { mut manifest, .. } = FallbackProposer.propose(&input) else {
        panic!("the fallback proposes");
    };
    manifest.built_on[0].source = "no.such.source".to_owned();
    let proposer = Fixed::new(ManifestOutcome::Manifest {
        manifest,
        trace: ProposerTrace::default(),
    });

    let report = propose(dir.path(), WORLD, &proposer, false).expect("propose");
    assert_eq!(report.decision.as_deref(), Some("reject"));
    let basis = report.basis.expect("basis");
    assert!(basis.starts_with("invalid: validator: "), "{basis}");
    let (proposals, _) = dashboard_rows(dir.path());
    let envelope = parse_dashboard_envelope(&proposals[0].payload).expect("envelope");
    assert!(
        envelope
            .provenance
            .raw
            .is_some_and(|raw| raw.contains("no.such.source"))
    );
}

#[test]
fn a_proposal_left_without_its_decision_is_decided_by_the_next_run() {
    let dir = mapped_log("dashboard-complete");
    let dry = propose_fallback(dir.path(), WORLD, true).expect("dry run");
    let envelope = dry.envelope.expect("envelope");
    let id = dry.proposal_id.expect("id");
    let mut store = SqliteProposalStore::open(dir.path()).expect("writer");
    store
        .append_proposal(&NewProposal {
            id: id.clone(),
            class: DASHBOARD_MANIFEST_CLASS.to_owned(),
            actor: actor(&FallbackProposer.id()),
            snapshot_offset: LogPosition::from_u64(300).expect("position"),
            payload: serde_json::to_vec(&envelope).expect("payload"),
            proposed_at_ms: 0,
        })
        .expect("proposal");
    drop(store);

    let report = propose_fallback(dir.path(), WORLD, false).expect("propose");
    assert_eq!(report.action, Action::Completed);
    assert_eq!(report.proposal_id.as_deref(), Some(id.as_str()));
    assert_eq!(report.decision.as_deref(), Some("accept"));
    let (proposals, decisions) = dashboard_rows(dir.path());
    assert_eq!((proposals.len(), decisions.len()), (1, 1));
    assert_eq!(
        read_dashboard(dir.path(), WORLD).expect("view").proposal_id,
        Some(id)
    );
}

#[test]
fn a_dry_run_reports_the_envelope_and_writes_nothing() {
    let dir = mapped_log("dashboard-dry");
    let before = dashboard_rows(dir.path());
    let report = propose_fallback(dir.path(), WORLD, true).expect("dry run");
    assert_eq!(report.action, Action::DryRun);
    assert_eq!(report.decision.as_deref(), Some("accept"));
    let envelope = report.envelope.expect("envelope");
    assert_eq!(envelope.input_hash, report.input_hash);
    assert!(envelope.manifest.is_some());
    assert_eq!(dashboard_rows(dir.path()), before);
}

#[test]
fn an_abstaining_proposer_writes_nothing_and_says_why() {
    let dir = mapped_log("dashboard-abstain");
    let proposer = Fixed::new(ManifestOutcome::Abstain("nothing to say".to_owned()));
    let report = propose(dir.path(), WORLD, &proposer, false).expect("propose");
    assert_eq!(report.action, Action::Abstained);
    assert_eq!(report.reason.as_deref(), Some("nothing to say"));
    assert!(dashboard_rows(dir.path()).0.is_empty());
}

#[test]
fn with_no_mapped_source_nothing_is_asked_and_no_store_is_created() {
    let dir = TestDirectory::new("dashboard-unmapped");
    append(dir.path(), SOURCE, stream(10));
    let proposer = Fixed::new(invalid("unused"));
    let report = propose(dir.path(), WORLD, &proposer, false).expect("propose");
    assert_eq!(report.action, Action::Abstained);
    assert_eq!(proposer.calls.get(), 0);
    assert!(!dir.path().join(s2w_log::PROPOSAL_DATABASE_FILE).exists());
}

#[test]
fn the_input_hash_is_stable_and_moves_with_the_tail_and_the_prompt() {
    let dir = mapped_log("dashboard-hash");
    let hash = || {
        propose_fallback(dir.path(), WORLD, true)
            .expect("dry")
            .input_hash
    };
    let first = hash();
    assert_eq!(hash(), first);
    let mut log = SqliteEventLog::open(dir.path()).expect("log");
    log.append_batch(vec![raw(SOURCE, 999)]).expect("append");
    drop(log);
    assert_ne!(hash(), first, "a moved tail moves the hash");

    let input = ManifestInput {
        world: WORLD.to_owned(),
        sources: Vec::new(),
    };
    assert_ne!(
        input_hash(&input, None).expect("hash"),
        input_hash(&input, Some("0123456789abcdef")).expect("hash")
    );
}

#[test]
fn the_input_hash_does_not_depend_on_key_order_in_a_sample() {
    let dir = mapped_log("dashboard-key-order");
    let input = built_input(dir.path());
    let with = |sample: &str| {
        let mut input = input.clone();
        input.sources[0].sample = vec![serde_json::from_str(sample).expect("sample")];
        input_hash(&input, None).expect("hash")
    };
    assert_eq!(
        with(r#"{"x":1,"y":{"p":2,"q":3}}"#),
        with(r#"{"y":{"q":3,"p":2},"x":1}"#)
    );
}

#[test]
fn the_proposal_id_moves_with_every_field() {
    let id = |model: &str, version: &str, world: &str, hash: &str, attempt| {
        proposal_id(
            &ProposerId {
                model: model.to_owned(),
                version: version.to_owned(),
            },
            world,
            hash,
            attempt,
        )
    };
    let base = id("m", "1", "w", "h", 1);
    assert_eq!(base, id("m", "1", "w", "h", 1));
    for other in [
        id("n", "1", "w", "h", 1),
        id("m", "2", "w", "h", 1),
        id("m", "1", "v", "h", 1),
        id("m", "1", "w", "g", 1),
        id("m", "1", "w", "h", 2),
        id("m1", "", "w", "h", 1),
    ] {
        assert_ne!(other, base);
    }
}

fn raw(source: &str, i: u64) -> RawEvent {
    RawEvent {
        source: SourceId::new(source).expect("source"),
        cursor: Cursor::new(format!("{source}-{i}").into_bytes()).expect("cursor"),
        received_at: Timestamp::from_millis(1_000),
        payload: format!("{{\"i\":{i}}}").into_bytes(),
    }
}

#[test]
fn the_sentence_tail_stops_at_n_events_in_total_and_doubles_past_other_sources() {
    use crate::query::read_last;
    let dir = TestDirectory::new("sentences-tail");
    let mut log = SqliteEventLog::open(dir.path()).expect("log");
    log.append_batch((0..3).map(|i| raw("rare", i)).collect())
        .expect("rare");
    log.append_batch((0..50).map(|i| raw("other", i)).collect())
        .expect("other");
    log.append_batch((0..4).map(|i| raw("common", i)).collect())
        .expect("common");
    let targets: BTreeSet<SourceId> = ["rare", "common"]
        .into_iter()
        .map(|s| SourceId::new(s).expect("source"))
        .collect();
    let positions = |n: usize| -> Vec<u64> {
        read_last(&log, &targets, n)
            .expect("tail")
            .iter()
            .map(|s| s.position.as_u64())
            .collect()
    };
    // 4 in total, not 4 per source: all from `common`, the newest.
    assert_eq!(positions(4), vec![54, 55, 56, 57]);
    // 6 needs `rare`, 50 non-member events back: the window doubles past them.
    assert_eq!(positions(6), vec![2, 3, 54, 55, 56, 57]);
    // More than the log holds: every target event.
    assert_eq!(positions(200).len(), 7);

    let empty = TestDirectory::new("sentences-tail-empty");
    let log = SqliteEventLog::open(empty.path()).expect("log");
    assert!(read_last(&log, &targets, 4).expect("tail").is_empty());
}

#[test]
fn the_tail_doubles_its_window_until_a_rare_source_is_full() {
    let dir = TestDirectory::new("dashboard-tail");
    let mut log = SqliteEventLog::open(dir.path()).expect("log");
    log.append_batch((0..6).map(|i| raw("rare", i)).collect())
        .expect("rare");
    log.append_batch((0..100).map(|i| raw("common", i)).collect())
        .expect("common");
    let targets: BTreeSet<SourceId> = ["rare", "common", "absent"]
        .into_iter()
        .map(|s| SourceId::new(s).expect("source"))
        .collect();
    let tails = read_tail(&log, &targets, 4).expect("tail");
    let positions = |source: &str| -> Vec<u64> {
        tails[&SourceId::new(source).expect("source")]
            .iter()
            .map(|s| s.position.as_u64())
            .collect()
    };
    assert_eq!(positions("rare"), vec![3, 4, 5, 6]);
    assert_eq!(positions("common"), vec![103, 104, 105, 106]);
    assert_eq!(tails.len(), 2, "a source with no events has no tail");

    let empty = TestDirectory::new("dashboard-tail-empty");
    let log = SqliteEventLog::open(empty.path()).expect("log");
    assert!(read_tail(&log, &targets, 4).expect("tail").is_empty());
}

#[test]
fn a_sampled_event_decodes_json_fields_and_cuts_long_strings() {
    let long = "é".repeat(SAMPLE_STRING_CHARS + 5);
    let payload = serde_json::json!({"d": "{\"x\":1}", "s": long}).to_string();
    let decode = vec![FieldPath(vec![Segment::Key("d".to_owned())])];
    let value = sample_event(payload.as_bytes(), &decode, SAMPLE_STRING_CHARS).expect("json");
    assert_eq!(value["d"]["x"], 1);
    assert_eq!(
        value["s"].as_str().expect("string").chars().count(),
        SAMPLE_STRING_CHARS
    );
    assert_eq!(
        sample_event(b"not json", &decode, SAMPLE_STRING_CHARS),
        None
    );
}

#[test]
fn raw_is_capped_at_a_character_boundary() {
    let raw = format!("a{}", "é".repeat(MAX_RAW_BYTES));
    let capped = cap_raw(raw);
    assert!(capped.len() <= MAX_RAW_BYTES);
    assert!(capped.len() >= MAX_RAW_BYTES - 1);
}

/// A provider that keeps every prompt and answers each with `reply`: run once on a dry run to
/// learn the prompts a replayed run will send.
struct Capture {
    reply: &'static str,
    prompts: std::sync::Mutex<Vec<String>>,
}

impl Capture {
    fn prompts(dir: &Path, reply: &'static str) -> Vec<String> {
        let capture = Self {
            reply,
            prompts: std::sync::Mutex::new(Vec::new()),
        };
        let proposer = System2Proposer::new(capture, system2_id());
        let report = propose(dir, WORLD, &proposer, true).expect("dry run");
        assert_eq!(report.action, Action::DryRun);
        proposer.provider().prompts.lock().expect("prompts").clone()
    }
}

impl s2w_system2::Provider for Capture {
    fn complete(&self, prompt: &str) -> Result<Reply, s2w_system2::ProviderError> {
        self.prompts
            .lock()
            .expect("prompts")
            .push(prompt.to_owned());
        Ok(reply(self.reply))
    }
}

fn reply(text: &str) -> Reply {
    Reply {
        text: text.to_owned(),
        input_tokens: Some(1_000),
        output_tokens: Some(200),
        latency_ms: Some(1_500),
        ..Reply::default()
    }
}

fn system2_id() -> ProposerId {
    ProposerId {
        model: "test-model".to_owned(),
        version: "2026-09".to_owned(),
    }
}

/// The one row `proposals` holds: by the System 2 actor, carrying `manifest`, the proposer's
/// prompt hash and the replayed cost.
fn assert_filed_by_system2(
    proposals: &[StoredProposal],
    proposer: &System2Proposer<ReplayProvider>,
    manifest: &DashboardManifest,
) {
    assert_eq!(proposals.len(), 1);
    assert_eq!(
        proposals[0].actor,
        Actor::Agent {
            model: "test-model".to_owned(),
            version: "2026-09".to_owned(),
        }
    );
    let envelope = parse_dashboard_envelope(&proposals[0].payload).expect("envelope");
    assert_eq!(envelope.manifest.as_ref(), Some(manifest));
    assert_eq!(envelope.provenance.prompt_hash, proposer.prompt_hash());
    assert_eq!(
        (
            envelope.provenance.input_tokens,
            envelope.provenance.output_tokens,
            envelope.provenance.latency_ms,
        ),
        (Some(1_000), Some(200), Some(1_500))
    );
}

#[test]
fn a_replayed_system2_manifest_is_filed_accepted_with_its_prompt_hash_and_cost() {
    let dir = mapped_log("dashboard-system2");
    let ManifestOutcome::Manifest { manifest, .. } =
        FallbackProposer.propose(&built_input(dir.path()))
    else {
        panic!("the fallback proposes");
    };
    let text = serde_json::to_string(&manifest).expect("manifest json");
    let prompts = Capture::prompts(dir.path(), "unused");
    assert_eq!(prompts.len(), 2, "a non-JSON reply is repaired once");
    assert!(
        dashboard_rows(dir.path()).0.is_empty(),
        "the dry run wrote nothing"
    );

    let mut replay = ReplayProvider::new();
    replay.insert(&prompts[0], reply(&format!("```json\n{text}\n```")));
    let proposer = System2Proposer::new(replay, system2_id());
    let report = propose(dir.path(), WORLD, &proposer, false).expect("propose");
    assert_eq!(report.action, Action::Filed, "{report:?}");
    assert_eq!(report.actor, "test-model/2026-09");
    assert_eq!(report.decision.as_deref(), Some("accept"), "{report:?}");
    assert_eq!(proposer.provider().calls().len(), 1);

    let (proposals, decisions) = dashboard_rows(dir.path());
    assert_filed_by_system2(&proposals, &proposer, &manifest);
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].outcome, Outcome::Accept);
    let view = read_dashboard(dir.path(), WORLD).expect("view");
    assert_eq!(view.proposal_id, report.proposal_id);

    let again = propose(dir.path(), WORLD, &proposer, false).expect("again");
    assert_eq!(again.action, Action::Skipped);
    assert_eq!(
        proposer.provider().calls().len(),
        1,
        "a re-run asks nothing"
    );
    assert_eq!(dashboard_rows(dir.path()), (proposals, decisions));
}

#[test]
fn a_replayed_system2_reply_that_fails_its_repair_is_a_null_row_with_a_reject() {
    let dir = mapped_log("dashboard-system2-bad");
    let prompts = Capture::prompts(dir.path(), "not json");
    assert_eq!(prompts.len(), 2);
    assert!(
        prompts[1].contains("not json"),
        "the repair carries the reply"
    );

    let mut replay = ReplayProvider::new();
    replay.insert(&prompts[0], reply("not json"));
    replay.insert(&prompts[1], reply("still not json"));
    let proposer = System2Proposer::new(replay, system2_id());
    let report = propose(dir.path(), WORLD, &proposer, false).expect("propose");
    assert_eq!(report.decision.as_deref(), Some("reject"), "{report:?}");
    assert_eq!(report.attempt, Some(1));
    assert_eq!(proposer.provider().calls().len(), 2);

    let (proposals, decisions) = dashboard_rows(dir.path());
    assert_eq!((proposals.len(), decisions.len()), (1, 1));
    assert_eq!(decisions[0].outcome, Outcome::Reject);
    let envelope = parse_dashboard_envelope(&proposals[0].payload).expect("envelope");
    assert_eq!(envelope.manifest, None);
    assert_eq!(envelope.provenance.raw.as_deref(), Some("still not json"));
    assert_eq!(
        (
            envelope.provenance.input_tokens,
            envelope.provenance.latency_ms
        ),
        (Some(2_000), Some(3_000)),
        "summed over both calls"
    );
    assert!(
        envelope
            .provenance
            .error
            .is_some_and(|e| e.starts_with("not JSON")),
        "the last fault is kept"
    );
}

#[test]
fn a_system2_command_that_cannot_be_set_up_is_a_bad_parameter_before_the_log_opens() {
    let missing = std::env::temp_dir().join("s2w-system2-no-such-log-dir");
    let empty = System2Command {
        argv: Vec::new(),
        model: "m".to_owned(),
        version: "v".to_owned(),
        env: Vec::new(),
    };
    let unset = System2Command {
        argv: vec!["/bin/true".to_owned()],
        env: vec!["S2W_TEST_SURELY_UNSET_VARIABLE".to_owned()],
        ..empty.clone()
    };
    let spaced = System2Command {
        argv: vec!["/bin/true".to_owned()],
        model: "my model".to_owned(),
        ..empty.clone()
    };
    let semicolon = System2Command {
        argv: vec!["/bin/true".to_owned()],
        version: "v;x".to_owned(),
        ..empty.clone()
    };
    for (command, name) in [
        (empty, "system2-cmd"),
        (unset, "system2-env"),
        (spaced, "system2-model"),
        (semicolon, "system2-model"),
    ] {
        match propose_system2(&missing, WORLD, &command, false) {
            Err(QueryError::BadParameter { name: got, .. }) => assert_eq!(got, name),
            other => panic!("{command:?}: {other:?}"),
        }
    }
    assert!(!missing.exists());
}
