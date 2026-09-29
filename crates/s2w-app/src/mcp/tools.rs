//! The six MCP tools: one per query API route, each calling the same [`QueryState`] method its
//! route calls. A tool's text is `serde_json::to_string` of the route's DTO — the same
//! serializer, so the same bytes — and an in-domain [`QueryError`] becomes an `is_error` result
//! whose text is the route's error body. Arguments that fail to deserialize at all are rejected
//! by the protocol before the tool runs; that is a different, protocol-level failure channel.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::schemars;
use rmcp::tool;
use rmcp::tool_router;

use crate::query::{
    Branch, HistoryEntry, QueryError, SourceInfo, TimeResult, ViewParams, WorldDiff, WorldView,
    check_branch, check_world, parse_lod, world_view,
};

use super::WorldMcp;

/// A tool result's shared shape: the route's JSON as text, or the route's error body as an
/// `is_error` text.
fn serve(dto: Result<impl serde::Serialize, QueryError>) -> CallToolResult {
    match dto {
        Ok(dto) => match serde_json::to_string(&dto) {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            // Unreachable for these DTOs (nothing fallible in them), but a serializer failure
            // must never take the server down.
            Err(e) => CallToolResult::error(vec![ContentBlock::text(format!(
                "serializing the result failed: {e}"
            ))]),
        },
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e.json_body().to_string())]),
    }
}

/// `world_view`'s parameters: the `/worlds/{world}/world` route's path and query parameters.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WorldViewArgs {
    /// The string identifier of the world to query.
    pub world: String,
    /// The world branch; only `actual` exists today.
    pub branch: Option<String>,
    /// The fold offset to view; the head when absent.
    pub at: Option<u64>,
    /// Level of detail: `entity` (the default) or `type`.
    pub lod: Option<String>,
    /// Restrict the view to this entity id's neighbourhood.
    pub focus: Option<u64>,
    /// The neighbourhood's radius in hops; 1 by default, 5 at most.
    pub hops: Option<u32>,
}

/// `world_diff`'s parameters: the `/worlds/{world}/diff` route's path and query parameters.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WorldDiffArgs {
    /// The string identifier of the world to query.
    pub world: String,
    /// The world branch; only `actual` exists today.
    pub branch: Option<String>,
    /// The earlier fold offset; 0 when absent.
    pub from: Option<u64>,
    /// The later fold offset; the head when absent.
    pub to: Option<u64>,
}

/// `entity_history`'s parameters: the `/worlds/{world}/entity/{id}/history` route's path and
/// query parameters.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct EntityHistoryArgs {
    /// The string identifier of the world to query.
    pub world: String,
    /// The world branch; only `actual` exists today.
    pub branch: Option<String>,
    /// The entity id whose history is asked for.
    pub id: u64,
    /// Fold offset to stop at; the head when absent.
    pub to: Option<u64>,
}

/// `branches`'s parameters: the `/worlds/{world}/branches` route's path parameter.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct BranchesArgs {
    /// The string identifier of the world to query.
    pub world: String,
}

/// `time`'s parameters: the `/worlds/{world}/time` route's path and query parameters.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct TimeArgs {
    /// The string identifier of the world to query.
    pub world: String,
    /// The world branch; only `actual` exists today.
    pub branch: Option<String>,
    /// A timestamp in milliseconds since the epoch.
    pub ts: Option<i64>,
}

/// `sources`'s parameters: the `/worlds/{world}/sources` route's path and query parameters.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SourcesArgs {
    /// The string identifier of the world to query.
    pub world: String,
    /// The fold offset whose membership to list; the head when absent.
    pub at: Option<u64>,
}

#[tool_router(vis = "pub(crate)")]
impl WorldMcp {
    /// The world as a d3 `{nodes, links}` graph at a fold offset, at a level of detail,
    /// optionally focused on one entity's neighbourhood. Requires `world` and mirrors
    /// `GET /worlds/{world}/world`; entities past the in-degree cap come back as one aggregate
    /// `hub` node. Try `time` first to find the offsets that exist.
    #[tool(name = "world_view", description = WORLD_VIEW, annotations(read_only_hint = true))]
    pub fn world_view(&self, Parameters(args): Parameters<WorldViewArgs>) -> CallToolResult {
        serve(self.view(args))
    }

    /// `/worlds/{world}/world`'s logic: validate exactly like the route, then project.
    fn view(&self, args: WorldViewArgs) -> Result<WorldView, QueryError> {
        check_world(&self.state, &args.world)?;
        check_branch(args.branch.as_deref())?;
        let params = ViewParams {
            lod: parse_lod(args.lod.as_deref())?,
            focus: args.focus,
            hops: args.hops.unwrap_or(1),
        };
        world_view(&self.state.world_at(args.at)?, &params)
    }

    /// What changed between two fold offsets: entity-level nodes, links and merges added,
    /// removed and changed. Requires `world` and mirrors `GET /worlds/{world}/diff`; `from`
    /// defaults to 0 and `to` to the head.
    #[tool(name = "world_diff", description = WORLD_DIFF, annotations(read_only_hint = true))]
    pub fn world_diff(&self, Parameters(args): Parameters<WorldDiffArgs>) -> CallToolResult {
        serve(self.diff(args))
    }

    /// `/worlds/{world}/diff`'s logic.
    fn diff(&self, args: WorldDiffArgs) -> Result<WorldDiff, QueryError> {
        check_world(&self.state, &args.world)?;
        check_branch(args.branch.as_deref())?;
        self.state.diff(args.from.unwrap_or(0), args.to)
    }

    /// Every delta naming an entity id up to a fold offset, including ids that were merged
    /// into it at that moment. Requires `world` and mirrors
    /// `GET /worlds/{world}/entity/{id}/history`; unknown ids are an error.
    #[tool(
        name = "entity_history",
        description = ENTITY_HISTORY,
        annotations(read_only_hint = true)
    )]
    pub fn entity_history(
        &self,
        Parameters(args): Parameters<EntityHistoryArgs>,
    ) -> CallToolResult {
        serve(self.history(args))
    }

    /// `/worlds/{world}/entity/{id}/history`'s logic.
    fn history(&self, args: EntityHistoryArgs) -> Result<Vec<HistoryEntry>, QueryError> {
        check_world(&self.state, &args.world)?;
        check_branch(args.branch.as_deref())?;
        self.state.history(args.id, args.to)
    }

    /// The world branches that exist. Requires `world` and mirrors
    /// `GET /worlds/{world}/branches`: exactly one, `actual`, with its head offset and fold
    /// version.
    #[tool(name = "branches", description = BRANCHES, annotations(read_only_hint = true))]
    pub fn branches(&self, Parameters(args): Parameters<BranchesArgs>) -> CallToolResult {
        serve(self.branches_checked(args))
    }

    /// `/worlds/{world}/branches`'s logic.
    fn branches_checked(&self, args: BranchesArgs) -> Result<Vec<Branch>, QueryError> {
        check_world(&self.state, &args.world)?;
        self.state.branches()
    }

    /// The time index. With `ts` (milliseconds since the epoch), the largest fold offset whose
    /// events were received at or before it; without it, the range summary (head, first and
    /// last timestamps, clamped append count). Requires `world` and mirrors
    /// `GET /worlds/{world}/time`.
    #[tool(name = "time", description = TIME, annotations(read_only_hint = true))]
    pub fn time(&self, Parameters(args): Parameters<TimeArgs>) -> CallToolResult {
        serve(self.time_of(args))
    }

    /// `/worlds/{world}/time`'s logic.
    fn time_of(&self, args: TimeArgs) -> Result<TimeResult, QueryError> {
        check_world(&self.state, &args.world)?;
        check_branch(args.branch.as_deref())?;
        self.state.time(args.ts)
    }

    /// Each member source with how many of its events the bridge consumed, how many no engine
    /// is routed for, and the most recent of those. Requires `world` and mirrors
    /// `GET /worlds/{world}/sources`.
    #[tool(name = "sources", description = SOURCES, annotations(read_only_hint = true))]
    pub fn sources(&self, Parameters(args): Parameters<SourcesArgs>) -> CallToolResult {
        serve(self.sources_of(args))
    }

    /// `/worlds/{world}/sources`'s logic.
    fn sources_of(&self, args: SourcesArgs) -> Result<Vec<SourceInfo>, QueryError> {
        check_world(&self.state, &args.world)?;
        self.state.sources(args.at)
    }
}

/// Descriptions are `&'static str`s the macro can quote; keeping them as named constants stops
/// the tool methods from becoming description carriers.
const WORLD_VIEW: &str = "Requires the world string parameter. The world as a d3 {nodes, links} \
    graph at a fold offset, at a level of detail (entity by default, or type), optionally \
    focused on one entity's neighbourhood (at most 5 hops). Mirrors GET \
    /worlds/{world}/world: entities past the in-degree cap come back as one aggregate hub node, \
    and errors are {\"error\", \"message\"} objects (offset_beyond_head, unknown_entity, \
    hops_too_large, lod_not_yet, branch_not_yet, unknown_world). Call `time` first to find the \
    offsets that exist.";
const WORLD_DIFF: &str = "Requires the world string parameter. What changed between two fold \
    offsets: entity-level nodes, links and merges added, removed and changed. Mirrors GET \
    /worlds/{world}/diff: from defaults to 0 and to to the head; an offset past the head is an \
    offset_beyond_head error.";
const ENTITY_HISTORY: &str = "Requires the world string parameter. Every delta naming one entity \
    id up to a fold offset, including ids that were merged into it at that moment. Mirrors GET \
    /worlds/{world}/entity/{id}/history: to defaults to the head, and an unknown id is an \
    unknown_entity error. Use world_view to find entity ids first.";
const BRANCHES: &str = "Requires the world string parameter. The world branches that exist. \
    Mirrors GET /worlds/{world}/branches: today exactly one, 'actual', with its head offset, \
    fold version and hub in-degree cap.";
const TIME: &str = "Requires the world string parameter. The world's time index. With ts \
    (milliseconds since the epoch), returns the largest fold offset whose events were received \
    at or before it; without ts, returns the range summary: head, first_ts, last_ts and how many \
    out-of-order timestamps were clamped. Mirrors GET /worlds/{world}/time. Fold offsets are \
    what world_view, world_diff and entity_history accept.";
const SOURCES: &str = "Requires the world string parameter. The world's member sources at a \
    fold offset (the head by default), each with consumed and unrouted event counts and \
    recent_unrouted, the most recent events no engine is routed for, most recent first. Mirrors \
    GET /worlds/{world}/sources; use it to see events logged that no engine has routed yet.";
