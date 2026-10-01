//! `h-measure obfuscate` on a synthetic, neutral-named stream (decision 0018) in a temporary
//! root: determinism, domain separation, folds and aliases, no plaintext in the output, a field
//! order per key, the timestamp shift, URL canonicalization, the fail-closed refusals, and the
//! B3 promise that an obfuscated corpus scores the same as the plain one up to renaming.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::meta::Meta;
use super::{Request, obfuscate};
use crate::discover_replay::envelopes;
use crate::h_measure::grade::grade;
use crate::h_measure::key::KeySpec;
use crate::h_measure::pins::{DATA, sha256};

const KEY_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const KEY_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const ANSWER: &str = "toy-key.json";

const RULES: &str = r#"
version = 1

[[rule]]
path = ["site"]
domain = "site"

[[rule]]
path = ["host"]
domain = "site"
from = ["site"]

[[rule]]
path = ["base_url"]
domain = "site"
from = ["site"]

[[rule]]
path = ["doc"]
domain = "doc"
fold = [["site"]]

[[rule]]
path = ["doc_link"]
domain = "doc"
fold = [["site"]]
url_path = { base = ["base_url"], marker = "/d/", replace = [["_", " "]] }

[[rule]]
path = ["who"]
domain = "who"

[[rule]]
path = ["ver", "new"]
domain = "ver"
fold = [["site"]]

[[rule]]
path = ["ver", "old"]
domain = "ver"
fold = [["site"]]

[[rule]]
path = ["link"]
url_query = [
  { param = "now", domain = "ver", fold = [["site"]] },
  { param = "was", domain = "ver", fold = [["site"]] },
  { param = "serial", domain = "serial" },
]

[[rule]]
path = ["serial"]
domain = "serial"

[[rule]]
path = ["entry"]
domain = "entry"

[[rule]]
path = ["stamp"]
unix_seconds = true

[[unobservable]]
path = ["note"]
reason = "names inside free text are destroyed with the text"

[[unobservable]]
from = "note"
to = "doc"
kind = "names"
reason = "a doc named inside a note"
"#;

/// The answer key for the synthetic stream.
fn answer() -> Value {
    let p = |path: &[&str]| {
        let mut full = vec!["data"];
        full.extend(path);
        json!(full)
    };
    json!({
        "version": 2,
        "decode": [["data"]],
        "types": [
            {"type": "site", "mentions": [
                {"path": p(&["site"]), "identity": [p(&["site"])]},
                {"path": p(&["host"]), "identity": [p(&["site"])]},
                {"path": p(&["base_url"]), "identity": [p(&["site"])]}
            ]},
            {"type": "doc", "mentions": [
                {"path": p(&["doc"]), "identity": [p(&["site"]), p(&["doc"])]},
                {"path": p(&["doc_link"]), "identity": [p(&["site"]), p(&["doc"])]}
            ]},
            {"type": "who", "mentions": [
                {"path": p(&["who"]), "identity": [p(&["site"]), p(&["who"])]}
            ]},
            {"type": "ver", "mentions": [
                {"path": p(&["ver", "new"]), "identity": [p(&["site"]), p(&["ver", "new"])]},
                {"path": p(&["ver", "old"]), "identity": [p(&["site"]), p(&["ver", "old"])]}
            ]},
            {"type": "entry", "mentions": [
                {"path": p(&["entry"]), "identity": [p(&["entry"])], "no_identity": [0]}
            ]}
        ],
        "unscored": [p(&["serial"]), p(&["link"]), {"prefix": p(&["meta"])}]
    })
}

/// One event on site `site` (`a` or `b`), doc `doc` (plain text with spaces).
fn event(site: &str, doc: &str, who: &str, new: u64, old: Option<u64>, entry: u64) -> Value {
    let base = format!("https://{site}.example");
    let link_doc = doc.replace(' ', "_").replace('ñ', "%C3%B1");
    let mut ver = json!({"new": new});
    let mut link = format!("{base}/x?now={new}&serial={}", new + 9000);
    if let Some(old) = old {
        ver["old"] = json!(old);
        link = format!("{base}/x?now={new}&was={old}&serial={}", new + 9000);
    }
    json!({
        "site": format!("{site}site"),
        "host": format!("{site}.example"),
        "base_url": base,
        "doc": doc,
        "doc_link": format!("{base}/d/{link_doc}"),
        "who": who,
        "ver": ver,
        "link": link,
        "serial": new + 9000,
        "entry": entry,
        "stamp": 1_759_190_400 + new,
        "meta": {"at": format!("2026-09-30T00:{:02}:00Z", new % 60), "seq": new},
        "note": format!("Moved {doc} quickly"),
        "kind": "change",
        "size": {"old": 10, "new": 12},
        "flag": new % 2 == 0,
        "tags": ["warm", "musky"],
        "gone": null
    })
}

/// Twelve events on two sites. `Big Thing` is a doc on both; `Ulla` acts on both; a later
/// event's `ver.old` is an earlier one's `ver.new`; one `doc_link` has an unmatched shape.
fn events() -> Vec<Value> {
    let mut all = vec![
        event("a", "Big Thing", "Ulla", 101, None, 0),
        event("a", "Big Thing", "Ulla", 102, Some(101), 0),
        event("a", "Caña Rio", "Ymir", 103, None, 0),
        event("a", "Caña Rio", "Ulla", 104, Some(103), 7),
        event("b", "Big Thing", "Ulla", 105, None, 0),
        event("b", "Big Thing", "Wren", 106, Some(105), 0),
        event("b", "Quiet Pond", "Wren", 107, None, 7),
        event("b", "Quiet Pond", "Ymir", 108, Some(107), 9101),
        event("a", "Moss Hill", "Ymir", 109, None, 0),
        event("a", "Moss Hill", "Wren", 110, Some(109), 0),
        event("b", "Moss Hill", "Ulla", 111, None, 0),
        event("b", "Moss Hill", "Ulla", 112, Some(111), 0),
    ];
    all[8]["doc_link"] = json!("https://a.example/w/run?x=1");
    all
}

fn sse(events: &[Value]) -> String {
    events
        .iter()
        .enumerate()
        .map(|(i, event)| format!("id: [{{\"topic\":\"tq\",\"offset\":{i}}}]\ndata: {event}\n\n"))
        .collect()
}

/// A temporary root with the rules, the pinned answer key, and the given corpora pinned; the key
/// files live outside it.
struct Fx {
    root: PathBuf,
    dir: PathBuf,
    rules: PathBuf,
    keys: PathBuf,
}

impl Fx {
    fn new(name: &str, corpora: &[(&str, String)]) -> Self {
        let base = std::env::temp_dir().join(format!("s2w-obf-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (root, keys) = (base.join("root"), base.join("keys"));
        let (data, dir) = (root.join(DATA), root.join("corpora"));
        for d in [&data, &dir, &keys] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(keys.join("a.key"), format!("{KEY_A}\n")).unwrap();
        fs::write(keys.join("b.key"), KEY_B).unwrap();
        let answer = serde_json::to_vec_pretty(&answer()).unwrap();
        fs::write(data.join(ANSWER), &answer).unwrap();
        let pin = format!("[[key]]\nfile = \"{ANSWER}\"\nvariant = \"base\"\nsha256 = \"{}\"\n", sha256(&answer));
        fs::write(data.join("keys.toml"), pin).unwrap();
        let mut manifest = String::new();
        for (corpus, text) in corpora {
            fs::write(dir.join(format!("{corpus}.raw.sse")), text).unwrap();
            let events = text.matches("\ndata: ").count();
            manifest += &format!(
                "[corpus.{corpus}]\nrole = \"development\"\nfile = \"{corpus}.raw.sse\"\nevents = {events}\nsha256 = \"{}\"\n\n",
                sha256(text.as_bytes())
            );
        }
        fs::write(data.join("corpora.toml"), manifest).unwrap();
        let rules = root.join("toy.rules.toml");
        fs::write(&rules, RULES).unwrap();
        Self { root, dir, rules, keys }
    }

    fn meta_path(&self) -> PathBuf {
        self.root.join(DATA).join("obfuscation/r1.meta.json")
    }

    fn run(&self, key: &str, corpora: &[&str], meta: &Path) -> Result<String, String> {
        let corpora: Vec<String> = corpora.iter().map(|c| (*c).to_owned()).collect();
        let keys = vec![ANSWER.to_owned()];
        obfuscate(
            &self.root,
            &Request {
                rules: &self.rules,
                key_file: &self.keys.join(key),
                replicate: "r1",
                corpora: &corpora,
                keys: &keys,
                meta,
                dir: &self.dir,
            },
        )
    }

    fn output(&self, corpus: &str) -> String {
        fs::read_to_string(self.dir.join(format!("{corpus}.obf-r1.raw.sse"))).unwrap()
    }

    fn renamed_key(&self) -> String {
        fs::read_to_string(self.root.join(DATA).join("toy-key.obf-r1.json")).unwrap()
    }

    fn meta(&self) -> Meta {
        serde_json::from_str(&fs::read_to_string(self.meta_path()).unwrap()).unwrap()
    }
}

/// A one-corpus fixture run with key A.
fn ran(name: &str) -> Fx {
    let fx = Fx::new(name, &[("dev", sse(&events()))]);
    fx.run("a.key", &["dev"], &fx.meta_path()).unwrap();
    fx
}

/// The obfuscated events, as JSON.
fn obfuscated(fx: &Fx, corpus: &str) -> Vec<Value> {
    envelopes(&fx.output(corpus))
        .unwrap()
        .iter()
        .map(|envelope| serde_json::from_str(envelope["data"].as_str().unwrap()).unwrap())
        .collect()
}

/// The value at plain `path` of an obfuscated event, through the metadata's field table.
fn at<'v>(meta: &Meta, event: &'v Value, path: &[&str]) -> &'v Value {
    let mut node = event;
    for depth in 1..=path.len() {
        let chain: Vec<String> = path[..depth].iter().map(|s| (*s).to_owned()).collect();
        let row = meta.fields.iter().find(|row| row.path == chain).unwrap();
        node = &node[&row.name];
    }
    node
}

#[test]
fn the_same_key_and_inputs_give_identical_bytes_and_meta_reuse_does_too() {
    let (one, two) = (ran("det-1"), ran("det-2"));
    assert_eq!(one.output("dev"), two.output("dev"));
    assert_eq!(one.renamed_key(), two.renamed_key());
    assert_eq!(
        fs::read(one.meta_path()).unwrap(),
        fs::read(two.meta_path()).unwrap()
    );
    let three = Fx::new("det-3", &[("dev", sse(&events()))]);
    three.run("a.key", &["dev"], &one.meta_path()).unwrap();
    assert_eq!(one.output("dev"), three.output("dev"));
    assert!(!three.meta_path().exists(), "a reused metadata file is not rewritten");
}

#[test]
fn domains_separate_values_and_one_domain_joins_them() {
    let fx = ran("domains");
    let (meta, out) = (fx.meta(), obfuscated(&fx, "dev"));
    // `who` is unfolded: one value, one hash in every record.
    assert_eq!(at(&meta, &out[0], &["who"]), at(&meta, &out[4], &["who"]));
    // One value in one domain at two paths: event 2's `ver.old` is event 1's `ver.new`.
    assert_eq!(at(&meta, &out[0], &["ver", "new"]), at(&meta, &out[1], &["ver", "old"]));
    // The same digits in two domains: event 1's `serial` and event 8's `entry` are both 9101.
    assert_eq!(events()[0]["serial"], events()[7]["entry"]);
    let serial = at(&meta, &out[0], &["serial"]);
    assert!(serial.is_string());
    assert_ne!(serial, at(&meta, &out[7], &["entry"]));
}

#[test]
fn folds_split_contexts_and_aliases_hash_byte_equal() {
    let fx = ran("fold");
    let (meta, out) = (fx.meta(), obfuscated(&fx, "dev"));
    // `Big Thing` on two sites: two docs.
    assert_ne!(at(&meta, &out[0], &["doc"]), at(&meta, &out[4], &["doc"]));
    assert_eq!(at(&meta, &out[0], &["doc"]), at(&meta, &out[1], &["doc"]));
    for event in &out {
        let site = at(&meta, event, &["site"]);
        assert_eq!(at(&meta, event, &["host"]), site);
        assert_eq!(at(&meta, event, &["base_url"]), site);
    }
}

/// Every string leaf and object key of a JSON value.
fn strings(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            out.insert(text.clone());
        }
        Value::Object(map) => {
            for (key, inner) in map {
                out.insert(key.clone());
                strings(inner, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|inner| strings(inner, out)),
        _ => {}
    }
}

#[test]
fn no_plaintext_string_or_key_survives() {
    let fx = ran("leak");
    let text = fx.output("dev");
    let mut plain = BTreeSet::new();
    for event in events() {
        strings(&event, &mut plain);
    }
    for leaked in plain.iter().filter(|s| text.contains(s.as_str())) {
        panic!("{leaked:?} occurs in the obfuscated corpus");
    }
    assert!(!text.contains("topic") && !text.contains("offset"), "id: lines are hashed");
    let names: BTreeSet<String> = fx.meta().fields.iter().map(|row| row.name.clone()).collect();
    for event in obfuscated(&fx, "dev") {
        let event_keys: Vec<_> = event.as_object().unwrap().keys().collect();
        assert!(event_keys.iter().all(|key| names.contains(*key)));
    }
}

#[test]
fn the_field_order_is_a_permutation_drawn_from_the_key() {
    let one = ran("order-a");
    let two = Fx::new("order-b", &[("dev", sse(&events()))]);
    two.run("b.key", &["dev"], &two.meta_path()).unwrap();
    let table = |fx: &Fx| -> Vec<(Vec<String>, String)> {
        fx.meta().fields.into_iter().map(|row| (row.path, row.name)).collect()
    };
    let (a, b) = (table(&one), table(&two));
    assert_ne!(a, b, "two keys gave one field order");
    for rows in [&a, &b] {
        let names: BTreeSet<&String> = rows.iter().map(|(_, name)| name).collect();
        let expected: BTreeSet<String> = (1..=rows.len()).map(|k| format!("f{k}")).collect();
        assert_eq!(names, expected.iter().collect());
    }
    assert!(a.len() >= 10, "the fixture has {} fields", a.len());
}

#[test]
fn timestamps_move_by_one_constant_and_a_leap_second_fails() {
    let fx = ran("shift");
    let (meta, out) = (fx.meta(), obfuscated(&fx, "dev"));
    let shift = meta.shift_seconds;
    assert!(shift.abs() >= 86_400);
    for (plain, obf) in events().iter().zip(&out) {
        let at_plain = plain["meta"]["at"].as_str().unwrap();
        let expected = s2w_discover::stamp::shift(at_plain, shift).unwrap();
        assert_eq!(at(&meta, obf, &["meta", "at"]), &json!(expected));
        let moved = at(&meta, obf, &["stamp"]).as_i64().unwrap();
        assert_eq!(moved - plain["stamp"].as_i64().unwrap(), shift);
    }
    let mut leap = events();
    leap[3]["meta"]["at"] = json!("2016-12-31T23:59:60Z");
    let fx = Fx::new("leap", &[("dev", sse(&leap))]);
    let err = fx.run("a.key", &["dev"], &fx.meta_path()).unwrap_err();
    assert!(err.contains("cannot be shifted"), "{err}");
    assert!(!fx.meta_path().exists() && !fx.dir.join("dev.obf-r1.raw.sse").exists());
}

#[test]
fn identifiers_in_urls_hash_as_their_fields_do() {
    let fx = ran("canon");
    let (meta, out) = (fx.meta(), obfuscated(&fx, "dev"));
    // `Caña_Rio`, percent-encoded with `_` for space, is the doc `Caña Rio` on its site.
    assert_eq!(at(&meta, &out[2], &["doc_link"]), at(&meta, &out[2], &["doc"]));
    assert_eq!(at(&meta, &out[0], &["doc_link"]), at(&meta, &out[0], &["doc"]));
    // Event 9's link has another shape: hashed whole, counted.
    assert_ne!(at(&meta, &out[8], &["doc_link"]), at(&meta, &out[8], &["doc"]));
    assert_eq!(meta.fallbacks.len(), 1);
    assert_eq!((meta.fallbacks[0].path.clone(), meta.fallbacks[0].count), (vec!["doc_link".to_owned()], 1));
    // The query parameters, each in its own domain, joined by `/`.
    let s = |v: &Value| v.as_str().unwrap().to_owned();
    let link = s(at(&meta, &out[1], &["link"]));
    let expected = [
        s(at(&meta, &out[1], &["ver", "new"])),
        s(at(&meta, &out[1], &["ver", "old"])),
        s(at(&meta, &out[1], &["serial"])),
    ]
    .join("/");
    assert_eq!(link, expected);
}

#[test]
fn a_reused_table_refuses_a_new_field() {
    let first = ran("reuse-1");
    let mut extra = events();
    extra[0]["novel"] = json!("Zest");
    let second = Fx::new("reuse-2", &[("dev", sse(&extra))]);
    let err = second.run("a.key", &["dev"], &first.meta_path()).unwrap_err();
    assert!(err.contains("not in the replicate's field table"), "{err}");
    assert!(!second.dir.join("dev.obf-r1.raw.sse").exists());
    let err = second.run("b.key", &["dev"], &first.meta_path()).unwrap_err();
    assert!(err.contains("different key"), "{err}");
}

#[test]
fn a_missing_fold_context_fails_and_nothing_is_written() {
    let mut broken = events();
    broken[5].as_object_mut().unwrap().remove("site");
    let fx = Fx::new("nofold", &[("dev", sse(&broken))]);
    let err = fx.run("a.key", &["dev"], &fx.meta_path()).unwrap_err();
    assert!(err.contains("holds no string or number"), "{err}");
    assert!(!fx.meta_path().exists() && !fx.dir.join("dev.obf-r1.raw.sse").exists());
}

#[test]
fn an_existing_output_is_never_overwritten() {
    let fx = ran("exists");
    let before = fx.output("dev");
    fs::remove_file(fx.meta_path()).unwrap();
    let err = fx.run("a.key", &["dev"], &fx.meta_path()).unwrap_err();
    assert!(err.contains("never overwritten"), "{err}");
    assert_eq!(fx.output("dev"), before);
}

#[test]
fn a_key_file_inside_the_repository_is_refused() {
    let fx = Fx::new("inrepo", &[("dev", sse(&events()))]);
    fs::write(fx.root.join("r.key"), KEY_A).unwrap();
    let err = obfuscate(
        &fx.root,
        &Request {
            rules: &fx.rules,
            key_file: &fx.root.join("r.key"),
            replicate: "r1",
            corpora: &["dev".to_owned()],
            keys: &[],
            meta: &fx.meta_path(),
            dir: &fx.dir,
        },
    )
    .unwrap_err();
    assert!(err.contains("inside the repository"), "{err}");
}

#[test]
fn a_truncated_hash_collision_fails() {
    let mut keyed = super::hash::Keyed::narrow([3; 32], 1);
    let found = (0..300).map(|i| keyed.value("text", &[&format!("v{i}")])).find(Result::is_err);
    let err = found.expect("300 values in 256 one-byte hashes must collide").unwrap_err();
    assert!(err.contains("collision"), "{err}");
    // The same input twice is not a collision.
    assert_eq!(keyed.value("text", &["v0"]), keyed.value("text", &["v0"]));
}

#[test]
fn rules_files_are_validated() {
    use super::rules::Rules;
    let bad = [
        ("version = 2\n", "version"),
        ("version = 1\n[[rule]]\npath = [\"a\"]\n", "set exactly one"),
        ("version = 1\n[[rule]]\npath = [\"a\"]\ndomain = \"d\"\nunix_seconds = true\n", "set exactly one"),
        ("version = 1\n[[rule]]\npath = [\"a\"]\nunix_seconds = true\nfold = [[\"b\"]]\n", "need a domain"),
        ("version = 1\n[[rule]]\npath = [\"a\"]\ndomain = \"d\"\n[[rule]]\npath = [\"a\"]\ndomain = \"e\"\n", "two rules"),
        ("version = 1\n[[unobservable]]\nfrom = \"a\"\nreason = \"r\"\n", "all of from, to and kind"),
        ("version = 1\n[[rule]]\npath = [\"a\"]\ndomian = \"d\"\n", "unknown field"),
    ];
    for (text, expected) in bad {
        let err = Rules::parse(text, String::new()).unwrap_err();
        assert!(err.contains(expected), "{text:?}: {err}");
    }
    assert!(Rules::parse(RULES, String::new()).is_ok());
}

/// `text` with every plain mention path id replaced by its renamed id, longest first.
fn renamed_ids(text: &str, meta: &Meta) -> String {
    let mut pairs: Vec<(String, String)> = meta
        .fields
        .iter()
        .map(|row| {
            let renamed: Vec<String> = (1..=row.path.len())
                .map(|depth| {
                    let chain = &row.path[..depth];
                    meta.fields.iter().find(|r| r.path == chain).unwrap().name.clone()
                })
                .collect();
            (format!("data.{}", row.path.join(".")), format!("data.{}", renamed.join(".")))
        })
        .collect();
    pairs.sort_by_key(|(plain, _)| std::cmp::Reverse(plain.len()));
    pairs
        .iter()
        .fold(text.to_owned(), |text, (plain, renamed)| text.replace(plain, renamed))
}

#[test]
fn an_obfuscated_corpus_scores_as_the_plain_one_up_to_renaming() {
    let fx = ran("score");
    let plain_key: KeySpec = serde_json::from_value(answer()).unwrap();
    let renamed_key: KeySpec = serde_json::from_str(&fx.renamed_key()).unwrap();
    renamed_key.validate().unwrap();
    let note = json!(["data", at_name(&fx.meta(), "note")]);
    let unscored = serde_json::to_value(&renamed_key.unscored).unwrap();
    assert!(unscored.as_array().unwrap().contains(&note), "the unobservable path is unscored");
    let plain_payloads = envelopes(&sse(&events())).unwrap();
    let obf_payloads = envelopes(&fx.output("dev")).unwrap();
    let plain = grade(&plain_key, &plain_key.oracle().unwrap(), &plain_payloads).unwrap();
    let obf = grade(&renamed_key, &renamed_key.oracle().unwrap(), &obf_payloads).unwrap();
    assert!(plain.mapping.micro.f1.is_some_and(|f1| f1 > 0.0), "the plain grade is vacuous");
    assert!(!plain.excluded.is_empty(), "the fixture exercises a no_identity sentinel");
    let plain = renamed_ids(&serde_json::to_string(&plain).unwrap(), &fx.meta());
    assert_eq!(plain, serde_json::to_string(&obf).unwrap());
}

fn at_name(meta: &Meta, top: &str) -> String {
    meta.fields
        .iter()
        .find(|row| row.path == [top])
        .unwrap()
        .name
        .clone()
}
