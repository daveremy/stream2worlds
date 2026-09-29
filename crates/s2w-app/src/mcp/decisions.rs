//! The opt-in `decision_record` tool (decision 0020): the MCP server's one write. It is
//! registered only when the server is built with [`WorldMcp::with_decisions`] (`s2w mcp
//! --allow-decisions`), appends exactly one agent decision per call and never writes a
//! proposal.

use crate::proposals::{Seat, parse_outcome, record_decision};
use crate::query::{DecisionDto, QueryError, QueryState, check_world};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars;
use rmcp::tool;
use rmcp::tool_router;

use super::WorldMcp;
use super::tools::serve;

/// `decision_record`'s parameters.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct DecisionRecordArgs {
    /// The string identifier of the world to query.
    pub world: String,
    /// The id of an existing stored proposal.
    pub proposal_id: String,
    /// `accept` or `reject`.
    pub outcome: String,
    /// Why: an opaque, non-empty reference recorded with the decision.
    pub basis: String,
}

#[tool_router(router = decision_tool_router, vis = "pub(crate)")]
impl WorldMcp {
    /// Appends one agent decision on an existing proposal and returns the stored row.
    #[tool(
        name = "decision_record",
        description = DECISION_RECORD,
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    pub fn decision_record(
        &self,
        Parameters(args): Parameters<DecisionRecordArgs>,
    ) -> CallToolResult {
        serve(record(&self.state, &args))
    }
}

fn record(state: &QueryState, args: &DecisionRecordArgs) -> Result<DecisionDto, QueryError> {
    check_world(state, &args.world)?;
    let outcome = parse_outcome(&args.outcome)?;
    let Some(log_dir) = state.log_dir() else {
        return Err(QueryError::Storage(
            "decision_record needs a log directory".to_owned(),
        ));
    };
    Ok(record_decision(
        log_dir,
        &Seat::Agent,
        &args.proposal_id,
        outcome,
        &args.basis,
    )?
    .decision)
}

const DECISION_RECORD: &str = "Requires the world string parameter. Opt-in write, present only \
    when the server runs with --allow-decisions: appends one agent decision (outcome accept or \
    reject, with a non-empty basis) on an existing proposal and returns the stored decision. \
    Agent decisions are recorded opinions: they never count as policy routing, human review or \
    evidence, and never change or add a proposal. Errors: unknown_proposal, store_locked \
    (another writer holds the store; retry), bad_parameter, unknown_world, storage. Use \
    proposals_list to find proposal ids.";
