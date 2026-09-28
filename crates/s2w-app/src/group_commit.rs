//! Group commit: buffer source events and write them with one `append_batch` per flush.
//!
//! A flush happens when [`MAX_BATCH`] events are buffered, [`MAX_DELAY`] after the first
//! buffered event, at the end of the stream, and before any error is returned. A crash loses at
//! most the unflushed buffer; Kafka re-fetches it from the stored per-partition cursors, stdin
//! cannot replay it.

use std::cell::Cell;
use std::time::Duration;

use s2w_log::{AppendOutcome, EventLog};
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
/// `watch` or `serve` prints nothing else, which reads as a hang to a viewer).
pub(crate) const PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

/// Consumes `source` into `log` with group commit until the stream ends.
///
/// `convert` turns a source item into a log event; its error stops the pump. `on_error` sees
/// every source error: it returns `Ok(())` for an error that was reported and skipped, or the
/// error that stops the pump. The buffer is flushed before any error is returned.
///
/// # Errors
///
/// Returns the first error from the log, `convert` or `on_error`.
pub(crate) async fn pump<L, S, T, E>(
    log: &mut L,
    mut source: S,
    mut convert: impl FnMut(T) -> Result<RawEvent, AppError>,
    mut on_error: impl FnMut(E) -> Result<(), AppError>,
) -> Result<(), AppError>
where
    L: EventLog,
    S: Stream<Item = Result<T, E>> + Unpin,
{
    let mut buffer: Vec<RawEvent> = Vec::with_capacity(MAX_BATCH);
    let mut deadline: Option<Instant> = None;
    loop {
        let next = match deadline {
            Some(at) => {
                if let Ok(next) = tokio::time::timeout_at(at, source.next()).await {
                    next
                } else {
                    flush(log, &mut buffer)?;
                    deadline = None;
                    continue;
                }
            }
            None => source.next().await,
        };
        let outcome = match next {
            None => {
                flush(log, &mut buffer)?;
                return Ok(());
            }
            Some(Ok(item)) => convert(item).map(|event| {
                if buffer.is_empty() {
                    deadline = Some(Instant::now() + MAX_DELAY);
                }
                buffer.push(event);
            }),
            Some(Err(error)) => on_error(error),
        };
        if let Err(error) = outcome {
            flush(log, &mut buffer)?;
            return Err(error);
        }
        if buffer.len() >= MAX_BATCH {
            flush(log, &mut buffer)?;
            deadline = None;
        }
    }
}

/// Consumes a started source's stream into `log` with group commit until it ends, printing a
/// periodic progress line on stderr (`name` identifies the source in that line) so a healthy
/// run is not silent.
///
/// A [`SourceError::Skipped`] item is reported on stderr and the stream continues; any other
/// error stops the pump after the buffer is flushed.
///
/// # Errors
///
/// Returns the first error from the log or the first fatal source error.
pub(crate) async fn pump_events<L: EventLog>(
    log: &mut L,
    stream: EventStream,
    name: &str,
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
    let pump_future = pump(log, stream, convert, |error: SourceError| {
        if error.is_fatal() {
            Err(AppError::Source(error))
        } else {
            eprintln!("s2w: {error}");
            Ok(())
        }
    });
    tokio::select! {
        result = pump_future => result,
        // report_progress never returns, so `never` can never be constructed; this is the
        // exhaustive match for an empty type, not a fallback branch.
        never = report_progress(name, &total, &last_event_at) => match never {},
    }
}

/// Prints `name`'s throughput, running total and time since the last event roughly every
/// [`PROGRESS_INTERVAL`], forever — the caller races it against the pump and drops it once the
/// pump finishes.
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

/// Writes the buffer as one batch and reports collapsed redeliveries on stderr.
fn flush<L: EventLog>(log: &mut L, buffer: &mut Vec<RawEvent>) -> Result<(), AppError> {
    if buffer.is_empty() {
        return Ok(());
    }
    let events = std::mem::replace(buffer, Vec::with_capacity(MAX_BATCH));
    for outcome in log.append_batch(events)? {
        if let AppendOutcome::Duplicate(position) = outcome {
            eprintln!(
                "s2w: duplicate event collapsed at log position {}",
                position.as_u64()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use s2w_log::{AppendOutcome, EventLog, InMemoryEventLog, LogError, LogPosition, StoredEvent};
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
    use tokio_stream::wrappers::ReceiverStream;

    use super::{MAX_BATCH, pump};
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
            let outcome = pump(&mut log, tokio_stream::iter(items), event, Err).await;
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
                pump(&mut log, ReceiverStream::new(receiver), event, Err).await
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
            let outcome = pump(&mut log, tokio_stream::iter(items), event, Err).await;
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
            let outcome = pump(&mut log, tokio_stream::iter(items), event, |_| Ok(())).await;
            assert!(outcome.is_ok(), "pump failed: {outcome:?}");
            assert_eq!(batches(&record), vec![2]);
        });
    }
}
