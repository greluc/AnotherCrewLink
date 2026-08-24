//! A fixed jitter buffer, with Opus in-band error correction.
//!
//! The plan asks for one as the baseline `neteq` is measured against, because it is what
//! most peer-to-peer voice applications ship and without it the comparison has no floor.
//! It turned out to be more than a baseline.
//!
//! # Why the error correction lives here and not in `neteq`
//!
//! `neteq` 0.9.1 cannot express it. Its `AudioDecoder` trait is `decode(&[u8])` and
//! nothing else, its source does not mention forward error correction anywhere, and it
//! fills a gap from its own expansion rather than by asking the decoder for the redundant
//! copy the next packet is carrying. That is the question the plan left open as a gate
//! item, and this is the answer: recovery has to be arranged by whatever owns the packet
//! sequence, which is this.
//!
//! # The order that matters
//!
//! To recover frame *N* you decode packet *N+1* with the correction flag set, and *N+1*
//! is then still to be played. So a buffer that wants the recovery has to be holding
//! *N+1* already — which means waiting at least one packet beyond the gap before giving
//! up on it. That is the depth this buffer keeps, and it is why "just play what arrives"
//! cannot recover anything.

use std::collections::BTreeMap;

use crate::codec::{self, CodecError, Decoder, FRAME_SAMPLES};

/// How many packets to hold before playing, as a starting depth.
///
/// Three is 60 ms at the 20 ms frame this client sends: enough to reorder around ordinary
/// network jitter and to have packet *N+1* in hand when *N* does not arrive, and short
/// enough that a conversation does not feel like a radio link.
pub const DEFAULT_DEPTH: usize = 3;

/// How many frames of silence count as the stream having stopped rather than stumbled.
///
/// Past this, the next packet to arrive re-primes the buffer instead of being judged
/// against a sequence number that kept counting while nothing was there. Ten frames is
/// 200 ms — longer than any reordering, shorter than a person notices as a decision.
///
/// Without it a wireless handover is permanent. The impairment harness measured exactly
/// that: a 500 ms freeze, and then every one of the 500 packets that arrived afterwards
/// was discarded as late, because `next` had advanced 25 places while the buffer was
/// empty. Half the call played as silence.
const STARVATION_FRAMES: u32 = 10;

/// How far out of order a packet may be and still be accepted.
///
/// Past this it is treated as a new stream rather than as a very late packet: a peer that
/// restarts its encoder begins again at a low sequence number, and a buffer that insisted
/// on the old numbering would discard everything it sent from then on.
const RESYNC_DISTANCE: u16 = 3000;

/// Where a frame of audio came from, which is what makes the buffer measurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSource {
    /// The packet arrived and was decoded.
    Packet,
    /// The packet was lost and rebuilt from the redundancy in the next one.
    Recovered,
    /// The packet was lost and there was no redundancy; the codec extrapolated.
    Concealed,
    /// Nothing was available at all, and silence went out.
    Silence,
}

/// One frame handed to the output device.
#[derive(Debug, Clone)]
pub struct Frame {
    /// The samples.
    pub samples: Vec<f32>,
    /// Where they came from.
    pub source: FrameSource,
}

/// Counters, for the impairment harness to report against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitterStats {
    /// Packets accepted into the buffer.
    pub accepted: u64,
    /// Packets dropped for arriving after their slot had already played.
    pub too_late: u64,
    /// Frames played from a packet.
    pub played: u64,
    /// Frames rebuilt from the next packet's redundancy.
    pub recovered: u64,
    /// Frames the codec extrapolated.
    pub concealed: u64,
    /// Frames that were silence because nothing was there.
    pub silent: u64,
    /// Times the buffer gave up on its sequence and started again from what arrived.
    pub resyncs: u64,
}

/// A fixed-depth reordering buffer over a sequence-numbered packet stream.
pub struct JitterBuffer {
    decoder: Decoder,
    packets: BTreeMap<u16, Vec<u8>>,
    /// The sequence number to play next, once playback has started.
    next: Option<u16>,
    depth: usize,
    /// Consecutive frames produced with nothing in the buffer.
    starved: u32,
    stats: JitterStats,
}

impl JitterBuffer {
    /// A buffer holding `depth` packets before it starts playing.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if the decoder cannot be created.
    pub fn new(depth: usize) -> Result<Self, CodecError> {
        Ok(Self {
            decoder: Decoder::new()?,
            packets: BTreeMap::new(),
            next: None,
            // A depth of zero can never recover anything: the recovery needs the packet
            // after the gap to already be in hand.
            depth: depth.max(1),
            starved: 0,
            stats: JitterStats::default(),
        })
    }

    /// The counters so far.
    #[must_use]
    pub const fn stats(&self) -> JitterStats {
        self.stats
    }

    /// How many packets are waiting.
    #[must_use]
    pub fn held(&self) -> usize {
        self.packets.len()
    }

    /// Takes one packet in.
    ///
    /// A packet whose slot has already played is dropped and counted: playing it would
    /// put audio out of order, which is worse than the gap it was meant to fill.
    pub fn push(&mut self, sequence: u16, payload: &[u8]) {
        // Nothing has been there for long enough that the sequence number this buffer is
        // waiting for is meaningless. Start again from whatever arrives.
        if self.starved >= STARVATION_FRAMES {
            self.next = None;
            self.starved = 0;
            self.stats.resyncs += 1;
        }
        if let Some(next) = self.next {
            let behind = next.wrapping_sub(sequence);
            // `behind` is small when the packet is late, and enormous when it is ahead —
            // wrapping subtraction is what makes the sequence number's wrap at 65535 a
            // non-event rather than a stall lasting a whole cycle.
            if behind > 0 && behind < RESYNC_DISTANCE {
                self.stats.too_late += 1;
                return;
            }
        }
        self.packets.insert(sequence, payload.to_vec());
        self.stats.accepted += 1;
    }

    /// Produces the next frame, or `None` while the buffer is still filling.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] if the decoder refuses a packet.
    pub fn pop(&mut self) -> Result<Option<Frame>, CodecError> {
        if self.next.is_none() {
            if self.packets.len() < self.depth {
                return Ok(None);
            }
            // Start at the oldest held packet, not at zero: a stream may begin anywhere.
            self.next = self.packets.keys().next().copied();
        }
        let Some(sequence) = self.next else {
            return Ok(None);
        };

        let mut samples = vec![0.0f32; FRAME_SAMPLES];
        let source = if let Some(payload) = self.packets.remove(&sequence) {
            self.decoder.decode(&payload, &mut samples)?;
            self.starved = 0;
            self.stats.played += 1;
            FrameSource::Packet
        } else if let Some(next) = self.packets.get(&sequence.wrapping_add(1)) {
            // The packet after the gap is here. Holding it long enough to look inside is
            // the whole reason the buffer holds more than one packet -- but whether it
            // carries a copy of this frame has to be asked, not assumed.
            //
            // The redundancy exists only when the sender has been told there is loss, and
            // `decode_lost` does not complain when there is none: it produces concealment
            // and returns the same frame size. Counting its successes therefore reported
            // identical recovery for a sender that had been told and one that never had --
            // 46 frames either way, measured -- which made the number meaningless in
            // exactly the direction that hides the fault §3e is about.
            if codec::has_redundancy(next) {
                self.decoder.decode_lost(next, &mut samples)?;
                self.stats.recovered += 1;
                FrameSource::Recovered
            } else {
                self.decoder.conceal(&mut samples)?;
                self.stats.concealed += 1;
                FrameSource::Concealed
            }
        } else if self.packets.is_empty() {
            // Nothing at all. Concealment extrapolates from what came before, but with an
            // empty buffer there is nothing to extrapolate towards and the stream has
            // probably stopped.
            self.starved = self.starved.saturating_add(1);
            self.stats.silent += 1;
            FrameSource::Silence
        } else {
            // Something later is waiting, so this really is a hole rather than the end.
            self.decoder.conceal(&mut samples)?;
            self.stats.concealed += 1;
            FrameSource::Concealed
        };

        self.next = Some(sequence.wrapping_add(1));
        Ok(Some(Frame { samples, source }))
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
    use crate::codec::Encoder;

    /// A stream of packets whose content changes every frame, so a recovered frame can be
    /// told apart from an extrapolated one.
    fn stream(count: usize, loss_percent: u8) -> Vec<Vec<u8>> {
        let mut encoder = Encoder::new().unwrap();
        encoder.set_packet_loss(loss_percent).unwrap();
        let mut packet = Vec::new();
        (0..count)
            .map(|frame| {
                let hertz = 200.0 + f64::from(u32::try_from(frame % 7).unwrap_or(0)) * 130.0;
                let samples: Vec<f32> = (0..FRAME_SAMPLES)
                    .map(|index| {
                        ((std::f64::consts::TAU * hertz * index as f64 / 48000.0).sin() * 0.5)
                            as f32
                    })
                    .collect();
                encoder.encode(&samples, &mut packet).unwrap();
                packet.clone()
            })
            .collect()
    }

    #[test]
    fn nothing_plays_until_the_buffer_has_filled() {
        // Playing the first packet the moment it lands leaves no room to reorder, and no
        // room to recover: the recovery needs the packet *after* the gap.
        let mut buffer = JitterBuffer::new(3).unwrap();
        let packets = stream(5, 0);
        buffer.push(0, &packets[0]);
        assert!(buffer.pop().unwrap().is_none());
        buffer.push(1, &packets[1]);
        assert!(buffer.pop().unwrap().is_none());
        buffer.push(2, &packets[2]);
        assert!(buffer.pop().unwrap().is_some());
    }

    #[test]
    fn packets_that_arrive_out_of_order_are_played_in_order() {
        let mut buffer = JitterBuffer::new(3).unwrap();
        let packets = stream(6, 0);
        for sequence in [2u16, 0, 1] {
            buffer.push(sequence, &packets[sequence as usize]);
        }
        for _ in 0..3 {
            let frame = buffer.pop().unwrap().unwrap();
            assert_eq!(frame.source, FrameSource::Packet);
        }
        assert_eq!(buffer.stats().played, 3);
    }

    #[test]
    fn a_lost_packet_is_recovered_from_the_next_one() {
        // The property the whole depth exists for, and the one `neteq` 0.9.1 has no way
        // to express.
        let mut buffer = JitterBuffer::new(3).unwrap();
        let packets = stream(8, 30);
        for sequence in [0u16, 1, 2, 4, 5] {
            buffer.push(sequence, &packets[sequence as usize]);
        }
        // Packet 3 never arrives; 4 is in hand.
        let mut sources = Vec::new();
        for _ in 0..5 {
            sources.push(buffer.pop().unwrap().unwrap().source);
        }
        assert_eq!(sources[3], FrameSource::Recovered, "{sources:?}");
        assert_eq!(buffer.stats().recovered, 1);
    }

    #[test]
    fn a_gap_with_nothing_after_it_is_concealed_rather_than_recovered() {
        // Two losses in a row: the second has no successor to recover from, so the codec
        // extrapolates instead. Telling the two apart is what makes the impairment
        // harness able to say *how* a stream survived, not just that it did.
        let mut buffer = JitterBuffer::new(3).unwrap();
        let packets = stream(8, 30);
        for sequence in [0u16, 1, 2, 5, 6] {
            buffer.push(sequence, &packets[sequence as usize]);
        }
        let mut sources = Vec::new();
        for _ in 0..6 {
            sources.push(buffer.pop().unwrap().unwrap().source);
        }
        // 3 has 4 missing too, so it cannot be recovered; 4 has 5, so it can.
        assert_eq!(sources[3], FrameSource::Concealed, "{sources:?}");
        assert_eq!(sources[4], FrameSource::Recovered, "{sources:?}");
    }

    #[test]
    fn a_packet_that_arrives_after_its_slot_is_dropped() {
        // Playing it would put audio out of order, which is worse than the gap it was
        // meant to fill.
        let mut buffer = JitterBuffer::new(3).unwrap();
        let packets = stream(8, 0);
        for sequence in [0u16, 1, 2, 3] {
            buffer.push(sequence, &packets[sequence as usize]);
        }
        for _ in 0..3 {
            buffer.pop().unwrap();
        }
        buffer.push(0, &packets[0]);
        assert_eq!(buffer.stats().too_late, 1);
        assert_eq!(buffer.held(), 1, "only packet 3 is still waiting");
    }

    #[test]
    fn the_sequence_number_wrapping_is_not_an_event() {
        // 65535 to 0 happens every twenty-two minutes at this frame size. A buffer that
        // read the wrap as "a packet 65535 places late" would stall for a whole cycle.
        let mut buffer = JitterBuffer::new(3).unwrap();
        let packets = stream(5, 0);
        for (offset, sequence) in [65534u16, 65535, 0, 1].into_iter().enumerate() {
            buffer.push(sequence, &packets[offset]);
        }
        let mut played = 0;
        while let Some(frame) = buffer.pop().unwrap() {
            if frame.source == FrameSource::Packet {
                played += 1;
            }
            if played == 4 {
                break;
            }
        }
        assert_eq!(played, 4);
        assert_eq!(buffer.stats().too_late, 0);
    }

    #[test]
    fn an_empty_buffer_produces_silence_rather_than_stopping() {
        // A peer that has gone quiet under DTX, or one that has left. The output device
        // still asks for a frame every twenty milliseconds either way.
        let mut buffer = JitterBuffer::new(1).unwrap();
        let packets = stream(2, 0);
        buffer.push(0, &packets[0]);
        buffer.pop().unwrap().unwrap();
        let frame = buffer.pop().unwrap().unwrap();
        assert_eq!(frame.source, FrameSource::Silence);
        assert!(frame.samples.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn a_depth_of_zero_is_treated_as_one() {
        // A caller asking for no buffering at all gets the minimum that can still work,
        // rather than a buffer that returns nothing for ever.
        let mut buffer = JitterBuffer::new(0).unwrap();
        let packets = stream(2, 0);
        buffer.push(0, &packets[0]);
        assert!(buffer.pop().unwrap().is_some());
    }
}
