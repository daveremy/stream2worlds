//! Kafka by explicit partition assignment, through `rskafka` (decision 0007).
//!
//! The source discovers the topic's partitions once, resolves one start offset per partition,
//! and runs one fetch task per partition. It never joins a consumer group and never commits
//! offsets: `rskafka` has neither concept, so the invariant holds by construction. Each record
//! leaves as a byte-deterministic JSON envelope that carries its own offset, so the log's
//! `(source, payload hash)` dedupe collapses a real redelivery but never two distinct records
//! whose values happen to be equal.

mod envelope;
mod fetch;

use std::collections::BTreeMap;

use s2w_model::{Cursor, RawEvent, SourceId, Timestamp};
use tokio_stream::StreamExt;

use crate::source::{
    CursorLookup, Ending, Source, SourceError, StartFuture, Started, refuse_since_with_stored,
};

pub(crate) use fetch::KafkaConnection;

/// The longest topic name guaranteed to fit under [`SourceId`]'s 128-byte cap once wrapped as
/// `kafka.<16-hex-cluster>.<topic>.p<partition>`: 128 minus `"kafka."` (6) minus the cluster's
/// 16 hex digits minus the two `.` separators minus `"p"` plus up to 10 digits for an `i32`
/// partition number.
const MAX_TOPIC_LEN: usize = 128 - 6 - 16 - 2 - 1 - 10;

/// The broker list and topic from a `kafka://<broker>[,<broker>…]/<topic>` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KafkaTarget {
    brokers: Vec<String>,
    topic: String,
}

impl KafkaTarget {
    /// Parses `kafka://host:port[,host:port…]/topic`.
    ///
    /// The topic must satisfy Kafka's own rules (1 to 249 bytes of ASCII letters, digits,
    /// `.`, `_` and `-`, and not `.` or `..`) AND fit under [`SourceId`]'s 128-byte cap once
    /// wrapped as `kafka.<16-hex-cluster>.<topic>.p<partition>` (`partition_source`) — Kafka's
    /// own limit is looser than ours, so this checks the tighter of the two up front instead
    /// of letting a topic that Kafka accepts fail later, at `start()`, naming the derived
    /// source id rather than "topic too long".
    ///
    /// # Errors
    /// Returns [`KafkaSourceError::InvalidTarget`] naming what is wrong.
    pub(crate) fn parse(url: &str) -> Result<Self, KafkaSourceError> {
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
            && topic.len() <= MAX_TOPIC_LEN
            && topic != "."
            && topic != ".."
            && topic
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !valid_topic {
            return Err(invalid(&format!(
                "topic must be 1-{MAX_TOPIC_LEN} ASCII letters, digits, '.', '_' or '-'"
            )));
        }
        Ok(Self {
            brokers,
            topic: topic.to_owned(),
        })
    }

    /// The bootstrap brokers, as given.
    #[cfg(test)]
    #[must_use]
    fn brokers(&self) -> &[String] {
        &self.brokers
    }

    /// The topic name.
    #[must_use]
    pub(crate) fn topic(&self) -> &str {
        &self.topic
    }
}

/// Where one partition starts reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KafkaStart {
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
pub(crate) fn parse_since(value: &str) -> Result<i64, KafkaSourceError> {
    crate::since::parse_since(value).map_err(|reason| KafkaSourceError::InvalidSince {
        value: value.to_owned(),
        reason,
    })
}

/// One Kafka record, ready for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KafkaEvent {
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
pub(crate) enum KafkaSourceError {
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

#[cfg(test)]
impl KafkaSourceError {
    /// Whether the source has stopped reading because of this error. A non-fatal error is
    /// reported and the source keeps going. Test-only: production code maps errors through
    /// [`seam_error`] directly; this exists to assert that mapping's fatality independently.
    #[must_use]
    fn is_fatal(&self) -> bool {
        !matches!(self, Self::Fetch { .. })
    }
}

/// The adapter's name in messages.
const NAME: &str = "kafka";

/// The `kafka://` adapter: every partition of one topic by explicit assignment, one log source
/// per partition (`kafka.<cluster>.<topic>.p<partition>`), each resumed after its stored
/// offset. The cluster component is derived from the broker list so two distinct clusters that
/// happen to share a topic name never share cursors.
///
/// Invariant: this adapter never yields [`SourceError::Skipped`]. Payloads are the
/// byte-deterministic envelope with no decode step, and a failed fetch is
/// [`SourceError::Retrying`] from the same offset, never a skip.
#[derive(Debug)]
pub(crate) struct KafkaAdapter {
    target: KafkaTarget,
}

impl KafkaAdapter {
    /// Parses a `kafka://<broker>[,<broker>…]/<topic>` URI.
    ///
    /// # Errors
    ///
    /// [`SourceError::InvalidTarget`] when the URI is malformed.
    pub(crate) fn parse(uri: &str) -> Result<Self, SourceError> {
        KafkaTarget::parse(uri)
            .map(|target| Self { target })
            .map_err(|error| SourceError::InvalidTarget {
                name: NAME,
                uri: uri.to_owned(),
                reason: error.to_string(),
            })
    }
}

/// A collision-resistant identity for one Kafka cluster: its brokers, order-independent.
/// Two targets naming the same brokers in a different order collapse to the same id; two
/// distinct broker sets do not — not even ones a naive separator-based join could confuse
/// (`["a:9092","b:9093"]` vs `["a_9092_b:9093"]`, both of which sanitize to the same string).
/// Each broker is length-prefixed before hashing so no choice of separator can make two
/// different broker lists concatenate to the same bytes.
fn cluster_id(brokers: &[String]) -> String {
    let mut sorted: Vec<&str> = brokers.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut canonical = String::new();
    for broker in sorted {
        canonical.push_str(&broker.len().to_string());
        canonical.push(':');
        canonical.push_str(broker);
    }
    s2w_model::fnv1a64_hex(canonical.as_bytes())
}

/// The log source id of one partition: `kafka.<cluster>.<topic>.p<partition>`. The cluster
/// component keeps two clusters that happen to share a topic name from sharing cursors.
fn partition_source(target: &KafkaTarget, partition: i32) -> Result<SourceId, SourceError> {
    Ok(SourceId::new(format!(
        "kafka.{}.{}.p{partition}",
        cluster_id(&target.brokers),
        target.topic
    ))?)
}

/// Decodes a stored partition cursor: the decimal offset of the last stored record.
fn stored_offset(source: &SourceId, cursor: &Cursor) -> Result<i64, SourceError> {
    std::str::from_utf8(cursor.as_bytes())
        .ok()
        .and_then(|text| text.parse::<i64>().ok())
        .filter(|offset| *offset >= 0)
        .ok_or_else(|| SourceError::StoredCursor {
            source_id: source.as_str().to_owned(),
            cursor: String::from_utf8_lossy(cursor.as_bytes()).into_owned(),
            reason: "not a non-negative decimal Kafka offset".to_owned(),
        })
}

/// Maps a Kafka error onto the seam: a fetch retries, a cursor that cannot resume is
/// [`SourceError::CursorUnresumable`], anything else stops the source.
fn seam_error(target: &KafkaTarget, error: KafkaSourceError) -> SourceError {
    match &error {
        KafkaSourceError::Fetch { .. } => SourceError::Retrying {
            name: NAME,
            reason: error.to_string(),
        },
        KafkaSourceError::CursorPruned { partition, .. }
        | KafkaSourceError::CursorAhead { partition, .. } => SourceError::CursorUnresumable {
            source_id: format!(
                "kafka.{}.{}.p{partition}",
                cluster_id(&target.brokers),
                target.topic
            ),
            reason: error.to_string(),
        },
        KafkaSourceError::InvalidSince { value, .. } => SourceError::InvalidSince {
            name: NAME,
            value: value.clone(),
            reason: error.to_string(),
        },
        _ => SourceError::Fatal {
            name: NAME,
            reason: error.to_string(),
        },
    }
}

impl Source for KafkaAdapter {
    fn name(&self) -> &'static str {
        NAME
    }

    fn start<'a>(
        self: Box<Self>,
        since: Option<&'a str>,
        cursors: &'a dyn CursorLookup,
    ) -> StartFuture<'a> {
        Box::pin(async move {
            let target = self.target.clone();
            let topic = target.topic().to_owned();
            let error = |error| seam_error(&target, error);
            let since_millis = since.map(parse_since).transpose().map_err(error)?;
            // Every topic has partition 0, so this refuses the common case before connecting.
            refuse_since_with_stored(since, cursors, &[partition_source(&target, 0)?])?;

            let connection = KafkaConnection::open(&self.target).await.map_err(error)?;
            let mut sources = BTreeMap::new();
            let mut starts = BTreeMap::new();
            let mut fresh = Vec::new();
            let mut stored = Vec::new();
            let mut removed = None;
            for &partition in connection.partitions() {
                let source = partition_source(&target, partition)?;
                if !cursors.is_member(&source)? {
                    removed = Some(source);
                    continue;
                }
                let start = match cursors.cursor(&source)? {
                    Some(cursor) => {
                        stored.push(source.clone());
                        KafkaStart::After(stored_offset(&source, &cursor)?)
                    }
                    None => {
                        fresh.push(partition);
                        since_millis.map_or(KafkaStart::Latest, KafkaStart::Timestamp)
                    }
                };
                starts.insert(partition, start);
                sources.insert(partition, source);
            }
            refuse_since_with_stored(since, cursors, &stored)?;

            if sources.is_empty()
                && let Some(source) = removed
            {
                return Ok(Started::removed(source));
            }
            let identities = sources.values().cloned().collect();
            let running = connection.start(&starts).await.map_err(error)?;
            let mut notes = Vec::new();
            if !stored.is_empty() {
                // A partition added since the last run has no cursor; it starts at its end.
                for (partition, offset) in running.start_offsets() {
                    if fresh.contains(partition) {
                        notes.push(format!(
                            "partition {partition} of {topic:?} has no stored cursor; starting at offset {offset}"
                        ));
                    }
                }
            }
            let stream = running.map(move |item| match item {
                Ok(event) => raw(&sources, event),
                Err(error) => Err(seam_error(&target, error)),
            });
            Ok(Started {
                sources: identities,
                stream: Box::pin(stream),
                ends: Ending::Never,
                notes,
            })
        })
    }
}

/// One Kafka record as a log event under its partition's source id.
fn raw(sources: &BTreeMap<i32, SourceId>, event: KafkaEvent) -> Result<RawEvent, SourceError> {
    let source = sources
        .get(&event.partition)
        .ok_or_else(|| SourceError::Fatal {
            name: NAME,
            reason: format!(
                "delivered a record from unassigned partition {}",
                event.partition
            ),
        })?;
    Ok(RawEvent {
        source: source.clone(),
        cursor: Cursor::new(event.offset.to_string().into_bytes())?,
        received_at: event.received_at,
        payload: event.payload.into_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use s2w_model::{Cursor, SourceId};

    use super::{
        KafkaAdapter, KafkaSourceError, KafkaTarget, MAX_TOPIC_LEN, cluster_id, parse_since,
        seam_error, stored_offset,
    };
    use crate::source::{CursorLookup, Source, SourceError};

    /// A log holding exactly one stored cursor.
    struct OneCursor(String, &'static [u8]);

    impl CursorLookup for OneCursor {
        fn cursor(&self, source: &SourceId) -> Result<Option<Cursor>, SourceError> {
            if source.as_str() == self.0 {
                Ok(Some(Cursor::new(self.1.to_vec())?))
            } else {
                Ok(None)
            }
        }
    }

    fn start(since: Option<&str>, cursors: &dyn CursorLookup) -> Result<(), SourceError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| SourceError::Lookup(error.to_string()))?;
        runtime.block_on(async {
            let adapter = Box::new(KafkaAdapter::parse("kafka://127.0.0.1:1/orders")?);
            adapter.start(since, cursors).await.map(|_| ())
        })
    }

    // Each start below fails before any connection is attempted, so these run offline.

    #[test]
    fn since_with_a_stored_partition_cursor_is_refused_before_connecting() {
        let source_id = format!(
            "kafka.{}.orders.p0",
            cluster_id(&["127.0.0.1:1".to_owned()])
        );
        let outcome = start(Some("2026-09-27T00:00:00Z"), &OneCursor(source_id, b"41"));
        assert!(
            matches!(&outcome, Err(error @ SourceError::SinceWithStoredCursor { .. })
                if error.to_string().contains("drop --since")),
            "got {outcome:?}"
        );
    }

    #[test]
    fn an_invalid_since_is_refused_before_connecting() {
        let outcome = start(Some("yesterday"), &OneCursor("none".to_owned(), b"0"));
        assert!(
            matches!(
                outcome,
                Err(SourceError::InvalidSince { name: "kafka", .. })
            ),
            "got {outcome:?}"
        );
    }

    #[test]
    fn stored_offsets_decode_or_fail_loudly() {
        let Ok(source) = SourceId::new("kafka.orders.p0") else {
            panic!("a valid source id")
        };
        let decode = |bytes: &[u8]| {
            Cursor::new(bytes.to_vec())
                .map_err(SourceError::from)
                .and_then(|cursor| stored_offset(&source, &cursor))
        };
        assert!(matches!(decode(b"41"), Ok(41)));
        for bad in [&b"-1"[..], b"4x", &[0xff, 0xfe]] {
            assert!(
                matches!(decode(bad), Err(SourceError::StoredCursor { .. })),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn kafka_errors_never_map_to_skipped() {
        let errors = [
            KafkaSourceError::Fetch {
                partition: 0,
                offset: 1,
                message: String::new(),
            },
            KafkaSourceError::CursorPruned {
                partition: 2,
                next: 1,
                earliest: 5,
            },
            KafkaSourceError::CursorAhead {
                partition: 2,
                next: 9,
                latest: 5,
            },
            KafkaSourceError::Connect(String::new()),
            KafkaSourceError::UnknownTopic("t".to_owned()),
        ];
        let target = KafkaTarget::parse("kafka://b:1/t").expect("valid target");
        for error in errors {
            let mapped = seam_error(&target, error);
            assert!(
                !matches!(mapped, SourceError::Skipped { .. }),
                "kafka must never skip: {mapped:?}"
            );
        }
        assert!(matches!(
            seam_error(
                &target,
                KafkaSourceError::CursorPruned {
                    partition: 2,
                    next: 1,
                    earliest: 5
                }
            ),
            SourceError::CursorUnresumable { source_id, .. }
                if source_id == format!("kafka.{}.t.p2", cluster_id(&["b:1".to_owned()]))
        ));
    }

    #[test]
    fn cluster_id_is_order_independent_and_distinguishes_clusters() {
        let a = KafkaTarget::parse("kafka://a:9092,b:9093/t").expect("valid target");
        let b = KafkaTarget::parse("kafka://b:9093,a:9092/t").expect("valid target");
        let c = KafkaTarget::parse("kafka://c:9094/t").expect("valid target");
        assert_eq!(
            super::partition_source(&a, 0).unwrap(),
            super::partition_source(&b, 0).unwrap(),
            "same brokers in a different order must share a source id"
        );
        assert_ne!(
            super::partition_source(&a, 0).unwrap(),
            super::partition_source(&c, 0).unwrap(),
            "distinct clusters sharing a topic name must not share a source id"
        );
        // codex-review round 2: a naive separator-based join let two DISTINCT broker lists
        // sanitize to the same string ("a:9092,b:9093" and "a_9092_b:9093" both collapse
        // every non-alnum byte to '_'). Length-prefixed hashing must keep them apart.
        let ambiguous = KafkaTarget::parse("kafka://a_9092_b:9093/t").expect("valid target");
        assert_ne!(
            super::partition_source(&a, 0).unwrap(),
            super::partition_source(&ambiguous, 0).unwrap(),
            "a broker list must not collide with an unrelated single broker of the same \
             sanitized shape"
        );
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
        let too_long = format!("kafka://localhost:9092/{}", "t".repeat(MAX_TOPIC_LEN + 1));
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
        let longest = format!("kafka://localhost:9092/{}", "t".repeat(MAX_TOPIC_LEN));
        assert!(KafkaTarget::parse(&longest).is_ok());
    }

    #[test]
    fn shared_since_parser_errors_keep_the_kafka_variant() -> Result<(), KafkaSourceError> {
        assert_eq!(parse_since("1790000000123")?, 1_790_000_000_123);
        assert!(matches!(
            parse_since("yesterday"),
            Err(KafkaSourceError::InvalidSince { .. })
        ));
        Ok(())
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
