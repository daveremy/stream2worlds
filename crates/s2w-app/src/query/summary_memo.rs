//! The type summary's memo (s2w#296). The summary (`lod=type&links=none`) measured 149 ms at
//! 337k entities (`tests/backfill_memory.rs`, `queries`), over the 100 ms the plan allowed, so
//! the last one built is kept, keyed by what its `ETag` names: the epoch, the hub cap and the
//! fold offset (the fold version is a constant of the build). A page polling an unmoved head
//! then pays a clone of a few dozen nodes, not a pass over every entity.
//!
//! One entry, replaced by the next offset: the summary is a few KB, and the head is the offset
//! nearly every request asks for. The build runs outside the memo's lock, so two requests for a
//! new offset may both build it; neither waits on the other.

use std::sync::{Mutex, MutexGuard, PoisonError};

use super::QueryError;
use super::epoch::Epoch;
use super::view::WorldView;

/// What a summary is a pure function of, as far as a served timeline goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SummaryKey {
    pub(super) epoch: Epoch,
    pub(super) hub_cap: u64,
    pub(super) offset: u64,
}

/// The last summary built, and its key.
#[derive(Default)]
pub(super) struct SummaryMemo {
    last: Mutex<Option<(SummaryKey, WorldView)>>,
}

impl SummaryMemo {
    fn lock(&self) -> MutexGuard<'_, Option<(SummaryKey, WorldView)>> {
        // The guarded value is replaced whole, never left half-written, so a poisoned lock
        // still holds a consistent entry.
        self.last.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The summary at `key`: the memo's copy when it has one, else `build`'s, which then
    /// replaces it.
    ///
    /// # Errors
    /// Whatever `build` returns; an error is not memoised.
    pub(super) fn get_or_build(
        &self,
        key: SummaryKey,
        build: impl FnOnce() -> Result<WorldView, QueryError>,
    ) -> Result<WorldView, QueryError> {
        if let Some((_, view)) = self.lock().as_ref().filter(|(k, _)| *k == key) {
            return Ok(view.clone());
        }
        let view = build()?;
        *self.lock() = Some((key, view.clone()));
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use s2w_core::World;

    use super::*;
    use crate::query::view::type_summary;

    fn key(offset: u64) -> SummaryKey {
        SummaryKey {
            epoch: Epoch(7),
            hub_cap: 3,
            offset,
        }
    }

    #[test]
    fn a_repeated_key_is_served_without_a_build() {
        let memo = SummaryMemo::default();
        let builds = Cell::new(0);
        let build = || {
            builds.set(builds.get() + 1);
            Ok(type_summary(&World::with_hub_cap(3)))
        };
        let first = memo.get_or_build(key(1), build).unwrap();
        let second = memo.get_or_build(key(1), build).unwrap();
        assert_eq!(first, second);
        assert_eq!(builds.get(), 1);
        // Any part of the key moving is a new build.
        memo.get_or_build(key(2), build).unwrap();
        memo.get_or_build(
            SummaryKey {
                hub_cap: 4,
                ..key(2)
            },
            build,
        )
        .unwrap();
        memo.get_or_build(
            SummaryKey {
                epoch: Epoch(8),
                ..key(2)
            },
            build,
        )
        .unwrap();
        assert_eq!(builds.get(), 4);
    }

    #[test]
    fn an_error_is_not_memoised() {
        let memo = SummaryMemo::default();
        let error = memo.get_or_build(key(1), || Err(QueryError::Unavailable));
        assert!(error.is_err());
        let builds = Cell::new(0);
        memo.get_or_build(key(1), || {
            builds.set(builds.get() + 1);
            Ok(type_summary(&World::with_hub_cap(3)))
        })
        .unwrap();
        assert_eq!(builds.get(), 1);
    }
}
