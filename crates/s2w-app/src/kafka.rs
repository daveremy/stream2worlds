//! `s2w watch kafka://<broker>/<topic>`: every partition by explicit assignment, one log source
//! per partition, resumed from each partition's stored offset.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use s2w_log::{EventLog, SqliteEventLog};
use s2w_model::{Cursor, RawEvent, SourceId};
use s2w_sources::kafka::{
    KafkaConnection, KafkaEvent, KafkaSourceError, KafkaStart, KafkaTarget, parse_since,
};

use crate::{AppError, group_commit};

/// Arguments for `s2w watch kafka://<broker>/<topic>`.
#[derive(Debug, Clone)]
pub struct WatchKafkaArgs {
    /// The `kafka://<broker>[,<broker>...]/<topic>` URL.
    pub target: String,
    /// RFC 3339 or epoch milliseconds: where partitions start when the log holds no cursor for
    /// the topic yet.
    pub since: Option<String>,
    /// The directory holding (or creating) the SQLite event log.
    pub log_dir: PathBuf,
}

/// Runs `s2w watch kafka://…` until the process is stopped.
///
/// # Errors
///
/// Returns the first failure that should stop the watch: an invalid target or `--since`, a
/// broker that cannot be reached, a missing topic, a stored cursor that cannot be decoded or
/// cannot resume (deleted by retention, or past the partition's end), or a log that cannot be
/// written. A failed fetch is reported and retried.
pub fn watch_kafka(args: WatchKafkaArgs) -> Result<(), AppError> {
    crate::current_thread_runtime()?.block_on(run_kafka(args))
}

/// The log source id of one partition: `kafka.<topic>.p<partition>`.
fn partition_source(topic: &str, partition: i32) -> Result<SourceId, AppError> {
    Ok(SourceId::new(format!("kafka.{topic}.p{partition}"))?)
}

/// Decodes a stored partition cursor: the decimal offset of the last stored record.
fn stored_offset(source: &SourceId, cursor: &Cursor) -> Result<i64, AppError> {
    std::str::from_utf8(cursor.as_bytes())
        .ok()
        .and_then(|text| text.parse::<i64>().ok())
        .filter(|offset| *offset >= 0)
        .ok_or_else(|| AppError::StoredCursorOffset {
            source_id: source.as_str().to_owned(),
            cursor: String::from_utf8_lossy(cursor.as_bytes()).into_owned(),
        })
}

/// The usage error for `--since` against a log that already holds a cursor for the topic.
fn since_with_cursor(log_dir: &Path, since: &str) -> AppError {
    AppError::Usage(format!(
        "a stored cursor already exists for this topic in --log-dir {}; drop --since to resume from it, or use a fresh --log-dir to replay from {since:?}",
        log_dir.display()
    ))
}

/// Composes the Kafka source with the durable log and consumes the stream.
async fn run_kafka(args: WatchKafkaArgs) -> Result<(), AppError> {
    let target = KafkaTarget::parse(&args.target)?;
    let since = args.since.as_deref().map(parse_since).transpose()?;
    let mut log = SqliteEventLog::open(&args.log_dir)?;
    let topic = target.topic().to_owned();

    // Every topic has partition 0, so this refuses the common case before any connection.
    if let Some(since) = &args.since
        && log.cursor(&partition_source(&topic, 0)?)?.is_some()
    {
        return Err(since_with_cursor(&args.log_dir, since));
    }

    let connection = KafkaConnection::open(&target).await?;
    let mut sources = BTreeMap::new();
    let mut starts = BTreeMap::new();
    let mut fresh = Vec::new();
    for &partition in connection.partitions() {
        let source = partition_source(&topic, partition)?;
        let start = match log.cursor(&source)? {
            Some(cursor) => KafkaStart::After(stored_offset(&source, &cursor)?),
            None => {
                fresh.push(partition);
                since.map_or(KafkaStart::Latest, KafkaStart::Timestamp)
            }
        };
        starts.insert(partition, start);
        sources.insert(partition, source);
    }
    let resuming = fresh.len() < sources.len();
    if resuming && let Some(since) = &args.since {
        return Err(since_with_cursor(&args.log_dir, since));
    }

    let source = connection.start(&starts).await?;
    if resuming {
        // A partition added since the last run has no cursor; it starts at its end.
        for (partition, offset) in source.start_offsets() {
            if fresh.contains(partition) {
                eprintln!(
                    "s2w: kafka partition {partition} of {topic:?} has no stored cursor; starting at offset {offset}"
                );
            }
        }
    }

    let log_dir = args.log_dir.clone();
    group_commit::pump(
        &mut log,
        source,
        |event: KafkaEvent| {
            let source = sources
                .get(&event.partition)
                .ok_or(AppError::UnexpectedPartition(event.partition))?;
            Ok(RawEvent {
                source: source.clone(),
                cursor: Cursor::new(event.offset.to_string().into_bytes())?,
                received_at: event.received_at,
                payload: event.payload.into_bytes(),
            })
        },
        |error: KafkaSourceError| {
            if !error.is_fatal() {
                eprintln!("s2w: kafka source error: {error}");
                return Ok(());
            }
            if matches!(
                error,
                KafkaSourceError::CursorPruned { .. } | KafkaSourceError::CursorAhead { .. }
            ) {
                eprintln!(
                    "s2w: the cursor stored in --log-dir {} cannot resume this topic; use a fresh --log-dir to start over",
                    log_dir.display()
                );
            }
            Err(error.into())
        },
    )
    .await?;
    Err(AppError::StreamEnded("kafka"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use s2w_log::{EventLog, SqliteEventLog};
    use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};

    use super::{WatchKafkaArgs, stored_offset, watch_kafka};
    use crate::AppError;
    use crate::tests::TestDirectory;

    fn seed(directory: &Path, source: &str, cursor: &[u8]) {
        let seeded = SourceId::new(source)
            .map_err(|error| error.to_string())
            .and_then(|source| {
                let cursor = Cursor::new(cursor.to_vec()).map_err(|error| error.to_string())?;
                let mut log = SqliteEventLog::open(directory).map_err(|error| error.to_string())?;
                log.append(RawEvent {
                    source,
                    cursor,
                    received_at: Timestamp::from_millis(1),
                    payload: b"{}".to_vec(),
                })
                .map_err(|error| error.to_string())
            });
        assert!(seeded.is_ok(), "seeding failed: {seeded:?}");
    }

    // Each case fails before any connection is attempted, so these run offline.

    #[test]
    fn since_with_a_stored_partition_cursor_is_a_usage_error() {
        let directory = TestDirectory::new("kafka-since-and-cursor");
        seed(directory.path(), "kafka.orders.p0", b"41");
        let outcome = watch_kafka(WatchKafkaArgs {
            target: "kafka://127.0.0.1:1/orders".to_owned(),
            since: Some("2026-09-27T00:00:00Z".to_owned()),
            log_dir: directory.path().to_path_buf(),
        });
        assert!(
            matches!(&outcome, Err(AppError::Usage(message)) if message.contains("drop --since")),
            "got {outcome:?}"
        );
    }

    #[test]
    fn an_invalid_since_is_refused_before_connecting() {
        let directory = TestDirectory::new("kafka-bad-since");
        let outcome = watch_kafka(WatchKafkaArgs {
            target: "kafka://127.0.0.1:1/orders".to_owned(),
            since: Some("yesterday".to_owned()),
            log_dir: directory.path().to_path_buf(),
        });
        assert!(
            matches!(outcome, Err(AppError::Kafka(_))),
            "got {outcome:?}"
        );
    }

    #[test]
    fn stored_offsets_decode_or_fail_loudly() {
        let source = SourceId::new("kafka.orders.p0");
        assert!(source.is_ok());
        let Ok(source) = source else { return };
        let decode = |bytes: &[u8]| {
            Cursor::new(bytes.to_vec())
                .map_err(AppError::from)
                .and_then(|cursor| stored_offset(&source, &cursor))
        };
        assert!(matches!(decode(b"41"), Ok(41)));
        for bad in [&b"-1"[..], b"4x", &[0xff, 0xfe]] {
            assert!(
                matches!(decode(bad), Err(AppError::StoredCursorOffset { .. })),
                "{bad:?} must be refused"
            );
        }
    }

    /// End to end against a real broker: every partition into the log with per-partition
    /// cursors, a restart that resumes without duplicates, `--since` refused on a resumed log,
    /// and `--since` into a fresh log starting at the right offset.
    #[test]
    #[ignore = "needs a local broker: S2W_KAFKA_BROKER=localhost:19092"]
    fn watches_resumes_and_replays_against_a_real_broker() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::collections::BTreeMap;
        use std::time::Duration;

        use rskafka::chrono::DateTime;
        use rskafka::client::ClientBuilder;
        use rskafka::client::partition::{Compression, UnknownTopicHandling};
        use rskafka::record::Record;

        use super::run_kafka;

        type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
        /// Every stored `(source, cursor)`, sorted, and the two partition cursors.
        type Contents = (Vec<(String, String)>, Vec<String>);

        let broker = std::env::var("S2W_KAFKA_BROKER")?;
        crate::tests::run(false, async {
            let base = 1_790_000_000_000_i64;
            let topic = format!("s2w-app-it-{}", std::process::id());
            let client = ClientBuilder::new(vec![broker.clone()]).build().await?;
            client
                .controller_client()?
                .create_topic(topic.clone(), 2, 1, 5_000)
                .await?;
            let produce = |range: std::ops::Range<i64>| {
                let client = &client;
                let topic = topic.clone();
                async move {
                    for partition in 0..2 {
                        let records = range
                            .clone()
                            .map(|index| Record {
                                key: None,
                                value: Some(format!("{{\"i\":{index}}}").into_bytes()),
                                headers: BTreeMap::new(),
                                timestamp: DateTime::from_timestamp_millis(base + index * 1_000)
                                    .unwrap_or_default(),
                            })
                            .collect();
                        client
                            .partition_client(topic.clone(), partition, UnknownTopicHandling::Retry)
                            .await?
                            .produce(records, Compression::NoCompression)
                            .await?;
                    }
                    Ok::<(), rskafka::client::error::Error>(())
                }
            };
            let url = format!("kafka://{broker}/{topic}");
            let watch = |log_dir: &Path, since: Option<String>| {
                let args = WatchKafkaArgs {
                    target: url.clone(),
                    since,
                    log_dir: log_dir.to_path_buf(),
                };
                // The watch never ends on its own; stop it once it has had time to catch up.
                tokio::time::timeout(Duration::from_secs(4), run_kafka(args))
            };
            let contents = |log_dir: &Path| -> TestResult<Contents> {
                let log = SqliteEventLog::open(log_dir)?;
                let mut stored = Vec::new();
                for item in log.replay(None)? {
                    let event = item?.event;
                    stored.push((
                        event.source.as_str().to_owned(),
                        String::from_utf8(event.cursor.as_bytes().to_vec())?,
                    ));
                }
                let mut cursors = Vec::new();
                for partition in 0..2 {
                    let source = SourceId::new(format!("kafka.{topic}.p{partition}"))?;
                    let cursor = log.cursor(&source)?.ok_or("missing cursor")?;
                    cursors.push(String::from_utf8(cursor.as_bytes().to_vec())?);
                }
                stored.sort();
                Ok((stored, cursors))
            };
            let expected = |offsets: std::ops::Range<i64>| {
                let mut all: Vec<(String, String)> = (0..2)
                    .flat_map(|partition| {
                        let topic = topic.clone();
                        offsets.clone().map(move |offset| {
                            (format!("kafka.{topic}.p{partition}"), offset.to_string())
                        })
                    })
                    .collect();
                all.sort();
                all
            };

            produce(0..3).await?;
            let resumed = TestDirectory::new("kafka-it-resume");
            assert!(watch(resumed.path(), Some(base.to_string())).await.is_err());
            assert_eq!(
                contents(resumed.path())?,
                (expected(0..3), vec!["2".to_owned(); 2])
            );

            produce(3..5).await?;
            assert!(watch(resumed.path(), None).await.is_err());
            assert_eq!(
                contents(resumed.path())?,
                (expected(0..5), vec!["4".to_owned(); 2]),
                "a restart resumes after the stored offsets with no duplicates"
            );
            let refused = watch(resumed.path(), Some(base.to_string())).await;
            assert!(
                matches!(refused, Ok(Err(AppError::Usage(_)))),
                "got {refused:?}"
            );

            let replayed = TestDirectory::new("kafka-it-since");
            assert!(
                watch(replayed.path(), Some((base + 2_000).to_string()))
                    .await
                    .is_err()
            );
            assert_eq!(
                contents(replayed.path())?,
                (expected(2..5), vec!["4".to_owned(); 2]),
                "--since is inclusive: the record at base+2000 is offset 2"
            );
            Ok(())
        })
    }
}
