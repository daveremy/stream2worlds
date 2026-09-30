//! Read access to a world's proposal store: the one place `query` opens it.

use std::collections::BTreeMap;
use std::path::Path;

use s2w_log::{LogError, ReadOnlySqliteProposalStore};
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

/// Every source's effective `stream-mapping` of `log_dir`'s store: decision 0023's rule
/// ([`resolve_class`]), keeping only the winners. Empty when the store file does not exist;
/// never creates it.
///
/// This repeats the exists-then-open steps of [`open_proposal_reader`] (and of
/// `routes::load`) on purpose: every store error here carries the `proposal store: ` prefix
/// `routes::load` gave `/sentences` before s2w#240, which `open_proposal_reader` does not. Do
/// not fold it into [`open_proposal_reader`] without changing that error text on purpose.
///
/// # Errors
/// [`QueryError::Storage`] if the store exists but cannot be opened or read.
pub(crate) fn read_mappings(
    log_dir: &Path,
) -> Result<BTreeMap<SourceId, StreamMapping>, QueryError> {
    let storage = |error: LogError| QueryError::Storage(format!("proposal store: {error}"));
    let exists = log_dir
        .join(s2w_log::PROPOSAL_DATABASE_FILE)
        .try_exists()
        .map_err(|error| storage(LogError::Io(error.to_string())))?;
    if !exists {
        return Ok(BTreeMap::new());
    }
    let store = ReadOnlySqliteProposalStore::open(log_dir).map_err(storage)?;
    let proposals = store.proposals().map_err(storage)?;
    let decisions = store.decisions().map_err(storage)?;
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
