//! Kafka by explicit partition assignment, through `rskafka` (decision 0006).
//!
//! The source discovers the topic's partitions once, resolves one start offset per partition,
//! and runs one fetch task per partition. It never joins a consumer group and never commits
//! offsets: `rskafka` has neither concept, so the invariant holds by construction. Each record
//! leaves as a byte-deterministic JSON envelope that carries its own offset, so the log's
//! `(source, payload hash)` dedupe collapses a real redelivery but never two distinct records
//! whose values happen to be equal.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rskafka::BackoffConfig;
use rskafka::chrono::{DateTime, Utc};
use rskafka::client::error::{Error as RsKafkaError, ProtocolError};
use rskafka::client::partition::{OffsetAt, PartitionClient, UnknownTopicHandling};
use rskafka::client::{Client, ClientBuilder};
use rskafka::record::RecordAndOffset;
use s2w_model::Timestamp;
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;

/// Records buffered between the fetch tasks and the consumer.
const CHANNEL_CAPACITY: usize = 1024;
/// The most bytes one fetch asks the broker for.
const MAX_FETCH_BYTES: i32 = 1024 * 1024;
/// How long the broker may hold a fetch open waiting for new records.
const MAX_WAIT_MS: i32 = 500;
/// How long `rskafka` retries a retriable error inside one call before giving the error back,
/// so a dead broker surfaces as an error item instead of looking like a quiet partition.
const CLIENT_RETRY_DEADLINE: Duration = Duration::from_secs(60);

/// The broker list and topic from a `kafka://<broker>[,<broker>…]/<topic>` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaTarget {
    brokers: Vec<String>,
    topic: String,
}

impl KafkaTarget {
    /// Parses `kafka://host:port[,host:port…]/topic`.
    ///
    /// The topic must satisfy Kafka's own rules: 1 to 249 bytes of ASCII letters, digits, `.`,
    /// `_` and `-`, and not `.` or `..`.
    ///
    /// # Errors
    /// Returns [`KafkaSourceError::InvalidTarget`] naming what is wrong.
    pub fn parse(url: &str) -> Result<Self, KafkaSourceError> {
        let invalid = |reason: &str| KafkaSourceError::InvalidTarget {
            value: url.to_owned(),
            reason: reason.to_owned(),
        };
        let rest = url
            .strip_prefix("kafka://")
            .ok_or_else(|| invalid("expected kafka://<broker>/<topic>"))?;
        let (brokers, topic) = rest
            .split_once('/')
            .ok_or_else(|| invalid("missing /<topic> after the broker"))?;
        let brokers: Vec<String> = brokers.split(',').map(str::to_owned).collect();
        if brokers.iter().any(|broker| {
            broker.is_empty() || broker.contains(char::is_whitespace) || !broker.contains(':')
        }) {
            return Err(invalid("each broker must be host:port"));
        }
        let valid_topic = !topic.is_empty()
            && topic.len() <= 249
            && topic != "."
            && topic != ".."
            && topic
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !valid_topic {
            return Err(invalid(
                "topic must be 1-249 ASCII letters, digits, '.', '_' or '-'",
            ));
        }
        Ok(Self {
            brokers,
            topic: topic.to_owned(),
        })
    }

    /// The bootstrap brokers, as given.
    #[must_use]
    pub fn brokers(&self) -> &[String] {
        &self.brokers
    }

    /// The topic name.
    #[must_use]
    pub fn topic(&self) -> &str {
        &self.topic
    }
}

/// Where one partition starts reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KafkaStart {
    /// The partition's high watermark: only records produced from now on.
    Latest,
    /// The first record whose timestamp is at or after this many epoch milliseconds
    /// (`offsetsForTimes`, inclusive). A time after the last record starts at the high
    /// watermark.
    Timestamp(i64),
    /// The record after this already-stored offset: a resume from the log's cursor.
    After(i64),
}

/// Parses a `--since` value: RFC 3339 (`2026-09-27T12:00:00Z`) or integer epoch milliseconds.
///
/// # Errors
/// Returns [`KafkaSourceError::InvalidSince`] when the value is neither.
pub fn parse_since(value: &str) -> Result<i64, KafkaSourceError> {
    if let Ok(millis) = value.parse::<i64>() {
        return Ok(millis);
    }
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.timestamp_millis())
        .map_err(|error| KafkaSourceError::InvalidSince {
            value: value.to_owned(),
            reason: error.to_string(),
        })
}

/// One Kafka record, ready for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaEvent {
    /// The partition the record came from.
    pub partition: i32,
    /// The record's offset within its partition; the resume cursor.
    pub offset: i64,
    /// The JSON envelope (see [`envelope`]).
    pub payload: String,
    /// The wall-clock time at which the source received the record.
    pub received_at: Timestamp,
}

/// Failures from the Kafka source.
#[derive(Debug, thiserror::Error)]
pub enum KafkaSourceError {
    /// The `kafka://` URL could not be used.
    #[error("invalid kafka target {value:?}: {reason}")]
    InvalidTarget {
        /// The URL as given.
        value: String,
        /// Why it was refused.
        reason: String,
    },
    /// The `--since` value is neither RFC 3339 nor epoch milliseconds.
    #[error("invalid --since {value:?}: expected RFC 3339 or epoch milliseconds ({reason})")]
    InvalidSince {
        /// The value as given.
        value: String,
        /// The parser's complaint.
        reason: String,
    },
    /// The brokers could not be reached, or metadata could not be read.
    #[error("could not connect to kafka: {0}")]
    Connect(String),
    /// The brokers do not know the topic.
    #[error("kafka topic {0:?} does not exist")]
    UnknownTopic(String),
    /// Resolving a partition's start offset failed.
    #[error("partition {partition}: could not resolve the start offset: {message}")]
    StartOffset {
        /// The partition.
        partition: i32,
        /// The client's error.
        message: String,
    },
    /// A fetch failed; the source retries from the same offset and never skips.
    #[error("partition {partition}: fetch at offset {offset} failed, retrying: {message}")]
    Fetch {
        /// The partition.
        partition: i32,
        /// The offset that will be fetched again.
        offset: i64,
        /// The client's error.
        message: String,
    },
    /// The next offset to read has been deleted by retention. Continuing would silently skip
    /// records, so the partition stops and the error is fatal.
    #[error(
        "partition {partition}: the next offset {next} was deleted by retention (earliest is {earliest}); resuming would skip records"
    )]
    CursorPruned {
        /// The partition.
        partition: i32,
        /// The offset the source needed next.
        next: i64,
        /// The partition's earliest remaining offset.
        earliest: i64,
    },
    /// The stored cursor is past the partition's end: the topic was probably deleted and
    /// recreated, so its offsets no longer mean what the log recorded.
    #[error(
        "partition {partition}: the next offset {next} is past the partition's end {latest}; was the topic recreated?"
    )]
    CursorAhead {
        /// The partition.
        partition: i32,
        /// The offset the source needed next.
        next: i64,
        /// The partition's high watermark.
        latest: i64,
    },
}

impl KafkaSourceError {
    /// Whether the source has stopped reading because of this error. A non-fatal error is
    /// reported and the source keeps going.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        !matches!(self, Self::Fetch { .. })
    }
}

/// A connection to the brokers with the topic's partitions discovered, not yet reading.
#[derive(Debug)]
pub struct KafkaConnection {
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
    pub async fn open(target: &KafkaTarget) -> Result<Self, KafkaSourceError> {
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
    pub fn partitions(&self) -> &[i32] {
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
    pub async fn start(
        self,
        starts: &BTreeMap<i32, KafkaStart>,
    ) -> Result<KafkaSource, KafkaSourceError> {
        let mut clients = Vec::with_capacity(self.partitions.len());
        let mut start_offsets = Vec::with_capacity(self.partitions.len());
        for &partition in &self.partitions {
            let client = self
                .client
                .partition_client(self.topic.clone(), partition, UnknownTopicHandling::Retry)
                .await
                .map_err(|error| KafkaSourceError::StartOffset {
                    partition,
                    message: error.to_string(),
                })?;
            let start = starts
                .get(&partition)
                .copied()
                .unwrap_or(KafkaStart::Latest);
            let offset = resolve_start(&client, partition, start).await?;
            start_offsets.push((partition, offset));
            clients.push((client, offset));
        }
        let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
        let tasks = clients
            .into_iter()
            .map(|(client, offset)| {
                let handle: JoinHandle<()> =
                    tokio::spawn(fetch_loop(client, offset, sender.clone()));
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
pub struct KafkaSource {
    receiver: ReceiverStream<Result<KafkaEvent, KafkaSourceError>>,
    tasks: Vec<AbortHandle>,
    start_offsets: Vec<(i32, i64)>,
}

impl KafkaSource {
    /// The resolved `(partition, first offset to read)` for every partition.
    #[must_use]
    pub fn start_offsets(&self) -> &[(i32, i64)] {
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
            // ListOffsets answers -1 when no record is at or after the time; the next record
            // produced is then the first one at or after it, which is the high watermark.
            match lookup(OffsetAt::Timestamp(at)).await? {
                -1 => lookup(OffsetAt::Latest).await,
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

/// Fetches one partition forever from `offset`, never skipping an offset.
///
/// Stops when the consumer is dropped or after sending a fatal error.
async fn fetch_loop(
    client: PartitionClient,
    mut offset: i64,
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
            Ok((records, _high_watermark)) => {
                backoff.reset();
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

/// Encodes one record as the byte-deterministic JSON envelope stored in the log:
/// `{"key":…,"offset":…,"partition":…,"timestamp_ms":…,"topic":…,"value":…}`.
///
/// Keys are in that fixed order. `key` and `value` are the record's raw bytes: a JSON string
/// when they are valid UTF-8, `{"hex":"…"}` otherwise, and `null` when absent. The value is never
/// parsed and re-serialised, so a redelivered record encodes to identical bytes.
#[must_use]
pub fn envelope(topic: &str, partition: i32, record: &RecordAndOffset) -> String {
    let mut fields = serde_json::Map::new();
    fields.insert("key".to_owned(), bytes_value(record.record.key.as_deref()));
    fields.insert("offset".to_owned(), record.offset.into());
    fields.insert("partition".to_owned(), partition.into());
    fields.insert(
        "timestamp_ms".to_owned(),
        record.record.timestamp.timestamp_millis().into(),
    );
    fields.insert("topic".to_owned(), topic.into());
    fields.insert(
        "value".to_owned(),
        bytes_value(record.record.value.as_deref()),
    );
    serde_json::Value::Object(fields).to_string()
}

fn bytes_value(bytes: Option<&[u8]>) -> serde_json::Value {
    match bytes {
        None => serde_json::Value::Null,
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => text.into(),
            Err(_) => {
                let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                let mut object = serde_json::Map::new();
                object.insert("hex".to_owned(), hex.into());
                serde_json::Value::Object(object)
            }
        },
    }
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
    use rskafka::record::{Record, RecordAndOffset};

    use super::{KafkaSourceError, KafkaTarget, envelope, parse_since};

    fn record(key: Option<&[u8]>, value: Option<&[u8]>, offset: i64) -> RecordAndOffset {
        RecordAndOffset {
            record: Record {
                key: key.map(<[u8]>::to_vec),
                value: value.map(<[u8]>::to_vec),
                headers: BTreeMap::new(),
                timestamp: DateTime::from_timestamp_millis(1_790_000_000_123).unwrap_or_default(),
            },
            offset,
        }
    }

    #[test]
    fn target_parses_brokers_and_topic() -> Result<(), KafkaSourceError> {
        let target = KafkaTarget::parse("kafka://a:9092,b:9093/orders.v1_x-y")?;
        assert_eq!(target.brokers(), ["a:9092", "b:9093"]);
        assert_eq!(target.topic(), "orders.v1_x-y");
        Ok(())
    }

    #[test]
    fn target_rejects_bad_urls_loudly() {
        let too_long = format!("kafka://localhost:9092/{}", "t".repeat(250));
        for url in [
            "http://localhost:9092/orders",
            "kafka://localhost:9092",
            "kafka://localhost/orders",
            "kafka://localhost:9092/",
            "kafka://localhost:9092/a/b",
            "kafka://localhost:9092/..",
            "kafka://,localhost:9092/orders",
            too_long.as_str(),
        ] {
            assert!(
                matches!(
                    KafkaTarget::parse(url),
                    Err(KafkaSourceError::InvalidTarget { .. })
                ),
                "{url} should be refused"
            );
        }
        let longest = format!("kafka://localhost:9092/{}", "t".repeat(249));
        assert!(KafkaTarget::parse(&longest).is_ok());
    }

    #[test]
    fn since_accepts_rfc3339_and_epoch_millis() -> Result<(), KafkaSourceError> {
        assert_eq!(parse_since("1790000000123")?, 1_790_000_000_123);
        assert_eq!(parse_since("2026-09-27T12:00:00Z")?, 1_790_510_400_000);
        assert_eq!(parse_since("2026-09-27T05:00:00-07:00")?, 1_790_510_400_000);
        assert!(matches!(
            parse_since("yesterday"),
            Err(KafkaSourceError::InvalidSince { .. })
        ));
        Ok(())
    }

    #[test]
    fn envelope_bytes_are_pinned() {
        let encoded = envelope(
            "orders",
            3,
            &record(Some(b"k1"), Some(b"{\"b\":1, \"a\":2}"), 42),
        );
        assert_eq!(
            encoded,
            r#"{"key":"k1","offset":42,"partition":3,"timestamp_ms":1790000000123,"topic":"orders","value":"{\"b\":1, \"a\":2}"}"#
        );
    }

    #[test]
    fn envelope_encodes_missing_and_binary_bytes() {
        let encoded = envelope("t", 0, &record(None, Some(&[0xff, 0x00, 0x10]), 7));
        assert_eq!(
            encoded,
            r#"{"key":null,"offset":7,"partition":0,"timestamp_ms":1790000000123,"topic":"t","value":{"hex":"ff0010"}}"#
        );
    }

    #[test]
    fn equal_values_at_different_offsets_encode_differently() {
        let first = envelope("t", 0, &record(None, Some(b"tick"), 1));
        let second = envelope("t", 0, &record(None, Some(b"tick"), 2));
        assert_ne!(first, second);
        assert_eq!(first, envelope("t", 0, &record(None, Some(b"tick"), 1)));
    }

    /// Against a real broker (decision 0006's measurement): assignment reads, timestamp starts,
    /// the after-the-last-record case, resume-after, and a compressed batch.
    #[test]
    #[ignore = "needs a local broker: S2W_KAFKA_BROKER=localhost:19092"]
    fn reads_by_assignment_against_a_real_broker() -> Result<(), Box<dyn std::error::Error>> {
        use rskafka::client::ClientBuilder;
        use rskafka::client::partition::{Compression, UnknownTopicHandling};
        use tokio_stream::StreamExt;

        use super::{KafkaConnection, KafkaStart};

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
            let mut source = connection.start(&starts).await?;
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
            let source = KafkaConnection::open(&target).await?.start(&starts).await?;
            assert_eq!(source.start_offsets(), [(0, 5), (1, 4), (2, 5)]);
            drop(source);

            // A stored cursor past the end is loud, never a silent restart.
            let starts = BTreeMap::from([(0, KafkaStart::After(10))]);
            match KafkaConnection::open(&target).await?.start(&starts).await {
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

    #[test]
    fn only_fetch_errors_are_non_fatal() {
        let fetch = KafkaSourceError::Fetch {
            partition: 0,
            offset: 1,
            message: String::new(),
        };
        let pruned = KafkaSourceError::CursorPruned {
            partition: 0,
            next: 1,
            earliest: 5,
        };
        assert!(!fetch.is_fatal());
        assert!(pruned.is_fatal());
    }
}
