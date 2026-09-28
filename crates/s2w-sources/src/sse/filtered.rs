//! A generic, data-driven filter wrapped around any [`SseDialect`] — not a dialect named after
//! or bound to a particular stream (decision 0018; see `AGENTS.md`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use s2w_model::Cursor;

use super::{SinceError, SseDialect};
use crate::filter::{FieldFilter, apply_all};

/// Consecutive events that reached the filter check but matched nothing before this wrapper
/// warns on stderr once per stall streak (carries s2w#101/#107 forward generically — see the
/// now-retired stall-tracking helper this generalizes, `git show a3b2079`).
const STALL_WARNING_THRESHOLD: u64 = 200;

/// Wraps `inner`, dropping any frame that does not match every filter in `filters`, applied at
/// [`SseDialect::accept`] — after the frame's cursor has advanced, the precedented hook for a
/// stream-specific keep/drop decision, and before the transport envelope
/// ([`super::envelope::envelope`]) so a path like `meta.domain` walks the raw frame, not the
/// stored `{"data":…,"id":…}` wrapper.
pub(crate) struct FilteredDialect {
    inner: Arc<dyn SseDialect>,
    filters: Vec<FieldFilter>,
    /// Interior mutability because [`SseDialect::accept`] takes `&self`. Inert (never
    /// increments, never warns) when `filters` is empty.
    unmatched_since_last: AtomicU64,
}

impl FilteredDialect {
    pub(crate) fn new(inner: Arc<dyn SseDialect>, filters: Vec<FieldFilter>) -> Self {
        Self {
            inner,
            filters,
            unmatched_since_last: AtomicU64::new(0),
        }
    }

    /// Tracks a match outcome and warns once per stall streak once
    /// [`STALL_WARNING_THRESHOLD`] consecutive events have matched nothing. No-op when no
    /// filter is set.
    fn track_stall(&self, matched: bool) {
        if self.filters.is_empty() {
            return;
        }
        if matched {
            self.unmatched_since_last.store(0, Ordering::Relaxed);
            return;
        }
        let count = self.unmatched_since_last.fetch_add(1, Ordering::Relaxed) + 1;
        if count == STALL_WARNING_THRESHOLD {
            eprintln!(
                "s2w: --filter has matched none of the last {STALL_WARNING_THRESHOLD} events; check the filter spec against the stream's payload shape"
            );
        }
    }

    /// Real events since the last match (or start); test-only window into `track_stall`.
    #[cfg(test)]
    fn unmatched_since_last(&self) -> u64 {
        self.unmatched_since_last.load(Ordering::Relaxed)
    }
}

impl SseDialect for FilteredDialect {
    fn cursor(&self, id: Option<&str>) -> Result<String, String> {
        self.inner.cursor(id)
    }

    fn validate_stored(&self, cursor: &Cursor) -> Result<String, String> {
        self.inner.validate_stored(cursor)
    }

    fn apply_since(&self, url: &mut reqwest::Url, since: &str) -> Result<(), SinceError> {
        self.inner.apply_since(url, since)
    }

    fn accept(&self, data: &str) -> Result<bool, String> {
        let inner_ok = self.inner.accept(data)?;
        let matched = inner_ok && apply_all(&self.filters, data.as_bytes());
        self.track_stall(matched);
        Ok(matched)
    }

    fn store(&self, cursor: &str, data: &str) -> Vec<u8> {
        self.inner.store(cursor, data)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{FilteredDialect, STALL_WARNING_THRESHOLD};
    use crate::filter::FieldFilter;
    use crate::sse::{Opaque, SseDialect};

    #[test]
    fn no_filters_keeps_every_frame_and_never_stalls() {
        let dialect = FilteredDialect::new(Arc::new(Opaque), vec![]);
        for _ in 0..STALL_WARNING_THRESHOLD {
            assert_eq!(dialect.accept(r#"{"a":1}"#), Ok(true));
        }
        assert_eq!(dialect.unmatched_since_last(), 0);
    }

    #[test]
    fn a_matching_filter_keeps_the_frame() {
        let filters = vec![FieldFilter::parse("wiki_id!=examplewiki").expect("valid")];
        let dialect = FilteredDialect::new(Arc::new(Opaque), filters);
        assert_eq!(dialect.accept(r#"{"wiki_id":"enwiki"}"#), Ok(true));
    }

    #[test]
    fn a_non_matching_filter_drops_the_frame_silently() {
        let filters = vec![FieldFilter::parse("wiki_id!=examplewiki").expect("valid")];
        let dialect = FilteredDialect::new(Arc::new(Opaque), filters);
        assert_eq!(dialect.accept(r#"{"wiki_id":"examplewiki"}"#), Ok(false));
    }

    #[test]
    fn a_stall_streak_resets_on_any_match() {
        let filters = vec![FieldFilter::parse("x=1").expect("valid")];
        let dialect = FilteredDialect::new(Arc::new(Opaque), filters);
        for _ in 0..50 {
            assert_eq!(dialect.accept(r#"{"x":2}"#), Ok(false));
        }
        assert_eq!(dialect.unmatched_since_last(), 50);
        assert_eq!(dialect.accept(r#"{"x":1}"#), Ok(true));
        assert_eq!(dialect.unmatched_since_last(), 0);
    }

    #[test]
    fn a_stall_streak_reaches_the_threshold() {
        let filters = vec![FieldFilter::parse("x=1").expect("valid")];
        let dialect = FilteredDialect::new(Arc::new(Opaque), filters);
        for _ in 0..STALL_WARNING_THRESHOLD {
            assert_eq!(dialect.accept(r#"{"x":2}"#), Ok(false));
        }
        assert_eq!(dialect.unmatched_since_last(), STALL_WARNING_THRESHOLD);
    }
}
