//! Loader for the recorded raw stream the scale measurements replay (s2w#174).
//!
//! The synthetic generator (`scale_generator.rs`) chooses its distributions; this file reads
//! ones a real stream produced: ten minutes of server-sent events, recorded byte-for-byte and
//! committed as `tests/fixtures/recorded-10min.raw.sse` (provenance and licence in
//! `tests/fixtures/README.md`). The two are two implementations of one seam, the event supply
//! for a scale measurement, and neither replaces the other.
//!
//! Shared through `#[path]` like the generator, so it is test code and adds no crate-graph
//! edge. The pipeline is the one a live source runs: SSE frames become stored envelopes
//! (`{"data": <payload>, "id": <event id>}`, the shape of `s2w-system1/testdata/raw-sample.jsonl`),
//! the committed stream mapping turns each envelope into claims, and the fold turns claims into
//! a world. Nothing reads a clock: replay order is file order and `received_at` is the frame's
//! index, so every run over the same bytes produces the same claims.
//!
//! The fixture is pinned (`FIXTURE_HASH`, and the counts in `tests/recorded_fixture.rs`): it is
//! human-owned like a golden file, never re-recorded or edited to make a measurement pass.

use std::error::Error;
use std::fs;

use s2w_model::{Cursor, Fnv64, RawEvent, SourceId, StreamMapping, Timestamp, WorldEvent};
use s2w_system1::{Engine, MappingEngine, Verdict};

/// A fallible result with a boxed error, for test code.
pub(crate) type Fallible<T> = Result<T, Box<dyn Error>>;

/// The recorded stream, raw SSE text.
pub(crate) const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/recorded-10min.raw.sse"
);

/// The stream mapping, a symlink to the one check 11 replays, so the two cannot drift.
pub(crate) const MAPPING: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/recorded.mapping.json"
);

/// FNV-1a 64 of the fixture's bytes, as `Fnv64` computes it.
pub(crate) const FIXTURE_HASH: u64 = 0x070e_ba9f_7d43_edcd;

/// The source id every loaded event carries.
const SOURCE: &str = "recorded";

/// The fixture's bytes.
pub(crate) fn bytes() -> Fallible<Vec<u8>> {
    Ok(fs::read(FIXTURE)?)
}

/// FNV-1a 64 of `bytes`.
pub(crate) fn hash(bytes: &[u8]) -> u64 {
    Fnv64::new().write(bytes).finish()
}

/// The committed stream mapping.
pub(crate) fn mapping() -> Fallible<StreamMapping> {
    Ok(serde_json::from_str(&fs::read_to_string(MAPPING)?)?)
}

/// The SSE frames in `text` that carry both a `data` and an `id` field, as `(data, id)`, in
/// file order. Comment lines (`:`) and other fields are skipped; multi-line `data` joins with
/// `\n`, as the SSE format specifies. A frame missing either field is dropped.
pub(crate) fn frames(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (mut data, mut id): (Option<String>, Option<String>) = (None, None);
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let (Some(d), Some(i)) = (data.take(), id.take()) {
                out.push((d, i));
            }
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => data = Some(data.map_or_else(|| value.to_owned(), |d| d + "\n" + value)),
            "id" => id = Some(value.to_owned()),
            _ => {}
        }
    }
    out
}

/// Every frame of the fixture as a stored envelope, in file order.
pub(crate) fn load() -> Fallible<Vec<RawEvent>> {
    let text = String::from_utf8(bytes()?)?;
    let source = SourceId::new(SOURCE)?;
    frames(&text)
        .into_iter()
        .zip(0_i64..)
        .map(|((data, id), index)| {
            let payload = serde_json::to_vec(&serde_json::json!({ "data": data, "id": id }))?;
            Ok(RawEvent {
                source: source.clone(),
                cursor: Cursor::new(id.into_bytes())?,
                received_at: Timestamp::from_millis(index),
                payload,
            })
        })
        .collect()
}

/// What the mapping made of a batch of events.
pub(crate) struct Claims {
    /// Every proposed claim, in event order, and within an event in emission order.
    pub(crate) claims: Vec<WorldEvent>,
    /// Events the engine proposed for (with or without claims).
    pub(crate) proposed: usize,
    /// Events the engine abstained on.
    pub(crate) abstained: usize,
}

/// Runs the mapping engine over `events`, single-threaded, in order.
pub(crate) fn claims(events: &[RawEvent], mapping: StreamMapping) -> Fallible<Claims> {
    let engine = MappingEngine::new(mapping)?;
    let mut out = Claims {
        claims: Vec::new(),
        proposed: 0,
        abstained: 0,
    };
    for event in events {
        match engine.evaluate(event) {
            Verdict::Propose { claims, .. } => {
                out.proposed += 1;
                out.claims.extend(claims);
            }
            Verdict::Abstain { .. } => out.abstained += 1,
        }
    }
    Ok(out)
}
