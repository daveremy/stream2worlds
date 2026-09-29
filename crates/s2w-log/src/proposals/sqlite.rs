//! SQLite storage, using the event/verdict store's connection and locking helpers.

use std::fmt;
use std::fs::File;
use std::path::Path;

use rusqlite::{Connection, params};

use super::{
    Actor, Decider, NewDecision, NewProposal, Outcome, ProposalStore, ProposalSummary,
    StoredDecision, StoredProposal, check_integrity, retry_proposal, stored_decision,
    stored_proposal, validate_decision, validate_proposal,
};
use crate::{
    LogError, LogPosition, map_constraint, map_sqlite, open_sqlite_store,
    open_sqlite_store_read_only,
};

/// The proposal database file name inside a log directory.
pub const PROPOSAL_DATABASE_FILE: &str = "proposals.sqlite3";
const LOCK_FILE: &str = "PROPOSALS_LOCK";
/// Version 2 adds the `agent` decider. A version-1 store is unsupported (`Corrupt`), like any
/// other mismatch: no producer ever wrote one, so there is no migration.
const SCHEMA_VERSION: i64 = 2;

/// Durable append-only proposals and decisions, independent of event/verdict writer locks.
pub struct SqliteProposalStore {
    connection: Connection,
    _lock: File,
}

/// Lockless reader of an initialized proposal database; has no write methods.
pub struct ReadOnlySqliteProposalStore {
    connection: Connection,
}

impl fmt::Debug for SqliteProposalStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SqliteProposalStore")
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for ReadOnlySqliteProposalStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadOnlySqliteProposalStore")
            .finish_non_exhaustive()
    }
}

fn schema_mismatch(version: i64) -> String {
    format!("unsupported proposal store schema version {version}; expected {SCHEMA_VERSION}")
}

impl SqliteProposalStore {
    /// Opens or creates `proposals.sqlite3` with its own writer lock, WAL and FULL durability.
    ///
    /// # Errors
    /// Returns `Locked` for another writer, `Corrupt` for unsupported schema, or storage errors.
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, LogError> {
        let (connection, lock) = open_sqlite_store(
            directory.as_ref(),
            PROPOSAL_DATABASE_FILE,
            LOCK_FILE,
            "PRAGMA synchronous = FULL;
             PRAGMA recursive_triggers = ON;
             PRAGMA foreign_keys = ON;",
            SCHEMA_VERSION,
            schema_mismatch,
            initialize_schema,
        )?;
        Ok(Self {
            connection,
            _lock: lock,
        })
    }
}

impl ReadOnlySqliteProposalStore {
    /// Opens an existing store without acquiring the writer lock or changing the schema.
    ///
    /// # Errors
    /// Returns storage errors or `Corrupt` for an absent/unsupported schema.
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, LogError> {
        let connection = open_sqlite_store_read_only(
            directory.as_ref(),
            PROPOSAL_DATABASE_FILE,
            SCHEMA_VERSION,
            schema_mismatch,
        )?;
        Ok(Self { connection })
    }

    /// Reads proposals in sequence order, checking payload integrity.
    ///
    /// # Errors
    /// Returns storage errors or `Corrupt` for an unknown actor or payload hash mismatch.
    pub fn proposals(&self) -> Result<Vec<StoredProposal>, LogError> {
        proposals_from(&self.connection)
    }

    /// Reads proposals in sequence order without payload bytes; the payload hash is the
    /// stored value and is NOT recomputed (only [`Self::proposals`] verifies it).
    ///
    /// # Errors
    /// Returns storage errors or `Corrupt` for an unknown actor.
    pub fn proposal_summaries(&self) -> Result<Vec<ProposalSummary>, LogError> {
        summaries_from(&self.connection)
    }

    /// Whether a proposal with this id is stored.
    ///
    /// # Errors
    /// Returns storage errors.
    pub fn has_proposal(&self, id: &str) -> Result<bool, LogError> {
        self.connection
            .prepare("SELECT 1 FROM proposals WHERE id = ?1")
            .and_then(|mut statement| statement.exists([id]))
            .map_err(map_sqlite)
    }

    /// Reads decisions in sequence order.
    ///
    /// # Errors
    /// Returns storage errors or `Corrupt` for unknown decider/outcome strings.
    pub fn decisions(&self) -> Result<Vec<StoredDecision>, LogError> {
        decisions_from(&self.connection)
    }
}

impl ProposalStore for SqliteProposalStore {
    fn append_proposal(&mut self, proposal: &NewProposal) -> Result<StoredProposal, LogError> {
        validate_proposal(proposal)?;
        if let Some(stored) = proposal_by_id(&self.connection, &proposal.id)? {
            return retry_proposal(stored, proposal);
        }
        let mut stored = stored_proposal(proposal, 0);
        let (kind, id, model, version) = match &proposal.actor {
            Actor::Human { id } => ("human", Some(id), None, None),
            Actor::Agent { model, version } => ("agent", None, Some(model), Some(version)),
        };
        self.connection
            .execute(
                "INSERT INTO proposals (id, class, actor_kind, actor_id, model, model_version,
             snapshot_offset, payload_hash, payload, proposed_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    proposal.id,
                    proposal.class,
                    kind,
                    id,
                    model,
                    version,
                    proposal.snapshot_offset.to_sql()?,
                    stored.payload_hash,
                    proposal.payload,
                    proposal.proposed_at_ms
                ],
            )
            .map_err(map_write_error)?;
        stored.seq = self.connection.last_insert_rowid();
        Ok(stored)
    }

    fn append_decision(&mut self, decision: &NewDecision) -> Result<StoredDecision, LogError> {
        validate_decision(decision)?;
        self.connection
            .execute(
                "INSERT INTO decisions (proposal_id, decider, outcome, basis, decided_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    decision.proposal_id,
                    decision.decider.as_str(),
                    decision.outcome.as_str(),
                    decision.basis,
                    decision.decided_at_ms
                ],
            )
            .map_err(map_write_error)?;
        Ok(stored_decision(
            decision,
            self.connection.last_insert_rowid(),
        ))
    }

    fn proposals(&self) -> Result<Vec<StoredProposal>, LogError> {
        proposals_from(&self.connection)
    }

    fn proposal_summaries(&self) -> Result<Vec<ProposalSummary>, LogError> {
        summaries_from(&self.connection)
    }

    fn decisions(&self) -> Result<Vec<StoredDecision>, LogError> {
        decisions_from(&self.connection)
    }
}

fn map_write_error(error: rusqlite::Error) -> LogError {
    let message = error.to_string();
    map_constraint(error, || LogError::Corrupt(message))
}

const PROPOSAL_COLUMNS: &str = "seq, id, class, actor_kind, actor_id, model, model_version,
    snapshot_offset, payload_hash, payload, proposed_at_ms";

fn proposal_by_id(connection: &Connection, id: &str) -> Result<Option<StoredProposal>, LogError> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {PROPOSAL_COLUMNS} FROM proposals WHERE id = ?1"
        ))
        .map_err(map_sqlite)?;
    let mut rows = statement.query([id]).map_err(map_sqlite)?;
    rows.next()
        .map_err(map_sqlite)?
        .map(decode_proposal)
        .transpose()
}

fn proposals_from(connection: &Connection) -> Result<Vec<StoredProposal>, LogError> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {PROPOSAL_COLUMNS} FROM proposals ORDER BY seq"
        ))
        .map_err(map_sqlite)?;
    let mut rows = statement.query([]).map_err(map_sqlite)?;
    let mut proposals = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        proposals.push(decode_proposal(row)?);
    }
    Ok(proposals)
}

/// Same column order as [`PROPOSAL_COLUMNS`] minus `payload`, so actor indices match.
const SUMMARY_COLUMNS: &str = "seq, id, class, actor_kind, actor_id, model, model_version,
    snapshot_offset, payload_hash, proposed_at_ms";

fn summaries_from(connection: &Connection) -> Result<Vec<ProposalSummary>, LogError> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM proposals ORDER BY seq"
        ))
        .map_err(map_sqlite)?;
    let mut rows = statement.query([]).map_err(map_sqlite)?;
    let mut summaries = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        summaries.push(ProposalSummary {
            seq: row.get(0).map_err(map_sqlite)?,
            id: row.get(1).map_err(map_sqlite)?,
            class: row.get(2).map_err(map_sqlite)?,
            actor: decode_actor(row)?,
            snapshot_offset: LogPosition::from_sql(row.get(7).map_err(map_sqlite)?)?,
            payload_hash: row.get(8).map_err(map_sqlite)?,
            proposed_at_ms: row.get(9).map_err(map_sqlite)?,
        });
    }
    Ok(summaries)
}

fn decode_actor(row: &rusqlite::Row<'_>) -> Result<Actor, LogError> {
    let kind: String = row.get(3).map_err(map_sqlite)?;
    let id: Option<String> = row.get(4).map_err(map_sqlite)?;
    let model: Option<String> = row.get(5).map_err(map_sqlite)?;
    let version: Option<String> = row.get(6).map_err(map_sqlite)?;
    match (kind.as_str(), id, model, version) {
        ("human", Some(id), None, None) => Ok(Actor::Human { id }),
        ("agent", None, Some(model), Some(version)) => Ok(Actor::Agent { model, version }),
        _ => Err(LogError::Corrupt(format!("invalid proposal actor {kind}"))),
    }
}

fn decode_proposal(row: &rusqlite::Row<'_>) -> Result<StoredProposal, LogError> {
    let proposal = StoredProposal {
        seq: row.get(0).map_err(map_sqlite)?,
        id: row.get(1).map_err(map_sqlite)?,
        class: row.get(2).map_err(map_sqlite)?,
        actor: decode_actor(row)?,
        snapshot_offset: LogPosition::from_sql(row.get(7).map_err(map_sqlite)?)?,
        payload_hash: row.get(8).map_err(map_sqlite)?,
        payload: row.get(9).map_err(map_sqlite)?,
        proposed_at_ms: row.get(10).map_err(map_sqlite)?,
    };
    check_integrity(&proposal)?;
    Ok(proposal)
}

fn decisions_from(connection: &Connection) -> Result<Vec<StoredDecision>, LogError> {
    let mut statement = connection.prepare(
        "SELECT seq, proposal_id, decider, outcome, basis, decided_at_ms FROM decisions ORDER BY seq"
    ).map_err(map_sqlite)?;
    let mut rows = statement.query([]).map_err(map_sqlite)?;
    let mut decisions = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        let decider: String = row.get(2).map_err(map_sqlite)?;
        let outcome: String = row.get(3).map_err(map_sqlite)?;
        decisions.push(StoredDecision {
            seq: row.get(0).map_err(map_sqlite)?,
            proposal_id: row.get(1).map_err(map_sqlite)?,
            decider: decode_decider(&decider)?,
            outcome: decode_outcome(&outcome)?,
            basis: row.get(4).map_err(map_sqlite)?,
            decided_at_ms: row.get(5).map_err(map_sqlite)?,
        });
    }
    Ok(decisions)
}

fn decode_decider(value: &str) -> Result<Decider, LogError> {
    match value {
        "policy" => Ok(Decider::Policy),
        "human" => Ok(Decider::Human),
        "evidence" => Ok(Decider::Evidence),
        "agent" => Ok(Decider::Agent),
        _ => Err(LogError::Corrupt(format!(
            "unknown proposal decider {value}"
        ))),
    }
}

fn decode_outcome(value: &str) -> Result<Outcome, LogError> {
    match value {
        "accept" => Ok(Outcome::Accept),
        "reject" => Ok(Outcome::Reject),
        _ => Err(LogError::Corrupt(format!(
            "unknown proposal outcome {value}"
        ))),
    }
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS proposals (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT UNIQUE NOT NULL,
        class TEXT NOT NULL,
        actor_kind TEXT NOT NULL CHECK (actor_kind IN ('human', 'agent')),
        actor_id TEXT,
        model TEXT,
        model_version TEXT,
        snapshot_offset INTEGER NOT NULL,
        payload_hash INTEGER NOT NULL,
        payload BLOB NOT NULL,
        proposed_at_ms INTEGER NOT NULL,
        CHECK ((actor_kind = 'human' AND actor_id IS NOT NULL
                AND model IS NULL AND model_version IS NULL)
            OR (actor_kind = 'agent' AND actor_id IS NULL
                AND model IS NOT NULL AND model_version IS NOT NULL))
    );
    CREATE TABLE IF NOT EXISTS decisions (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        proposal_id TEXT NOT NULL REFERENCES proposals(id),
        decider TEXT NOT NULL CHECK (decider IN ('policy', 'human', 'evidence', 'agent')),
        outcome TEXT NOT NULL CHECK (outcome IN ('accept', 'reject')),
        basis TEXT NOT NULL,
        decided_at_ms INTEGER NOT NULL
    );
    CREATE TRIGGER IF NOT EXISTS proposals_no_update BEFORE UPDATE ON proposals BEGIN
        SELECT RAISE(ABORT, 'proposals are append-only: update refused');
    END;
    CREATE TRIGGER IF NOT EXISTS proposals_no_delete BEFORE DELETE ON proposals BEGIN
        SELECT RAISE(ABORT, 'proposals are append-only: delete refused');
    END;
    CREATE TRIGGER IF NOT EXISTS decisions_no_update BEFORE UPDATE ON decisions BEGIN
        SELECT RAISE(ABORT, 'decisions are append-only: update refused');
    END;
    CREATE TRIGGER IF NOT EXISTS decisions_no_delete BEFORE DELETE ON decisions BEGIN
        SELECT RAISE(ABORT, 'decisions are append-only: delete refused');
    END;
    PRAGMA user_version = 2;";

fn initialize_schema(connection: &mut Connection) -> Result<(), LogError> {
    let transaction = connection.transaction().map_err(map_sqlite)?;
    transaction.execute_batch(SCHEMA).map_err(map_sqlite)?;
    transaction.commit().map_err(map_sqlite)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TestDirectory, retry_until_unlocked};

    #[test]
    fn pragmas_apply_on_every_open() -> Result<(), Box<dyn std::error::Error>> {
        let directory = TestDirectory::new("proposal-pragmas")?;
        for _ in 0..2 {
            let store = retry_until_unlocked(|| SqliteProposalStore::open(directory.path()))?;
            let journal: String = store
                .connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
            assert_eq!(journal.to_ascii_lowercase(), "wal");
            for (pragma, expected) in [
                ("synchronous", 2),
                ("recursive_triggers", 1),
                ("foreign_keys", 1),
                ("user_version", SCHEMA_VERSION),
            ] {
                let value: i64 =
                    store
                        .connection
                        .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))?;
                assert_eq!(value, expected, "{pragma}");
            }
        }
        Ok(())
    }
}
