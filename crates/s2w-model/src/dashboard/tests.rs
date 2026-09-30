use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;
use crate::{AttrRule, EntityRule, RelationshipRule, Segment, StreamMapping};

const SOURCE: &str = "s1";

fn path(keys: &[&str]) -> FieldPath {
    FieldPath(keys.iter().map(|k| Segment::Key((*k).to_owned())).collect())
}

fn rule(id: &str, label: &str, key: &str, attrs: &[(&str, &[&str])]) -> EntityRule {
    EntityRule {
        id: id.to_owned(),
        type_label: label.to_owned(),
        key: vec![path(&[key])],
        attrs: attrs
            .iter()
            .map(|(name, p)| AttrRule {
                name: (*name).to_owned(),
                path: path(p),
            })
            .collect(),
    }
}

fn mapping() -> StreamMapping {
    StreamMapping {
        version: crate::MAPPING_VERSION,
        decode: vec![],
        entities: vec![
            rule(
                "doc",
                "item",
                "id",
                &[("title", &["title"]), ("size", &["size", "new"])],
            ),
            rule("who", "user", "user", &[]),
            rule("tag", "group", "group", &[]),
        ],
        relationships: vec![RelationshipRule {
            from: "doc".to_owned(),
            to: "who".to_owned(),
            kind: "by".to_owned(),
        }],
        links: vec![],
    }
}

fn accepted(mapping: StreamMapping) -> AcceptedMapping {
    AcceptedMapping {
        source: SOURCE.to_owned(),
        identity: mapping.identity().expect("valid mapping"),
        mapping,
    }
}

fn context() -> ManifestContext {
    let paths = [
        &["id"][..],
        &["title"],
        &["user"],
        &["group"],
        &["type"],
        &["size", "new"],
        &["size", "old"],
        &["lat"],
        &["lon"],
        &["price"],
        &["qty"],
        &["side"],
    ]
    .iter()
    .map(|p| path(p))
    .collect();
    ManifestContext {
        mappings: vec![accepted(mapping())],
        paths: BTreeMap::from([(SOURCE.to_owned(), paths)]),
    }
}

fn identity() -> String {
    mapping().identity().expect("valid mapping")
}

fn full_json() -> Value {
    json!({
        "built_on": [{ "source": SOURCE, "mapping": identity() }],
        "domain": { "name": "A world", "summary": "One sentence about it." },
        "quintessential_projection": {
            "template": "document",
            "rationale": "Its natives read one item at a time.",
            "slots": { "subject_type": "item", "actor_type": "user", "links": [["item", "user"]] }
        },
        "roles": [
            { "id": "r1", "name": "Reader", "default": true, "questions": ["What changed?"],
              "projection": { "template": "feed", "slots": { "subject_type": "item" } } },
            { "id": "r2", "name": "Mapper", "default": false, "questions": [],
              "projection": { "template": "graph", "slots": { "types": ["item", "user"] } } }
        ],
        "types": [
            { "type": "item", "primary": true, "noun": "item", "label": { "attr": "title" },
              "kind": "document" },
            { "type": "user", "primary": false, "label": { "key": 0 }, "kind": "person" }
        ],
        "events": [
            { "source": SOURCE, "when": { "path": ["type"], "equals": "edit" },
              "sentence": { "text": "{0} changed {1} ({2})",
                            "fields": [["user"], { "truncate": ["title"] },
                                       { "delta": [["size", "new"], ["size", "old"]] }] } }
        ]
    })
}

fn domain_level_json() -> Value {
    json!({
        "built_on": [{ "source": SOURCE, "mapping": identity() }],
        "domain": { "name": "A world", "summary": "One sentence about it." },
        "quintessential_projection": { "template": "feed", "rationale": "Activity.", "slots": {} },
        "roles": [{ "id": "r1", "name": "Watcher", "default": true, "questions": [],
                    "projection": { "template": "feed", "slots": {} } }],
        "types": [{ "type": "item", "primary": true }]
    })
}

fn full() -> DashboardManifest {
    serde_json::from_value(full_json()).expect("the full manifest decodes")
}

#[test]
fn a_full_manifest_validates_and_round_trips() {
    let manifest = full();
    assert_eq!(manifest.validate(&context()), Ok(()));
    let back: DashboardManifest =
        serde_json::from_slice(&serde_json::to_vec(&manifest).expect("encodes")).expect("decodes");
    assert_eq!(back, manifest);
}

#[test]
fn a_domain_level_only_manifest_validates() {
    let manifest: DashboardManifest =
        serde_json::from_value(domain_level_json()).expect("the domain-level manifest decodes");
    assert_eq!(manifest.types[0].label, None);
    assert_eq!(manifest.events, None);
    assert_eq!(manifest.validate(&context()), Ok(()));
}

/// Pins format 1's identity. A change here moves every stored manifest's identity, and with it
/// every human decision bound to one: that needs a decision record (0029).
#[test]
fn identities_are_pinned() {
    assert_eq!(identity(), "eba51e126c2ffde0", "fixture mapping identity");
    assert_eq!(full().identity().expect("encodes"), "e561684ba3a24b35");
    let domain: DashboardManifest = serde_json::from_value(domain_level_json()).expect("decodes");
    assert_eq!(domain.identity().expect("encodes"), "ab2ea6789280cac2");
    // The canonical bytes: declaration order, absent optional fields skipped.
    let canonical =
        String::from_utf8(serde_json::to_vec(&domain).expect("encodes")).expect("utf-8");
    assert_eq!(
        canonical,
        format!(
            r#"{{"built_on":[{{"source":"s1","mapping":"{}"}}],"domain":{{"name":"A world","summary":"One sentence about it."}},"quintessential_projection":{{"template":"feed","rationale":"Activity.","slots":{{}}}},"roles":[{{"id":"r1","name":"Watcher","default":true,"questions":[],"projection":{{"template":"feed","slots":{{}}}}}}],"types":[{{"type":"item","primary":true}}]}}"#,
            identity()
        )
    );
    let digest = crate::Fnv64::new()
        .write_field(&DASHBOARD_FORMAT.to_le_bytes())
        .write_field(canonical.as_bytes())
        .finish();
    assert_eq!(format!("{digest:016x}"), "ab2ea6789280cac2");
}

#[test]
fn identity_ignores_whitespace_and_key_order_but_not_values() {
    let text = r#"{ "types": [{"primary": true, "type": "item"}],
        "roles": [{"projection": {"slots": {}, "template": "feed"}, "questions": [],
                   "default": true, "name": "Watcher", "id": "r1"}],
        "quintessential_projection": {"slots": {}, "rationale": "Activity.", "template": "feed"},
        "domain": {"summary": "One sentence about it.", "name": "A world"},
        "built_on": [{"mapping": "IDENTITY", "source": "s1"}] }"#
        .replace("IDENTITY", &identity());
    let reordered: DashboardManifest = serde_json::from_str(&text).expect("decodes");
    let canonical: DashboardManifest =
        serde_json::from_value(domain_level_json()).expect("decodes");
    assert_eq!(reordered.identity(), canonical.identity());
    let mut changed = canonical.clone();
    changed.types[0].primary = false;
    assert_ne!(changed.identity(), canonical.identity());
}

#[test]
fn unknown_fields_are_refused_at_every_level() {
    for pointer in [
        "",
        "/built_on/0",
        "/domain",
        "/quintessential_projection",
        "/quintessential_projection/slots",
        "/roles/0",
        "/roles/0/projection",
        "/roles/0/projection/slots",
        "/types/0",
        "/types/0/label",
        "/types/1/label",
        "/events/0",
        "/events/0/when",
        "/events/0/sentence",
        "/events/0/sentence/fields/1",
        "/events/0/sentence/fields/2",
    ] {
        let mut value = full_json();
        value
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("{pointer} is an object"))
            .insert("unexpected".to_owned(), json!(1));
        assert!(
            serde_json::from_value::<DashboardManifest>(value).is_err(),
            "an unknown field at {pointer:?} must be refused"
        );
    }
}

type ValueMutation = fn(&mut Value);

#[test]
fn closed_sets_and_required_fields_are_refused_when_decoding() {
    let cases: [(&str, ValueMutation); 7] = [
        ("an unknown template", |v| {
            v["quintessential_projection"]["template"] = json!("chart");
        }),
        ("an unknown kind", |v| {
            v["types"][0]["kind"] = json!("animal")
        }),
        ("a label with both attr and key", |v| {
            v["types"][0]["label"] = json!({ "attr": "title", "key": 0 });
        }),
        ("a missing domain", |v| {
            v.as_object_mut().expect("object").remove("domain");
        }),
        ("a missing primary", |v| {
            v["types"][0]
                .as_object_mut()
                .expect("object")
                .remove("primary");
        }),
        ("a missing rationale", |v| {
            v["quintessential_projection"]
                .as_object_mut()
                .expect("object")
                .remove("rationale");
        }),
        ("an unknown formatter", |v| {
            v["events"][0]["sentence"]["fields"][1] = json!({ "upper": ["title"] });
        }),
    ];
    for (name, mutate) in cases {
        let mut value = full_json();
        mutate(&mut value);
        assert!(
            serde_json::from_value::<DashboardManifest>(value).is_err(),
            "{name} must be refused"
        );
    }
}

type Mutation = fn(&mut DashboardManifest);
type Expect = fn(&DashboardError) -> bool;

fn refuses(cases: &[(&str, Mutation, Expect)]) {
    for (name, mutate, expect) in cases {
        let mut manifest = full();
        mutate(&mut manifest);
        let error = manifest
            .validate(&context())
            .expect_err(&format!("{name} must be refused"));
        assert!(expect(&error), "{name}: unexpected fault {error:?}");
    }
}

fn long() -> String {
    "x".repeat(MAX_STRING_CHARS + 1)
}

fn role(id: &str, default: bool) -> Role {
    Role {
        id: id.to_owned(),
        name: "Role".to_owned(),
        default,
        questions: vec![],
        projection: Projection {
            template: Template::Feed,
            slots: Slots::default(),
        },
    }
}

#[test]
fn each_built_on_and_strings_rule_refuses_its_fault() {
    use DashboardError as E;
    refuses(&[
        (
            "no built_on",
            |m| m.built_on.clear(),
            |e| matches!(e, E::NoItems { at } if at == "built_on"),
        ),
        (
            "too many built_on",
            |m| {
                m.built_on = (0..=MAX_BUILT_ON)
                    .map(|i| BuiltOn {
                        source: format!("s{i}"),
                        mapping: "0000000000000000".to_owned(),
                    })
                    .collect();
            },
            |e| matches!(e, E::TooMany { .. }),
        ),
        (
            "a repeated built_on source",
            |m| m.built_on.push(m.built_on[0].clone()),
            |e| matches!(e, E::Duplicate { .. }),
        ),
        (
            "a built_on mapping that is no identity",
            |m| m.built_on[0].mapping = "ABC".to_owned(),
            |e| matches!(e, E::NotAnIdentity { .. }),
        ),
        (
            "a built_on source with no accepted mapping",
            |m| m.built_on[0].source = "s9".to_owned(),
            |e| matches!(e, E::UnknownSource { .. }),
        ),
        (
            "a built_on mapping that is not the accepted one",
            |m| {
                m.built_on[0].mapping = "0123456789abcdef".to_owned();
            },
            |e| matches!(e, E::MappingMismatch { .. }),
        ),
        (
            "an empty domain name",
            |m| m.domain.name.clear(),
            |e| matches!(e, E::Empty { at } if at == "domain.name"),
        ),
        (
            "a string past the cap",
            |m| m.domain.summary = long(),
            |e| matches!(e, E::TooLong { .. }),
        ),
        (
            "an angle bracket",
            |m| m.roles[0].questions[0] = "<b>x</b>".to_owned(),
            |e| matches!(e, E::AngleBracket { .. }),
        ),
    ]);
}

#[test]
fn each_roles_rule_refuses_its_fault() {
    use DashboardError as E;
    refuses(&[
        (
            "no roles",
            |m| m.roles.clear(),
            |e| matches!(e, E::NoItems { at } if at == "roles"),
        ),
        (
            "too many roles",
            |m| {
                m.roles = (0..=MAX_ROLES)
                    .map(|i| role(&format!("r{i}"), i == 0))
                    .collect();
            },
            |e| matches!(e, E::TooMany { .. }),
        ),
        (
            "no default role",
            |m| m.roles[0].default = false,
            |e| *e == E::DefaultRoles { count: 0 },
        ),
        (
            "two default roles",
            |m| m.roles[1].default = true,
            |e| *e == E::DefaultRoles { count: 2 },
        ),
        (
            "a repeated role id",
            |m| m.roles[1].id = "r1".to_owned(),
            |e| matches!(e, E::Duplicate { .. }),
        ),
        (
            "too many questions",
            |m| m.roles[0].questions = vec!["Q?".to_owned(); MAX_QUESTIONS + 1],
            |e| matches!(e, E::TooMany { .. }),
        ),
    ]);
}

#[test]
fn each_types_rule_refuses_its_fault() {
    use DashboardError as E;
    refuses(&[
        (
            "too many types",
            |m| {
                m.types = (0..=MAX_TYPES)
                    .map(|i| TypeRow {
                        type_label: format!("t{i}"),
                        primary: false,
                        noun: None,
                        label: None,
                        kind: None,
                    })
                    .collect();
            },
            |e| matches!(e, E::TooMany { .. }),
        ),
        (
            "a repeated type",
            |m| m.types[1].type_label = "item".to_owned(),
            |e| matches!(e, E::Duplicate { .. }),
        ),
        (
            "an unknown type",
            |m| m.types[0].type_label = "thing".to_owned(),
            |e| matches!(e, E::UnknownType { .. }),
        ),
        (
            "an empty noun",
            |m| m.types[0].noun = Some(String::new()),
            |e| matches!(e, E::Empty { .. }),
        ),
        (
            "an unknown label attribute",
            |m| {
                m.types[0].label = Some(Label::Attr(AttrLabel {
                    attr: "name".to_owned(),
                }));
            },
            |e| matches!(e, E::UnknownAttr { .. }),
        ),
        (
            "a label key part out of range",
            |m| {
                m.types[1].label = Some(Label::Key(KeyLabel { key: 1 }));
            },
            |e| matches!(e, E::KeyOutOfRange { index: 1, .. }),
        ),
    ]);
}

#[test]
fn each_events_rule_refuses_its_fault() {
    use DashboardError as E;
    refuses(&[
        (
            "too many events",
            |m| {
                let one = m.events.as_ref().expect("events")[0].clone();
                m.events = Some(vec![one; MAX_EVENTS + 1]);
            },
            |e| matches!(e, E::TooMany { .. }),
        ),
        (
            "an event source with no accepted mapping",
            |m| {
                m.events.as_mut().expect("events")[0].source = "s9".to_owned();
            },
            |e| matches!(e, E::UnknownSource { .. }),
        ),
        (
            "an event-type path outside the profile",
            |m| {
                m.events.as_mut().expect("events")[0]
                    .when
                    .as_mut()
                    .expect("when")
                    .path = path(&["kind"]);
            },
            |e| matches!(e, E::UnknownPath { .. }),
        ),
        (
            "an empty event-type path",
            |m| {
                m.events.as_mut().expect("events")[0]
                    .when
                    .as_mut()
                    .expect("when")
                    .path = FieldPath(vec![]);
            },
            |e| matches!(e, E::EmptyPath { .. }),
        ),
        (
            "a sentence path outside the profile",
            |m| {
                m.events.as_mut().expect("events")[0].sentence.fields[0] =
                    SentenceField::Path(path(&["nope"]));
            },
            |e| matches!(e, E::UnknownPath { .. }),
        ),
        (
            "a delta path outside the profile",
            |m| {
                m.events.as_mut().expect("events")[0].sentence.fields[2] =
                    SentenceField::Delta(DeltaField {
                        delta: [path(&["size", "new"]), path(&["nope"])],
                    });
            },
            |e| matches!(e, E::UnknownPath { .. }),
        ),
    ]);
}

#[test]
fn each_sentence_rule_refuses_its_fault() {
    use DashboardError as E;
    refuses(&[
        (
            "a placeholder with no field",
            |m| {
                m.events.as_mut().expect("events")[0].sentence.text = "{0} {1} {2} {3}".to_owned();
            },
            |e| matches!(e, E::Placeholder { reason, .. } if reason.contains("{3}")),
        ),
        (
            "a field never shown",
            |m| {
                m.events.as_mut().expect("events")[0].sentence.text = "{0} {1}".to_owned();
            },
            |e| matches!(e, E::Placeholder { reason, .. } if reason.contains("never shown")),
        ),
        (
            "a stray opening brace",
            |m| {
                m.events.as_mut().expect("events")[0].sentence.text = "{0} {1} {2} {x}".to_owned();
            },
            |e| matches!(e, E::Placeholder { .. }),
        ),
        (
            "a stray closing brace",
            |m| {
                m.events.as_mut().expect("events")[0].sentence.text = "{0} {1} {2} }".to_owned();
            },
            |e| matches!(e, E::Placeholder { .. }),
        ),
        (
            "too many sentence fields",
            |m| {
                m.events.as_mut().expect("events")[0].sentence.fields =
                    vec![SentenceField::Path(path(&["user"])); MAX_SENTENCE_FIELDS + 1];
            },
            |e| matches!(e, E::TooMany { .. }),
        ),
    ]);
}

#[test]
fn strings_at_the_cap_and_empty_event_values_are_accepted() {
    let mut manifest = full();
    manifest.domain.summary = "x".repeat(MAX_STRING_CHARS);
    manifest.events.as_mut().expect("events")[0]
        .when
        .as_mut()
        .expect("when")
        .equals
        .clear();
    assert_eq!(manifest.validate(&context()), Ok(()));
}

/// Validates `slots` under `template` as the quintessential projection and as a role's.
fn slot_result(template: Template, slots: &Value) -> Result<(), DashboardError> {
    let slots: Slots = serde_json::from_value(slots.clone()).expect("slots decode");
    let mut quintessential = full();
    quintessential.quintessential_projection.template = template;
    quintessential.quintessential_projection.slots = slots.clone();
    let mut in_role = full();
    in_role.roles[1].projection = Projection { template, slots };
    let first = quintessential.validate(&context());
    let second = in_role.validate(&context());
    // The same rule refuses a role's projection; only the location differs.
    assert_eq!(
        first.as_ref().err().map(std::mem::discriminant),
        second.as_ref().err().map(std::mem::discriminant),
        "{template:?}: a role's projection is judged by the same table"
    );
    first
}

#[test]
fn every_template_accepts_its_slots() {
    for (template, slots) in [
        (Template::Document, json!({ "subject_type": "item" })),
        (
            Template::Document,
            json!({ "subject_type": "item", "actor_type": "user", "links": [["item", "user"]] }),
        ),
        (Template::Feed, json!({})),
        (
            Template::Feed,
            json!({ "subject_type": "item", "actor_type": "user" }),
        ),
        (Template::Graph, json!({ "types": ["item"] })),
        (
            Template::Graph,
            json!({ "types": ["item", "user", "group"] }),
        ),
        (Template::Table, json!({ "type": "item" })),
        (
            Template::Table,
            json!({ "type": "item", "columns": ["title", "size"] }),
        ),
        (Template::Map, json!({ "lat": ["lat"], "lon": ["lon"] })),
        (
            Template::Map,
            json!({ "lat": ["lat"], "lon": ["lon"], "subject_type": "item" }),
        ),
        (
            Template::Ladder,
            json!({ "price": ["price"], "quantity": ["qty"] }),
        ),
        (
            Template::Ladder,
            json!({ "price": ["price"], "quantity": ["qty"], "side": ["side"] }),
        ),
    ] {
        assert_eq!(
            slot_result(template, &slots),
            Ok(()),
            "{template:?} {slots}"
        );
    }
}

fn refuses_slots(cases: Vec<(Template, Value, Expect)>) {
    for (template, slots, expect) in cases {
        let error = slot_result(template, &slots)
            .expect_err(&format!("{template:?} {slots} must be refused"));
        assert!(
            expect(&error),
            "{template:?} {slots}: unexpected fault {error:?}"
        );
    }
}

fn nine() -> Vec<String> {
    (0..=MAX_SLOT_ITEMS).map(|i| format!("t{i}")).collect()
}

#[test]
fn each_document_slot_rule_refuses_its_fault() {
    use DashboardError as E;
    let t = Template::Document;
    refuses_slots(vec![
        (t, json!({}), |e| {
            matches!(
                e,
                E::SlotMissing {
                    slot: "subject_type",
                    ..
                }
            )
        }),
        (t, json!({ "subject_type": "thing" }), |e| {
            matches!(e, E::UnknownType { .. })
        }),
        (
            t,
            json!({ "subject_type": "item", "actor_type": "thing" }),
            |e| matches!(e, E::UnknownType { .. }),
        ),
        (
            t,
            json!({ "subject_type": "item", "actor_type": "item" }),
            |e| matches!(e, E::SameType { .. }),
        ),
        (t, json!({ "subject_type": "item", "links": [] }), |e| {
            matches!(e, E::NoItems { .. })
        }),
        (
            t,
            json!({ "subject_type": "item", "links": [["user", "item"]] }),
            |e| matches!(e, E::UnknownRelationship { .. }),
        ),
        (
            t,
            json!({ "subject_type": "item", "links": [["item", "thing"]] }),
            |e| matches!(e, E::UnknownType { .. }),
        ),
        (
            t,
            json!({ "subject_type": "item", "types": ["item"] }),
            |e| matches!(e, E::SlotNotAllowed { slot: "types", .. }),
        ),
    ]);
}

#[test]
fn each_feed_slot_rule_refuses_its_fault() {
    use DashboardError as E;
    let t = Template::Feed;
    refuses_slots(vec![
        (t, json!({ "subject_type": "thing" }), |e| {
            matches!(e, E::UnknownType { .. })
        }),
        (t, json!({ "actor_type": "thing" }), |e| {
            matches!(e, E::UnknownType { .. })
        }),
        (
            t,
            json!({ "subject_type": "item", "actor_type": "item" }),
            |e| matches!(e, E::SameType { .. }),
        ),
        (t, json!({ "type": "item" }), |e| {
            matches!(e, E::SlotNotAllowed { slot: "type", .. })
        }),
        (t, json!({ "links": [["item", "user"]] }), |e| {
            matches!(e, E::SlotNotAllowed { slot: "links", .. })
        }),
    ]);
}

#[test]
fn each_graph_slot_rule_refuses_its_fault() {
    use DashboardError as E;
    let t = Template::Graph;
    refuses_slots(vec![
        (t, json!({}), |e| {
            matches!(e, E::SlotMissing { slot: "types", .. })
        }),
        (t, json!({ "types": [] }), |e| {
            matches!(e, E::NoItems { .. })
        }),
        (t, json!({ "types": nine() }), |e| {
            matches!(e, E::TooMany { .. })
        }),
        (t, json!({ "types": ["item", "item"] }), |e| {
            matches!(e, E::Duplicate { .. })
        }),
        (t, json!({ "types": ["thing"] }), |e| {
            matches!(e, E::UnknownType { .. })
        }),
        (
            t,
            json!({ "types": ["item"], "subject_type": "item" }),
            |e| matches!(e, E::SlotNotAllowed { .. }),
        ),
    ]);
}

#[test]
fn each_table_slot_rule_refuses_its_fault() {
    use DashboardError as E;
    let t = Template::Table;
    refuses_slots(vec![
        (t, json!({}), |e| {
            matches!(e, E::SlotMissing { slot: "type", .. })
        }),
        (t, json!({ "type": "thing" }), |e| {
            matches!(e, E::UnknownType { .. })
        }),
        (t, json!({ "type": "item", "columns": ["name"] }), |e| {
            matches!(e, E::UnknownAttr { .. })
        }),
        (t, json!({ "type": "item", "columns": [] }), |e| {
            matches!(e, E::NoItems { .. })
        }),
        (t, json!({ "type": "item", "columns": nine() }), |e| {
            matches!(e, E::TooMany { .. })
        }),
        (
            t,
            json!({ "type": "item", "columns": ["title", "title"] }),
            |e| matches!(e, E::Duplicate { .. }),
        ),
        (t, json!({ "type": "item", "actor_type": "user" }), |e| {
            matches!(e, E::SlotNotAllowed { .. })
        }),
    ]);
}

#[test]
fn each_map_slot_rule_refuses_its_fault() {
    use DashboardError as E;
    let t = Template::Map;
    refuses_slots(vec![
        (t, json!({ "lon": ["lon"] }), |e| {
            matches!(e, E::SlotMissing { slot: "lat", .. })
        }),
        (t, json!({ "lat": ["lat"] }), |e| {
            matches!(e, E::SlotMissing { slot: "lon", .. })
        }),
        (t, json!({ "lat": ["nope"], "lon": ["lon"] }), |e| {
            matches!(e, E::UnknownPath { .. })
        }),
        (t, json!({ "lat": [], "lon": ["lon"] }), |e| {
            matches!(e, E::EmptyPath { .. })
        }),
        (
            t,
            json!({ "lat": ["lat"], "lon": ["lon"], "subject_type": "thing" }),
            |e| matches!(e, E::UnknownType { .. }),
        ),
        (
            t,
            json!({ "lat": ["lat"], "lon": ["lon"], "price": ["price"] }),
            |e| matches!(e, E::SlotNotAllowed { .. }),
        ),
    ]);
}

#[test]
fn each_ladder_slot_rule_refuses_its_fault() {
    use DashboardError as E;
    let t = Template::Ladder;
    refuses_slots(vec![
        (t, json!({ "quantity": ["qty"] }), |e| {
            matches!(e, E::SlotMissing { slot: "price", .. })
        }),
        (t, json!({ "price": ["price"] }), |e| {
            matches!(
                e,
                E::SlotMissing {
                    slot: "quantity",
                    ..
                }
            )
        }),
        (
            t,
            json!({ "price": ["price"], "quantity": ["nope"] }),
            |e| matches!(e, E::UnknownPath { .. }),
        ),
        (
            t,
            json!({ "price": ["price"], "quantity": ["qty"], "side": ["nope"] }),
            |e| matches!(e, E::UnknownPath { .. }),
        ),
        (
            t,
            json!({ "price": ["price"], "quantity": ["qty"], "subject_type": "item" }),
            |e| matches!(e, E::SlotNotAllowed { .. }),
        ),
    ]);
}

#[test]
fn an_unknown_slot_name_is_refused_when_decoding() {
    assert!(serde_json::from_value::<Slots>(json!({ "colour": "item" })).is_err());
}

#[test]
fn stale_entries_name_what_the_current_mappings_no_longer_have() {
    let manifest = full();
    assert_eq!(
        manifest.stale_entries(&context().mappings),
        Vec::<String>::new()
    );

    let mut changed = mapping();
    changed.entities[0]
        .attrs
        .retain(|attr| attr.name != "title");
    changed.entities.retain(|rule| rule.id != "tag");
    let stale = manifest.stale_entries(&[accepted(changed)]);
    assert_eq!(stale.len(), 2, "{stale:?}");
    assert!(
        stale[0].starts_with("built_on[0].mapping: mapping "),
        "{stale:?}"
    );
    assert!(stale[1].starts_with("types[0].label.attr: "), "{stale:?}");

    let gone = manifest.stale_entries(&[]);
    assert!(
        gone.iter().any(|e| e.starts_with("built_on[0]: source")),
        "{gone:?}"
    );
    assert!(
        gone.iter().any(|e| e.starts_with("types[0].type: ")),
        "{gone:?}"
    );
    assert!(
        gone.iter().any(|e| e.starts_with("events[0].source: ")),
        "{gone:?}"
    );
}

#[test]
fn the_read_path_does_not_check_paths() {
    let mut manifest = full();
    manifest.events.as_mut().expect("events")[0].sentence.fields[0] =
        SentenceField::Path(path(&["nope"]));
    assert!(manifest.stale_entries(&context().mappings).is_empty());
    assert!(matches!(
        manifest.validate(&context()),
        Err(DashboardError::UnknownPath { .. })
    ));
}

#[test]
fn a_manifest_input_implies_its_own_validation_context() {
    let expected = context();
    let accepted = &expected.mappings[0];
    let stats = |p: &FieldPath| PathStats {
        path: p.clone(),
        count: 1,
        distinct: 1,
        str_count: 0,
        str_len_mean: 0,
    };
    let input = ManifestInput {
        world: "w".to_owned(),
        sources: vec![SourceInput {
            source: accepted.source.clone(),
            mapping_identity: accepted.identity.clone(),
            mapping: accepted.mapping.clone(),
            events: 1,
            event_type: None,
            paths: expected.paths[SOURCE].iter().map(stats).collect(),
            sample: Vec::new(),
        }],
    };
    assert_eq!(input.context(), expected);
    assert_eq!(full().validate(&input.context()), Ok(()));
}

#[test]
fn fits_text_is_the_required_string_rule() {
    assert!(fits_text("a label"));
    assert!(!fits_text(""));
    assert!(!fits_text("a <b>"));
    assert!(!fits_text(&long()));
}
