//! Group commit: buffer source events and write them with one `append_batch` per flush.
//!
//! A flush happens when [`MAX_BATCH`] events are buffered, [`MAX_DELAY`] after the first
//! buffered event, at the end of the stream, and before any error is returned. A crash loses at
//! most the unflushed buffer; Kafka re-fetches it from the stored per-partition cursors, stdin
//! cannot replay it.

use std::cell::Cell;
use std::time::Duration;

use s2w_log::{AppendOutcome, EventLog, LogPosition};
use s2w_model::RawEvent;
use s2w_sources::source::{EventStream, SourceError};
use tokio::time::{Instant, MissedTickBehavior};
use tokio_stream::{Stream, StreamExt};

use crate::AppError;

/// The most events one flush writes.
pub(crate) const MAX_BATCH: usize = 100;

/// The longest an event waits in the buffer before a flush.
pub(crate) const MAX_DELAY: Duration = Duration::from_millis(50);

/// How often the progress line prints on stderr while a source is healthy (s2w#87: a running
/// `watch` or `serve` prints nothing else, which reads as a hang to a viewer). Human mode only
/// (s2w#79): `--json` reports every flush instead, so a second timer would be redundant noise.
pub(crate) const PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

/// Where the pump reports progress and errors, split out from [`pump`] so `s2w` can choose
/// human text (the long-standing default) or NDJSON (`--json`, s2w#79) without a second pump
/// implementation.
pub(crate) trait Reporter: Send {
    /// A batch was written to the log. `appended` and `duplicates` are running totals since the
    /// pump started; `cursor` is the last event's cursor, lossily decoded, once any event has
    /// been logged.
    fn flushed(&mut self, appended: u64, duplicates: u64, cursor: Option<&str>);
    /// One duplicate was collapsed at this log position (folded into the next [`Self::flushed`]
    /// call's `duplicates` total, not necessarily its own line).
    fn duplicate(&mut self, position: u64);
    /// A start note, or a non-fatal source error already reported — the pump continues past
    /// it. `retry` marks [`SourceError::Retrying`]: a transient failure retried from the same
    /// position, counted toward [`Self::flushed`]'s next reconnect total. (The one fatal error
    /// that stops the pump is rendered by the caller, not through this trait — see
    /// `s2w::run_watch`.)
    fn note(&mut self, message: &str, retry: bool);
    /// Whether the periodic human rate line ([`report_progress`]) should run alongside this
    /// reporter. Human: yes, so a quiet source doesn't read as a hang. Json: no — `flushed`
    /// already reports every batch, and a plain-text line would break an NDJSON stderr reader.
    fn wants_ticker(&self) -> bool {
        true
    }
}

/// Prints the human-readable lines `watch` has always printed.
#[derive(Default)]
pub(crate) struct HumanReporter;

impl Reporter for HumanReporter {
    fn flushed(&mut self, _appended: u64, _duplicates: u64, _cursor: Option<&str>) {
        // The periodic ticker (`report_progress`) owns the human progress line; a per-flush
        // line here would be far chattier than the 5s cadence readers are used to.
    }

    fn duplicate(&mut self, position: u64) {
        eprintln!("s2w: duplicate event collapsed at log position {position}");
    }

    fn note(&mut self, message: &str, _retry: bool) {
        eprintln!("s2w: {message}");
    }
}

/// Prints NDJSON (s2w#79): one progress object per flush on stdout, one
/// `{"error": "…", "fatal": bool}` object per note or source error on stderr.
#[derive(Default)]
pub(crate) struct JsonReporter {
    /// Running total of [`SourceError::Retrying`] reports, surfaced in every `flushed` line.
    reconnects: u64,
}

impl Reporter for JsonReporter {
    fn flushed(&mut self, appended: u64, duplicates: u64, cursor: Option<&str>) {
        let line = serde_json::json!({
            "appended": appended,
            "duplicates": duplicates,
            "reconnects": self.reconnects,
            "cursor": cursor,
            "at": now_millis(),
        });
        println!("{line}");
    }

    fn duplicate(&mut self, _position: u64) {
        // Folded into the next `flushed` call's `duplicates` total instead of its own line —
        // the log position isn't part of the sketched shape and would need its own field.
    }

    fn note(&mut self, message: &str, retry: bool) {
        if retry {
            self.reconnects += 1;
        }
        let line = serde_json::json!({ "error": message, "fatal": false });
        eprintln!("{line}");
    }

    fn wants_ticker(&self) -> bool {
        false
    }
}

/// Milliseconds since the Unix epoch, for a progress line's `at` field.
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

/// Consumes `source` into `log` with group commit until the stream ends.
///
/// `convert` turns a source item into a log event; its error stops the pump. `on_error` sees
/// every source error: `Ok((message, retry))` for one that was reported and skipped (`retry`
/// marks a transient failure retried from the same position), or the error that stops the pump.
/// The buffer is flushed, and `report` told about it, before any error is returned.
///
/// # Errors
///
/// Returns the first error from the log, `convert` or `on_error`.
pub(crate) async fn pump<L, S, T, E>(
    log: &mut L,
    mut source: S,
    mut convert: impl FnMut(T) -> Result<RawEvent, AppError>,
    mut on_error: impl FnMut(E) -> Result<(String, bool), AppError>,
    report: &mut dyn Reporter,
) -> Result<(), AppError>
where
    L: EventLog,
    S: Stream<Item = Result<T, E>> + Unpin,
{
    let mut buffer: Vec<RawEvent> = Vec::with_capacity(MAX_BATCH);
    let mut deadline: Option<Instant> = None;
    let mut appended: u64 = 0;
    let mut duplicates: u64 = 0;
    let mut last_cursor: Option<String> = None;
    loop {
        let next = match deadline {
            Some(at) => {
                if let Ok(next) = tokio::time::timeout_at(at, source.next()).await {
                    next
                } else {
                    flush_and_report(
                        log,
                        &mut buffer,
                        &mut appended,
                        &mut duplicates,
                        &last_cursor,
                        report,
                    )?;
                    deadline = None;
                    continue;
                }
            }
            None => source.next().await,
        };
        let outcome = match next {
            None => {
                flush_and_report(
                    log,
                    &mut buffer,
                    &mut appended,
                    &mut duplicates,
                    &last_cursor,
                    report,
                )?;
                return Ok(());
            }
            Some(Ok(item)) => convert(item).map(|event| {
                if buffer.is_empty() {
                    deadline = Some(Instant::now() + MAX_DELAY);
                }
                last_cursor = Some(String::from_utf8_lossy(event.cursor.as_bytes()).into_owned());
                buffer.push(event);
            }),
            Some(Err(error)) => on_error(error).map(|(message, retry)| {
                report.note(&message, retry);
            }),
        };
        if let Err(error) = outcome {
            flush_and_report(
                log,
                &mut buffer,
                &mut appended,
                &mut duplicates,
                &last_cursor,
                report,
            )?;
            return Err(error);
        }
        if buffer.len() >= MAX_BATCH {
            flush_and_report(
                log,
                &mut buffer,
                &mut appended,
                &mut duplicates,
                &last_cursor,
                report,
            )?;
            deadline = None;
        }
    }
}

/// Consumes a started source's stream into `log` with group commit until it ends, reporting
/// progress and errors through `report` (human lines, or `--json`'s NDJSON, s2w#79).
///
/// A [`SourceError::Skipped`] item is reported and the stream continues; a
/// [`SourceError::Retrying`] one is reported as a reconnect and the stream continues; any other
/// error stops the pump after the buffer is flushed.
///
/// # Errors
///
/// Returns the first error from the log or the first fatal source error.
pub(crate) async fn pump_events<L: EventLog>(
    log: &mut L,
    stream: EventStream,
    name: &str,
    report: &mut dyn Reporter,
) -> Result<(), AppError> {
    // `pump_future` and `report_progress` are only ever joined here with `select!`, never
    // spawned onto another task, so a plain borrow (no `Rc`, no `Send` bound) is enough — the
    // borrow checker itself is the proof that both stay on this one task.
    let total = Cell::new(0_u64);
    let last_event_at: Cell<Option<Instant>> = Cell::new(None);
    let convert = |event: RawEvent| {
        total.set(total.get() + 1);
        last_event_at.set(Some(Instant::now()));
        Ok(event)
    };
    let on_error = |error: SourceError| {
        if error.is_fatal() {
            Err(AppError::Source(error))
        } else {
            let retry = matches!(error, SourceError::Retrying { .. });
            Ok((error.to_string(), retry))
        }
    };
    if report.wants_ticker() {
        tokio::select! {
            result = pump(log, stream, convert, on_error, report) => result,
            // report_progress never returns, so `never` can never be constructed; this is the
            // exhaustive match for an empty type, not a fallback branch.
            never = report_progress(name, &total, &last_event_at) => match never {},
        }
    } else {
        pump(log, stream, convert, on_error, report).await
    }
}

/// Prints `name`'s throughput, running total and time since the last event roughly every
/// [`PROGRESS_INTERVAL`], forever — the caller races it against the pump and drops it once the
/// pump finishes. Human mode only; see [`Reporter::wants_ticker`].
async fn report_progress(
    name: &str,
    total: &Cell<u64>,
    last_event_at: &Cell<Option<Instant>>,
) -> ! {
    let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // the first tick fires immediately; nothing to report yet
    let mut previous = total.get();
    let mut previous_tick_at = Instant::now();
    loop {
        ticker.tick().await;
        let now = Instant::now();
        // The real gap since the last tick, not the nominal interval: a slow synchronous
        // flush can delay a tick past PROGRESS_INTERVAL, and dividing by the nominal value
        // would then overstate the rate.
        let elapsed = now.duration_since(previous_tick_at).as_secs_f64();
        previous_tick_at = now;
        let current = total.get();
        let rate = current.saturating_sub(previous) as f64 / elapsed;
        previous = current;
        match last_event_at.get() {
            Some(at) => eprintln!(
                "s2w: {name}: {rate:.1} events/s, {current} total, last event {:.1?} ago",
                at.elapsed()
            ),
            None => eprintln!("s2w: {name}: {rate:.1} events/s, {current} total, no events yet"),
        }
    }
}

/// Flushes the buffer, if there's anything in it, and reports the batch's duplicates and the
/// running totals so far.
fn flush_and_report<L: EventLog>(
    log: &mut L,
    buffer: &mut Vec<RawEvent>,
    appended: &mut u64,
    duplicates: &mut u64,
    last_cursor: &Option<String>,
    report: &mut dyn Reporter,
) -> Result<(), AppError> {
    if buffer.is_empty() {
        return Ok(());
    }
    let (inserted, duplicate_positions) = flush(log, buffer)?;
    *appended += inserted;
    *duplicates += duplicate_positions.len() as u64;
    for position in duplicate_positions {
        report.duplicate(position.as_u64());
    }
    report.flushed(*appended, *duplicates, last_cursor.as_deref());
    Ok(())
}

/// Writes the buffer as one batch. Returns how many events were newly inserted and the log
/// positions of any duplicates collapsed among them; the buffer is always empty on return.
fn flush<L: EventLog>(
    log: &mut L,
    buffer: &mut Vec<RawEvent>,
) -> Result<(u64, Vec<LogPosition>), AppError> {
    let events = std::mem::replace(buffer, Vec::with_capacity(MAX_BATCH));
    let mut inserted = 0_u64;
    let mut duplicates = Vec::new();
    for outcome in log.append_batch(events)? {
        match outcome {
            AppendOutcome::Inserted(_) => inserted += 1,
            AppendOutcome::Duplicate(position) => duplicates.push(position),
        }
    }
    Ok((inserted, duplicates))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use s2w_log::{AppendOutcome, EventLog, InMemoryEventLog, LogError, LogPosition, StoredEvent};
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
    use tokio_stream::wrappers::ReceiverStream;

    use super::{HumanReporter, MAX_BATCH, pump};
    use crate::AppError;

    /// An in-memory log that records the size of every batch it is handed.
    struct CountingLog {
        inner: InMemoryEventLog,
        batches: Arc<Mutex<Vec<usize>>>,
    }

    impl EventLog for CountingLog {
        fn append(&mut self, event: RawEvent) -> Result<AppendOutcome, LogError> {
            self.inner.append(event)
        }

        fn append_batch(&mut self, events: Vec<RawEvent>) -> Result<Vec<AppendOutcome>, LogError> {
            if let Ok(mut batches) = self.batches.lock() {
                batches.push(events.len());
            }
            self.inner.append_batch(events)
        }

        fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, LogError> {
            self.inner.cursor(source)
        }

        fn replay(
            &self,
            from: Option<LogPosition>,
        ) -> Result<Box<dyn Iterator<Item = Result<StoredEvent, LogError>> + '_>, LogError>
        {
            self.inner.replay(from)
        }
    }

    fn counting_log() -> (CountingLog, Arc<Mutex<Vec<usize>>>) {
        let batches = Arc::new(Mutex::new(Vec::new()));
        (
            CountingLog {
                inner: InMemoryEventLog::default(),
                batches: Arc::clone(&batches),
            },
            batches,
        )
    }

    fn batches(record: &Arc<Mutex<Vec<usize>>>) -> Vec<usize> {
        record.lock().map(|b| b.clone()).unwrap_or_default()
    }

    fn event(number: u64) -> Result<RawEvent, AppError> {
        Ok(RawEvent {
            source: SourceId::new("test")?,
            cursor: Cursor::new(number.to_string().into_bytes())?,
            received_at: Timestamp::from_millis(1),
            payload: format!("{{\"n\":{number}}}").into_bytes(),
        })
    }

    #[test]
    fn flushes_every_hundred_events_and_the_rest_at_the_end() {
        crate::tests::run(true, async {
            let (mut log, record) = counting_log();
            let items: Vec<Result<u64, AppError>> = (1..=250).map(Ok).collect();
            let outcome = pump(
                &mut log,
                tokio_stream::iter(items),
                event,
                Err,
                &mut HumanReporter,
            )
            .await;
            assert!(outcome.is_ok(), "pump failed: {outcome:?}");
            assert_eq!(batches(&record), vec![MAX_BATCH, MAX_BATCH, 50]);
        });
    }

    #[test]
    fn flushes_fifty_milliseconds_after_the_first_buffered_event() {
        crate::tests::run(true, async {
            let (mut log, record) = counting_log();
            let (sender, receiver) = tokio::sync::mpsc::channel::<Result<u64, AppError>>(8);
            let task = tokio::spawn(async move {
                pump(
                    &mut log,
                    ReceiverStream::new(receiver),
                    event,
                    Err,
                    &mut HumanReporter,
                )
                .await
            });
            for number in 1..=3 {
                assert!(sender.send(Ok(number)).await.is_ok());
            }
            tokio::time::sleep(std::time::Duration::from_millis(49)).await;
            assert!(batches(&record).is_empty(), "flushed before the deadline");
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            assert_eq!(
                batches(&record),
                vec![3],
                "the deadline must flush an open stream"
            );
            drop(sender);
            let outcome = task.await.map_err(|error| error.to_string());
            assert!(matches!(outcome, Ok(Ok(()))), "pump failed: {outcome:?}");
            assert_eq!(
                batches(&record),
                vec![3],
                "an empty buffer is never flushed"
            );
        });
    }

    #[test]
    fn flushes_the_buffer_before_returning_an_error() {
        crate::tests::run(true, async {
            let (mut log, record) = counting_log();
            let items: Vec<Result<u64, AppError>> =
                vec![Ok(1), Ok(2), Err(AppError::Usage("stop".to_owned())), Ok(3)];
            let outcome = pump(
                &mut log,
                tokio_stream::iter(items),
                event,
                Err,
                &mut HumanReporter,
            )
            .await;
            assert!(
                matches!(outcome, Err(AppError::Usage(_))),
                "got {outcome:?}"
            );
            assert_eq!(batches(&record), vec![2]);
        });
    }

    #[test]
    fn a_skipped_error_keeps_the_pump_running() {
        crate::tests::run(true, async {
            let (mut log, record) = counting_log();
            let items: Vec<Result<u64, AppError>> =
                vec![Ok(1), Err(AppError::Usage("skip".to_owned())), Ok(2)];
            let outcome = pump(
                &mut log,
                tokio_stream::iter(items),
                event,
                |_| Ok((String::new(), false)),
                &mut HumanReporter,
            )
            .await;
            assert!(outcome.is_ok(), "pump failed: {outcome:?}");
            assert_eq!(batches(&record), vec![2]);
        });
    }
}
