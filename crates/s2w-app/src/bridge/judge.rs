//! Judging one poll batch: serve each stored verdict, evaluate what has none, and collect the
//! new rows and claims for [`Bridge::poll_once`] to commit and then serve.

use s2w_log::{LogError, LogPosition, LogReader, StoredEvent, StoredVerdict, VerdictStore};
use s2w_model::{Timestamp, WorldEvent};
use s2w_system1::{AbstainReason, Engine, Verdict};

use super::{Bridge, BridgeStats, evaluate_one};
use crate::query::SourceStats;

/// One poll batch, judged but not yet committed or served.
#[derive(Default)]
pub(super) struct Judged {
    pub(super) stats: BridgeStats,
    pub(super) per_source: std::collections::BTreeMap<s2w_model::SourceId, SourceStats>,
    pub(super) new_rows: Vec<StoredVerdict>,
    pub(super) claims: Vec<(Timestamp, WorldEvent)>,
    pub(super) through: Option<LogPosition>,
}

impl<R: LogReader, V: VerdictStore> Bridge<R, V> {
    /// At most `batch` events after `last`, and the log error that cut the read short.
    pub(super) fn read_batch(&self) -> (Vec<StoredEvent>, Option<LogError>) {
        let mut events = Vec::new();
        let items = match self.reader.read_after(self.last) {
            Ok(items) => items,
            Err(error) => return (events, Some(error)),
        };
        for item in items.take(self.config.batch) {
            match item {
                Ok(stored) => events.push(stored),
                Err(error) => return (events, Some(error)),
            }
        }
        (events, None)
    }

    /// Judges `events` in order. On an error, returns what was judged before the bad event
    /// together with the error, so the good prefix is still committed and served.
    pub(super) fn judge(&mut self, events: &[StoredEvent]) -> (Judged, Option<LogError>) {
        let mut judged = Judged::default();
        let Some(batch_end) = events.last().map(|e| e.position) else {
            return (judged, None);
        };
        // Registered names only: rows of a replaced engine are never read (decision 0023).
        let stored = match self
            .verdicts
            .read_range_of(self.last, batch_end, &self.engine_names)
        {
            Ok(stored) => stored,
            Err(error) => return (judged, Some(error)),
        };
        let mut stored = stored.as_slice();
        for event in events {
            let here = stored.partition_point(|row| row.position <= event.position);
            let (at_event, rest) = stored.split_at(here);
            if let Err(error) = self.judge_event(event, at_event, &mut judged) {
                return (judged, Some(error));
            }
            stored = rest;
            judged.through = Some(event.position);
        }
        (judged, None)
    }

    /// Judges one event into `judged`, which it leaves untouched on error. `rows` are the
    /// stored verdicts at positions after the previous event through this one.
    fn judge_event(
        &mut self,
        event: &StoredEvent,
        rows: &[StoredVerdict],
        judged: &mut Judged,
    ) -> Result<(), LogError> {
        check_rows(event, rows)?;
        let engines = self.registry.engines_for(&event.event.source);
        let mut stats = BridgeStats {
            consumed: 1,
            ..BridgeStats::default()
        };
        // Local delta, folded into `judged.per_source` only once every fallible step below has
        // succeeded — mirrors `stats` above, so a mid-event error leaves `judged` (both fields)
        // untouched, per this function's own contract.
        let mut source_stats = SourceStats {
            consumed: 1,
            ..SourceStats::default()
        };
        if engines.is_empty() {
            stats.unrouted += 1;
            source_stats.unrouted += 1;
            source_stats.push_recent_unrouted(event.clone());
            if !self.warned_unrouted.contains(&event.event.source) {
                self.warned_unrouted.insert(event.event.source.clone());
                eprintln!(
                    "s2w: bridge: no System 1 engine is routed for source '{}'; its events are skipped",
                    event.event.source.as_str()
                );
            }
        }
        let (verdicts, new_rows) = judge_engines(event, rows, &engines, &mut stats)?;
        let claims = tally(verdicts, event.event.received_at, &mut stats);
        judged.stats.add(&stats);
        judged
            .per_source
            .entry(event.event.source.clone())
            .or_default()
            .add(&source_stats);
        judged.new_rows.extend(new_rows);
        judged.claims.extend(claims);
        Ok(())
    }
}

/// Checks that every stored row in `rows` names `event`'s position and judged its content.
fn check_rows(event: &StoredEvent, rows: &[StoredVerdict]) -> Result<(), LogError> {
    if let Some(row) = rows.iter().find(|row| row.position != event.position) {
        return Err(LogError::Corrupt(format!(
            "a stored verdict names log position {}, which holds no event",
            row.position.as_u64()
        )));
    }
    if rows.iter().any(|row| row.event_hash != event.content_hash) {
        return Err(LogError::Corrupt(format!(
            "stored verdicts at log position {} judged a different event than the log holds",
            event.position.as_u64()
        )));
    }
    Ok(())
}

/// Each engine's verdict on `event`, in `engines` order: the stored one from `rows` when the
/// engine has one, otherwise a fresh evaluation, which also yields a new row to store. Counts
/// both kinds into `stats`.
fn judge_engines(
    event: &StoredEvent,
    rows: &[StoredVerdict],
    engines: &[&dyn Engine],
    stats: &mut BridgeStats,
) -> Result<(Vec<Verdict>, Vec<StoredVerdict>), LogError> {
    let at = event.position.as_u64();
    let mut verdicts = Vec::with_capacity(engines.len());
    let mut new_rows = Vec::new();
    for &engine in engines {
        // Rows at one position are in write order, so the first match is the one first
        // served (the lowest seq), whatever its version.
        if let Some(row) = rows.iter().find(|row| row.engine == engine.name()) {
            let verdict: Verdict = serde_json::from_slice(&row.verdict).map_err(|error| {
                LogError::Corrupt(format!(
                    "stored verdict of engine '{}' at log position {at} does not decode: {error}",
                    row.engine
                ))
            })?;
            stats.replayed += 1;
            if row.version != engine.version() {
                stats.replayed_stale_version += 1;
            }
            verdicts.push(verdict);
        } else {
            let record = evaluate_one(event, engine);
            stats.evaluated += 1;
            if let Verdict::Abstain {
                reason: AbstainReason::Panicked(message),
            } = &record.verdict
            {
                eprintln!(
                    "s2w: bridge: engine '{}' panicked at log position {at}: {message}",
                    record.engine
                );
            }
            new_rows.push(record.to_stored(event.content_hash)?);
            verdicts.push(record.verdict);
        }
    }
    Ok((verdicts, new_rows))
}

/// The claims `verdicts` propose, each stamped `received`, counting every outcome into `stats`.
fn tally(
    verdicts: Vec<Verdict>,
    received: Timestamp,
    stats: &mut BridgeStats,
) -> Vec<(Timestamp, WorldEvent)> {
    let mut claims = Vec::new();
    for verdict in verdicts {
        match verdict {
            Verdict::Propose {
                claims: proposed, ..
            } => {
                if proposed.is_empty() {
                    stats.proposed_empty += 1;
                }
                stats.proposed_claims += proposed.len() as u64;
                claims.extend(proposed.into_iter().map(|claim| (received, claim)));
            }
            Verdict::Abstain { reason } => match reason {
                AbstainReason::NotMine => stats.abstained.not_mine += 1,
                AbstainReason::Unparseable(_) => stats.abstained.unparseable += 1,
                AbstainReason::Insufficient(_) => stats.abstained.insufficient += 1,
                AbstainReason::Panicked(_) => stats.engine_panics += 1,
                AbstainReason::BelowThreshold { .. } => stats.abstained.below_threshold += 1,
                AbstainReason::Ambiguous { .. } => stats.abstained.ambiguous += 1,
            },
        }
    }
    claims
}
