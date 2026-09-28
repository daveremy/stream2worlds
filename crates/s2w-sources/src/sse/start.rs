//! Pure selection of a fresh or resumed SSE start.

use s2w_model::{Cursor, SourceId};

use super::{SinceError, SseDialect};
use crate::source::SourceError;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct StartPlan {
    pub(super) since: Option<String>,
    pub(super) initial_cursor: Option<String>,
}

pub(super) fn choose(
    name: &'static str,
    dialect: &dyn SseDialect,
    url: &reqwest::Url,
    source_id: &SourceId,
    stored: Option<&Cursor>,
    since: Option<&str>,
) -> Result<StartPlan, SourceError> {
    if stored.is_some()
        && let Some(since) = since
    {
        return Err(SourceError::SinceWithStoredCursor {
            source_id: source_id.as_str().to_owned(),
            since: since.to_owned(),
        });
    }
    let initial_cursor = stored
        .map(|stored| {
            dialect
                .validate_stored(stored)
                .map_err(|reason| SourceError::StoredCursor {
                    source_id: source_id.as_str().to_owned(),
                    cursor: String::from_utf8_lossy(stored.as_bytes()).into_owned(),
                    reason,
                })
        })
        .transpose()?;
    if let Some(value) = since {
        // Validation-only dry run: the real mutation happens per-connect in
        // `connect.rs::build_request`.
        dialect
            .apply_since(&mut url.clone(), value)
            .map_err(|error| match error {
                SinceError::Unsupported(reason) => SourceError::SinceUnsupported { name, reason },
                SinceError::Invalid(reason) => SourceError::InvalidSince {
                    name,
                    value: value.to_owned(),
                    reason,
                },
            })?;
    }
    Ok(StartPlan {
        since: since.map(str::to_owned),
        initial_cursor,
    })
}
