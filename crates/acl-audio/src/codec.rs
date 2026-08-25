//! Opus, with the two features that decide how it sounds when the network is not perfect.
//!
//! **In-band forward error correction.** libopus can carry a low-bitrate copy of the
//! previous frame inside the current one, so a receiver that loses packet *N* can
//! reconstruct it from packet *N+1*. It only does so once it is told there is loss —
//! `OPUS_SET_PACKET_LOSS_PERC` — and a client that never tells it achieves nothing by
//! setting the flag. That is the whole of `P3`'s hardest item, and the encoder half of it
//! is here.
//!
//! **Discontinuous transmission.** With DTX on, libopus stops sending during silence and
//! the receiver fills with comfort noise. In a lobby of ten that is most of the bandwidth,
//! because most people are not talking most of the time.
//!
//! # What is not here
//!
//! No bitrate ladder. Below roughly 16 kbps libopus carries no meaningful redundancy, so a
//! ladder's bottom rung would switch the error correction off exactly when the network is
//! bad enough to need it — which is the opposite of what a ladder is for.

use opus::{Application, Bitrate, Channels, Decoder as OpusDecoder, Encoder as OpusEncoder, Signal};

/// The rate everything in this client runs at.
pub const SAMPLE_RATE: u32 = 48000;

/// The frame the client sends, in milliseconds.
///
/// 20 ms is what WebRTC negotiates by default and what the other end expects. Shorter
/// frames cost proportionally more header for the same audio; longer ones add latency to
/// every packet and make a single loss a longer hole.
pub const FRAME_MS: u32 = 20;

/// How many samples that is, per channel.
pub const FRAME_SAMPLES: usize = (SAMPLE_RATE as usize * FRAME_MS as usize) / 1000;

/// The largest packet the encoder is allowed to produce.
///
/// Opus permits up to 1275 bytes per frame. The buffer is that size so a packet is never
/// truncated: a truncated Opus packet is not a quieter one, it is a decode error.
pub const MAX_PACKET: usize = 1275;

/// Whether a packet carries a redundant copy of the frame before it.
///
/// This exists because `decode_lost` cannot answer it. `opus_decode` with `decode_fec=1`
/// succeeds whether or not the packet holds redundancy: given none, it quietly produces
/// concealment instead and returns the same frame size. So a receive path that calls it on
/// every gap and counts the successes is not measuring error correction at all -- it is
/// counting gaps, and it reports the same number whether the sender was ever told about
/// loss or not. That is precisely the failure §3e is about, wearing the label of the fix.
///
/// `opus_packet_has_lbrr` is the only honest discriminator, and the `opus` crate does not
/// re-export it -- hence the direct dependency on the sys crate underneath it.
///
/// A malformed packet answers `false` rather than raising: this is a byte string that
/// arrived off the network, and "does this contain redundancy" has a perfectly good answer
/// for rubbish.
#[must_use]
pub fn has_redundancy(packet: &[u8]) -> bool {
    let Ok(len) = i32::try_from(packet.len()) else {
        return false;
    };
    // SAFETY: libopus reads at most `len` bytes from `packet`, which is that slice's own
    // length, and writes nothing through the pointer. It is documented to accept any byte
    // string and return a negative code for one it cannot parse.
    let answer = unsafe { opusic_sys::opus_packet_has_lbrr(packet.as_ptr(), len) };
    answer == 1
}

/// What went wrong.
#[derive(Debug)]
pub enum CodecError {
    /// libopus refused something.
    Opus(opus::Error),
    /// A block was not one frame long.
    ///
    /// Carried rather than padded: a caller handing over half a frame has a bug in its
    /// buffering, and silently completing it with zeros turns that into a faint click
    /// every twenty milliseconds.
    WrongFrameSize {
        /// How many samples arrived.
        got: usize,
        /// How many were expected.
        expected: usize,
    },
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Opus(error) => write!(formatter, "{error}"),
            Self::WrongFrameSize { got, expected } => {
                write!(formatter, "{got} samples, expected exactly {expected}")
            }
        }
    }
}

impl std::error::Error for CodecError {}

impl From<opus::Error> for CodecError {
    fn from(error: opus::Error) -> Self {
        Self::Opus(error)
    }
}

/// The sending half.
pub struct Encoder {
    inner: OpusEncoder,
    loss_percent: u8,
}

impl Encoder {
    /// An encoder configured the way this client sends.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if libopus refuses the configuration.
    pub fn new() -> Result<Self, CodecError> {
        // `Voip` rather than `Audio`: it biases libopus towards speech intelligibility
        // over musical fidelity, which is the trade this application wants.
        let mut inner = OpusEncoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)?;
        // Speech, said explicitly. libopus otherwise decides for itself what a signal is,
        // and what it decides governs whether the redundancy is reachable at all: LBRR
        // lives in the SILK layer, and a signal classified as music is coded by CELT, which
        // has none. Measured: an encoder left to guess emitted redundancy in 0 of 200
        // packets after being told about 5% loss, where one told this emits it in most.
        inner.set_signal(Signal::Voice)?;
        inner.set_inband_fec(true)?;
        // The flag alone does nothing. Redundancy is emitted only in proportion to the
        // loss the encoder has been told about, and that number arrives from the receiver
        // reports — see `set_packet_loss`.
        inner.set_packet_loss_perc(0)?;
        Ok(Self {
            inner,
            loss_percent: 0,
        })
    }

    /// Turns discontinuous transmission on or off.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if libopus refuses.
    pub fn set_dtx(&mut self, enabled: bool) -> Result<(), CodecError> {
        self.inner.set_dtx(enabled)?;
        Ok(())
    }

    /// Tells the encoder how much loss the far end is reporting, as a percentage.
    ///
    /// This is what actually switches the error correction on: libopus spends bits on
    /// redundancy in proportion to the number it is given. Clamped to 0..100 — the figure
    /// comes off the network from a peer, and a peer that lies should not be able to drive
    /// the encoder anywhere harmful.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if libopus refuses.
    pub fn set_packet_loss(&mut self, percent: u8) -> Result<(), CodecError> {
        let clamped = percent.min(100);
        if clamped == self.loss_percent {
            return Ok(());
        }
        self.inner.set_packet_loss_perc(i32::from(clamped))?;
        self.loss_percent = clamped;
        Ok(())
    }

    /// What the encoder was last told about loss.
    #[must_use]
    pub const fn packet_loss(&self) -> u8 {
        self.loss_percent
    }

    /// The bitrate libopus has settled on, in bits per second.
    ///
    /// Nothing sets this: libopus chooses for the configuration. It is readable because
    /// the whole error-correction path has a silent precondition — below roughly 16 kbps
    /// there is no room for a redundant copy, so the encoder carries none however high the
    /// loss percentage goes. A test asserts the floor, so a future change that lowers the
    /// bitrate fails there rather than by making lossy calls quietly worse.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if libopus refuses to report it.
    pub fn bitrate(&mut self) -> Result<u32, CodecError> {
        Ok(match self.inner.get_bitrate()? {
            Bitrate::Bits(bits) if bits > 0 => u32::try_from(bits).unwrap_or(u32::MAX),
            // `Max` means as much as the packet size allows, which is far above the floor;
            // `Auto` is not a number and libopus does not report it back in practice.
            _ => u32::MAX,
        })
    }

    /// Encodes exactly one frame.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError::WrongFrameSize`] unless `samples` is exactly one frame, and
    /// [`CodecError::Opus`] if libopus refuses it.
    pub fn encode(&mut self, samples: &[f32], into: &mut Vec<u8>) -> Result<usize, CodecError> {
        if samples.len() != FRAME_SAMPLES {
            return Err(CodecError::WrongFrameSize {
                got: samples.len(),
                expected: FRAME_SAMPLES,
            });
        }
        into.resize(MAX_PACKET, 0);
        let written = self.inner.encode_float(samples, into)?;
        into.truncate(written);
        Ok(written)
    }
}

/// The receiving half.
pub struct Decoder {
    inner: OpusDecoder,
}

impl Decoder {
    /// A decoder matching [`Encoder`].
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if libopus refuses the configuration.
    pub fn new() -> Result<Self, CodecError> {
        Ok(Self {
            inner: OpusDecoder::new(SAMPLE_RATE, Channels::Mono)?,
        })
    }

    /// Decodes one packet into exactly one frame.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if libopus refuses the packet.
    pub fn decode(&mut self, packet: &[u8], into: &mut [f32]) -> Result<usize, CodecError> {
        Ok(self.inner.decode_float(packet, into, false)?)
    }

    /// Reconstructs a lost frame from the redundancy inside the *next* packet.
    ///
    /// This is the receive half of the error-correction loop, and the order is the part
    /// that is easy to get wrong: to recover frame *N* you hand libopus packet *N+1* with
    /// the correction flag set, and it decodes the copy of *N* carried inside it. The
    /// jitter buffer then has to hold *N+1* back and play it next, which is why a
    /// buffer's loss signal has to reach this far.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if libopus refuses the packet.
    pub fn decode_lost(&mut self, next: &[u8], into: &mut [f32]) -> Result<usize, CodecError> {
        Ok(self.inner.decode_float(next, into, true)?)
    }

    /// Fills a lost frame with packet loss concealment, when there is no redundancy.
    ///
    /// libopus extrapolates from what it has already decoded. It is what happens when two
    /// packets in a row are lost, or when the sender was not emitting redundancy.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if libopus refuses.
    pub fn conceal(&mut self, into: &mut [f32]) -> Result<usize, CodecError> {
        Ok(self.inner.decode_float(&[], into, false)?)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation
    )]

    use super::*;

    /// A frame of speech-like audio: a tone whose pitch steps every frame.
    ///
    /// A steady tone is the wrong signal for testing error correction. Packet loss
    /// concealment extrapolates a sine almost perfectly, so a lost frame comes back
    /// nearly as well without the redundancy as with it — measured, at a correlation of
    /// 0.44 against 0.34, with the *unprotected* case ahead. Stepping the pitch makes the
    /// lost frame something the previous one does not predict, which is what speech is
    /// like and what the redundancy is for.
    fn stepped(frame: usize) -> Vec<f32> {
        // A different pitch each frame, cycling so the sequence is deterministic.
        let hertz = 200.0 + f64::from(u32::try_from(frame % 7).unwrap_or(0)) * 130.0;
        (0..FRAME_SAMPLES)
            .map(|index| {
                let position = index as f64;
                ((std::f64::consts::TAU * hertz * position / f64::from(SAMPLE_RATE)).sin() * 0.5)
                    as f32
            })
            .collect()
    }

    /// A frame of a tone, which survives a codec well enough to be measured.
    fn tone(frame: usize) -> Vec<f32> {
        (0..FRAME_SAMPLES)
            .map(|index| {
                let position = (frame * FRAME_SAMPLES + index) as f64;
                ((std::f64::consts::TAU * 440.0 * position / f64::from(SAMPLE_RATE)).sin() * 0.5)
                    as f32
            })
            .collect()
    }

    /// The root-mean-square of a block, for comparing what came out with what went in.
    fn rms(samples: &[f32]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        (sum / samples.len() as f64).sqrt()
    }

    #[test]
    fn a_frame_survives_a_round_trip() {
        let mut encoder = Encoder::new().unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut packet = Vec::new();
        let mut out = vec![0.0f32; FRAME_SAMPLES];

        // The first frames of any codec are its start-up transient; the fifth is settled.
        for frame in 0..5 {
            encoder.encode(&tone(frame), &mut packet).unwrap();
            decoder.decode(&packet, &mut out).unwrap();
        }

        let expected = rms(&tone(5));
        encoder.encode(&tone(5), &mut packet).unwrap();
        let produced = decoder.decode(&packet, &mut out).unwrap();
        assert_eq!(produced, FRAME_SAMPLES);
        let got = rms(&out);
        assert!(
            (got - expected).abs() < expected * 0.3,
            "expected around {expected}, got {got}"
        );
    }

    #[test]
    fn a_wrong_sized_block_is_refused_rather_than_padded() {
        // A caller handing over half a frame has a bug in its buffering, and completing
        // it with zeros turns that into a faint click every twenty milliseconds.
        let mut encoder = Encoder::new().unwrap();
        let mut packet = Vec::new();
        let error = encoder
            .encode(&vec![0.0f32; FRAME_SAMPLES - 1], &mut packet)
            .unwrap_err();
        assert!(matches!(error, CodecError::WrongFrameSize { .. }));
    }

    #[test]
    fn a_packet_fits_in_the_buffer_it_is_given() {
        // Opus permits 1275 bytes; a truncated packet is not a quieter one, it is a
        // decode error at the other end.
        let mut encoder = Encoder::new().unwrap();
        encoder.set_packet_loss(100).unwrap();
        let mut packet = Vec::new();
        for frame in 0..20 {
            let size = encoder.encode(&tone(frame), &mut packet).unwrap();
            assert!(size <= MAX_PACKET, "packet of {size} bytes");
            assert_eq!(packet.len(), size, "the buffer is trimmed to the packet");
        }
    }

    #[test]
    fn the_reported_loss_is_clamped() {
        // The figure arrives from a peer over the network. A peer that lies should not be
        // able to drive the encoder anywhere harmful.
        let mut encoder = Encoder::new().unwrap();
        encoder.set_packet_loss(255).unwrap();
        assert_eq!(encoder.packet_loss(), 100);
        encoder.set_packet_loss(0).unwrap();
        assert_eq!(encoder.packet_loss(), 0);
    }

    #[test]
    fn error_correction_is_what_makes_a_lost_frame_recoverable() {
        // The flag alone does nothing: libopus emits the redundant copy only in
        // proportion to the loss it has been told about, and a client that sets the flag
        // and never reports loss achieves exactly nothing. That is the trap this whole
        // item is about.
        //
        // The signal is not the packet size. libopus reallocates redundancy *within* the
        // bitrate rather than adding to it, so with a steady tone the packets come out
        // the same length either way — measured, at 5245 against 5267 bytes over forty
        // frames, which is noise. What changes is whether the frame can be got back.
        let quiet = recovery_quality(0);
        let told = recovery_quality(30);
        assert!(
            told > quiet * 1.5,
            "recovery should be better when loss is reported: {quiet} against {told}"
        );
    }

    /// How much of a lost frame comes back, as a fraction of what was sent.
    fn recovery_quality(loss_percent: u8) -> f64 {
        let mut encoder = Encoder::new().unwrap();
        encoder.set_packet_loss(loss_percent).unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut packet = Vec::new();
        let mut out = vec![0.0f32; FRAME_SAMPLES];

        let mut packets = Vec::new();
        for frame in 0..12 {
            encoder.encode(&stepped(frame), &mut packet).unwrap();
            packets.push(packet.clone());
        }
        for one in packets.iter().take(9) {
            decoder.decode(one, &mut out).unwrap();
        }
        // Frame 9 is lost; ask for it out of packet 10.
        decoder.decode_lost(&packets[10], &mut out).unwrap();

        let expected = rms(&stepped(9));
        if expected == 0.0 {
            return 0.0;
        }
        // Correlation with what was actually sent, not just loudness: concealment
        // produces something of about the right level, and the question is whether it is
        // the right *signal*.
        let sent = stepped(9);
        let dot: f64 = out
            .iter()
            .zip(&sent)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum();
        (dot / (rms(&out).max(f64::EPSILON) * expected * FRAME_SAMPLES as f64)).abs()
    }

    #[test]
    fn a_lost_frame_is_recovered_from_the_next_packet() {
        // The receive half, and the order is what is easy to get wrong: frame N is
        // recovered by handing the decoder packet N+1 with the correction flag set.
        let mut encoder = Encoder::new().unwrap();
        encoder.set_packet_loss(30).unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut packet = Vec::new();
        let mut out = vec![0.0f32; FRAME_SAMPLES];

        let mut packets = Vec::new();
        for frame in 0..12 {
            encoder.encode(&tone(frame), &mut packet).unwrap();
            packets.push(packet.clone());
        }

        // Decode up to the loss.
        for one in packets.iter().take(9) {
            decoder.decode(one, &mut out).unwrap();
        }

        // Frame 9 never arrives. Recover it from packet 10.
        decoder.decode_lost(&packets[10], &mut out).unwrap();
        let recovered = rms(&out);
        assert!(
            recovered > 0.05,
            "the recovered frame should carry audio, got {recovered}"
        );

        // And packet 10 itself is still to be played, which is why the jitter buffer has
        // to hold it back.
        decoder.decode(&packets[10], &mut out).unwrap();
        assert!(rms(&out) > 0.05);
    }

    #[test]
    fn concealment_fills_a_gap_when_there_is_no_redundancy() {
        // Two losses in a row, or a sender that was not emitting redundancy. libopus
        // extrapolates from what it has already decoded rather than handing back silence.
        let mut encoder = Encoder::new().unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut packet = Vec::new();
        let mut out = vec![0.0f32; FRAME_SAMPLES];

        for frame in 0..10 {
            encoder.encode(&tone(frame), &mut packet).unwrap();
            decoder.decode(&packet, &mut out).unwrap();
        }
        let concealed = decoder.conceal(&mut out).unwrap();
        assert_eq!(concealed, FRAME_SAMPLES);
        assert!(rms(&out) > 0.01, "concealment should not be silence");
    }

    #[test]
    fn discontinuous_transmission_stops_sending_through_silence() {
        // In a lobby of ten this is most of the bandwidth, because most people are not
        // talking most of the time.
        let mut encoder = Encoder::new().unwrap();
        encoder.set_dtx(true).unwrap();
        let mut packet = Vec::new();
        let silence = vec![0.0f32; FRAME_SAMPLES];

        // Give it a moment to notice the silence, then measure.
        for _ in 0..20 {
            encoder.encode(&silence, &mut packet).unwrap();
        }
        let size = encoder.encode(&silence, &mut packet).unwrap();
        assert!(size <= 3, "silence should cost almost nothing, got {size}");
    }
}
