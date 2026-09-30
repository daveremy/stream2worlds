//! The in-memory event log: the same seam as the SQLite log, without I/O.

use std::collections::HashMap;

use s2w_model::{Cursor, RawEvent, SourceId};

use crate::append::collision;
use crate::{
    AppendOutcome, EventLog, LogError, LogPosition, StoredEvent, check_payload_size, content_hash,
};

/// An append-only in-memory event log.
#[derive(Debug, Default)]
pub struct InMemoryEventLog {
    pub(crate) events: Vec<StoredEvent>,
    pub(crate) cursors: HashMap<SourceId, (Cursor, LogPosition)>,
    pub(crate) seen: HashMap<(SourceId, i64), LogPosition>,
}

impl InMemoryEventLog {
    /// Constructs an empty in-memory log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Classifies and, when new, stores one event; the shared body of both append paths.
    fn insert(&mut self, event: RawEvent) -> Result<AppendOutcome, LogError> {
        check_payload_size(event.payload.len())?;
        let key = (event.source.clone(), content_hash(&event.payload));
        if let Some(&position) = self.seen.get(&key) {
            // A hash hit is a duplicate only when the payload bytes match, mirroring the
            // SQLite path: a genuine hash collision is loud, never a silent drop.
            let stored = self.stored_payload(position).ok_or_else(|| {
                LogError::Corrupt(format!(
                    "content hash points at missing position {}",
                    position.as_u64()
                ))
            })?;
            if stored != event.payload.as_slice() {
                return Err(collision(event.source.as_str(), key.1, position));
            }
            // Duplicate: the cursor is untouched, so a redelivery cannot move it backwards.
            return Ok(AppendOutcome::Duplicate(position));
        }
        let next = u64::try_from(self.events.len())
            .map_err(|error| LogError::Io(error.to_string()))?
            .checked_add(1)
            .ok_or_else(|| LogError::Io("log position overflow".to_owned()))?;
        let position = LogPosition(next);
        self.cursors
            .insert(event.source.clone(), (event.cursor.clone(), position));
        let content_hash = key.1;
        self.events.push(StoredEvent {
            position,
            event,
            content_hash,
        });
        self.seen.insert(key, position);
        Ok(AppendOutcome::Inserted(position))
    }

    /// The stored payload at `position`, if it is still held.
    fn stored_payload(&self, position: LogPosition) -> Option<&[u8]> {
        let index = usize::try_from(position.as_u64().checked_sub(1)?).ok()?;
        self.events
            .get(index)
            .map(|stored| stored.event.payload.as_slice())
    }
}

impl EventLog for InMemoryEventLog {
    fn append(&mut self, event: RawEvent) -> Result<AppendOutcome, LogError> {
        self.insert(event)
    }

    fn append_batch(&mut self, events: Vec<RawEvent>) -> Result<Vec<AppendOutcome>, LogError> {
        for event in &events {
            check_payload_size(event.payload.len())?;
        }
        // All or nothing: apply to a copy and swap it in only when every event succeeded.
        let mut staged = InMemoryEventLog {
            events: self.events.clone(),
            cursors: self.cursors.clone(),
            seen: self.seen.clone(),
        };
        let outcomes = events
            .into_iter()
            .map(|event| staged.insert(event))
            .collect::<Result<Vec<_>, _>>()?;
        *self = staged;
        Ok(outcomes)
    }

    fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, LogError> {
        Ok(self.cursors.get(source).map(|(cursor, _)| cursor.clone()))
    }

    fn replay(
        &self,
        from: Option<LogPosition>,
    ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError> {
        let after = from.map_or(0, LogPosition::as_u64);
        Ok(Box::new(
            self.events
                .iter()
                .filter(move |stored| stored.position.as_u64() > after)
                .cloned()
                .map(Ok),
        ))
    }

    fn head(&self) -> Result<Option<LogPosition>, LogError> {
        Ok(self.events.last().map(|stored| stored.position))
    }
}
