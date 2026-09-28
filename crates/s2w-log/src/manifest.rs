//! Immutable world identity stored alongside membership and events.
use crate::{LogError, ReadOnlySqliteEventLog, SqliteEventLog, map_sqlite};
use rusqlite::{Connection, OptionalExtension, params};
use s2w_model::Timestamp;

/// Persistent world identity and its original engine and policy sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldManifest {
    /// Configured world identifier.
    pub world: String,
    /// Human-readable name.
    pub name: String,
    /// Creation time supplied by the application.
    pub created_at: Timestamp,
    /// Original engine identifiers, in sorted order.
    pub engines: Vec<String>,
    /// Original policy identifiers, in sorted order.
    pub policies: Vec<String>,
}
impl WorldManifest {
    /// Loads the directory's manifest, refusing reuse under another world identifier.
    pub fn load(log: &SqliteEventLog, world: &str) -> Result<Option<Self>, LogError> {
        load_from(&log.connection, world)
    }
    /// Creates identity once; subsequent opens return the original metadata unchanged.
    pub fn create_if_absent(
        log: &mut SqliteEventLog,
        world: &str,
        name: &str,
        now: Timestamp,
        engines: &[String],
        policies: &[String],
    ) -> Result<Self, LogError> {
        if let Some(manifest) = Self::load(log, world)? {
            return Ok(manifest);
        }
        let tx = log
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        tx.execute(
            "INSERT INTO world_manifest VALUES (?1,?2,?3)",
            params![world, name, now.as_millis()],
        )
        .map_err(map_sqlite)?;
        for (table, values) in [
            ("world_manifest_engines", engines),
            ("world_manifest_policies", policies),
        ] {
            let unique: std::collections::BTreeSet<_> = values.iter().collect();
            for value in unique {
                tx.execute(
                    &format!("INSERT INTO {table} VALUES (?1,?2)"),
                    params![world, value],
                )
                .map_err(map_sqlite)?;
            }
        }
        tx.commit().map_err(map_sqlite)?;
        Self::load(log, world)?.ok_or_else(|| LogError::Corrupt("created manifest missing".into()))
    }
}

impl ReadOnlySqliteEventLog {
    /// Loads the directory's manifest without taking the writer lock.
    pub fn world_manifest(&self, world: &str) -> Result<Option<WorldManifest>, LogError> {
        load_from(&self.connection, world)
    }
}

fn load_from(connection: &Connection, world: &str) -> Result<Option<WorldManifest>, LogError> {
    let row = connection
        .query_row(
            "SELECT world,name,created_at FROM world_manifest LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite)?;
    let Some((stored, name, created_at)) = row else {
        return Ok(None);
    };
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM world_manifest", [], |r| r.get(0))
        .map_err(map_sqlite)?;
    if stored != world || count != 1 {
        return Err(LogError::Corrupt(format!(
            "manifest world {stored:?} disagrees with configured world {world:?}"
        )));
    }
    let load_set = |table: &str, column: &str| -> Result<Vec<String>, LogError> {
        connection
            .prepare(&format!(
                "SELECT {column} FROM {table} WHERE world=?1 ORDER BY {column}"
            ))
            .map_err(map_sqlite)?
            .query_map([world], |r| r.get(0))
            .map_err(map_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite)
    };
    Ok(Some(WorldManifest {
        world: stored,
        name,
        created_at: Timestamp::from_millis(created_at),
        engines: load_set("world_manifest_engines", "engine")?,
        policies: load_set("world_manifest_policies", "policy")?,
    }))
}
