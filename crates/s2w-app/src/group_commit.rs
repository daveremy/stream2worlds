//! Group commit: buffer source events and write them with one `append_batch` per flush.
//!
//! A flush happens when [`MAX_BATCH`] events are buffered, [`MAX_DELAY`] after the first
//! buffered event, at the end of the stream, and before any error is returned. A crash loses at
//! most the unflushed buffer; Kafka re-fetches it from the stored per-partition cursors, stdin
//! cannot replay it.

use std::cell::Cell;
use std::time::Duration;

use s2w_log::{AppendOutcome, EventLog, LogError, LogPosition};
use s2w_model::{RawEvent, SourceId};
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
/// implementation. `pub` (s2w#79 round 2): the concrete `--json` reporter lives in `crates/s2w`
/// (it needs `output.rs`'s rendering seam, which this crate cannot depend on), so the trait and
/// the default [`HumanReporter`] have to cross the crate boundary; `watch`/`run_watch` no
/// longer pick a reporter themselves — the caller supplies one.
pub trait Reporter: Send {
    /// A batch was written to the log. `appended`, `duplicates` and `reconnects` are running
    /// totals since the pump started (s2w#79 round 2: `reconnects` counted by the pump itself,
    /// per `crates/s2w/AGENTS.md`'s "no logic here beyond argument parsing and output
    /// formatting" — a reporter only renders it); `cursor` is the last event's cursor, lossily
    /// decoded, once any event has been logged.
    fn flushed(&mut self, appended: u64, duplicates: u64, reconnects: u64, cursor: Option<&str>);
    /// One duplicate was collapsed at this log position (folded into the next [`Self::flushed`]
    /// call's `duplicates` total, not necessarily its own line).
    fn duplicate(&mut self, position: u64);
    /// A benign informational note — never a source error (e.g. "no stored cursor; starting
    /// fresh"). A `--json` reader filtering stderr for `"error"` must not see one of these
    /// (s2w#79 round 2: [`Self::source_error`] is the only method that renders `"error"`).
    fn note(&mut self, message: &str);
    /// A non-fatal source error, already reported — the pump continues past it. `retry` marks
    /// [`SourceError::Retrying`]: a transient failure retried from the same position, already
    /// folded into the `reconnects` total [`Self::flushed`] reports next — a reporter only
    /// renders the message, it does not count. (The one fatal error that stops the pump is
    /// rendered by the caller, not through this trait — see `s2w::run_watch`.)
    fn source_error(&mut self, message: &str, retry: bool);
    /// Whether the periodic human rate line ([`report_progress`]) should run alongside this
    /// reporter. Human: yes, so a quiet source doesn't read as a hang. Json: no — `flushed`
    /// already reports every batch, and a plain-text line would break an NDJSON stderr reader.
    fn wants_ticker(&self) -> bool {
        true
    }
}

/// Prints the human-readable lines `watch` has always printed.
#[derive(Default)]
pub struct HumanReporter;

impl Reporter for HumanReporter {
    fn flushed(
        &mut self,
        _appended: u64,
        _duplicates: u64,
        _reconnects: u64,
        _cursor: Option<&str>,
    ) {
        // The periodic ticker (`report_progress`) owns the human progress line; a per-flush
        // line here would be far chattier than the 5s cadence readers are used to.
    }

    fn duplicate(&mut self, position: u64) {
        eprintln!("s2w: duplicate event collapsed at log position {position}");
    }

    fn note(&mut self, message: &str) {
        eprintln!("s2w: {message}");
    }

    fn source_error(&mut self, message: &str, _retry: bool) {
        // Same text as a benign note today — nothing in the human output distinguishes them
        // yet (pre-existing; only the trait split matters for `--json`, s2w#79 round 2).
        eprintln!("s2w: {message}");
    }
}

/// Consumes `source` into `log` with group commit until the stream ends.
///
/// `convert` turns a source item into a log event; its error stops the pump. `on_error` sees
/// every source error: `Ok((message, retry))` for one that was reported and skipped (`retry`
/// marks a transient failure retried from the same position), or the error that stops the pump.
/// The buffer is flushed, and `report` told about it, before any error is returned.
///
/// Returns `Ok(true)` if the pump stopped early because a gated source's membership changed
/// (removed, or a commit came back `StaleGeneration`) rather than because the stream itself
/// ended. A `StaleGeneration` commit means the in-memory stream was opened against a
/// membership generation that a concurrent removal/re-add has since superseded — its cursor no
/// longer corresponds to what's actually stored, so the pump stops unconditionally (even if the
/// source is a member again under the new generation) rather than keep reading a stream whose
/// next flush would silently overwrite a fresh re-add cursor with a stale one, or skip events a
/// resumed stream would need to re-fetch (s2w#95 round-1 code review, both reviewers).
///
/// # Errors
///
/// Returns the first error from the log, `convert` or `on_error`.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a distinct input; a parameter struct is a follow-up refactor"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one sequential pass whose steps share local state; splitting it is a follow-up refactor"
)]
pub(crate) async fn pump<S, T, E>(
    mut write: impl FnMut(Vec<RawEvent>, &[(SourceId, i64)]) -> Result<Vec<AppendOutcome>, LogError>,
    mut source: S,
    mut convert: impl FnMut(T) -> Result<RawEvent, AppError>,
    mut on_error: impl FnMut(E) -> Result<(String, bool), AppError>,
    report: &mut dyn Reporter,
    sources: &[SourceId],
    mut membership: impl FnMut(&SourceId) -> Result<(bool, i64), AppError>,
) -> Result<bool, AppError>
where
    S: Stream<Item = Result<T, E>> + Unpin,
{
    let mut generations = Vec::new();
    for source in sources {
        let (member, generation) = membership(source)?;
        if !member {
            return Ok(true);
        }
        generations.push((source.clone(), generation));
    }
    let mut stopped = false;
    let mut buffer: Vec<RawEvent> = Vec::with_capacity(MAX_BATCH);
    let mut deadline: Option<Instant> = None;
    let mut appended: u64 = 0;
    let mut duplicates: u64 = 0;
    // Count of `SourceError::Retrying` reports so far (s2w#79 round 2: counted here, in
    // s2w-app, per `crates/s2w/AGENTS.md`'s "no logic here beyond argument parsing and output
    // formatting" — a `Reporter` only renders it, never counts it).
    let mut reconnects: u64 = 0;
    let mut last_cursor: Option<String> = None;
    loop {
        // Never poll a removed source, including on a restart with an already-removed row.
        if stopped {
            return Ok(true);
        }
        for (source, _) in &generations {
            if !membership(source)?.0 {
                return Ok(true);
            }
        }
        let mut commit = |events| {
            let outcomes = write(events, &generations)?;
            if outcomes.contains(&AppendOutcome::StaleGeneration) {
                // This stream's cursor is anchored to a membership generation a concurrent
                // remove/re-add has already superseded — stop unconditionally, even if the
                // source is a member again, rather than let a later flush from this same
                // stream commit under the new generation and clobber the re-add's fresh
                // cursor (or silently drop events a restarted stream would re-fetch).
                stopped = true;
            }
            Ok(outcomes)
        };
        let next = match deadline {
            Some(at) => {
                if let Ok(next) = tokio::time::timeout_at(at, source.next()).await {
                    next
                } else {
                    flush_and_report(
                        &mut commit,
                        &mut buffer,
                        &mut appended,
                        &mut duplicates,
                        reconnects,
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
                    &mut commit,
                    &mut buffer,
                    &mut appended,
                    &mut duplicates,
                    reconnects,
                    &last_cursor,
                    report,
                )?;
                return Ok(stopped);
            }
            Some(Ok(item)) => convert(item).map(|event| {
                if buffer.is_empty() {
                    deadline = Some(Instant::now() + MAX_DELAY);
                }
                last_cursor = Some(String::from_utf8_lossy(event.cursor.as_bytes()).into_owned());
                buffer.push(event);
            }),
            Some(Err(error)) => on_error(error).map(|(message, retry)| {
                if retry {
                    reconnects += 1;
                }
                report.source_error(&message, retry);
            }),
        };
        if let Err(error) = outcome {
            flush_and_report(
                &mut commit,
                &mut buffer,
                &mut appended,
                &mut duplicates,
                reconnects,
                &last_cursor,
                report,
            )?;
            return Err(error);
        }
        if buffer.len() >= MAX_BATCH {
            flush_and_report(
                &mut commit,
                &mut buffer,
                &mut appended,
                &mut duplicates,
                reconnects,
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
    pump_events_gated(
        |events, _| log.append_batch(events),
        stream,
        name,
        report,
        &[],
        |_| Ok((true, 0)),
    )
    .await
    .map(|_stopped_early| ())
}

/// Returns `Ok(true)` if `pump` stopped early on a membership change rather than a natural end
/// of stream — see [`pump`]'s doc comment.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a distinct input; a parameter struct is a follow-up refactor"
)]
pub(crate) async fn pump_events_gated(
    write: impl FnMut(Vec<RawEvent>, &[(SourceId, i64)]) -> Result<Vec<AppendOutcome>, LogError>,
    stream: EventStream,
    name: &str,
    report: &mut dyn Reporter,
    sources: &[SourceId],
    membership: impl FnMut(&SourceId) -> Result<(bool, i64), AppError>,
) -> Result<bool, AppError> {
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
            result = pump(write, stream, convert, on_error, report, sources, membership) => result,
            // report_progress never returns, so `never` can never be constructed; this is the
            // exhaustive match for an empty type, not a fallback branch.
            never = report_progress(name, &total, &last_event_at) => match never {},
        }
    } else {
        pump(
            write, stream, convert, on_error, report, sources, membership,
        )
        .await
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
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a distinct input; a parameter struct is a follow-up refactor"
)]
fn flush_and_report(
    log: &mut impl FnMut(Vec<RawEvent>) -> Result<Vec<AppendOutcome>, AppError>,
    buffer: &mut Vec<RawEvent>,
    appended: &mut u64,
    duplicates: &mut u64,
    reconnects: u64,
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
    report.flushed(*appended, *duplicates, reconnects, last_cursor.as_deref());
    Ok(())
}

/// Writes the buffer as one batch. Returns how many events were newly inserted and the log
/// positions of any duplicates collapsed among them; the buffer is always empty on return.
fn flush(
    log: &mut impl FnMut(Vec<RawEvent>) -> Result<Vec<AppendOutcome>, AppError>,
    buffer: &mut Vec<RawEvent>,
) -> Result<(u64, Vec<LogPosition>), AppError> {
    let events = std::mem::replace(buffer, Vec::with_capacity(MAX_BATCH));
    let mut inserted = 0_u64;
    let mut duplicates = Vec::new();
    for outcome in log(events)? {
        match outcome {
            AppendOutcome::Inserted(_) => inserted += 1,
            AppendOutcome::Duplicate(position) => duplicates.push(position),
            AppendOutcome::Rejected | AppendOutcome::StaleGeneration => {}
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

    use super::{HumanReporter, MAX_BATCH, Reporter, pump};
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
                |events, _| log.append_batch(events),
                tokio_stream::iter(items),
                event,
                Err,
                &mut HumanReporter,
                &[],
                |_| Ok((true, 0)),
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
                    |events, _| log.append_batch(events),
                    ReceiverStream::new(receiver),
                    event,
                    Err,
                    &mut HumanReporter,
                    &[],
                    |_| Ok((true, 0)),
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
            assert!(matches!(outcome, Ok(Ok(false))), "pump failed: {outcome:?}");
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
                |events, _| log.append_batch(events),
                tokio_stream::iter(items),
                event,
                Err,
                &mut HumanReporter,
                &[],
                |_| Ok((true, 0)),
            )
            .await;
            assert!(
                matches!(outcome, Err(AppError::Usage(_))),
                "got {outcome:?}"
            );
            assert_eq!(batches(&record), vec![2]);
        });
    }

    /// One call the fake `Reporter` below recorded, in order, so a test can assert both the
    /// final totals and the sequence they arrived in.
    #[derive(Debug, PartialEq, Eq)]
    enum RecordedCall {
        Flushed {
            appended: u64,
            duplicates: u64,
            reconnects: u64,
            cursor: Option<String>,
        },
        Duplicate {
            position: u64,
        },
        SourceError {
            message: String,
            retry: bool,
        },
    }

    /// A [`Reporter`] that records every call instead of rendering it, so a test can assert on
    /// `pump`'s own bookkeeping (s2w#105 follow-up from #79 round 3: nothing previously asserted
    /// `appended`/`duplicates`/`reconnects`, `last_cursor`, or call order directly).
    #[derive(Default)]
    struct RecordingReporter {
        calls: Vec<RecordedCall>,
    }

    impl Reporter for RecordingReporter {
        fn flushed(
            &mut self,
            appended: u64,
            duplicates: u64,
            reconnects: u64,
            cursor: Option<&str>,
        ) {
            self.calls.push(RecordedCall::Flushed {
                appended,
                duplicates,
                reconnects,
                cursor: cursor.map(str::to_owned),
            });
        }

        fn duplicate(&mut self, position: u64) {
            self.calls.push(RecordedCall::Duplicate { position });
        }

        fn note(&mut self, _message: &str) {}

        fn source_error(&mut self, message: &str, retry: bool) {
            self.calls.push(RecordedCall::SourceError {
                message: message.to_owned(),
                retry,
            });
        }

        fn wants_ticker(&self) -> bool {
            false
        }
    }

    #[test]
    fn pump_reports_totals_cursor_and_call_order() {
        crate::tests::run(true, async {
            let (mut log, _record) = counting_log();
            // 99 unique inserts, then a duplicate of cursor "2" as the 100th item — reaching
            // MAX_BATCH forces the first flush mid-stream (one duplicate, no reconnects yet).
            // Then a retried source error, a skipped (non-retried) one, and one more unique
            // insert, so the stream-end flush reports the second batch with a reconnect.
            let mut items: Vec<Result<u64, AppError>> = (1..=99).map(Ok).collect();
            items.push(Ok(2)); // duplicate of the 2nd insert above
            items.push(Err(AppError::Usage("retrying".to_owned())));
            items.push(Err(AppError::Usage("skipped".to_owned())));
            items.push(Ok(1000));

            let mut reporter = RecordingReporter::default();
            let mut errors_seen = 0_u32;
            let outcome = pump(
                |events, _| log.append_batch(events),
                tokio_stream::iter(items),
                event,
                |error: AppError| {
                    // First source error retries (reconnect), the second is skipped — exercises
                    // both `retry=true` and `retry=false` in one run.
                    errors_seen += 1;
                    Ok((error.to_string(), errors_seen == 1))
                },
                &mut reporter,
                &[],
                |_| Ok((true, 0)),
            )
            .await;
            assert!(outcome.is_ok(), "pump failed: {outcome:?}");

            assert_eq!(
                reporter.calls,
                vec![
                    RecordedCall::Duplicate { position: 2 },
                    RecordedCall::Flushed {
                        appended: 99,
                        duplicates: 1,
                        reconnects: 0,
                        cursor: Some("2".to_owned()),
                    },
                    RecordedCall::SourceError {
                        message: "retrying".to_owned(),
                        retry: true,
                    },
                    RecordedCall::SourceError {
                        message: "skipped".to_owned(),
                        retry: false,
                    },
                    RecordedCall::Flushed {
                        appended: 100,
                        duplicates: 1,
                        reconnects: 1,
                        cursor: Some("1000".to_owned()),
                    },
                ],
                "duplicate must be reported before its batch's flushed call, source errors must \
                 be reported as they happen, and each flushed call's totals/cursor must reflect \
                 only what happened up to that flush"
            );
        });
    }

    #[test]
    fn a_skipped_error_keeps_the_pump_running() {
        crate::tests::run(true, async {
            let (mut log, record) = counting_log();
            let items: Vec<Result<u64, AppError>> =
                vec![Ok(1), Err(AppError::Usage("skip".to_owned())), Ok(2)];
            let outcome = pump(
                |events, _| log.append_batch(events),
                tokio_stream::iter(items),
                event,
                |_| Ok((String::new(), false)),
                &mut HumanReporter,
                &[],
                |_| Ok((true, 0)),
            )
            .await;
            assert!(outcome.is_ok(), "pump failed: {outcome:?}");
            assert_eq!(batches(&record), vec![2]);
        });
    }
}

#[cfg(test)]
mod membership_tests {
    use super::*;
    use s2w_log::{EffectiveFrom, SqliteEventLog};
    use s2w_model::{Cursor, Timestamp};
    use std::{
        cell::RefCell,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll},
    };
    struct CountPolls(Rc<Cell<usize>>);
    impl Stream for CountPolls {
        type Item = Result<RawEvent, AppError>;
        fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.0.set(self.0.get() + 1);
            Poll::Ready(None)
        }
    }
    #[test]
    fn restart_after_removal_never_fetches() {
        crate::tests::run(false, async {
            let dir = crate::tests::TestDirectory::new("never-fetch");
            let mut log = SqliteEventLog::open(dir.path()).unwrap();
            let source = SourceId::new("removed").unwrap();
            log.record_source_removed(&source).unwrap();
            drop(log);
            let log = Rc::new(RefCell::new(SqliteEventLog::open(dir.path()).unwrap()));
            let polls = Rc::new(Cell::new(0));
            pump(
                |events, generations| {
                    log.borrow_mut()
                        .append_batch_with_generations(events, generations)
                },
                CountPolls(polls.clone()),
                Ok,
                Err,
                &mut HumanReporter,
                &[source],
                |source| Ok(log.borrow().source_membership(source)?),
            )
            .await
            .unwrap();
            assert_eq!(polls.get(), 0);
        });
    }
    #[test]
    fn stale_flush_stops_pump_without_touching_the_reset_cursor() {
        crate::tests::run(false, async {
            let dir = crate::tests::TestDirectory::new("pump-generation");
            let log = Rc::new(RefCell::new(SqliteEventLog::open(dir.path()).unwrap()));
            let source = SourceId::new("member").unwrap();
            log.borrow_mut().bootstrap_source(&source).unwrap();
            let old = log.borrow().membership_generation(&source).unwrap();
            let mut batches = 0;
            // 101 items so a second flush would happen if the pump kept reading past the
            // stale one — it must not: the stream's next item (n=100) is never fetched.
            let items = (0..101)
                .map(|n| {
                    Ok::<_, AppError>(RawEvent {
                        source: source.clone(),
                        cursor: Cursor::new(n.to_string().into_bytes()).unwrap(),
                        received_at: Timestamp::from_millis(n),
                        payload: n.to_string().into_bytes(),
                    })
                })
                .collect::<Vec<_>>();
            let stopped_early = pump(
                |events, generations| {
                    batches += 1;
                    assert_eq!(
                        batches, 1,
                        "pump must not flush again after a stale generation"
                    );
                    assert_eq!(generations[0].1, old);
                    // A concurrent remove + re-add lands between this flush being built (with
                    // the OLD generation) and it being written — the write below is rejected
                    // as StaleGeneration for every event in the batch.
                    log.borrow_mut().record_source_removed(&source)?;
                    log.borrow_mut().record_source_added(
                        &source,
                        EffectiveFrom::FromCursor(Cursor::new(b"reset".to_vec()).unwrap()),
                    )?;
                    log.borrow_mut()
                        .append_batch_with_generations(events, generations)
                },
                tokio_stream::iter(items),
                Ok,
                Err,
                &mut HumanReporter,
                std::slice::from_ref(&source),
                |source| Ok(log.borrow().source_membership(source)?),
            )
            .await
            .unwrap();
            assert!(stopped_early, "a StaleGeneration flush must stop the pump");
            assert_eq!(batches, 1);
            assert_eq!(log.borrow().replay(None).unwrap().count(), 0);
            assert_eq!(
                log.borrow().cursor(&source).unwrap().unwrap().as_bytes(),
                b"reset"
            );
        });
    }
    #[test]
    fn removal_before_next_poll_discards_buffer() {
        crate::tests::run(false, async {
            let dir = crate::tests::TestDirectory::new("pump-removed-buffer");
            let log = Rc::new(RefCell::new(SqliteEventLog::open(dir.path()).unwrap()));
            let source = SourceId::new("member").unwrap();
            log.borrow_mut().bootstrap_source(&source).unwrap();
            let polls = Cell::new(0);
            let stream = tokio_stream::iter((0..2).map(|n| {
                polls.set(polls.get() + 1);
                Ok::<_, AppError>(RawEvent {
                    source: source.clone(),
                    cursor: Cursor::new(vec![n]).unwrap(),
                    received_at: Timestamp::from_millis(0),
                    payload: vec![n],
                })
            }));
            pump(
                |events, generations| {
                    log.borrow_mut()
                        .append_batch_with_generations(events, generations)
                },
                stream,
                |event| {
                    log.borrow_mut().record_source_removed(&source)?;
                    Ok(event)
                },
                Err,
                &mut HumanReporter,
                std::slice::from_ref(&source),
                |source| Ok(log.borrow().source_membership(source)?),
            )
            .await
            .unwrap();
            assert_eq!(polls.get(), 1);
            assert_eq!(log.borrow().replay(None).unwrap().count(), 0);
        });
    }
}
