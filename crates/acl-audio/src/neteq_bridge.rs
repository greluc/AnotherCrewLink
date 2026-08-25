//! Lets `neteq` decode with the libopus this crate already carries.
//!
//! §4.5 item 3d: an implementation of `neteq::codec::AudioDecoder` over the `opus` crate,
//! *so libopus stays the only codec in the binary*. `neteq`'s own decoder features pull
//! `ropus`, a second Opus implementation, into a binary that already links the reference
//! one — which is why the dependency is taken with `default-features = false` and this
//! file exists to fill the hole that leaves.
//!
//! # What this is for
//!
//! Not for shipping, yet. The plan asks for `neteq` to be **measured against** a
//! well-tuned fixed buffer under the same impairment, because without that comparison the
//! gate has no baseline: "`NetEQ` is better" is an assumption everybody shares and nobody
//! here had tested. `crate::jitter` is the fixed buffer, and
//! `tests/jitter_comparison.rs` is the measurement.
//!
//! The comparison is not symmetric, and the asymmetry is the point. `neteq` 0.9.1 cannot
//! express in-band FEC — its decoder trait is `decode(&[u8])`, with no way to say "this
//! payload is the next packet, recover the previous one from its redundant copy" — so it
//! conceals where the fixed buffer recovers. What it has instead is a delay manager that
//! adapts to the network rather than sitting at a fixed depth. Which of those is worth
//! more is exactly what a measurement can answer and an opinion cannot.

use neteq::codec::AudioDecoder;
use neteq::{NetEqError, Result as NetEqResult};

use crate::codec::{Decoder, FRAME_SAMPLES, SAMPLE_RATE};

/// The RTP payload type this client uses for Opus.
///
/// 111 is what browsers negotiate for Opus in practice. It is not fixed by the standard —
/// the range is dynamic — but both ends here are this client, and a number both sides
/// agree on is all that is required.
pub const OPUS_PAYLOAD_TYPE: u8 = 111;

/// `neteq`'s decoder, backed by the same libopus everything else in this crate uses.
pub struct OpusForNetEq {
    decoder: Decoder,
    /// Reused across calls. The trait returns an owned `Vec`, so one allocation per frame
    /// is unavoidable at the boundary; decoding into this and copying out at least keeps
    /// libopus writing into a buffer that is already the right size.
    scratch: Vec<f32>,
}

impl OpusForNetEq {
    /// Builds one.
    ///
    /// # Errors
    ///
    /// Whatever libopus says when a decoder cannot be created.
    pub fn new() -> Result<Self, crate::codec::CodecError> {
        Ok(Self {
            decoder: Decoder::new()?,
            scratch: vec![0.0; FRAME_SAMPLES],
        })
    }
}

impl AudioDecoder for OpusForNetEq {
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn channels(&self) -> u8 {
        1
    }

    fn decode(&mut self, encoded: &[u8]) -> NetEqResult<Vec<f32>> {
        self.decoder
            .decode(encoded, &mut self.scratch)
            .map_err(|error| NetEqError::InvalidPacket(error.to_string()))?;
        Ok(self.scratch.clone())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation
    )]

    use super::*;
    use crate::codec::Encoder;

    fn tone(frame: usize) -> Vec<f32> {
        (0..FRAME_SAMPLES)
            .map(|index| {
                let at = (frame * FRAME_SAMPLES + index) as f64;
                ((std::f64::consts::TAU * 440.0 * at / f64::from(SAMPLE_RATE)).sin() * 0.5) as f32
            })
            .collect()
    }

    #[test]
    fn it_decodes_what_this_crates_encoder_produced() {
        // The whole purpose: `neteq` asking libopus rather than a second Opus.
        let mut encoder = Encoder::new().unwrap();
        let mut packet = Vec::new();
        encoder.encode(&tone(0), &mut packet).unwrap();

        let mut bridge = OpusForNetEq::new().unwrap();
        let samples = bridge.decode(&packet).unwrap();
        assert_eq!(samples.len(), FRAME_SAMPLES);
        let peak = samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(peak > 0.1, "decoded to near silence, peak {peak}");
    }

    #[test]
    fn it_reports_what_the_rest_of_the_crate_runs_at() {
        // A mismatch here does not fail: `neteq` would resample around it and the delay
        // manager would be tuned against a rate nothing else uses.
        let bridge = OpusForNetEq::new().unwrap();
        assert_eq!(bridge.sample_rate(), SAMPLE_RATE);
        assert_eq!(bridge.channels(), 1);
    }

    #[test]
    fn a_corrupt_payload_is_an_error_rather_than_a_panic() {
        // It arrives from the network. A panic here is a denial of service that any peer
        // can trigger by sending four bytes.
        let mut bridge = OpusForNetEq::new().unwrap();
        assert!(bridge.decode(&[0xff, 0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn an_empty_payload_is_concealment_and_not_an_error() {
        // libopus reads a zero-length packet as "this one is missing" and conceals, which
        // is the same thing it does for a genuine gap. Worth a test of its own rather than
        // an assertion tucked into the one above: the obvious guess is that it errors, and
        // a caller written against that guess would treat every silent frame as a fault.
        let mut bridge = OpusForNetEq::new().unwrap();
        let samples = bridge.decode(&[]).unwrap();
        assert_eq!(samples.len(), FRAME_SAMPLES);
    }
}
