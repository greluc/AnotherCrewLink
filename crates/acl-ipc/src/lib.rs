//! The boundary between the elevated helper and the unelevated core.
//!
//! `docs/rust-port/03-target-architecture.md` §3.2 splits the client in two processes:
//! `acl-helper` runs elevated and holds the memory reader, the injection path, the
//! keyboard hook and the overlay window; `acl-core` holds tokio, signalling, WebRTC,
//! audio and the GUI and never runs elevated. This crate is the only thing both depend
//! on.
//!
//! It exists in P1+ rather than in P5 for one reason: a boundary that is written down
//! first is one the phases in between build against, and a boundary discovered in month
//! nine is a rewrite of everything that crossed it. What the messages *carry* is still the
//! later phases' business — the game state fields arrive with P2, the overlay commands
//! with P5. What is fixed here is the shape.
//!
//! # Framing
//!
//! Length-prefixed postcard. A four-byte little-endian length, then that many bytes.
//! Postcard because the payload is a ~200-byte struct at 5 Hz across a pipe: it is
//! `no_std`-friendly, has no schema negotiation to get wrong, and encodes a struct of
//! numbers to almost exactly its own size.
//!
//! The length prefix is bounded. A helper is trusted more than the network but it is
//! still a separate process that can crash mid-write, and a four-byte length read out of
//! a torn frame otherwise becomes a four-gigabyte allocation.

use serde::{Deserialize, Serialize};

/// The largest frame either side will send or accept.
///
/// Two orders of magnitude above the ~200 bytes the plan measures, and small enough that
/// a garbage length is refused rather than allocated.
pub const MAX_FRAME: usize = 64 * 1024;

/// How many bytes the length prefix takes.
pub const LENGTH_PREFIX: usize = 4;

/// What the elevated helper tells the core.
///
/// Only what exists today. `P2` fills [`HelperMessage::GameState`] out with the reader's
/// fields; the variant is here now so the channel it travels on is not invented later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HelperMessage {
    /// The helper is up, with the protocol version it speaks.
    Ready {
        /// See [`PROTOCOL_VERSION`].
        protocol: u32,
    },
    /// One sample of what the helper can see in the game.
    ///
    /// The payload is opaque here on purpose: `acl-ipc` must not depend on `acl-game`,
    /// or the boundary crate grows a dependency on one of the two sides it separates.
    GameState(Vec<u8>),
    /// A key the helper's hook saw, by the code the platform layer defines.
    KeyEvent {
        /// Platform key code.
        code: u32,
        /// Whether it went down.
        pressed: bool,
    },
    /// The helper is stopping, and why.
    Stopping {
        /// A message for the log, not for a user.
        reason: String,
    },
}

/// What the core asks the elevated helper to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CoreMessage {
    /// Begin sampling the game.
    StartReading,
    /// Stop sampling, without exiting.
    StopReading,
    /// Show or hide the overlay window.
    SetOverlayVisible(bool),
    /// Exit.
    Shutdown,
}

/// The version both sides check on connect.
///
/// An elevated process and an unelevated one are updated together, but not atomically: an
/// installer that replaces one binary and fails on the other leaves a mismatched pair on
/// disk. Refusing to talk is better than reading a struct that has changed shape.
pub const PROTOCOL_VERSION: u32 = 1;

pub mod stream;

/// Why a frame could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The length prefix named more bytes than [`MAX_FRAME`].
    #[error("frame of {0} bytes is over the {MAX_FRAME} byte limit")]
    TooLarge(usize),
    /// The bytes were not a message.
    #[error("could not decode frame: {0}")]
    Decode(#[from] postcard::Error),
    /// The underlying pipe failed.
    #[error("ipc transport failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Encodes one message as a length-prefixed frame.
///
/// # Errors
///
/// Returns [`FrameError::TooLarge`] if the encoded message is over [`MAX_FRAME`], and
/// [`FrameError::Decode`] if it cannot be encoded at all.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, FrameError> {
    let body = postcard::to_allocvec(message)?;
    if body.len() > MAX_FRAME {
        return Err(FrameError::TooLarge(body.len()));
    }
    let length = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(body.len()))?;
    let mut frame = Vec::with_capacity(LENGTH_PREFIX + body.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Reads one message out of a buffer, returning it and how many bytes it consumed.
///
/// Returns `Ok(None)` when the buffer does not yet hold a whole frame, which is the
/// normal case on a stream: the caller keeps reading and calls again.
///
/// # Errors
///
/// Returns [`FrameError::TooLarge`] for a length prefix over [`MAX_FRAME`], and
/// [`FrameError::Decode`] for bytes that are not a message.
pub fn decode<T: for<'de> Deserialize<'de>>(
    buffer: &[u8],
) -> Result<Option<(T, usize)>, FrameError> {
    let Some(prefix) = buffer.get(..LENGTH_PREFIX) else {
        return Ok(None);
    };
    let mut length_bytes = [0u8; LENGTH_PREFIX];
    length_bytes.copy_from_slice(prefix);
    let length = u32::from_le_bytes(length_bytes) as usize;

    // Checked before the buffer is indexed, not after: a torn frame is exactly how a
    // four-byte length becomes a four-gigabyte allocation.
    if length > MAX_FRAME {
        return Err(FrameError::TooLarge(length));
    }
    let end = LENGTH_PREFIX + length;
    let Some(body) = buffer.get(LENGTH_PREFIX..end) else {
        return Ok(None);
    };
    Ok(Some((postcard::from_bytes(body)?, end)))
}

/// A duplex message channel between the two processes.
///
/// Deliberately not `async`: the helper has no runtime and should not gain one. The core
/// drives its end from a blocking thread that forwards into tokio, which keeps tokio out
/// of the elevated process entirely.
pub trait Transport {
    /// Sends one message.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] if the message cannot be encoded or the pipe fails.
    fn send<T: Serialize>(&mut self, message: &T) -> Result<(), FrameError>;

    /// Blocks until one message arrives, or the peer closes.
    ///
    /// Returns `Ok(None)` when the peer has closed cleanly, which is a shutdown rather
    /// than an error.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] if a frame cannot be read or decoded.
    fn recv<T: for<'de> Deserialize<'de>>(&mut self) -> Result<Option<T>, FrameError>;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_message_survives_the_round_trip() {
        let message = HelperMessage::KeyEvent {
            code: 0x11,
            pressed: true,
        };
        let frame = encode(&message).expect("encodes");
        let (decoded, used) = decode::<HelperMessage>(&frame)
            .expect("decodes")
            .expect("a whole frame");
        assert_eq!(decoded, message);
        assert_eq!(used, frame.len());
    }

    #[test]
    fn the_sample_a_helper_sends_five_times_a_second_stays_small() {
        // The plan's figure is a ~200-byte struct at 5 Hz. This is the check that the
        // framing does not quietly multiply it.
        let message = HelperMessage::GameState(vec![0u8; 200]);
        let frame = encode(&message).expect("encodes");
        assert!(
            frame.len() < 256,
            "a 200 byte sample framed to {}",
            frame.len()
        );
    }

    #[test]
    fn a_partial_frame_is_not_an_error() {
        let frame = encode(&CoreMessage::StartReading).expect("encodes");
        // Nothing at all.
        assert!(decode::<CoreMessage>(&[]).expect("no error").is_none());
        // Half a length prefix.
        assert!(
            decode::<CoreMessage>(&frame[..2])
                .expect("no error")
                .is_none()
        );
        // A length prefix and not all of the body.
        let short = encode(&HelperMessage::GameState(vec![7u8; 64])).expect("encodes");
        assert!(
            decode::<HelperMessage>(&short[..8])
                .expect("no error")
                .is_none()
        );
    }

    #[test]
    fn two_frames_in_one_buffer_are_read_one_at_a_time() {
        let mut buffer = encode(&CoreMessage::StartReading).expect("encodes");
        buffer.extend(encode(&CoreMessage::Shutdown).expect("encodes"));

        let (first, used) = decode::<CoreMessage>(&buffer)
            .expect("ok")
            .expect("a frame");
        assert_eq!(first, CoreMessage::StartReading);

        let (second, _) = decode::<CoreMessage>(&buffer[used..])
            .expect("ok")
            .expect("a frame");
        assert_eq!(second, CoreMessage::Shutdown);
    }

    #[test]
    fn refuses_a_length_prefix_it_will_not_allocate_for() {
        // A helper that crashes mid-write leaves a torn frame behind, and the four bytes
        // that follow are whatever was in the pipe.
        let mut torn = u32::MAX.to_le_bytes().to_vec();
        torn.extend_from_slice(b"nonsense");
        let error = decode::<CoreMessage>(&torn).expect_err("must refuse");
        assert!(matches!(error, FrameError::TooLarge(_)));
    }

    #[test]
    fn refuses_to_send_more_than_it_will_accept() {
        // The two limits have to be the same one, or a sender happily writes frames its
        // peer will refuse.
        let too_big = HelperMessage::GameState(vec![0u8; MAX_FRAME + 1]);
        assert!(matches!(
            encode(&too_big).expect_err("must refuse"),
            FrameError::TooLarge(_)
        ));
    }

    #[test]
    fn bytes_that_are_not_a_message_are_rejected_rather_than_guessed_at() {
        let mut frame = 3u32.to_le_bytes().to_vec();
        frame.extend_from_slice(&[0xff, 0xff, 0xff]);
        assert!(matches!(
            decode::<CoreMessage>(&frame).expect_err("must refuse"),
            FrameError::Decode(_)
        ));
    }
}
