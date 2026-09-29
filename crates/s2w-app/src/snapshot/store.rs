//! The snapshot directory, `<log_dir>/snapshots/`: one file per snapshot, named
//! `snapshot-<feed fingerprint, 16 hex>-<offset, 20 digits>.s2w`, written atomically, loaded
//! newest first and pruned to the newest [`KEEP`], both per feed fingerprint (s2w#184): a
//! rebuild restarts offsets near zero, and ordering every fingerprint's files together would
//! prune the new routing's snapshots and keep the old one's. Only files matching that name are
//! ever read or removed, and only the caller's fingerprint's. The pre-#184 name
//! (`snapshot-<20 digits>.s2w`) is not read (decision 0023).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Invalid, SnapshotError, SnapshotV1, codec};

/// How many snapshots [`prune`] keeps.
pub const KEEP: usize = 3;

const PREFIX: &str = "snapshot-";
const SUFFIX: &str = ".s2w";

/// The snapshot directory inside an event-log directory.
#[must_use]
pub fn dir(log_dir: &Path) -> PathBuf {
    log_dir.join("snapshots")
}

/// The file name for a snapshot at `offset` written under the feed fingerprint `feed`.
/// Zero-padded, so one fingerprint's names sort by offset.
#[must_use]
pub fn file_name(feed: u64, offset: u64) -> String {
    format!("{PREFIX}{feed:016x}-{offset:020}{SUFFIX}")
}

/// The feed fingerprint and offset a snapshot file name encodes, or `None` for any other file.
fn parse_name(name: &str) -> Option<(u64, u64)> {
    let rest = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let (hex, digits) = rest.split_once('-')?;
    let is_hex = hex.len() == 16 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if is_hex && digits.len() == 20 && digits.bytes().all(|b| b.is_ascii_digit()) {
        Some((u64::from_str_radix(hex, 16).ok()?, digits.parse().ok()?))
    } else {
        None
    }
}

/// Writes `snapshot` into `dir` atomically: a temporary file in the same directory, written
/// and `fsync`ed, renamed over the final name, then the directory `fsync`ed. A crash leaves
/// either the old state or the whole new file, never a partial snapshot under a loadable name.
/// A snapshot at an offset that already has one replaces it.
///
/// # Errors
/// [`SnapshotError`] if encoding or any filesystem step fails.
pub fn write(dir: &Path, snapshot: &SnapshotV1) -> Result<PathBuf, SnapshotError> {
    write_bytes(
        dir,
        snapshot.feed_hash,
        snapshot.offset,
        &codec::encode(snapshot)?,
    )
}

/// Writes already-encoded snapshot file `bytes` for `offset` under the feed fingerprint `feed`,
/// atomically as [`write()`] does. `serve` encodes on the bridge thread and hands the bytes to its
/// writer thread (#179).
///
/// # Errors
/// [`SnapshotError`] if any filesystem step fails.
pub fn write_bytes(
    dir: &Path,
    feed: u64,
    offset: u64,
    bytes: &[u8],
) -> Result<PathBuf, SnapshotError> {
    fs::create_dir_all(dir)?;
    let name = file_name(feed, offset);
    let tmp = dir.join(format!(".{name}.tmp"));
    let path = dir.join(name);
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, &path)?;
    fs::File::open(dir)?.sync_all()?;
    Ok(path)
}

/// Every snapshot file in `dir` written under the feed fingerprint `feed`, with its offset,
/// newest first. A missing directory has none; another fingerprint's files are not listed.
///
/// # Errors
/// Any I/O error other than the directory not existing.
pub fn list(dir: &Path, feed: u64) -> std::io::Result<Vec<(u64, PathBuf)>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        if let Some((file_feed, offset)) = entry.file_name().to_str().and_then(parse_name)
            && file_feed == feed
        {
            found.push((offset, entry.path()));
        }
    }
    found.sort_by_key(|(offset, _)| std::cmp::Reverse(*offset));
    Ok(found)
}

/// What [`load_latest`] found.
#[derive(Debug, Default)]
pub struct Loaded {
    /// The newest valid snapshot and its file, if any.
    pub snapshot: Option<(PathBuf, SnapshotV1)>,
    /// Each newer file that was ignored, and why.
    pub skipped: Vec<(PathBuf, Invalid)>,
}

/// The newest snapshot in `dir` written under the feed fingerprint `feed` that decodes and
/// passes `accept` (the caller's rules 2 to 5), skipping, never deleting, every newer file that
/// does not. `accept` sees only snapshots whose payload offset matches their file name. Another
/// fingerprint's files are never read.
///
/// # Errors
/// An I/O error listing the directory. Unreadable files are skipped, not errors.
pub fn load_latest(
    dir: &Path,
    feed: u64,
    mut accept: impl FnMut(&SnapshotV1) -> Result<(), Invalid>,
) -> std::io::Result<Loaded> {
    let mut loaded = Loaded::default();
    for (offset, path) in list(dir, feed)? {
        let checked = fs::read(&path)
            .map_err(|e| Invalid::Io(e.to_string()))
            .and_then(|bytes| codec::decode(&bytes))
            .and_then(|snapshot| {
                if snapshot.offset == offset {
                    Ok(snapshot)
                } else {
                    Err(Invalid::Corrupt(format!(
                        "file name says offset {offset}, payload says {}",
                        snapshot.offset
                    )))
                }
            })
            .and_then(|snapshot| accept(&snapshot).map(|()| snapshot));
        match checked {
            Ok(snapshot) => {
                loaded.snapshot = Some((path, snapshot));
                break;
            }
            Err(why) => loaded.skipped.push((path, why)),
        }
    }
    Ok(loaded)
}

/// Removes all but the newest `keep` snapshot files in `dir` written under the feed fingerprint
/// `feed` and returns the removed paths. Only that fingerprint's files matching the snapshot
/// name pattern are touched; another fingerprint's snapshots stay (#33 part 2 owns retention).
///
/// # Errors
/// An I/O error listing the directory or removing a file.
pub fn prune(dir: &Path, feed: u64, keep: usize) -> std::io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for (_, path) in list(dir, feed)?.into_iter().skip(keep) {
        fs::remove_file(&path)?;
        removed.push(path);
    }
    Ok(removed)
}

/// Removes temporary files a crashed [`write()`] left behind (`.snapshot-<16 hex>-<20
/// digits>.s2w.tmp`, any fingerprint) and returns their paths. Only one writer runs per log directory (the process holding the
/// log's writer lock), so call this before it starts, never while a write may be in flight.
///
/// # Errors
/// Any I/O error other than the directory not existing.
pub fn clean_tmp(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut removed = Vec::new();
    for entry in entries {
        let entry = entry?;
        let is_tmp = entry.file_name().to_str().is_some_and(|name| {
            name.strip_prefix('.')
                .and_then(|rest| rest.strip_suffix(".tmp"))
                .and_then(parse_name)
                .is_some()
        });
        if is_tmp {
            fs::remove_file(entry.path())?;
            removed.push(entry.path());
        }
    }
    Ok(removed)
}
