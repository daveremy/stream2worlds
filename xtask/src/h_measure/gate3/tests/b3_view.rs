//! What B3 shows the model is what the executor reads (decision 0032, dated note 2026-10-01):
//! a stub model that writes the key's oracle mapping against the record its b3 prompt shows
//! scores above 0, and the same stub given the old inner-`data` view scores 0. This is the
//! test that would have caught leg D's dry-run finding 1 before any money was spent.

use s2w_system2::{MappingProposer, MappingResult, Provider, ProviderError, Reply};
use serde_json::Value;

use super::super::super::freeze_tests::{KEY, fixture};
use super::super::super::grade::grade;
use super::super::super::pins::Pins;
use super::super::b3::{self, Target};
use super::price;

/// A model that knows the key's oracle mapping and writes every path of it against the record
/// the prompt's first event shows: a decode step only where the record holds a JSON string
/// there, and each other path as its longest suffix that the (decoded) record holds. It never
/// sees the stored envelope except through the prompt.
struct Oracle(Value);

impl Provider for Oracle {
    fn complete(&self, prompt: &str) -> Result<Reply, ProviderError> {
        let data = prompt
            .lines()
            .skip_while(|line| *line != "BEGIN DATA")
            .nth(1)
            .expect("the prompt has a data line");
        let input: Value = serde_json::from_str(data).expect("the data line is JSON");
        let first = input["events"][0].as_str().expect("an event is a string");
        let shown: Value = serde_json::from_str(first).expect("the fixture's events are JSON");
        Ok(Reply {
            text: written_against(&self.0, &shown).to_string(),
            input_tokens: Some(10),
            output_tokens: Some(10),
            ..Reply::default()
        })
    }
}

fn at<'a>(record: &'a Value, path: &[Value]) -> Option<&'a Value> {
    path.iter()
        .try_fold(record, |value, step| value.get(step.as_str()?))
}

fn is_path(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|steps| !steps.is_empty() && steps.iter().all(Value::is_string))
}

/// `oracle` with its paths rewritten against `shown`.
fn written_against(oracle: &Value, shown: &Value) -> Value {
    let mut record = shown.clone();
    let mut decode = Vec::new();
    for step in oracle["decode"].as_array().into_iter().flatten() {
        let path = step.as_array().expect("a decode step is a path");
        let Some(text) = at(&record, path).and_then(Value::as_str) else {
            continue;
        };
        let decoded: Value = serde_json::from_str(text).expect("the decoded field is JSON");
        let mut slot = &mut record;
        for key in path {
            slot = &mut slot[key.as_str().expect("a path step is a string")];
        }
        *slot = decoded;
        decode.push(step.clone());
    }
    let mut mapping = oracle.clone();
    rewrite(&mut mapping, &record);
    mapping["decode"] = Value::Array(decode);
    mapping
}

fn rewrite(value: &mut Value, record: &Value) {
    if is_path(value) {
        let path = value.as_array().expect("a path").clone();
        if let Some(from) = (0..path.len()).find(|&from| at(record, &path[from..]).is_some()) {
            *value = Value::Array(path[from..].to_vec());
        }
        return;
    }
    match value {
        Value::Array(items) => items.iter_mut().for_each(|item| rewrite(item, record)),
        Value::Object(fields) => fields.values_mut().for_each(|item| rewrite(item, record)),
        _ => {}
    }
}

#[test]
fn b3_shows_the_record_the_executor_reads() {
    let (root, dir) = fixture("gate3-b3-view");
    let pins = Pins::load(&root).unwrap();
    let window = pins.payloads(&dir, "dev").unwrap();
    let (_, spec) = pins.key(&root, KEY).unwrap();
    let oracle = serde_json::to_value(spec.oracle().unwrap()).unwrap();
    assert!(
        !oracle["decode"].as_array().unwrap().is_empty(),
        "the key decodes a field, or the old view would not differ"
    );
    let proposer = MappingProposer::new(Oracle(oracle));
    let target = Target {
        corpus: "dev",
        window: window.len() as u64,
        replicate: 1,
        prompt_bytes: usize::MAX,
        input_tokens: 1000,
    };
    let recall = |events: &[String]| {
        let (proposed, fitted) = b3::propose(&proposer, &price(), events, &target, 0.0);
        assert_eq!(fitted.fits.len(), 1);
        let MappingResult::Mapping(mapping) = proposed.outcome.result else {
            panic!(
                "the stub always answers a mapping: {:?}",
                proposed.outcome.result
            );
        };
        let graded = grade(&spec, &mapping, &window).unwrap();
        (graded.mapping.micro.recall, graded.ceiling.micro.recall)
    };
    let (shown, ceiling) = recall(&b3::raw_events(&window).unwrap());
    assert!(shown.unwrap() > 0.0, "{shown:?}");
    // The oracle written against the stored record is the oracle itself: the ceiling.
    assert_eq!(shown, ceiling);
    // The view leg D's dry run sent: each envelope's inner `data` string.
    let inner: Vec<String> = window
        .iter()
        .map(|envelope| envelope["data"].as_str().unwrap().to_owned())
        .collect();
    let (old, _) = recall(&inner);
    assert_eq!(old, Some(0.0), "the inner view's paths miss every record");
}
