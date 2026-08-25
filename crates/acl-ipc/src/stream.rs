//! [`crate::Transport`] over anything that reads and writes bytes.
//!
//! A named pipe is `Read + Write`, so the part that can be wrong is not the pipe — it is
//! what happens between one and a frame.
//! Four things go wrong with a length-prefixed protocol on a stream, and none of them is
//! visible in a test that hands the decoder a whole frame at once:
//!
//! 1. **A frame arrives in pieces.** A pipe returns what it has, not what was asked for.
//! 2. **Several frames arrive in one read**, and the second must not be lost with the
//!    buffer.
//! 3. **A length prefix is nonsense**, because the writer died mid-frame. Four bytes of
//!    whatever was in the pipe becomes a four-gigabyte allocation if it is believed.
//! 4. **The peer closes mid-frame.** That is a truncated message, not a clean shutdown,
//!    and the two must not be reported the same way.
//!
//! The buffer is bounded by [`crate::MAX_FRAME`] plus the prefix, so a peer that never
//! sends a complete frame cannot grow it without limit either.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::{FrameError, LENGTH_PREFIX, MAX_FRAME, Transport};

/// A [`Transport`] over one byte stream.
///
/// Owns a read buffer because frames do not arrive whole; everything left over after a
/// message is kept for the next call.
#[derive(Debug)]
pub struct StreamTransport<S> {
    stream: S,
    buffer: Vec<u8>,
    closed: bool,
}

impl<S> StreamTransport<S> {
    /// Wraps a stream.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
            closed: false,
        }
    }

    /// How many bytes are held from a partial frame. For tests, and for a log line.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }

    /// Gives the stream back.
    pub fn into_inner(self) -> S {
        self.stream
    }
}

/// The most the buffer may hold: one whole frame and its prefix, and nothing beyond.
const BUFFER_LIMIT: usize = LENGTH_PREFIX + MAX_FRAME;

impl<S: Read + Write> Transport for StreamTransport<S> {
    fn send<T: Serialize>(&mut self, message: &T) -> Result<(), FrameError> {
        let frame = crate::encode(message)?;
        self.stream.write_all(&frame)?;
        // A pipe that buffers a frame until the next one arrives turns a 5 Hz state feed
        // into a 5 Hz feed one frame behind, which reads as latency nobody can find.
        self.stream.flush()?;
        Ok(())
    }

    fn recv<T: for<'de> Deserialize<'de>>(&mut self) -> Result<Option<T>, FrameError> {
        loop {
            if let Some((message, consumed)) = crate::decode::<T>(&self.buffer)? {
                self.buffer.drain(..consumed);
                return Ok(Some(message));
            }

            if self.closed {
                // Nothing left to read and not a whole frame in hand. An empty buffer is
                // the peer shutting down; anything else is a message cut in half, and
                // reporting that as a clean close would hide a helper that crashed.
                return if self.buffer.is_empty() {
                    Ok(None)
                } else {
                    Err(FrameError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!(
                            "peer closed with {} bytes of an incomplete frame",
                            self.buffer.len()
                        ),
                    )))
                };
            }

            // Bounded, because a peer that sends a prefix and then nothing else would
            // otherwise be answered by reading forever.
            if self.buffer.len() >= BUFFER_LIMIT {
                return Err(FrameError::TooLarge(self.buffer.len()));
            }

            let mut chunk = [0u8; 4096];
            let read = self.stream.read(&mut chunk)?;
            if read == 0 {
                self.closed = true;
                continue;
            }
            // `read` should never exceed the buffer, and a stream that says it did is
            // broken in a way that must not be believed: indexing on its word is how a
            // misreporting pipe becomes an out-of-bounds read.
            let Some(fresh) = chunk.get(..read) else {
                return Err(FrameError::Io(std::io::Error::other(format!(
                    "the stream reported {read} bytes into a {} byte buffer",
                    chunk.len()
                ))));
            };
            self.buffer.extend_from_slice(fresh);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    /// A stream that hands back at most `chunk` bytes per read, which is what a pipe does
    /// and what a `Cursor` does not.
    struct Trickle {
        data: Vec<u8>,
        position: usize,
        chunk: usize,
        written: Vec<u8>,
    }

    impl Trickle {
        fn new(data: Vec<u8>, chunk: usize) -> Self {
            Self {
                data,
                position: 0,
                chunk,
                written: Vec::new(),
            }
        }
    }

    impl Read for Trickle {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let remaining = self.data.len() - self.position;
            let take = remaining.min(self.chunk).min(out.len());
            out[..take].copy_from_slice(&self.data[self.position..self.position + take]);
            self.position += take;
            Ok(take)
        }
    }

    impl Write for Trickle {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn frames(messages: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for message in messages {
            out.extend_from_slice(&crate::encode(&message.to_string()).unwrap());
        }
        out
    }

    #[test]
    fn a_frame_split_across_reads_is_reassembled() {
        // One byte at a time, which is legal and which a `Cursor`-based test never sees.
        let mut transport = StreamTransport::new(Trickle::new(frames(&["hello"]), 1));
        let message: Option<String> = transport.recv().unwrap();
        assert_eq!(message.as_deref(), Some("hello"));
    }

    #[test]
    fn several_frames_in_one_read_are_not_lost_with_the_buffer() {
        // The second frame is in the buffer when the first is returned. A decoder that
        // cleared the buffer after each message would drop it, and the symptom is every
        // other state update going missing under load.
        let mut transport =
            StreamTransport::new(Trickle::new(frames(&["one", "two", "three"]), 4096));
        for expected in ["one", "two", "three"] {
            let message: Option<String> = transport.recv().unwrap();
            assert_eq!(message.as_deref(), Some(expected));
        }
        assert_eq!(transport.buffered(), 0);
    }

    #[test]
    fn a_clean_close_is_not_an_error() {
        let mut transport = StreamTransport::new(Trickle::new(frames(&["only"]), 4096));
        let first: Option<String> = transport.recv().unwrap();
        assert_eq!(first.as_deref(), Some("only"));
        let then: Option<String> = transport.recv().unwrap();
        assert!(then.is_none());
        // And it stays closed rather than blocking again.
        let again: Option<String> = transport.recv().unwrap();
        assert!(again.is_none());
    }

    #[test]
    fn a_peer_that_dies_mid_frame_is_not_a_clean_close() {
        // The difference between "the helper exited" and "the helper crashed while
        // writing". Reporting the second as the first loses the only evidence there was.
        let mut whole = frames(&["a message long enough to be cut in half"]);
        whole.truncate(whole.len() - 5);
        let mut transport = StreamTransport::new(Trickle::new(whole, 4096));
        let error = transport.recv::<String>().unwrap_err();
        assert!(
            matches!(&error, FrameError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof),
            "{error:?}"
        );
    }

    #[test]
    fn a_nonsense_length_prefix_is_refused_before_anything_is_allocated() {
        // Four bytes of whatever was in the pipe when the writer died. Believed, it is a
        // four-gigabyte allocation; the check is before the buffer is indexed.
        let mut nonsense = u32::MAX.to_le_bytes().to_vec();
        nonsense.extend_from_slice(b"whatever followed");
        let mut transport = StreamTransport::new(Trickle::new(nonsense, 4096));
        let error = transport.recv::<String>().unwrap_err();
        assert!(matches!(error, FrameError::TooLarge(_)), "{error:?}");
    }

    #[test]
    fn a_peer_that_never_completes_a_frame_does_not_grow_the_buffer_forever() {
        // A plausible prefix followed by an endless dribble that never reaches it. Without
        // the bound this reads until the process dies.
        let mut endless = u32::try_from(MAX_FRAME).unwrap().to_le_bytes().to_vec();
        endless.extend(std::iter::repeat_n(0u8, MAX_FRAME - 1));
        let mut transport = StreamTransport::new(Trickle::new(endless, 4096));
        // The stream runs out before the frame completes, so this is the truncation case
        // rather than the bound — the bound is what stops it before that on a live pipe.
        assert!(transport.recv::<String>().is_err());
        assert!(transport.buffered() <= BUFFER_LIMIT);
    }

    #[test]
    fn what_is_sent_is_a_frame_the_decoder_accepts() {
        let mut transport = StreamTransport::new(Trickle::new(Vec::new(), 4096));
        transport.send(&"round trip".to_string()).unwrap();
        let written = transport.into_inner().written;
        assert_eq!(written, frames(&["round trip"]));
    }
}
