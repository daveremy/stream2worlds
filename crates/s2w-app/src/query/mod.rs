//! The world query API (decision 0006): the one contract the web view, `--json` and the MCP
//! server read the world through.
//!
//! The pure half ([`world_view`], [`diff`], [`fold_with_delta`], [`Timeline`]) does no I/O and
//! can be called directly; [`router`] serves it over HTTP with SSE deltas. Time is the fold
//! offset everywhere, and only the actual world branch exists: any other branch is
//! [`QueryError::BranchNotYet`].

mod dashboard;
mod delta;
mod diff;
mod epoch;
mod generation;
mod http;
mod projection;
mod proposal_store;
mod proposals;
mod read_timings;
mod resolve;
mod sentences;
mod sources;
mod stream;
mod stream_mapping;
mod summary_memo;
mod timeline;
mod view;

use thiserror::Error;

pub use dashboard::{
    DASHBOARD_ENVELOPE_FORMAT, DASHBOARD_MANIFEST_CLASS, DashboardEnvelope, DashboardView,
    ExcludedDto, MAX_ATTEMPTS, MAX_RAW_BYTES, Provenance, dashboard_view,
    decode_envelope as decode_dashboard_envelope, parse_envelope as parse_dashboard_envelope,
    read_dashboard,
};
pub use delta::{Delta, fold_with_delta};
pub use diff::{Changed, Changes, MergeEdge, WorldDiff, diff};
pub use epoch::Epoch;
pub use http::{
    Branch, QueryState, RawEventInfo, Rebuilding, SourceInfo, TimeAt, TimeResult, WorldSummary,
    router,
};
pub use projection::{HeadView, Projection};
pub use proposal_store::read_view;
pub use proposals::{
    ActorDto, DecisionDto, GradeDto, ProposalDto, ProposalsView, TallyDto, proposals_view,
};
pub use read_timings::{PhaseTiming, ReadTimingsSnapshot};
pub use resolve::{ClassResolution, Excluded, Winner, resolve_class};
pub use sentences::{
    MAX_SENTENCES, SentenceEntity, SentenceRow, SentencesView, check_last as check_sentences_last,
    read_last, read_sentences,
};
pub use sources::{RECENT_UNROUTED_CAP, SourceStats};
pub use stream_mapping::{
    ENVELOPE_FORMAT, MappingEnvelope, STREAM_MAPPING_CLASS, decode_envelope, proposal_id,
};
pub use timeline::{BaseTime, DEFAULT_HISTORY_CAP, HistoryEntry, TimeRange, TimedEvent, Timeline};
pub use view::{
    ACTUAL_BRANCH, HubRef, Link, LinkDetail, Lod, MAX_HOPS, Node, ViewParams, WorldView,
    type_summary, world_view,
};

// The parameter validators the HTTP handlers and the MCP tools share, so the two surfaces can
// never disagree about what a valid `world`, `branch` or `lod` is.
pub(crate) use http::{check_branch, check_world, parse, parse_links, parse_lod};
pub(crate) use proposal_store::open_proposal_reader;
pub(crate) use view::check_links;

/// Why a query could not be answered. Each variant has a stable `code` for JSON errors.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum QueryError {
    /// The requested offset is past the timeline's head.
    #[error("offset {at} is past the head ({head}); try at={head} or omit at")]
    OffsetBeyondHead {
        /// The requested offset.
        at: u64,
        /// The latest offset.
        head: u64,
    },
    /// The requested offset is below what this route serves: the process was restored from a
    /// snapshot (decision 0024) or has dropped events past its history cap (decision 0026).
    #[error(
        "offset {at} is before the earliest offset this route serves ({base}); history below it \
         is gone from this process, try an offset of at least {base} (see /time's base and \
         replay_base)"
    )]
    OffsetBeforeBase {
        /// The requested offset.
        at: u64,
        /// The earliest servable offset.
        base: u64,
    },
    /// The requested time maps to an offset world queries cannot serve: inside a snapshot, whose
    /// per-event times are not kept (decision 0024), or, once the window has dropped events,
    /// before the newest event (decision 0026). Shares the `offset_before_base` code.
    #[error(
        "ts {ts} is before the earliest time this process serves (offset {base}); try a ts of \
         at least time.last_ts"
    )]
    TimeBeforeBase {
        /// The requested timestamp in milliseconds.
        ts: i64,
        /// The earliest servable offset.
        base: u64,
    },
    /// The client's offset belongs to another history than the one served now: the serving
    /// registry's feed fingerprint changed since the client read it (s2w#184, amending 0006).
    /// The same offset may name a different world, so it is refused rather than answered.
    #[error(
        "epoch {supplied} is not the served history ({current}); the world was rebuilt under \
         other routes, re-read /world or /time and continue from their epoch and offset"
    )]
    StaleEpoch {
        /// The epoch the client sent.
        supplied: Epoch,
        /// The epoch served now.
        current: Epoch,
    },
    /// Only the actual world exists; branches are a later gate.
    #[error(
        "branch '{branch}' does not exist yet: only 'actual' is served until world branches land"
    )]
    BranchNotYet {
        /// The requested branch.
        branch: String,
    },
    /// `lod=cluster` is reserved; clustering is not defined yet.
    #[error("lod '{lod}' is not served yet; try lod=entity or lod=type")]
    LodNotYet {
        /// The requested level of detail.
        lod: String,
    },
    /// A parameter did not parse.
    #[error("bad parameter '{name}': {reason}")]
    BadParameter {
        /// The parameter name.
        name: &'static str,
        /// What was wrong.
        reason: String,
    },
    /// No entity has this id at the requested offset.
    #[error("no entity with id {id} at this offset")]
    UnknownEntity {
        /// The requested id.
        id: u64,
    },
    /// This process does not serve the requested world.
    #[error("world '{world}' is not served by this process")]
    UnknownWorld {
        /// The requested world identifier.
        world: String,
    },
    /// `hops` is past [`MAX_HOPS`].
    #[error("hops {hops} is more than the maximum of {max}", max = MAX_HOPS)]
    HopsTooLarge {
        /// The requested hops.
        hops: u32,
    },
    /// The shared timeline's lock was poisoned by a panicking writer.
    #[error("the timeline is unavailable (a writer panicked); restart the server")]
    Unavailable,
    /// The concurrent `/events` stream cap is full — transient, unlike every other 503-shaped
    /// case above; the client should back off and retry rather than treat this like a poisoned
    /// lock (round-1 review finding: reusing `Unavailable` here showed a misleading "a writer
    /// panicked" message for an ordinary "too many tabs open" condition).
    #[error("too many concurrent event streams; retry shortly")]
    StreamLimit,
    /// Too many `/world` requests are already queued for a single-flight generation (s2w#270);
    /// transient, the client should retry.
    #[error("too many /world requests are waiting; retry shortly")]
    WorldQueueFull,
    /// The log directory could not be opened or read while serving a fresh-per-request value
    /// (presentation, proposals). Distinct from [`Self::Unavailable`], which means the in-memory timeline
    /// lock was poisoned by a panicking writer — this is a storage-layer failure instead.
    #[error("storage error: {0}")]
    Storage(String),
    /// No stored proposal has this id (including when no proposal store exists yet).
    #[error("no proposal with id '{id}'")]
    UnknownProposal {
        /// The requested proposal id.
        id: String,
    },
    /// Another process holds the proposal store's writer lock; retry later.
    #[error("the proposal store is locked by another writer; retry shortly")]
    StoreLocked,
}

impl From<s2w_log::LogError> for QueryError {
    fn from(error: s2w_log::LogError) -> Self {
        Self::Storage(error.to_string())
    }
}

impl QueryError {
    /// A stable machine-readable code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::OffsetBeyondHead { .. } => "offset_beyond_head",
            Self::OffsetBeforeBase { .. } | Self::TimeBeforeBase { .. } => "offset_before_base",
            Self::StaleEpoch { .. } => "stale_epoch",
            Self::BranchNotYet { .. } => "branch_not_yet",
            Self::LodNotYet { .. } => "lod_not_yet",
            Self::BadParameter { .. } => "bad_parameter",
            Self::UnknownEntity { .. } => "unknown_entity",
            Self::UnknownWorld { .. } => "unknown_world",
            Self::HopsTooLarge { .. } => "hops_too_large",
            Self::Unavailable => "unavailable",
            Self::StreamLimit => "stream_limit",
            Self::WorldQueueFull => "world_queue_full",
            Self::Storage(_) => "storage",
            Self::UnknownProposal { .. } => "unknown_proposal",
            Self::StoreLocked => "store_locked",
        }
    }

    /// The error body every surface serves: `{"error": <code>, "message": <text>}`. HTTP puts
    /// it in the response with a status code; MCP puts it in an error tool result's text. One
    /// constructor, so the two can never drift.
    #[must_use]
    pub fn json_body(&self) -> serde_json::Value {
        serde_json::json!({ "error": self.code(), "message": self.to_string() })
    }
}
