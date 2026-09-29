//! Storage figures for the progress line (s2w#32): how big the store is (every SQLite file in
//! the log directory), how fast it grows, and how many days until its disk is full.
//!
//! A storage number measured on a RAM-backed filesystem says nothing about a disk, so every
//! figure here carries the [`Filesystem`] it was measured on, and a tmpfs one never prints a
//! days-to-full estimate. Classification needs `statfs`, which only Linux reports in a form
//! this module reads; elsewhere, and whenever `statfs` fails, the answer is
//! [`Filesystem::Unknown`], never [`Filesystem::Disk`].

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use tokio::time::Instant;

/// `statfs` magic for tmpfs (`TMPFS_MAGIC` in `linux/magic.h`).
pub(crate) const TMPFS_MAGIC: u32 = 0x0102_1994;

/// `statfs` magic for ramfs (`RAMFS_MAGIC` in `linux/magic.h`).
pub(crate) const RAMFS_MAGIC: u32 = 0x8584_58f6;

/// `statfs` magic for overlayfs (`OVERLAYFS_SUPER_MAGIC`). Its backing store could be a disk or
/// RAM and `statfs` does not say which, so it classifies as [`Filesystem::Unknown`].
pub(crate) const OVERLAYFS_MAGIC: u32 = 0x794c_7630;

/// The text a storage number measured on tmpfs must carry.
pub const TMPFS_WARNING: &str = "MEASURED ON tmpfs, NOT A DISK NUMBER";

/// What kind of filesystem a path lives on, as far as storage numbers are concerned.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Filesystem {
    /// RAM-backed (tmpfs or ramfs): a storage number here is not a disk number.
    Tmpfs,
    /// Any other filesystem `statfs` names, assumed to be backed by a disk.
    Disk,
    /// `statfs` failed, is not read on this platform, or named an overlay.
    Unknown,
}

impl Filesystem {
    /// Classifies a `statfs` magic number; `None` means `statfs` failed.
    #[must_use]
    pub const fn from_magic(magic: Option<u32>) -> Self {
        match magic {
            Some(TMPFS_MAGIC | RAMFS_MAGIC) => Self::Tmpfs,
            Some(OVERLAYFS_MAGIC) | None => Self::Unknown,
            Some(_) => Self::Disk,
        }
    }

    /// A short lowercase name: `tmpfs`, `disk` or `unknown`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tmpfs => "tmpfs",
            Self::Disk => "disk",
            Self::Unknown => "unknown",
        }
    }
}

/// The filesystem `path` lives on. [`Filesystem::Unknown`] if `statfs` fails or off Linux.
#[must_use]
pub fn filesystem_kind(path: &Path) -> Filesystem {
    Filesystem::from_magic(statfs_facts(path).map(|facts| facts.magic))
}

/// Bytes available to an unprivileged writer on `path`'s filesystem (`f_bavail` blocks), or
/// `None` if `statfs` fails or off Linux.
#[must_use]
pub(crate) fn free_bytes(path: &Path) -> Option<u64> {
    statfs_facts(path).and_then(|facts| facts.free_bytes)
}

/// The two `statfs` fields this module reads.
struct StatFsFacts {
    magic: u32,
    free_bytes: Option<u64>,
}

#[cfg(target_os = "linux")]
fn statfs_facts(path: &Path) -> Option<StatFsFacts> {
    let stat = rustix::fs::statfs(path).ok()?;
    // `f_type` is a signed or unsigned word depending on the target; the magic is its low 32
    // bits either way.
    let magic = u32::try_from(i128::from(stat.f_type) & 0xFFFF_FFFF).ok()?;
    let free_bytes = i128::from(stat.f_bavail)
        .checked_mul(i128::from(stat.f_bsize))
        .and_then(|bytes| u64::try_from(bytes).ok());
    Some(StatFsFacts { magic, free_bytes })
}

#[cfg(not(target_os = "linux"))]
fn statfs_facts(_path: &Path) -> Option<StatFsFacts> {
    None
}

/// The store's size: the total of every regular file in `dir` whose name contains `.sqlite3`
/// (the event log, any other database beside it, and their `-wal`/`-shm` sidecars). Not
/// recursive.
///
/// # Errors
///
/// Returns the error from reading `dir` or one of its entries.
pub fn store_bytes(dir: &Path) -> io::Result<u64> {
    let mut total = 0_u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().contains(".sqlite3") {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

/// Days until `free_bytes` is used up at `bytes_per_sec`, or `None` if the rate is not a
/// positive finite number.
#[must_use]
pub(crate) fn days_to_full(free_bytes: u64, bytes_per_sec: f64) -> Option<f64> {
    if bytes_per_sec.is_finite() && bytes_per_sec > 0.0 {
        Some(free_bytes as f64 / bytes_per_sec / 86_400.0)
    } else {
        None
    }
}

/// One reading of the storage figures, ready to render.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct StorageReading {
    /// The filesystem the log lives on.
    pub filesystem: Filesystem,
    /// The store's size ([`store_bytes`]), if it could be read.
    pub store_bytes: Option<u64>,
    /// Store growth since the first reading; `None` until there are two readings.
    pub growth_bytes_per_sec: Option<f64>,
    /// Free bytes on the log's filesystem, if known.
    pub free_bytes: Option<u64>,
}

/// Renders the storage segment of the progress line, e.g. `store 1.9 GB | disk full in 412 d`.
///
/// `—` stands in for the days figure until there are two readings; a tmpfs or unknown-filesystem
/// store
/// prints a label and never a days figure.
#[must_use]
pub fn render_storage(reading: &StorageReading) -> String {
    let mut line = match reading.store_bytes {
        Some(bytes) => format!("store {}", human_bytes(bytes)),
        None => "store ?".to_owned(),
    };
    if reading.filesystem == Filesystem::Tmpfs {
        line.push_str(" | tmpfs: NOT A DISK NUMBER");
        return line;
    }
    if reading.filesystem == Filesystem::Unknown {
        // An overlay can sit on tmpfs, so no days figure: it could be a RAM number.
        line.push_str(" | filesystem unknown: no disk estimate");
        return line;
    }
    line.push_str(" | disk full in ");
    match reading.growth_bytes_per_sec {
        None => line.push('—'),
        Some(rate) if rate <= 0.0 => line.push_str("— (store not growing)"),
        Some(rate) => match reading.free_bytes.and_then(|free| days_to_full(free, rate)) {
            Some(days) => {
                let _ = write!(line, "{days:.0} d");
            }
            None => line.push('?'),
        },
    }
    line
}

/// `bytes` in decimal units with one decimal place: `512 B`, `1.9 GB`.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["kB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = UNITS[0];
    for next in &UNITS[1..] {
        if value < 1000.0 {
            break;
        }
        value /= 1000.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

/// What the periodic progress line reports on: the source's name and, when a log directory is
/// known (`watch`), that directory's storage figures. `serve` passes a name only.
pub(crate) struct Progress<'a> {
    name: &'a str,
    storage: Option<StorageProbe>,
}

impl<'a> Progress<'a> {
    /// A progress line for `name` without storage figures.
    pub(crate) const fn named(name: &'a str) -> Self {
        Self {
            name,
            storage: None,
        }
    }

    /// Adds the store's size and days-to-disk-full for `log_dir` to the line.
    pub(crate) fn with_log_dir(mut self, log_dir: &Path) -> Self {
        self.storage = Some(StorageProbe::new(log_dir.to_path_buf()));
        self
    }

    /// The source's name.
    pub(crate) const fn name(&self) -> &'a str {
        self.name
    }

    /// Takes the first storage reading, the baseline store growth is measured from.
    pub(crate) fn start(&mut self, now: Instant) {
        if let Some(probe) = self.storage.as_mut() {
            probe.read(now);
        }
    }

    /// ` | store 1.9 GB | disk full in 412 d`, or empty without a log directory.
    pub(crate) fn storage_segment(&mut self, now: Instant) -> String {
        self.storage
            .as_mut()
            .map(|probe| format!(" | {}", render_storage(&probe.read(now))))
            .unwrap_or_default()
    }
}

/// Takes storage readings of one log directory for the progress line. The first reading is
/// the baseline growth is measured from.
#[derive(Debug)]
pub(crate) struct StorageProbe {
    log_dir: PathBuf,
    filesystem: Filesystem,
    first: Option<(Instant, u64)>,
}

impl StorageProbe {
    /// A probe of `log_dir`, classifying its filesystem once.
    pub(crate) fn new(log_dir: PathBuf) -> Self {
        let filesystem = filesystem_kind(&log_dir);
        Self {
            log_dir,
            filesystem,
            first: None,
        }
    }

    /// Reads the store's size and free space now. Growth is `None` on the first reading.
    pub(crate) fn read(&mut self, now: Instant) -> StorageReading {
        let store_bytes = store_bytes(&self.log_dir).ok();
        let growth_bytes_per_sec = match (self.first, store_bytes) {
            (Some((then, before)), Some(bytes)) => {
                let elapsed = now.duration_since(then).as_secs_f64();
                (elapsed > 0.0).then(|| (bytes as f64 - before as f64) / elapsed)
            }
            (None, Some(bytes)) => {
                self.first = Some((now, bytes));
                None
            }
            (_, None) => None,
        };
        StorageReading {
            filesystem: self.filesystem,
            store_bytes,
            growth_bytes_per_sec,
            free_bytes: free_bytes(&self.log_dir),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        Filesystem, OVERLAYFS_MAGIC, RAMFS_MAGIC, StorageReading, TMPFS_MAGIC, days_to_full,
        filesystem_kind, human_bytes, render_storage, store_bytes,
    };

    /// `EXT4_SUPER_MAGIC`.
    const EXT4_MAGIC: u32 = 0xEF53;

    fn disk(store_bytes: Option<u64>, growth: Option<f64>, free: Option<u64>) -> StorageReading {
        StorageReading {
            filesystem: Filesystem::Disk,
            store_bytes,
            growth_bytes_per_sec: growth,
            free_bytes: free,
        }
    }

    #[test]
    fn classifies_fake_statfs_magics() {
        assert_eq!(Filesystem::from_magic(Some(TMPFS_MAGIC)), Filesystem::Tmpfs);
        assert_eq!(Filesystem::from_magic(Some(RAMFS_MAGIC)), Filesystem::Tmpfs);
        assert_eq!(Filesystem::from_magic(Some(EXT4_MAGIC)), Filesystem::Disk);
        assert_eq!(
            Filesystem::from_magic(Some(OVERLAYFS_MAGIC)),
            Filesystem::Unknown
        );
    }

    #[test]
    fn a_statfs_error_is_unknown_never_disk() {
        assert_eq!(Filesystem::from_magic(None), Filesystem::Unknown);
        let missing = Path::new("/nonexistent/s2w-status-test/definitely-missing");
        assert_eq!(filesystem_kind(missing), Filesystem::Unknown);
    }

    #[test]
    fn tmpfs_prints_the_warning_and_no_days_figure() {
        let reading = StorageReading {
            filesystem: Filesystem::Tmpfs,
            store_bytes: Some(1_900_000_000),
            growth_bytes_per_sec: Some(1000.0),
            free_bytes: Some(1_000_000_000_000),
        };
        let line = render_storage(&reading);
        assert_eq!(line, "store 1.9 GB | tmpfs: NOT A DISK NUMBER");
        assert!(!line.contains(" d"), "{line}");
    }

    #[test]
    fn days_figure_needs_two_readings_and_growth() {
        assert_eq!(
            render_storage(&disk(Some(1_900_000_000), None, Some(10))),
            "store 1.9 GB | disk full in —"
        );
        assert_eq!(
            render_storage(&disk(Some(5), Some(0.0), Some(10))),
            "store 5 B | disk full in — (store not growing)"
        );
        // 412 days of free space at 1 kB/s.
        let free = 412 * 86_400 * 1000;
        assert_eq!(
            render_storage(&disk(Some(1_900_000_000), Some(1000.0), Some(free))),
            "store 1.9 GB | disk full in 412 d"
        );
        assert_eq!(
            render_storage(&disk(None, Some(1000.0), None)),
            "store ? | disk full in ?"
        );
    }

    #[test]
    fn unknown_filesystem_is_flagged() {
        let reading = StorageReading {
            filesystem: Filesystem::Unknown,
            ..disk(Some(10), None, None)
        };
        assert_eq!(
            render_storage(&reading),
            "store 10 B | filesystem unknown: no disk estimate"
        );
    }

    #[test]
    fn days_to_full_refuses_a_non_positive_rate() {
        assert_eq!(days_to_full(86_400, 1.0), Some(1.0));
        assert_eq!(days_to_full(86_400, 0.0), None);
        assert_eq!(days_to_full(86_400, -1.0), None);
        assert_eq!(days_to_full(86_400, f64::NAN), None);
    }

    #[test]
    fn human_bytes_uses_decimal_units() {
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1_000), "1.0 kB");
        assert_eq!(human_bytes(1_900_000_000), "1.9 GB");
        assert_eq!(human_bytes(2_500_000_000_000), "2.5 TB");
    }

    #[test]
    fn store_bytes_sums_every_sqlite_file() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("s2w-status-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("events.sqlite3"), [0_u8; 100])?;
        std::fs::write(dir.join("events.sqlite3-wal"), [0_u8; 20])?;
        std::fs::write(dir.join("other.sqlite3"), [0_u8; 3])?;
        std::fs::write(dir.join("notes.txt"), [0_u8; 7])?;
        std::fs::create_dir_all(dir.join("nested.sqlite3.d"))?;
        let total = store_bytes(&dir);
        std::fs::remove_dir_all(&dir)?;
        assert_eq!(total?, 123);
        Ok(())
    }
}
