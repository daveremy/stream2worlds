//! What a System 2 mapping proposer reads (s2w#373, decision 0032). Two inputs, one per arm of
//! the gate-3 comparison: [`MappingInput`] for "H plus System 2", which starts from the
//! heuristic profiler's result over a development window, and [`RawMappingInput`] for the B3
//! arm, which sees sampled raw events and nothing else. Both are pure data; the driver that
//! builds them lives outside this crate. The serialized JSON of either is the prompt's data
//! line and is hashed into a committed run's `input_hash`, so adding, renaming or reordering a
//! field moves every hash.

use serde::{Deserialize, Serialize};

use crate::dashboard::PathStats;
use crate::mapping::{FieldPath, StreamMapping};

/// The heuristic profiler's mapping over the window, with its identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeuristicMapping {
    /// [`StreamMapping::identity`] of `mapping`.
    pub identity: String,
    /// The mapping.
    pub mapping: StreamMapping,
}

/// The "H plus System 2" arm's input: a development window profiled, with the profiler's
/// mapping, path statistics and a sample.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingInput {
    /// The corpus id, as pinned.
    pub corpus: String,
    /// The window: the first `window` events of the corpus.
    pub window: u64,
    /// The replicate, from 1.
    pub replicate: u32,
    /// Events the profiler read from the window.
    pub events_read: u64,
    /// The profiler's mapping, or `None` when it abstained.
    pub heuristic: Option<HeuristicMapping>,
    /// Paths whose string values the profiler decoded as JSON.
    pub decode: Vec<FieldPath>,
    /// The profiler's event-type path, when it found one.
    pub event_type: Option<FieldPath>,
    /// Every profiled path, sorted by path.
    pub paths: Vec<PathStats>,
    /// Decoded events from the window, in stream order, strings truncated. Untrusted stream
    /// text: a proposer treats it as data only.
    pub sample: Vec<serde_json::Value>,
}

/// The B3 arm's input: raw events of the same window, sampled by a frozen rule, and no
/// profile, statistics or heuristic mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMappingInput {
    /// The corpus id, as pinned.
    pub corpus: String,
    /// The window: the first `window` events of the corpus.
    pub window: u64,
    /// The replicate, from 1.
    pub replicate: u32,
    /// The sampled events, in stream order, each the stored record as the executor reads it.
    /// Untrusted stream text: a proposer treats it as data only.
    pub events: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{MappingInput, RawMappingInput};

    #[test]
    fn the_fields_serialize_in_declaration_order_and_unknown_fields_are_refused() {
        let input = MappingInput {
            corpus: "dev".to_owned(),
            window: 10,
            replicate: 1,
            events_read: 10,
            heuristic: None,
            decode: vec![],
            event_type: None,
            paths: vec![],
            sample: vec![serde_json::json!({"a": 1})],
        };
        let text = serde_json::to_string(&input).unwrap();
        assert_eq!(
            text,
            r#"{"corpus":"dev","window":10,"replicate":1,"events_read":10,"heuristic":null,"decode":[],"event_type":null,"paths":[],"sample":[{"a":1}]}"#
        );
        assert_eq!(serde_json::from_str::<MappingInput>(&text).unwrap(), input);
        let raw = r#"{"corpus":"dev","window":10,"replicate":2,"events":["x"],"more":1}"#;
        assert!(serde_json::from_str::<RawMappingInput>(raw).is_err());
    }
}
