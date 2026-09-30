//! The storage-neutral error type and the SQLite/filesystem error mappings.

use std::error::Error;
use std::fmt;

use rusqlite::ErrorCode;
use s2w_model::ModelError;

/// Storage-neutral failures from an event log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    /// An unclassified storage or filesystem operation failed.
    Io(String),
    /// The database or its declared schema version is corrupt or unsupported.
    Corrupt(String),
    /// Another durable log handle already holds the directory's writer lock.
    Locked,
    /// An event payload exceeded the 8 MiB defensive cap.
    TooLarge,
    /// A persisted cursor failed model validation.
    InvalidCursor(ModelError),
    /// Re-adding requires a cursor until adapters can resolve a live tail.
    ReaddRequiresCursor,
    /// A presentation record failed validation before being written.
    InvalidPresentation(String),
}

impl fmt::Display for LogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "event-log I/O failed: {message}"),
            Self::Corrupt(message) => write!(formatter, "event log is corrupt: {message}"),
            Self::Locked => formatter.write_str("event log is already open by another writer"),
            Self::TooLarge => formatter.write_str("event payload exceeds the 8 MiB limit"),
            Self::ReaddRequiresCursor => formatter.write_str(
                "re-add requires an explicit cursor: adapters cannot resolve a live tail",
            ),
            Self::InvalidCursor(error) => write!(formatter, "invalid stored cursor: {error}"),
            Self::InvalidPresentation(message) => {
                write!(formatter, "invalid presentation: {message}")
            }
        }
    }
}

impl Error for LogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCursor(error) => Some(error),
            Self::Io(_)
            | Self::Corrupt(_)
            | Self::Locked
            | Self::TooLarge
            | Self::ReaddRequiresCursor
            | Self::InvalidPresentation(_) => None,
        }
    }
}

pub(crate) fn map_sqlite(error: rusqlite::Error) -> LogError {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
            LogError::Corrupt(error.to_string())
        }
        _ => LogError::Io(error.to_string()),
    }
}

/// Maps a SQLite write error: a constraint violation becomes `on_violation()`; anything else
/// goes through [`map_sqlite`].
pub(crate) fn map_constraint(
    error: rusqlite::Error,
    on_violation: impl FnOnce() -> LogError,
) -> LogError {
    if error.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) {
        on_violation()
    } else {
        map_sqlite(error)
    }
}

pub(crate) fn map_fs_error(error: std::io::Error) -> LogError {
    LogError::Io(error.to_string())
}

/// Maps a failed [`File::try_lock`] to a [`LogError`]: only [`std::fs::TryLockError::WouldBlock`]
/// means another handle holds the lock. Any other error is a real I/O failure and must not be
/// mistaken for [`LogError::Locked`].
pub(crate) fn map_try_lock_error(error: std::fs::TryLockError) -> LogError {
    match error {
        std::fs::TryLockError::WouldBlock => LogError::Locked,
        std::fs::TryLockError::Error(io_error) => LogError::Io(io_error.to_string()),
    }
}
