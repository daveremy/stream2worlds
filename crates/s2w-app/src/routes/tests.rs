use s2w_log::{Actor, LogPosition};

use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A one-rule mapping whose entities get type label `label`.
fn mapping_json(label: &str) -> String {
    format!(
        r#"{{"version":1,"decode":[],"entities":[{{"id":"e","type_label":"{label}","key":[["id"]],"attrs":[]}}],"relationships":[]}}"#
    )
}

fn envelope(source: &str, label: &str) -> Vec<u8> {
    format!(
        r#"{{"format":1,"source":"{source}","mapping":{}}}"#,
        mapping_json(label)
    )
    .into_bytes()
}

struct Rows {
    proposals: Vec<StoredProposal>,
    decisions: Vec<StoredDecision>,
}

impl Rows {
    fn new() -> Self {
        Self {
            proposals: Vec::new(),
            decisions: Vec::new(),
        }
    }

    fn propose_raw(&mut self, id: &str, class: &str, payload: Vec<u8>) -> &mut Self {
        self.proposals.push(StoredProposal {
            seq: i64::try_from(self.proposals.len()).unwrap_or(i64::MAX) + 1,
            id: id.to_owned(),
            class: class.to_owned(),
            actor: Actor::Agent {
                model: "m".to_owned(),
                version: "1".to_owned(),
            },
            snapshot_offset: LogPosition::from_u64(1).unwrap_or_else(|| unreachable!()),
            payload_hash: 0,
            payload,
            proposed_at_ms: 0,
        });
        self
    }

    fn propose(&mut self, id: &str, source: &str, label: &str) -> &mut Self {
        self.propose_raw(id, STREAM_MAPPING_CLASS, envelope(source, label))
    }

    fn decide(&mut self, id: &str, decider: Decider, outcome: Outcome) -> &mut Self {
        self.decisions.push(StoredDecision {
            seq: i64::try_from(self.decisions.len()).unwrap_or(i64::MAX) + 1,
            proposal_id: id.to_owned(),
            decider,
            outcome,
            basis: String::new(),
            decided_at_ms: 0,
        });
        self
    }

    fn resolve(&self) -> Resolution {
        resolve(&self.proposals, &self.decisions)
    }

    /// `(source, proposal id)` per routed source.
    fn routed(&self) -> Vec<(String, String)> {
        self.resolve()
            .routes
            .into_iter()
            .map(|(source, resolved)| (source.as_str().to_owned(), resolved.proposal_id))
            .collect()
    }
}

fn routed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(s, p)| ((*s).to_owned(), (*p).to_owned()))
        .collect()
}

use Decider::{Agent, Evidence, Human, Policy};
use Outcome::{Accept, Reject};

#[test]
fn no_decision_leaves_the_source_unrouted() {
    let mut rows = Rows::new();
    rows.propose("a", "src", "A");
    assert_eq!(rows.resolve(), Resolution::default());
}

#[test]
fn the_newest_accepted_proposal_by_seq_wins() {
    let mut rows = Rows::new();
    rows.propose("a", "src", "A")
        .propose("b", "src", "B")
        .propose("c", "src", "C")
        .decide("b", Policy, Accept)
        .decide("a", Policy, Accept);
    assert_eq!(rows.routed(), routed(&[("src", "b")]));
}

#[test]
fn a_human_reject_beats_a_policy_accept_in_either_order() {
    let mut after = Rows::new();
    after
        .propose("a", "src", "A")
        .decide("a", Policy, Accept)
        .decide("a", Human, Reject);
    assert!(after.routed().is_empty());

    let mut before = Rows::new();
    before
        .propose("a", "src", "A")
        .decide("a", Human, Reject)
        .decide("a", Policy, Accept);
    assert!(before.routed().is_empty());
}

#[test]
fn a_human_accept_after_a_policy_reject_accepts() {
    let mut rows = Rows::new();
    rows.propose("a", "src", "A")
        .decide("a", Policy, Reject)
        .decide("a", Human, Accept);
    assert_eq!(rows.routed(), routed(&[("src", "a")]));
}

#[test]
fn a_policy_reject_after_a_policy_accept_rejects() {
    let mut rows = Rows::new();
    rows.propose("a", "src", "A")
        .decide("a", Policy, Accept)
        .decide("a", Policy, Reject);
    assert!(rows.routed().is_empty());
}

#[test]
fn agent_and_evidence_decisions_never_count() {
    let mut rows = Rows::new();
    rows.propose("a", "src", "A")
        .decide("a", Agent, Accept)
        .propose("b", "src", "B")
        .decide("b", Evidence, Accept);
    assert!(rows.routed().is_empty());

    // Nor do they override: an agent reject leaves a policy accept standing.
    rows.decide("a", Policy, Accept).decide("a", Agent, Reject);
    assert_eq!(rows.routed(), routed(&[("src", "a")]));
}

#[test]
fn revoking_the_newest_falls_back_and_revoking_every_one_unroutes() {
    let mut rows = Rows::new();
    rows.propose("a", "src", "A")
        .propose("b", "src", "B")
        .decide("a", Policy, Accept)
        .decide("b", Policy, Accept);
    assert_eq!(rows.routed(), routed(&[("src", "b")]));
    rows.decide("b", Human, Reject);
    assert_eq!(rows.routed(), routed(&[("src", "a")]), "go back, not dark");
    rows.decide("a", Human, Reject);
    assert!(rows.routed().is_empty());
}

#[test]
fn a_human_reject_binds_the_mapping_not_only_the_proposal_id() -> TestResult {
    let mut rows = Rows::new();
    rows.propose("a", "src", "A")
        .decide("a", Policy, Accept)
        .decide("a", Human, Reject)
        // A producer re-proposes the same bytes under a new id, and policy accepts it.
        .propose("a2", "src", "A")
        .decide("a2", Policy, Accept);
    assert!(
        rows.routed().is_empty(),
        "the revoked mapping stays revoked"
    );

    // Different bytes are a different mapping: the bind does not reach them.
    rows.propose("b", "src", "B").decide("b", Policy, Accept);
    assert_eq!(rows.routed(), routed(&[("src", "b")]));

    // The bind is per source: the same mapping for another source is unaffected.
    rows.propose("x", "other", "A").decide("x", Policy, Accept);
    assert_eq!(rows.routed(), routed(&[("other", "x"), ("src", "b")]));

    // A later human accept on any proposal of that identity lifts it. With b revoked, the
    // effective proposal is the newest accepted (a2), reported under the earliest id of its
    // identity (a).
    rows.decide("a2", Human, Accept).decide("b", Human, Reject);
    let resolution = rows.resolve();
    let effective = &resolution.routes[&SourceId::new("src")?];
    assert_eq!(effective.proposal_id, "a");
    assert_eq!(
        effective.mapping,
        serde_json::from_str::<StreamMapping>(&mapping_json("A"))?
    );
    Ok(())
}

#[test]
fn a_same_bytes_re_proposal_keeps_the_earliest_proposal_id() -> TestResult {
    let mut rows = Rows::new();
    rows.propose("a", "src", "A")
        .decide("a", Policy, Accept)
        .propose("a2", "src", "A")
        .decide("a2", Policy, Accept);
    let before: StreamMapping = serde_json::from_str(&mapping_json("A"))?;
    let resolution = rows.resolve();
    let effective = &resolution.routes[&SourceId::new("src")?];
    assert_eq!(effective.proposal_id, "a");
    assert_eq!(effective.identity, before.identity()?);
    Ok(())
}

#[test]
fn two_sources_get_two_routes() {
    let mut rows = Rows::new();
    rows.propose("a", "one", "A")
        .propose("b", "two", "B")
        .decide("a", Policy, Accept)
        .decide("b", Human, Accept);
    assert_eq!(rows.routed(), routed(&[("one", "a"), ("two", "b")]));
}

#[test]
fn unusable_payloads_are_excluded_and_named_and_other_rows_still_route() {
    let mut rows = Rows::new();
    rows.propose_raw("not-json", STREAM_MAPPING_CLASS, b"{".to_vec())
        .propose_raw(
            "format-2",
            STREAM_MAPPING_CLASS,
            format!(
                r#"{{"format":2,"source":"src","mapping":{}}}"#,
                mapping_json("A")
            )
            .into_bytes(),
        )
        .propose_raw(
            "bad-source",
            STREAM_MAPPING_CLASS,
            format!(
                r#"{{"format":1,"source":"a b","mapping":{}}}"#,
                mapping_json("A")
            )
            .into_bytes(),
        )
        .propose_raw(
            "invalid",
            STREAM_MAPPING_CLASS,
            envelope("src", "A").replace_version(),
        )
        .propose_raw("other-class", "class-x", b"{".to_vec())
        .propose("good", "src", "A");
    for id in [
        "not-json",
        "format-2",
        "bad-source",
        "invalid",
        "other-class",
        "good",
    ] {
        rows.decide(id, Policy, Accept);
    }
    let resolution = rows.resolve();
    let excluded: Vec<&str> = resolution
        .excluded
        .iter()
        .map(|e| e.proposal_id.as_str())
        .collect();
    assert_eq!(excluded, ["not-json", "format-2", "bad-source", "invalid"]);
    assert!(
        resolution.excluded[3].reason.contains("version"),
        "{:?}",
        resolution.excluded[3]
    );
    assert_eq!(rows.routed(), routed(&[("src", "good")]));
}

trait ReplaceVersion {
    fn replace_version(self) -> Vec<u8>;
}

impl ReplaceVersion for Vec<u8> {
    /// The mapping's `"version":1` becomes `"version":9`: decodes, fails validation.
    fn replace_version(self) -> Vec<u8> {
        String::from_utf8_lossy(&self)
            .replace(r#""version":1"#, r#""version":9"#)
            .into_bytes()
    }
}

#[test]
fn identical_mappings_under_different_whitespace_share_an_identity() -> TestResult {
    let compact = envelope("src", "A");
    let spaced = format!(
        "{{ \"source\" : \"src\",\n \"format\": 1, \"mapping\": {} }}",
        serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(&mapping_json(
            "A"
        ))?)?
    );
    let (_, _, first) = decode_envelope(&compact)?;
    let (_, _, second) = decode_envelope(spaced.as_bytes())?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn the_registry_routes_each_resolved_source_to_its_named_mapping_engine() -> TestResult {
    let mut rows = Rows::new();
    rows.propose("a", "one", "A").decide("a", Policy, Accept);
    let resolution = rows.resolve();
    let registry = registry(&resolution)?;
    let one = registry.engines_for(&SourceId::new("one")?);
    assert_eq!(one.len(), 1);
    assert_eq!(
        one[0].name(),
        format!(
            "mapping-{}",
            resolution.routes[&SourceId::new("one")?].identity
        )
    );
    let provenance: serde_json::Value =
        serde_json::from_slice(&one[0].provenance().unwrap_or_default())?;
    assert_eq!(provenance["proposal_id"], "a");
    assert_eq!(registry.engines_for(&SourceId::new("stdin")?).len(), 1);
    assert!(registry.engines_for(&SourceId::new("two")?).is_empty());
    Ok(())
}

#[test]
fn a_missing_store_resolves_to_no_routes_and_is_not_created() -> TestResult {
    let dir = crate::tests::TestDirectory::new("routes-missing-store");
    assert_eq!(load(dir.path())?, Resolution::default());
    assert!(!dir.path().join(PROPOSAL_DATABASE_FILE).exists());
    Ok(())
}
