//! Loader for the recorded raw stream the scale measurements replay (s2w#174).
//!
//! The synthetic generator (`scale_generator.rs`) chooses its distributions; this file reads
//! ones a real stream produced: ten minutes of server-sent events, recorded byte-for-byte and
//! committed as `tests/fixtures/recorded-10min.raw.sse` (provenance and licence in
//! `tests/fixtures/README.md`). The two are two implementations of one seam, the event supply
//! for a scale measurement, and neither replaces the other.
//!
//! Shared through `#[path]` like the generator, so it is test code and adds no crate-graph
//! edge. SSE frames become stored envelopes of the same shape a live source stores
//! (`{"data": <payload>, "id": <event id>}`, as in `s2w-system1/testdata/raw-sample.jsonl`;
//! the cursor here is the raw `id:` text), the committed stream mapping turns each envelope
//! into claims, and the fold turns claims into a world. Nothing reads a clock: replay order is file order and `received_at` is the frame's
//! index, so every run over the same bytes produces the same claims.
//!
//! The fixture is pinned: [`load`] refuses bytes whose hash is not [`FIXTURE_HASH`], so no
//! measurement runs on a changed recording, and `tests/recorded_fixture.rs` pins the counts. It
//! is human-owned like a golden file, never re-recorded or edited to make a measurement pass.

use std::error::Error;
use std::fs;
use std::sync::OnceLock;

use s2w_model::{Cursor, Fnv64, RawEvent, SourceId, StreamMapping, Timestamp, WorldEvent};
use s2w_sources::replay_frames;
use s2w_system1::{Engine, MappingEngine, Verdict};

/// A fallible result with a boxed error, for test code.
pub(crate) type Fallible<T> = Result<T, Box<dyn Error>>;

/// The recorded stream, raw SSE text.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/recorded-10min.raw.sse"
);

/// The stream mapping, a symlink to the one check 11 replays, so the two cannot drift.
const MAPPING: &str = concat!(
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

/// FNV-1a 64 of `bytes`, the fixture's pin.
pub(crate) fn hash(bytes: &[u8]) -> u64 {
    Fnv64::new().write(bytes).finish()
}

/// The committed stream mapping.
pub(crate) fn mapping() -> Fallible<StreamMapping> {
    Ok(serde_json::from_str(&fs::read_to_string(MAPPING)?)?)
}

/// Every frame of the fixture as a stored envelope, in file order, read, hashed and parsed once
/// per test binary. Fails if the fixture's bytes are not the pinned ones.
pub(crate) fn load() -> Fallible<&'static [RawEvent]> {
    static LOADED: OnceLock<Result<Vec<RawEvent>, String>> = OnceLock::new();
    match LOADED.get_or_init(|| read_pinned().map_err(|e| e.to_string())) {
        Ok(events) => Ok(events),
        Err(e) => Err(e.clone().into()),
    }
}

fn read_pinned() -> Fallible<Vec<RawEvent>> {
    let bytes = bytes()?;
    let actual = hash(&bytes);
    if actual != FIXTURE_HASH {
        return Err(format!(
            "the recorded fixture's FNV-1a 64 is {actual:#x}, not the pinned {FIXTURE_HASH:#x}; \
             restore it (see tests/fixtures/README.md)"
        )
        .into());
    }
    let source = SourceId::new(SOURCE)?;
    // The live SSE adapter's own framing (s2w-sources), shared with xtask's check 12.
    replay_frames(&bytes)?
        .into_iter()
        .zip(0_i64..)
        .map(|((id, data), index)| {
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
    /// Events the engine abstained on.
    pub(crate) abstained: usize,
}

/// Runs the mapping engine over `events`, single-threaded, in order.
pub(crate) fn claims(events: &[RawEvent], mapping: StreamMapping) -> Fallible<Claims> {
    let engine = MappingEngine::new(mapping)?;
    let mut out = Claims {
        claims: Vec::new(),
        abstained: 0,
    };
    for event in events {
        match engine.evaluate(event) {
            Verdict::Propose { claims, .. } => out.claims.extend(claims),
            Verdict::Abstain { .. } => out.abstained += 1,
        }
    }
    Ok(out)
}
