//! The world query API (decision 0006): the one contract the web view, `--json` and the MCP
//! server read the world through.
//!
//! The pure half ([`world_view`], [`diff`], [`fold_with_delta`], [`Timeline`]) does no I/O and
//! can be called directly; [`router`] serves it over HTTP with SSE deltas. Time is the fold
//! offset everywhere, and only the actual world branch exists: any other branch is
//! [`QueryError::BranchNotYet`].

mod delta;
mod diff;
mod http;
mod timeline;
mod view;

use thiserror::Error;

pub use delta::{Delta, fold_with_delta};
pub use diff::{Changed, Changes, MergeEdge, WorldDiff, diff};
pub use http::{
    Branch, QueryState, RawEventInfo, SourceInfo, TimeAt, TimeResult, WorldSummary, router,
};
pub use timeline::{HistoryEntry, TimeRange, TimedEvent, Timeline};
pub use view::{
    ACTUAL_BRANCH, HubRef, Link, Lod, MAX_HOPS, Node, ViewParams, WorldView, world_view,
};

// The parameter validators the HTTP handlers and the MCP tools share, so the two surfaces can
// never disagree about what a valid `world`, `branch` or `lod` is.
pub(crate) use http::{check_branch, check_world, parse_lod};

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
}

impl QueryError {
    /// A stable machine-readable code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::OffsetBeyondHead { .. } => "offset_beyond_head",
            Self::BranchNotYet { .. } => "branch_not_yet",
            Self::LodNotYet { .. } => "lod_not_yet",
            Self::BadParameter { .. } => "bad_parameter",
            Self::UnknownEntity { .. } => "unknown_entity",
            Self::UnknownWorld { .. } => "unknown_world",
            Self::HopsTooLarge { .. } => "hops_too_large",
            Self::Unavailable => "unavailable",
            Self::StreamLimit => "stream_limit",
        }
    }

    /// The error body every surface serves: `{"error": <code>, "message": <text>}`. HTTP puts
    /// it in the response with a status code; MCP puts it in an error tool result's text. One
    /// constructor, so the two can never drift.
    #[must_use]
    pub(crate) fn json_body(&self) -> serde_json::Value {
        serde_json::json!({ "error": self.code(), "message": self.to_string() })
    }
}
