//! Append throughput, one event per transaction, on the real SQLite log (s2w#32).
//!
//! A plain `harness = false` binary: `cargo bench -p s2w-app --bench scale_wall`. It writes
//! [`scale_generator::WALL_EVENTS`] generated events into a fresh `SqliteEventLog` under
//! `$CARGO_TARGET_TMPDIR/scale/` (the target directory, never the system temp directory, which
//! is often tmpfs), and prints one JSON line with the rate and the filesystem it measured on.
//! A tmpfs measurement carries a `warning` field, [`TMPFS_WARNING`], which `cargo xtask scale`
//! prints as is.
//!
//! Wall-clock numbers depend on the machine, so this one is reported, never gated.

#[path = "../tests/support/scale_generator.rs"]
#[expect(
    dead_code,
    reason = "the shared generator has items only the fold measurements use"
)]
mod scale_generator;

use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

use s2w_app::status::{TMPFS_WARNING, filesystem_kind};
use s2w_log::{EventLog, SqliteEventLog};
use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};

use scale_generator::{SEED, WALL_EVENTS, entity_events};

fn main() -> Result<(), Box<dyn Error>> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("scale");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let filesystem = filesystem_kind(&dir);

    let source = SourceId::new("scale")?;
    let mut events = Vec::with_capacity(WALL_EVENTS);
    for (index, event) in entity_events(WALL_EVENTS, SEED).iter().enumerate() {
        let position = u64::try_from(index)?;
        events.push(RawEvent {
            source: source.clone(),
            cursor: Cursor::new(position.to_be_bytes().to_vec())?,
            received_at: Timestamp::from_millis(i64::try_from(index)?),
            payload: serde_json::to_vec(event)?,
        });
    }

    let mut log = SqliteEventLog::open(&dir)?;
    let started = Instant::now();
    for event in events {
        log.append(event)?;
    }
    let elapsed = started.elapsed().as_secs_f64();
    drop(log);

    let rate = if elapsed > 0.0 {
        WALL_EVENTS as f64 / elapsed
    } else {
        0.0
    };
    let mut line = serde_json::json!({
        "append_events_per_s_tx1": (rate * 10.0).round() / 10.0,
        "events": WALL_EVENTS,
        "elapsed_s": elapsed,
        "filesystem": filesystem.label(),
        "dir": dir.display().to_string(),
    });
    if filesystem == s2w_app::status::Filesystem::Tmpfs {
        line["warning"] = TMPFS_WARNING.into();
    }
    println!("{line}");
    Ok(())
}
