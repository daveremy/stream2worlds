//! A JSON response body written on a blocking thread into a bounded channel (#216): the
//! serializer never holds more than [`CHUNKS_IN_FLIGHT`] chunks of [`CHUNK_BYTES`], whatever
//! the size of the body.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use tokio::sync::mpsc::{self, error::TrySendError};

/// Bytes per chunk handed to the response body.
pub(crate) const CHUNK_BYTES: usize = 64 * 1024;

/// Chunks the channel holds before the writer waits for the client.
pub(crate) const CHUNKS_IN_FLIGHT: usize = 64;

/// How long the writer waits for room in the channel before giving up. The `/world` writer
/// holds no timeline guard (s2w#272), so a client that stops reading no longer blocks appends;
/// this bounds how long it keeps its generation (and every subscriber sharing it, and the
/// requests queued behind it) waiting per chunk.
pub(crate) const STALL: Duration = Duration::from_secs(5);

/// How long the writer may wait for the client in total, counted from the channel's creation and
/// checked whenever the channel is full. [`STALL`] bounds one chunk, so a client reading just
/// under it per chunk could otherwise hold its generation, and the queue behind it, for hours on
/// a large body; past this, the body ends with an error. A client that keeps up is never cut.
/// A local read of the recorded load's ~190 MiB world takes 2-4 s (#216).
pub(crate) const BODY_BUDGET: Duration = Duration::from_secs(60);

/// The longest the writer sleeps between tries while the channel is full.
const MAX_BACKOFF: Duration = Duration::from_millis(50);

/// One message on a body's channel. Cloning a `Data` chunk shares its bytes, so a
/// generation serving several subscribers (s2w#270) copies nothing per subscriber.
#[derive(Clone)]
enum Chunk {
    Data(Bytes),
    /// The body is complete. A channel that closes without it was cut short.
    End,
}

/// A connected writer and one body stream.
#[cfg(test)]
fn channel() -> (ChunkWriter, ChunkStream) {
    let (writer, mut streams) = fan_out(1);
    (writer, streams.remove(0))
}

/// One writer feeding `count` body streams (s2w#270): every stream gets every chunk, each
/// through its own bounded channel with its own [`STALL`] and [`BODY_BUDGET`] cut.
pub(crate) fn fan_out(count: usize) -> (ChunkWriter, Vec<ChunkStream>) {
    fan_out_within(std::iter::repeat_n(BODY_BUDGET, count))
}

/// [`fan_out`] with one whole-body budget per stream.
fn fan_out_within(budgets: impl Iterator<Item = Duration>) -> (ChunkWriter, Vec<ChunkStream>) {
    let now = Instant::now();
    let (subscribers, streams) = budgets
        .map(|budget| {
            let body_deadline = now + budget;
            let (tx, rx) = mpsc::channel(CHUNKS_IN_FLIGHT);
            (
                Subscriber { tx, body_deadline },
                ChunkStream { rx, done: false },
            )
        })
        .unzip();
    (
        ChunkWriter {
            buf: Vec::with_capacity(CHUNK_BYTES),
            subscribers,
        },
        streams,
    )
}

/// One body stream's sending end.
struct Subscriber {
    tx: mpsc::Sender<Chunk>,
    /// When the whole body must be sent by ([`BODY_BUDGET`]).
    body_deadline: Instant,
}

impl Subscriber {
    /// Sends one chunk, waiting at most [`STALL`] for room and never past the body's deadline.
    /// Never blocks on the runtime, so it is safe on a `spawn_blocking` thread of a
    /// current-thread runtime; it polls with a sleep that backs off to [`MAX_BACKOFF`].
    fn send(&self, chunk: Chunk) -> io::Result<()> {
        let deadline = (Instant::now() + STALL).min(self.body_deadline);
        let mut chunk = chunk;
        let mut backoff = Duration::from_millis(1);
        loop {
            match self.tx.try_send(chunk) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Closed(_)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "the client went away",
                    ));
                }
                Err(TrySendError::Full(back)) => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "the client read too slowly",
                        ));
                    }
                    chunk = back;
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
        }
    }
}

/// The blocking half: buffers writes into chunks and sends each full one to every live
/// subscriber. Call [`ChunkWriter::finish`] after the last write; dropping it unfinished ends
/// every body with an error, so no client sees a truncated body as a complete one.
///
/// A subscriber whose send fails (gone, or cut at [`STALL`] / [`BODY_BUDGET`]) is dropped,
/// which ends its body with an error; the others keep receiving. A write fails only once no
/// subscriber is left, with the last subscriber's error. Subscribers are sent to in turn, so a
/// stalled one delays the others by up to [`STALL`] per chunk before it is cut.
pub(crate) struct ChunkWriter {
    buf: Vec<u8>,
    subscribers: Vec<Subscriber>,
}

impl ChunkWriter {
    /// Sends the buffered tail and marks every body complete.
    ///
    /// # Errors
    /// As [`io::Write::write`].
    pub(crate) fn finish(mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            let tail = Bytes::from(std::mem::take(&mut self.buf));
            self.broadcast(&Chunk::Data(tail))?;
        }
        self.broadcast(&Chunk::End)
    }

    /// Sends `chunk` to every subscriber, dropping those whose send fails.
    fn broadcast(&mut self, chunk: &Chunk) -> io::Result<()> {
        let mut last_error = None;
        self.subscribers
            .retain(|subscriber| match subscriber.send(chunk.clone()) {
                Ok(()) => true,
                Err(error) => {
                    last_error = Some(error);
                    false
                }
            });
        match (self.subscribers.is_empty(), last_error) {
            (false, _) => Ok(()),
            (true, Some(error)) => Err(error),
            (true, None) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "no client is left",
            )),
        }
    }
}

impl io::Write for ChunkWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() >= CHUNK_BYTES {
            let full = std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK_BYTES));
            self.broadcast(&Chunk::Data(Bytes::from(full)))?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The async half: the response body's chunks, then an error if the writer stopped before
/// [`ChunkWriter::finish`] (a panic, a serialization error, a stalled client).
pub(crate) struct ChunkStream {
    rx: mpsc::Receiver<Chunk>,
    done: bool,
}

impl tokio_stream::Stream for ChunkStream {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        match this.rx.poll_recv(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Chunk::Data(bytes))) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(Some(Chunk::End)) => {
                this.done = true;
                Poll::Ready(None)
            }
            Poll::Ready(None) => {
                this.done = true;
                Poll::Ready(Some(Err(io::Error::other(
                    "the body ended before it was complete",
                ))))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tokio_stream::StreamExt;

    use super::*;

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
            .block_on(f)
    }

    #[test]
    fn a_finished_writer_delivers_every_byte_in_order() {
        let (mut writer, mut stream) = channel();
        let bytes: Vec<u8> = (0..3 * CHUNK_BYTES + 17).map(|i| (i % 251) as u8).collect();
        let reader = std::thread::spawn(move || {
            run(async move {
                let mut got = Vec::new();
                while let Some(chunk) = stream.next().await {
                    got.extend(chunk.expect("no error"));
                }
                got
            })
        });
        writer.write_all(&bytes).expect("write");
        writer.finish().expect("finish");
        assert_eq!(reader.join().expect("reader"), bytes);
    }

    #[test]
    fn an_unfinished_writer_ends_the_body_with_an_error() {
        let (mut writer, mut stream) = channel();
        writer.write_all(b"{\"nodes\":[").expect("write");
        drop(writer);
        let items = run(async move {
            let mut items = Vec::new();
            while let Some(item) = stream.next().await {
                items.push(item.is_ok());
            }
            items
        });
        // The partial buffer never reached a full chunk, so the only item is the error.
        assert_eq!(items, [false]);
    }

    #[test]
    fn a_gone_client_fails_the_write() {
        let (mut writer, stream) = channel();
        drop(stream);
        let err = writer
            .write_all(&vec![0; CHUNK_BYTES])
            .expect_err("closed channel");
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn a_slow_client_is_cut_off_at_the_body_budget() {
        // A zero budget: the first full channel ends the body even though no single chunk
        // waited out STALL.
        let (mut writer, _streams) = fan_out_within(std::iter::once(Duration::ZERO));
        let started = Instant::now();
        let chunk = vec![0; CHUNK_BYTES];
        let err = (0..=CHUNKS_IN_FLIGHT)
            .find_map(|_| writer.write_all(&chunk).err())
            .expect("budget spent");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < STALL,
            "cut off by the budget, not the stall"
        );
    }

    #[test]
    fn a_fan_out_delivers_the_same_bytes_to_every_stream() {
        let (mut writer, streams) = fan_out(2);
        let bytes: Vec<u8> = (0..2 * CHUNK_BYTES + 5).map(|i| (i % 241) as u8).collect();
        let readers: Vec<_> = streams
            .into_iter()
            .map(|mut stream| {
                std::thread::spawn(move || {
                    run(async move {
                        let mut got = Vec::new();
                        while let Some(chunk) = stream.next().await {
                            got.extend(chunk.expect("no error"));
                        }
                        got
                    })
                })
            })
            .collect();
        writer.write_all(&bytes).expect("write");
        writer.finish().expect("finish");
        for reader in readers {
            assert_eq!(reader.join().expect("reader"), bytes);
        }
    }

    #[test]
    fn a_cut_stream_leaves_the_others_whole() {
        // The first stream is never read and has no budget: the first full channel cuts it.
        let (mut writer, mut streams) = fan_out_within([Duration::ZERO, BODY_BUDGET].into_iter());
        let mut reading = streams.pop().expect("second");
        let mut stalled = streams.pop().expect("first");
        let bytes: Vec<u8> = (0..(CHUNKS_IN_FLIGHT + 3) * CHUNK_BYTES)
            .map(|i| (i % 239) as u8)
            .collect();
        let reader = std::thread::spawn(move || {
            run(async move {
                let mut got = Vec::new();
                while let Some(chunk) = reading.next().await {
                    got.extend(chunk.expect("no error"));
                }
                got
            })
        });
        // Chunk by chunk: one write of everything would be sent as a single chunk.
        for piece in bytes.chunks(CHUNK_BYTES) {
            writer.write_all(piece).expect("one stream is still live");
        }
        writer.finish().expect("finish");
        assert_eq!(reader.join().expect("reader"), bytes);
        let items = run(async move {
            let mut items = Vec::new();
            while let Some(item) = stalled.next().await {
                items.push(item.is_ok());
            }
            items
        });
        assert_eq!(
            items.len(),
            CHUNKS_IN_FLIGHT + 1,
            "a full channel, then the error"
        );
        assert_eq!(
            items.last(),
            Some(&false),
            "the cut body ends with an error"
        );
    }

    #[test]
    fn a_fan_out_fails_once_every_stream_is_gone() {
        let (mut writer, streams) = fan_out(2);
        drop(streams);
        let err = writer
            .write_all(&vec![0; CHUNK_BYTES])
            .expect_err("no stream left");
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
    }
}
