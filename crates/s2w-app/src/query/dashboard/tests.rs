use s2w_log::{Actor, Decider, LogPosition, Outcome};
use serde_json::{Value, json};

use super::*;
use crate::tests::TestDirectory;

const WORLD: &str = "w";
const SOURCE: &str = "src";

/// A one-rule mapping whose entities get type label `label`.
fn mapping_json(label: &str) -> Value {
    json!({ "version": 1, "decode": [],
            "entities": [{ "id": "e", "type_label": label, "key": [["id"]], "attrs": [] }],
            "relationships": [] })
}

fn mapping_identity(label: &str) -> String {
    let payload = json!({ "format": 1, "source": SOURCE, "mapping": mapping_json(label) });
    decode_mapping(payload.to_string().as_bytes())
        .expect("valid mapping")
        .2
}

/// A domain-level manifest built on the `label` mapping; `name` varies its identity.
fn manifest_json(label: &str, name: &str) -> Value {
    json!({
        "built_on": [{ "source": SOURCE, "mapping": mapping_identity(label) }],
        "domain": { "name": name, "summary": "One sentence about it." },
        "quintessential_projection": { "template": "feed", "rationale": "Activity.", "slots": {} },
        "roles": [{ "id": "r1", "name": "Watcher", "default": true, "questions": [],
                    "projection": { "template": "feed", "slots": {} } }],
        "types": [{ "type": label, "primary": true }]
    })
}

fn envelope_json(world: &str, manifest: &Value) -> Value {
    let error = if manifest.is_null() {
        json!("timeout")
    } else {
        Value::Null
    };
    json!({
        "format": 1, "world": world, "input_hash": "0123456789abcdef", "attempt": 1,
        "manifest": manifest,
        "provenance": { "prompt_hash": null, "input_tokens": null, "output_tokens": null,
                        "latency_ms": null, "raw": null, "error": error }
    })
}

#[derive(Default)]
struct Rows {
    proposals: Vec<StoredProposal>,
    decisions: Vec<StoredDecision>,
}

impl Rows {
    fn propose(&mut self, id: &str, class: &str, payload: &Value) -> &mut Self {
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
            payload: payload.to_string().into_bytes(),
            proposed_at_ms: 0,
        });
        self
    }

    fn mapping(&mut self, id: &str, label: &str) -> &mut Self {
        let payload = json!({ "format": 1, "source": SOURCE, "mapping": mapping_json(label) });
        self.propose(id, STREAM_MAPPING_CLASS, &payload).decide(
            id,
            Decider::Policy,
            Outcome::Accept,
        )
    }

    fn dashboard(&mut self, id: &str, world: &str, manifest: &Value) -> &mut Self {
        self.propose(
            id,
            DASHBOARD_MANIFEST_CLASS,
            &envelope_json(world, manifest),
        )
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

    fn view(&self) -> DashboardView {
        dashboard_view(WORLD, &self.proposals, &self.decisions)
    }
}

#[test]
fn a_valid_envelope_decodes_to_its_world_manifest_and_identity() {
    let manifest = manifest_json("A", "one");
    let payload = envelope_json(WORLD, &manifest).to_string();
    let (world, decoded, identity) = decode_envelope(payload.as_bytes()).expect("valid");
    assert_eq!(world, WORLD);
    assert_eq!(identity, decoded.identity().expect("identity"));
    let reparsed: DashboardManifest = serde_json::from_value(manifest).expect("manifest");
    assert_eq!(decoded, reparsed);
}

#[test]
fn each_envelope_rule_refuses_its_fault() {
    type Mutation = fn(&mut Value);
    let cases: [(&str, Mutation); 13] = [
        ("unknown field", |e| e["extra"] = json!(1)),
        ("unknown field", |e| e["provenance"]["extra"] = json!(1)),
        ("missing field `manifest`", |e| {
            e.as_object_mut().map(|o| o.remove("manifest"));
        }),
        ("missing field `raw`", |e| {
            e["provenance"].as_object_mut().map(|o| o.remove("raw"));
        }),
        ("envelope format 2", |e| e["format"] = json!(2)),
        ("world: must not be empty", |e| e["world"] = json!("")),
        ("input_hash", |e| {
            e["input_hash"] = json!("0123456789ABCDEF")
        }),
        ("attempt: 0", |e| e["attempt"] = json!(0)),
        ("attempt: 4", |e| e["attempt"] = json!(4)),
        ("provenance.prompt_hash", |e| {
            e["provenance"]["prompt_hash"] = json!("abc")
        }),
        ("provenance.raw", |e| {
            e["provenance"]["raw"] = json!("x".repeat(MAX_RAW_BYTES + 1));
        }),
        ("provenance.error must be null", |e| {
            e["provenance"]["error"] = json!("late")
        }),
        ("provenance.error must be set", |e| {
            e["manifest"] = Value::Null
        }),
    ];
    for (expect, mutate) in cases {
        let mut envelope = envelope_json(WORLD, &manifest_json("A", "one"));
        mutate(&mut envelope);
        let error = parse_envelope(envelope.to_string().as_bytes()).expect_err(expect);
        assert!(error.contains(expect), "{expect}: {error}");
    }
}

#[test]
fn a_null_or_malformed_manifest_has_no_identity() {
    let payload = envelope_json(WORLD, &Value::Null).to_string();
    let error = decode_envelope(payload.as_bytes()).expect_err("null");
    assert_eq!(error, "manifest is null: timeout");

    let mut manifest = manifest_json("A", "one");
    manifest["roles"][0]["default"] = json!(false);
    let payload = envelope_json(WORLD, &manifest).to_string();
    let error = decode_envelope(payload.as_bytes()).expect_err("no default role");
    assert!(error.starts_with("manifest: "), "{error}");
}

#[test]
fn resolution_follows_decision_0023_per_world() {
    use Decider::{Human, Policy};
    use Outcome::{Accept, Reject};

    // The largest accepted seq wins; another world's manifest is not this world's.
    let mut rows = Rows::default();
    rows.mapping("m", "A")
        .dashboard("d1", WORLD, &manifest_json("A", "one"))
        .decide("d1", Policy, Accept)
        .dashboard("d2", WORLD, &manifest_json("A", "two"))
        .decide("d2", Policy, Accept)
        .dashboard("other", "elsewhere", &manifest_json("A", "three"))
        .decide("other", Policy, Accept);
    assert_eq!(rows.view().proposal_id.as_deref(), Some("d2"));

    // A human reject of the newest falls back to the one before it.
    rows.decide("d2", Human, Reject);
    assert_eq!(rows.view().proposal_id.as_deref(), Some("d1"));

    // The reject binds the identity: the same bytes re-proposed and policy-accepted stay out.
    rows.dashboard("d3", WORLD, &manifest_json("A", "two"))
        .decide("d3", Policy, Accept);
    assert_eq!(rows.view().proposal_id.as_deref(), Some("d1"));

    let view = rows.view();
    assert_eq!(
        view.actor,
        Some(ActorDto::Agent {
            model: "m".to_owned(),
            version: "1".to_owned()
        })
    );
    let manifest = view.manifest.expect("manifest");
    assert_eq!(view.identity, Some(manifest.identity().expect("identity")));
    assert!(!view.stale && view.stale_entries.is_empty());
    assert!(view.excluded.is_empty());
}

#[test]
fn null_and_undecodable_rows_are_excluded_and_reported_for_this_world_only() {
    let mut rows = Rows::default();
    rows.mapping("m", "A")
        .dashboard("null", WORLD, &Value::Null)
        .decide("null", Decider::Policy, Outcome::Reject)
        .dashboard("theirs", "elsewhere", &Value::Null)
        .propose("junk", DASHBOARD_MANIFEST_CLASS, &json!("not an envelope"));
    let view = rows.view();
    assert_eq!(view.manifest, None);
    let excluded: Vec<&str> = view
        .excluded
        .iter()
        .map(|row| row.proposal_id.as_str())
        .collect();
    assert_eq!(excluded, ["null", "junk"]);
    assert_eq!(view.excluded[0].reason, "manifest is null: timeout");
}

#[test]
fn a_mapping_change_makes_the_manifest_stale() {
    let mut rows = Rows::default();
    rows.mapping("m1", "A")
        .dashboard("d", WORLD, &manifest_json("A", "one"))
        .decide("d", Decider::Policy, Outcome::Accept);
    assert!(!rows.view().stale);

    rows.mapping("m2", "B");
    let view = rows.view();
    assert!(view.stale);
    assert!(!view.stale_entries.is_empty());
    assert_eq!(
        view.proposal_id.as_deref(),
        Some("d"),
        "stale is served, not dropped"
    );
}

#[test]
fn a_missing_store_reads_as_no_dashboard_and_is_not_created() {
    let dir = TestDirectory::new("dashboard-missing");
    let view = read_dashboard(dir.path(), WORLD).expect("read");
    assert_eq!(view, DashboardView::default());
    let body = serde_json::to_value(&view).expect("json");
    assert_eq!(
        body,
        json!({ "manifest": null, "proposal_id": null, "actor": null, "identity": null,
                "stale": false, "stale_entries": [], "excluded": [] })
    );
    assert!(!dir.path().join(s2w_log::PROPOSAL_DATABASE_FILE).exists());
}
