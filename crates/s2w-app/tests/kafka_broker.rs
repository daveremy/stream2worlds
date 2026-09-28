//! `s2w watch kafka://…` end to end against a real broker. Ignored by default:
//! `S2W_KAFKA_BROKER=localhost:19092 cargo test -p s2w-app --test kafka_broker -- --include-ignored`.

use std::path::{Path, PathBuf};

use s2w_app::{AppError, HumanReporter, WatchArgs, run_watch};
use s2w_log::{EventLog, SqliteEventLog};
use s2w_model::SourceId;

/// A per-test scratch directory under the system temp dir, removed on drop.
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        Self(std::env::temp_dir().join(format!("s2w-app-{name}-{}-{nanos}", std::process::id())))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

fn run<F: std::future::Future>(test: F) -> F::Output {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(test),
        Err(error) => panic!("building the test runtime should succeed: {error}"),
    }
}

/// End to end against a real broker: every partition into the log with per-partition
/// cursors, a restart that resumes without duplicates, `--since` refused on a resumed log,
/// and `--since` into a fresh log starting at the right offset.
#[test]
#[ignore = "needs a local broker: S2W_KAFKA_BROKER=localhost:19092"]
fn watches_resumes_and_replays_against_a_real_broker() -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use rskafka::chrono::DateTime;
    use rskafka::client::ClientBuilder;
    use rskafka::client::partition::{Compression, UnknownTopicHandling};
    use rskafka::record::Record;

    type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
    /// Every stored `(source, cursor)`, sorted, and the two partition cursors.
    type Contents = (Vec<(String, String)>, Vec<String>);

    let broker = std::env::var("S2W_KAFKA_BROKER")?;
    // Mirrors s2w_sources::kafka's private `cluster_id`: a length-prefixed hash of the sorted
    // broker list, so the source id includes cluster identity and two clusters sharing a
    // topic name never share cursors (#7 code review, round 2).
    fn fnv1a64_hex(bytes: &[u8]) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{hash:016x}")
    }
    let cluster = fnv1a64_hex(format!("{}:{broker}", broker.len()).as_bytes());
    run(async {
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
            let args = WatchArgs {
                uri: url.clone(),
                since,
                log_dir: log_dir.to_path_buf(),
                json: false,
                filters: Vec::new(),
            };
            async move {
                let mut reporter = HumanReporter;
                // The watch never ends on its own; stop it once it has had time to catch up.
                tokio::time::timeout(Duration::from_secs(4), run_watch(args, &mut reporter)).await
            }
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
                let source = SourceId::new(format!("kafka.{cluster}.{topic}.p{partition}"))?;
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
                    let cluster = cluster.clone();
                    offsets.clone().map(move |offset| {
                        (
                            format!("kafka.{cluster}.{topic}.p{partition}"),
                            offset.to_string(),
                        )
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
