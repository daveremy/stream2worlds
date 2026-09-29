//! Read access to a world's proposal store: the one place `query` opens it.

use std::path::Path;

use s2w_log::ReadOnlySqliteProposalStore;

use super::QueryError;
use super::proposals::{ProposalsView, proposals_view};

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
