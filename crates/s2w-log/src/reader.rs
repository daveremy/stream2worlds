//! The read half of the log: what a consumer that never appends needs.

use crate::{EventLog, LogError, LogPosition, StoredEvent};

/// Replays stored events. Every [`EventLog`] is a reader through the blanket impl below.
///
/// The method is not named `replay`, so a scope importing both traits (as `s2w-log`'s own
/// tests do) never has an ambiguous call.
///
/// [`crate::ReadOnlySqliteEventLog`] is the lockless implementation a process other than the
/// writer can open while the writer continues appending.
pub trait LogReader {
    /// Same as [`EventLog::replay`]: events strictly after `from`, or the entire log when it
    /// is `None`.
    ///
    /// # Errors
    /// Returns an error if the replay cannot start. Errors while loading later pages are
    /// yielded by the returned iterator.
    fn read_after(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError>;
}

// `EventLog::replay` already takes `&self`, so this delegates exactly.
impl<T: EventLog> LogReader for T {
    fn read_after(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        EventLog::replay(self, from)
    }
}
