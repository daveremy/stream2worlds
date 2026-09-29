//! A JSON response body written on a blocking thread into a bounded channel (#216): the
//! serializer never holds more than [`CHUNKS_IN_FLIGHT`] chunks of [`CHUNK_BYTES`], whatever
//! the size of the body.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, error::TrySendError};

/// Bytes per chunk handed to the response body.
pub(crate) const CHUNK_BYTES: usize = 64 * 1024;

/// Chunks the channel holds before the writer waits for the client.
pub(crate) const CHUNKS_IN_FLIGHT: usize = 64;

/// How long the writer waits for room in the channel before giving up. The writer may hold the
/// timeline's read guard, which blocks every append, so a client that stops reading must not
/// hold it for longer than this per chunk.
pub(crate) const STALL: Duration = Duration::from_secs(5);

/// How long the writer may wait for the client in total, counted from the channel's creation and
/// checked whenever the channel is full. [`STALL`] bounds one chunk, so a client reading just
/// under it per chunk could otherwise hold the read guard for hours on a large body; past this,
/// the body ends with an error and the guard is released. A client that keeps up is never cut. A local read of the recorded load's
/// ~190 MiB world takes 2-4 s (#216).
pub(crate) const BODY_BUDGET: Duration = Duration::from_secs(60);

/// The longest the writer sleeps between tries while the channel is full.
const MAX_BACKOFF: Duration = Duration::from_millis(50);

enum Chunk {
    Data(Vec<u8>),
    /// The body is complete. A channel that closes without it was cut short.
    End,
}

/// A connected writer and body stream.
pub(crate) fn channel() -> (ChunkWriter, ChunkStream) {
    channel_within(BODY_BUDGET)
}

/// [`channel`] with a whole-body `budget`.
fn channel_within(budget: Duration) -> (ChunkWriter, ChunkStream) {
    let (tx, rx) = mpsc::channel(CHUNKS_IN_FLIGHT);
    (
        ChunkWriter {
            buf: Vec::with_capacity(CHUNK_BYTES),
            tx,
            body_deadline: Instant::now() + budget,
        },
        ChunkStream { rx, done: false },
    )
}

/// The blocking half: buffers writes into chunks and sends each full one. Call
/// [`ChunkWriter::finish`] after the last write; dropping it unfinished ends the body with an
/// error, so the client never sees a truncated body as a complete one.
pub(crate) struct ChunkWriter {
    buf: Vec<u8>,
    tx: mpsc::Sender<Chunk>,
    /// When the whole body must be sent by ([`BODY_BUDGET`]).
    body_deadline: Instant,
}

impl ChunkWriter {
    /// Sends the buffered tail and marks the body complete.
    ///
    /// # Errors
    /// As [`io::Write::write`].
    pub(crate) fn finish(mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            let tail = std::mem::take(&mut self.buf);
            self.send(Chunk::Data(tail))?;
        }
        self.send(Chunk::End)
    }

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

impl io::Write for ChunkWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() >= CHUNK_BYTES {
            let full = std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK_BYTES));
            self.send(Chunk::Data(full))?;
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
    type Item = io::Result<Vec<u8>>;

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
        let (mut writer, _stream) = channel_within(Duration::ZERO);
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
}
