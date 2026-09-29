//! The opt-in `decision_record` tool (decision 0020): the MCP server's one write. It is
//! registered only when the server is built with [`WorldMcp::with_decisions`] (`s2w mcp
//! --allow-decisions`), appends exactly one agent decision per call and never writes a
//! proposal.

use std::time::{SystemTime, UNIX_EPOCH};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars;
use rmcp::tool;
use rmcp::tool_router;
use s2w_log::{Decider, LogError, NewDecision, Outcome, ProposalStore, SqliteProposalStore};

use crate::query::{DecisionDto, QueryError, QueryState, check_world, open_proposal_reader};

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

fn parse_outcome(raw: &str) -> Result<Outcome, QueryError> {
    match raw {
        "accept" => Ok(Outcome::Accept),
        "reject" => Ok(Outcome::Reject),
        other => Err(QueryError::BadParameter {
            name: "outcome",
            reason: format!("'{other}' is not one of accept, reject"),
        }),
    }
}

fn non_blank(name: &'static str, value: &str) -> Result<(), QueryError> {
    if value.trim().is_empty() {
        return Err(QueryError::BadParameter {
            name,
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(())
}

fn now_ms() -> Result<i64, QueryError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| QueryError::Storage(format!("system clock before epoch: {error}")))?;
    i64::try_from(elapsed.as_millis())
        .map_err(|error| QueryError::Storage(format!("system clock out of range: {error}")))
}

/// Checks the proposal exists through a lockless reader, so a missing store or id never opens
/// (and so never creates) the writable store.
fn check_known(log_dir: &std::path::Path, proposal_id: &str) -> Result<(), QueryError> {
    match open_proposal_reader(log_dir)? {
        Some(reader) if reader.has_proposal(proposal_id)? => Ok(()),
        _ => Err(QueryError::UnknownProposal {
            id: proposal_id.to_owned(),
        }),
    }
}

fn record(state: &QueryState, args: &DecisionRecordArgs) -> Result<DecisionDto, QueryError> {
    check_world(state, &args.world)?;
    let outcome = parse_outcome(&args.outcome)?;
    non_blank("proposal_id", &args.proposal_id)?;
    non_blank("basis", &args.basis)?;
    let Some(log_dir) = state.log_dir() else {
        return Err(QueryError::Storage(
            "decision_record needs a log directory".to_owned(),
        ));
    };
    check_known(log_dir, &args.proposal_id)?;
    let mut store = SqliteProposalStore::open(log_dir).map_err(|error| match error {
        LogError::Locked => QueryError::StoreLocked,
        other => other.into(),
    })?;
    let stored = store.append_decision(&NewDecision {
        proposal_id: args.proposal_id.clone(),
        decider: Decider::Agent,
        outcome,
        basis: args.basis.clone(),
        decided_at_ms: now_ms()?,
    })?;
    Ok(DecisionDto::from(&stored))
}

const DECISION_RECORD: &str = "Requires the world string parameter. Opt-in write, present only \
    when the server runs with --allow-decisions: appends one agent decision (outcome accept or \
    reject, with a non-empty basis) on an existing proposal and returns the stored decision. \
    Agent decisions are recorded opinions: they never count as policy routing, human review or \
    evidence, and never change or add a proposal. Errors: unknown_proposal, store_locked \
    (another writer holds the store; retry), bad_parameter, unknown_world, storage. Use \
    proposals_list to find proposal ids.";
