//! The connection, per-partition fetch tasks and the merged record stream.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rskafka::BackoffConfig;
use rskafka::chrono::{DateTime, Utc};
use rskafka::client::error::{Error as RsKafkaError, ProtocolError};
use rskafka::client::partition::{OffsetAt, PartitionClient, UnknownTopicHandling};
use rskafka::client::{Client, ClientBuilder};
use s2w_model::Timestamp;
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;

use super::envelope::envelope;
use super::{KafkaEvent, KafkaSourceError, KafkaStart, KafkaTarget};
use crate::watermark::Watermark;

/// Records buffered between the fetch tasks and the consumer.
const CHANNEL_CAPACITY: usize = 1024;
/// The most bytes one fetch asks the broker for.
const MAX_FETCH_BYTES: i32 = 1024 * 1024;
/// How long the broker may hold a fetch open waiting for new records.
const MAX_WAIT_MS: i32 = 500;
/// How long `rskafka` retries a retriable error inside one call before giving the error back,
/// so a dead broker surfaces as an error item instead of looking like a quiet partition.
const CLIENT_RETRY_DEADLINE: Duration = Duration::from_secs(60);

/// A connection to the brokers with the topic's partitions discovered, not yet reading.
#[derive(Debug)]
pub(crate) struct KafkaConnection {
    client: Arc<Client>,
    topic: String,
    partitions: Vec<i32>,
}

impl KafkaConnection {
    /// Connects to the brokers and reads the topic's partitions from cluster metadata.
    ///
    /// Partitions are discovered once; a partition added later is read after a restart.
    ///
    /// # Errors
    /// Returns [`KafkaSourceError::Connect`] when the brokers cannot be reached and
    /// [`KafkaSourceError::UnknownTopic`] when the topic does not exist.
    pub(crate) async fn open(target: &KafkaTarget) -> Result<Self, KafkaSourceError> {
        let client = ClientBuilder::new(target.brokers.clone())
            .client_id("s2w")
            .backoff_config(BackoffConfig {
                deadline: Some(CLIENT_RETRY_DEADLINE),
                ..BackoffConfig::default()
            })
            .build()
            .await
            .map_err(|error| KafkaSourceError::Connect(error.to_string()))?;
        let topics = client
            .list_topics()
            .await
            .map_err(|error| KafkaSourceError::Connect(error.to_string()))?;
        let partitions: Vec<i32> = topics
            .into_iter()
            .find(|topic| topic.name == target.topic)
            .ok_or_else(|| KafkaSourceError::UnknownTopic(target.topic.clone()))?
            .partitions
            .into_iter()
            .collect();
        if partitions.is_empty() {
            return Err(KafkaSourceError::UnknownTopic(target.topic.clone()));
        }
        Ok(Self {
            client: Arc::new(client),
            topic: target.topic.clone(),
            partitions,
        })
    }

    /// The topic's partitions, ascending.
    #[must_use]
    pub(crate) fn partitions(&self) -> &[i32] {
        &self.partitions
    }

    /// Resolves every partition's start offset, then starts one fetch task per partition.
    ///
    /// A partition missing from `starts` starts at [`KafkaStart::Latest`].
    ///
    /// # Errors
    /// Returns the first partition whose start cannot be resolved, including
    /// [`KafkaSourceError::CursorPruned`] and [`KafkaSourceError::CursorAhead`] for a stored
    /// cursor that no longer lines up with the partition.
    pub(crate) async fn start(
        self,
        starts: &BTreeMap<i32, KafkaStart>,
        marks: &BTreeMap<i32, Arc<Watermark>>,
    ) -> Result<KafkaSource, KafkaSourceError> {
        let mut clients = Vec::with_capacity(self.partitions.len());
        let mut start_offsets = Vec::with_capacity(self.partitions.len());
        for &partition in &self.partitions {
            let Some(&start) = starts.get(&partition) else {
                continue;
            };
            let client = self
                .client
                .partition_client(self.topic.clone(), partition, UnknownTopicHandling::Retry)
                .await
                .map_err(|error| KafkaSourceError::StartOffset {
                    partition,
                    message: error.to_string(),
                })?;
            let offset = resolve_start(&client, partition, start).await?;
            let mark = marks
                .get(&partition)
                .map_or_else(|| Arc::new(Watermark::unknown()), Arc::clone);
            mark.resume_at(offset);
            start_offsets.push((partition, offset));
            clients.push((client, offset, mark));
        }
        let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
        let tasks = clients
            .into_iter()
            .map(|(client, offset, mark)| {
                let handle: JoinHandle<()> =
                    tokio::spawn(fetch_loop(client, offset, mark, sender.clone()));
                handle.abort_handle()
            })
            .collect();
        Ok(KafkaSource {
            receiver: ReceiverStream::new(receiver),
            tasks,
            start_offsets,
        })
    }
}

/// The running Kafka source: a stream of records from every partition.
///
/// Records from one partition arrive in offset order; partitions interleave in arrival order.
/// The stream ends only if every fetch task stops, which happens after a fatal error.
#[derive(Debug)]
pub(crate) struct KafkaSource {
    receiver: ReceiverStream<Result<KafkaEvent, KafkaSourceError>>,
    tasks: Vec<AbortHandle>,
    start_offsets: Vec<(i32, i64)>,
}

impl KafkaSource {
    /// The resolved `(partition, first offset to read)` for every partition.
    #[must_use]
    pub(crate) fn start_offsets(&self) -> &[(i32, i64)] {
        &self.start_offsets
    }
}

impl Stream for KafkaSource {
    type Item = Result<KafkaEvent, KafkaSourceError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.receiver).poll_next(context)
    }
}

impl Drop for KafkaSource {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Turns a [`KafkaStart`] into the first offset to fetch.
async fn resolve_start(
    client: &PartitionClient,
    partition: i32,
    start: KafkaStart,
) -> Result<i64, KafkaSourceError> {
    let lookup = |at: OffsetAt| async move {
        client
            .get_offset(at)
            .await
            .map_err(|error| KafkaSourceError::StartOffset {
                partition,
                message: error.to_string(),
            })
    };
    match start {
        KafkaStart::Latest => lookup(OffsetAt::Latest).await,
        KafkaStart::Timestamp(millis) => {
            let at = DateTime::<Utc>::from_timestamp_millis(millis).ok_or_else(|| {
                KafkaSourceError::StartOffset {
                    partition,
                    message: format!("timestamp {millis} ms is out of range"),
                }
            })?;
            // Capture the high watermark BEFORE the timestamp lookup, not after. ListOffsets
            // answers -1 when no record at or after `at` exists yet; if we fetched the
            // watermark only in that branch, a record satisfying `at` produced between the two
            // requests would fall below the second (later) watermark and be skipped forever.
            // A watermark taken first is always a safe start: if the later timestamp lookup
            // still says -1, no qualifying record exists as of that lookup either, so nothing
            // at or after `earlier` can have been missed.
            let earlier = lookup(OffsetAt::Latest).await?;
            match lookup(OffsetAt::Timestamp(at)).await? {
                -1 => Ok(earlier),
                offset => Ok(offset),
            }
        }
        KafkaStart::After(stored) => {
            let next = stored.saturating_add(1);
            let earliest = lookup(OffsetAt::Earliest).await?;
            if earliest > next {
                return Err(KafkaSourceError::CursorPruned {
                    partition,
                    next,
                    earliest,
                });
            }
            let latest = lookup(OffsetAt::Latest).await?;
            if next > latest {
                return Err(KafkaSourceError::CursorAhead {
                    partition,
                    next,
                    latest,
                });
            }
            Ok(next)
        }
    }
}

/// Fetches one partition forever from `offset`, never skipping an offset, and records each
/// reply's high watermark in `mark`.
///
/// Stops when the consumer is dropped or after sending a fatal error.
#[expect(
    clippy::too_many_lines,
    reason = "the fetch loop is one state machine over offset, retry and shutdown; splitting it scatters its invariants"
)]
async fn fetch_loop(
    client: PartitionClient,
    mut offset: i64,
    mark: Arc<Watermark>,
    sender: mpsc::Sender<Result<KafkaEvent, KafkaSourceError>>,
) {
    let partition = client.partition();
    let topic = client.topic().to_owned();
    let mut backoff = Backoff::new();
    loop {
        match client
            .fetch_records(offset, 1..MAX_FETCH_BYTES, MAX_WAIT_MS)
            .await
        {
            Ok((records, high_watermark)) => {
                backoff.reset();
                // Every reply carries the head, even an empty one after MAX_WAIT_MS, so an
                // idle partition's lag drains to zero (s2w#168).
                mark.observe_high(high_watermark);
                for record in records {
                    // A compressed batch can start before the requested offset.
                    if record.offset < offset {
                        continue;
                    }
                    let next = record.offset.saturating_add(1);
                    let event = KafkaEvent {
                        partition,
                        offset: record.offset,
                        payload: envelope(&topic, partition, &record),
                        received_at: now(),
                    };
                    if sender.send(Ok(event)).await.is_err() {
                        return;
                    }
                    offset = next;
                }
            }
            Err(RsKafkaError::ServerError {
                protocol_error: ProtocolError::OffsetOutOfRange,
                ..
            }) => {
                if let Ok(earliest) = client.get_offset(OffsetAt::Earliest).await
                    && earliest > offset
                {
                    let _closed = sender
                        .send(Err(KafkaSourceError::CursorPruned {
                            partition,
                            next: offset,
                            earliest,
                        }))
                        .await;
                    return;
                }
                if !report_and_wait(
                    &sender,
                    &mut backoff,
                    partition,
                    offset,
                    "offset out of range",
                )
                .await
                {
                    return;
                }
            }
            Err(error) => {
                if !report_and_wait(&sender, &mut backoff, partition, offset, &error.to_string())
                    .await
                {
                    return;
                }
            }
        }
    }
}

/// Sends a non-fatal fetch error and sleeps the backoff. Returns `false` once the consumer is
/// gone.
async fn report_and_wait(
    sender: &mpsc::Sender<Result<KafkaEvent, KafkaSourceError>>,
    backoff: &mut Backoff,
    partition: i32,
    offset: i64,
    message: &str,
) -> bool {
    let error = KafkaSourceError::Fetch {
        partition,
        offset,
        message: message.to_owned(),
    };
    if sender.send(Err(error)).await.is_err() {
        return false;
    }
    tokio::time::sleep(backoff.next_delay()).await;
    true
}

/// Doubling delay between failed fetches: 250 ms up to 30 s, reset by a good fetch.
struct Backoff {
    delay: Duration,
}

impl Backoff {
    const INITIAL: Duration = Duration::from_millis(250);
    const MAX: Duration = Duration::from_secs(30);

    fn new() -> Self {
        Self {
            delay: Self::INITIAL,
        }
    }

    fn reset(&mut self) {
        self.delay = Self::INITIAL;
    }

    fn next_delay(&mut self) -> Duration {
        let delay = self.delay;
        self.delay = (self.delay * 2).min(Self::MAX);
        delay
    }
}

fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        });
    Timestamp::from_millis(millis)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rskafka::chrono::DateTime;
    use rskafka::record::Record;

    use crate::kafka::{KafkaSourceError, KafkaTarget};

    /// Against a real broker (decision 0007's measurement): assignment reads, timestamp starts,
    /// the after-the-last-record case, resume-after, and a compressed batch.
    #[test]
    #[ignore = "needs a local broker: S2W_KAFKA_BROKER=localhost:19092"]
    #[expect(
        clippy::too_many_lines,
        reason = "broker test walks five scenarios against one fixture topic"
    )]
    fn reads_by_assignment_against_a_real_broker() -> Result<(), Box<dyn std::error::Error>> {
        use rskafka::client::ClientBuilder;
        use rskafka::client::partition::{Compression, UnknownTopicHandling};
        use tokio_stream::StreamExt;

        use std::sync::Arc;

        use s2w_model::{ModelError, SourceId};

        use super::{KafkaConnection, KafkaStart};
        use crate::watermark::{Watermark, Watermarks};

        let broker = std::env::var("S2W_KAFKA_BROKER")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let base = 1_790_000_000_000_i64;
            let topic = format!("s2w-it-{}", std::process::id());
            let client = ClientBuilder::new(vec![broker.clone()]).build().await?;
            client
                .controller_client()?
                .create_topic(topic.clone(), 3, 1, 5_000)
                .await?;
            for partition in 0..3 {
                let producer = client
                    .partition_client(topic.clone(), partition, UnknownTopicHandling::Retry)
                    .await?;
                let records = |range: std::ops::Range<i64>| {
                    range
                        .map(|index| Record {
                            key: None,
                            // Equal values on purpose: the envelope keeps them distinct.
                            value: Some(b"tick".to_vec()),
                            headers: BTreeMap::new(),
                            timestamp: DateTime::from_timestamp_millis(base + index * 1_000)
                                .unwrap_or_default(),
                        })
                        .collect::<Vec<_>>()
                };
                producer
                    .produce(records(0..3), Compression::NoCompression)
                    .await?;
                producer.produce(records(3..5), Compression::Zstd).await?;
            }
            let target = KafkaTarget::parse(&format!("kafka://{broker}/{topic}"))?;

            // offsetsForTimes is inclusive: base+2000 is offset 2 in every partition.
            let connection = KafkaConnection::open(&target).await?;
            assert_eq!(connection.partitions(), [0, 1, 2]);
            let starts = (0..3)
                .map(|partition| (partition, KafkaStart::Timestamp(base + 2_000)))
                .collect();
            let marks: BTreeMap<i32, Arc<Watermark>> = (0..3)
                .map(|partition| (partition, Arc::new(Watermark::unknown())))
                .collect();
            let watermarks = Watermarks::tracked(
                marks
                    .iter()
                    .map(|(partition, mark)| {
                        let source = SourceId::new(format!("k.p{partition}"))?;
                        Ok((source, format!("p{partition}"), Arc::clone(mark)))
                    })
                    .collect::<Result<Vec<_>, ModelError>>()?,
            );
            let mut source = connection.start(&starts, &marks).await?;
            assert_eq!(source.start_offsets(), [(0, 2), (1, 2), (2, 2)]);
            let mut seen = Vec::new();
            while seen.len() < 9 {
                let event = tokio::time::timeout(std::time::Duration::from_secs(10), source.next())
                    .await?
                    .ok_or("stream ended")??;
                assert!(event.payload.contains("\"value\":\"tick\""));
                seen.push((event.partition, event.offset));
            }
            seen.sort_unstable();
            let expected: Vec<(i32, i64)> = (0..3)
                .flat_map(|partition| (2..5).map(move |offset| (partition, offset)))
                .collect();
            assert_eq!(seen, expected);
            // Every record came from a reply that reported the head first (s2w#168). Nothing
            // here marks records delivered (the adapter's stream does), so each partition is
            // still behind by the three records past its start offset.
            let heads: Vec<_> = watermarks
                .read()
                .ok_or("tracked watermarks read as not reported")?
                .into_iter()
                .map(|reading| (reading.label, reading.high_watermark, reading.behind))
                .collect();
            let expected_heads: Vec<_> = (0..3)
                .map(|partition| (format!("p{partition}"), Some(5), Some(3)))
                .collect();
            assert_eq!(heads, expected_heads);
            drop(source);

            // Measured on Apache Kafka 4.1: ListOffsets answers -1 for a time after the last
            // record, and the source maps that to the high watermark.
            let raw = client
                .partition_client(topic.clone(), 0, UnknownTopicHandling::Retry)
                .await?
                .get_offset(rskafka::client::partition::OffsetAt::Timestamp(
                    DateTime::from_timestamp_millis(base + 99_000).unwrap_or_default(),
                ))
                .await?;
            assert_eq!(raw, -1);
            let starts = BTreeMap::from([
                (0, KafkaStart::Timestamp(base + 99_000)),
                (1, KafkaStart::After(3)),
                (2, KafkaStart::Latest),
            ]);
            let source = KafkaConnection::open(&target)
                .await?
                .start(&starts, &BTreeMap::new())
                .await?;
            assert_eq!(source.start_offsets(), [(0, 5), (1, 4), (2, 5)]);
            drop(source);

            // A stored cursor past the end is loud, never a silent restart.
            let starts = BTreeMap::from([(0, KafkaStart::After(10))]);
            match KafkaConnection::open(&target)
                .await?
                .start(&starts, &BTreeMap::new())
                .await
            {
                Err(KafkaSourceError::CursorAhead {
                    partition: 0,
                    next: 11,
                    latest: 5,
                }) => {}
                other => panic!("expected CursorAhead, got {other:?}"),
            }
            Ok(())
        })
    }
}
