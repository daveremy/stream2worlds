//! The read-only MCP server (decision 0007): `s2w mcp` serves the query API's five read tools
//! over stdio, one per HTTP route, each returning the route's exact JSON bytes as its text.
//!
//! Stdout is the JSON-RPC channel, so nothing on this path writes to it; the only failure that
//! stops the server is the transport itself ending.

mod tools;

pub use tools::{EntityHistoryArgs, TimeArgs, WorldDiffArgs, WorldViewArgs};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool_handler};

use crate::AppError;
use crate::query::QueryState;

/// The MCP server over a [`QueryState`]: the five tools of `tools.rs`, each a read-only mirror
/// of one query API route.
#[derive(Clone)]
pub struct WorldMcp {
    /// The timeline every tool reads; cheap to clone (an `Arc` inside), so a later live
    /// instance can be shared with the HTTP server unchanged.
    state: QueryState,
    /// The macro-generated router over the `#[tool]` methods.
    tool_router: ToolRouter<Self>,
}

impl WorldMcp {
    /// Serves queries against `state`.
    #[must_use]
    pub fn new(state: QueryState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for WorldMcp {
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // rmcp 3.4.1's ToolRouter::call converts deserialization errors into an `is_error` tool
        // result (`into_tool_argument_error`); we want them as a protocol-level INVALID_PARAMS
        // error instead, so this bypasses that one conversion step. `has_route` reproduces
        // `call`'s own "unknown or disabled" check first, so a future disabled tool (nothing
        // calls `ToolRouter::disable_route` today) is rejected exactly as `call` would reject it.
        if !self.tool_router.has_route(request.name.as_ref()) {
            return Err(ErrorData::invalid_params("tool not found", None));
        }
        let Some(route) = self.tool_router.map.get(request.name.as_ref()) else {
            // `has_route` just confirmed this name is present and enabled.
            return Err(ErrorData::invalid_params("tool not found", None));
        };
        (route.call)(ToolCallContext::new(self, request, context)).await
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("s2w", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Read-only view of the s2w world model. Every tool returns the same JSON as the \
                 matching s2w query API route; errors come back as {\"error\", \"message\"} \
                 objects with stable codes. Until the live event-log bridge lands, the server \
                 serves an empty world.",
            )
    }
}

/// Runs `s2w mcp` over stdio until the client disconnects.
///
/// Builds a current-thread Tokio runtime (the same shape as [`crate::watch_wikipedia`]) from a
/// plain sync entry point, so the CLI never nests runtimes. The serving future returns when the
/// client closes the pipe; stderr stays free for diagnostics, stdout does not.
///
/// # Errors
///
/// [`AppError::Mcp`] if the initialize handshake or the serving task fails,
/// [`AppError::Runtime`] if the runtime cannot be built.
pub fn run_mcp(state: QueryState) -> Result<(), AppError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(AppError::Runtime)?;
    runtime.block_on(async move {
        let service = WorldMcp::new(state)
            .serve(stdio())
            .await
            .map_err(mcp_error)?;
        service.waiting().await.map(|_| ()).map_err(mcp_error)?;
        Ok(())
    })
}

/// Wraps an rmcp or tokio serving failure for [`AppError::Mcp`].
fn mcp_error(error: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Mcp(Box::new(error))
}
