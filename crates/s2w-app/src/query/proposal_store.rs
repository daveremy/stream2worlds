//! Read access to a world's proposal store: the one place `query` opens it.

use std::collections::BTreeMap;
use std::path::Path;

use s2w_log::{LogError, ReadOnlySqliteProposalStore, StoredDecision, StoredProposal};
use s2w_model::{SourceId, StreamMapping};

use super::QueryError;
use super::proposals::{ProposalsView, proposals_view};
use super::resolve::resolve_class;
use super::stream_mapping::{STREAM_MAPPING_CLASS, decode_envelope};

/// Opens `log_dir`'s proposal store read-only, or `None` when the store file does not exist.
/// The existence check comes first so a read never creates the store.
///
/// # Errors
/// [`QueryError::Storage`] if the store exists but cannot be opened.
pub(crate) fn open_proposal_reader(
    log_dir: &Path,
) -> Result<Option<ReadOnlySqliteProposalStore>, QueryError> {
    let exists = log_dir
        .join(s2w_log::PROPOSAL_DATABASE_FILE)
        .try_exists()
        .map_err(|error| QueryError::Storage(error.to_string()))?;
    if !exists {
        return Ok(None);
    }
    Ok(Some(ReadOnlySqliteProposalStore::open(log_dir)?))
}

/// The proposals view of `log_dir`'s store: every summary, decision and grade. Empty when the
/// store file does not exist; never creates it.
///
/// # Errors
/// [`QueryError::Storage`] if the store exists but cannot be opened or read.
pub fn read_view(log_dir: &Path) -> Result<ProposalsView, QueryError> {
    let Some(reader) = open_proposal_reader(log_dir)? else {
        return Ok(ProposalsView::default());
    };
    Ok(proposals_view(
        &reader.proposal_summaries()?,
        &reader.decisions()?,
    ))
}

/// A proposal store's rows: its proposals and its decisions.
pub(crate) type ProposalRows = (Vec<StoredProposal>, Vec<StoredDecision>);

/// `log_dir`'s proposal and decision rows, or `None` when the store file does not exist. The
/// existence check comes first so a read never creates the store. Errors stay [`LogError`] so
/// each caller keeps its own error text: `routes::load` wraps them in `AppError::Proposals`,
/// [`read_mappings`] in the same `proposal store: ` prefix.
///
/// # Errors
/// [`LogError`] if the store exists but cannot be opened or read.
pub(crate) fn read_proposal_rows(log_dir: &Path) -> Result<Option<ProposalRows>, LogError> {
    let exists = log_dir
        .join(s2w_log::PROPOSAL_DATABASE_FILE)
        .try_exists()
        .map_err(|error| LogError::Io(error.to_string()))?;
    if !exists {
        return Ok(None);
    }
    let store = ReadOnlySqliteProposalStore::open(log_dir)?;
    Ok(Some((store.proposals()?, store.decisions()?)))
}

/// Every source's effective `stream-mapping` of `log_dir`'s store: decision 0023's rule
/// ([`resolve_class`]), keeping only the winners. Empty when the store file does not exist;
/// never creates it. A store error reads `proposal store: <error>`, the text `/sentences`
/// answered with when it read `routes::load`.
///
/// # Errors
/// [`QueryError::Storage`] if the store exists but cannot be opened or read.
pub(crate) fn read_mappings(
    log_dir: &Path,
) -> Result<BTreeMap<SourceId, StreamMapping>, QueryError> {
    let rows = read_proposal_rows(log_dir)
        .map_err(|error| QueryError::Storage(format!("proposal store: {error}")))?;
    let Some((proposals, decisions)) = rows else {
        return Ok(BTreeMap::new());
    };
    Ok(resolve_class(
        STREAM_MAPPING_CLASS,
        decode_envelope,
        &proposals,
        &decisions,
    )
    .winners
    .into_iter()
    .map(|(source, winner)| (source, winner.value))
    .collect())
}
