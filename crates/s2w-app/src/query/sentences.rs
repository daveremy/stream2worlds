//! `GET /worlds/{world}/sentences?last=N` and MCP `sentences` (s2w#302): the last N logged
//! events of the world's member sources, each as the effective dashboard manifest's sentence
//! (decision 0029) and the entities the source's effective mapping observes in it.
//!
//! It reads the log itself, never the fold: a row is what a source said, rendered. The head
//! world only fills in which entity a key names today.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use s2w_log::{LogError, LogPosition, LogReader, ReadOnlySqliteEventLog, StoredEvent, members_at};
use s2w_model::{SourceId, WorldEvent, sentence_for};
use s2w_system1::{Engine, MappingEngine, Verdict};
use serde::Serialize;
use serde_json::Value;

use super::QueryError;

/// The largest `last=`: a sentence list is a feed's seed, not a replay.
pub const MAX_SENTENCES: u64 = 200;

/// The answer: newest last.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SentencesView {
    /// One row per event, oldest first.
    pub rows: Vec<SentenceRow>,
}

/// One logged event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SentenceRow {
    /// The event's log position.
    pub position: u64,
    /// The source id.
    pub source: String,
    /// The manifest's sentence for the event; `None` when no manifest is in effect or no
    /// sentence of the source renders on this payload.
    pub sentence: Option<String>,
    /// The entities the source's effective mapping observes in the event, in claim order,
    /// each once; empty for an unrouted source.
    pub entities: Vec<SentenceEntity>,
}

/// One entity an event names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SentenceEntity {
    /// The entity type the mapping claims.
    #[serde(rename = "type")]
    pub entity_type: String,
    /// The natural key, as the world's node keys show it.
    pub key: String,
    /// The head world's entity id for the key (merges followed); absent when the head world
    /// does not hold the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<u64>,
}

/// Checks `last` against `1..=MAX_SENTENCES`.
///
/// # Errors
/// [`QueryError::BadParameter`] outside the range.
pub fn check_last(last: u64) -> Result<usize, QueryError> {
    if (1..=MAX_SENTENCES).contains(&last) {
        return usize::try_from(last).map_err(|e| QueryError::BadParameter {
            name: "last",
            reason: e.to_string(),
        });
    }
    Err(QueryError::BadParameter {
        name: "last",
        reason: format!("'{last}': must be from 1 to {MAX_SENTENCES}"),
    })
}

/// The last `last` events of `world`'s member sources in `log_dir`, rendered, with every
/// `entity` unset. With no membership rows every routed source is a member.
///
/// # Errors
/// [`QueryError::Storage`] if the log or the proposal store cannot be read.
pub fn read_sentences(
    log_dir: &Path,
    world: &str,
    last: usize,
) -> Result<SentencesView, QueryError> {
    let routes = crate::routes::load(log_dir)
        .map_err(|error| QueryError::Storage(error.to_string()))?
        .routes;
    let log = ReadOnlySqliteEventLog::open(log_dir)?;
    let history = log.membership_history()?;
    let targets: BTreeSet<SourceId> = if history.is_empty() {
        routes.keys().cloned().collect()
    } else {
        members_at(&history, u64::MAX).into_iter().collect()
    };
    let tail = read_last(&log, &targets, last)?;
    let events = super::read_dashboard(log_dir, world)?
        .manifest
        .and_then(|manifest| manifest.events)
        .unwrap_or_default();
    // A mapping the engine refuses routes nothing in `serve` either, so it observes nothing.
    let engines: BTreeMap<&SourceId, MappingEngine> = routes
        .iter()
        .filter(|(source, _)| targets.contains(*source))
        .filter_map(|(source, r)| Some((source, MappingEngine::new(r.mapping.clone()).ok()?)))
        .collect();
    let rows = tail
        .into_iter()
        .map(|stored| {
            let route = routes.get(&stored.event.source);
            let engine = engines.get(&stored.event.source);
            let payload = route.and_then(|r| decoded(&stored.event.payload, &r.mapping.decode));
            let source = stored.event.source.as_str();
            SentenceRow {
                position: stored.position.as_u64(),
                source: source.to_owned(),
                sentence: payload
                    .as_ref()
                    .and_then(|payload| sentence_for(&events, source, payload)),
                entities: engine
                    .map(|engine| observed(engine, &stored))
                    .unwrap_or_default(),
            }
        })
        .collect();
    Ok(SentencesView { rows })
}

/// The payload as JSON with the mapping's decode paths parsed in place; a decode path that is
/// absent or does not hold JSON text is left as it is.
fn decoded(payload: &[u8], decode: &[s2w_model::FieldPath]) -> Option<Value> {
    let mut value: Value = serde_json::from_slice(payload).ok()?;
    for path in decode {
        let _ = s2w_system1::decode::decode_path(&mut value, path);
    }
    Some(value)
}

/// The entities the engine observes in the event, in claim order, each (type, key) once.
fn observed(engine: &MappingEngine, stored: &StoredEvent) -> Vec<SentenceEntity> {
    let Verdict::Propose { claims, .. } = engine.evaluate(&stored.event) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    claims
        .into_iter()
        .filter_map(|claim| match claim {
            WorldEvent::EntityObserved {
                key, entity_type, ..
            } => Some((entity_type, key.as_str().to_owned())),
            _ => None,
        })
        .filter(|pair| seen.insert(pair.clone()))
        .map(|(entity_type, key)| SentenceEntity {
            entity_type,
            key,
            entity: None,
        })
        .collect()
}

/// The newest `n` events of the sources in `targets` together, oldest first. It reads a window
/// of the log's end, `2n` positions at first, and doubles it until the window holds `n` target
/// events or covers the whole log, so it stops at `n` events in total, never `n` per source.
/// Events appended after the head read are ignored.
///
/// # Errors
/// Any [`LogError`] from the log.
pub fn read_last(
    log: &impl LogReader,
    targets: &BTreeSet<SourceId>,
    n: usize,
) -> Result<Vec<StoredEvent>, LogError> {
    let Some(head) = log.read_head()? else {
        return Ok(Vec::new());
    };
    if targets.is_empty() || n == 0 {
        return Ok(Vec::new());
    }
    let head = head.as_u64();
    let mut window = u64::try_from(n).unwrap_or(u64::MAX).saturating_mul(2);
    loop {
        let start = if head <= window {
            None
        } else {
            LogPosition::from_u64(head - window)
        };
        let mut tail: VecDeque<StoredEvent> = VecDeque::with_capacity(n + 1);
        for stored in log.read_after(start)? {
            let stored = stored?;
            if stored.position.as_u64() > head {
                break;
            }
            if !targets.contains(&stored.event.source) {
                continue;
            }
            tail.push_back(stored);
            if tail.len() > n {
                tail.pop_front();
            }
        }
        if start.is_none() || tail.len() >= n {
            return Ok(tail.into());
        }
        window = window.saturating_mul(2);
    }
}
