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
/// Two is 40 ms at the 20 ms frame this client sends. It is the shallowest depth that can
/// work at all: the recovery needs packet *N+1* in hand while *N* is missing, so anything
/// less makes the redundancy Opus carries unreachable.
///
/// It is a starting point rather than the depth. See [`MAX_DEPTH`].
pub const DEFAULT_DEPTH: usize = 2;

/// The shallowest the buffer may go, for the reason above.
pub const MIN_DEPTH: usize = 2;

/// The deepest it will go, at 20 ms a packet: 200 ms.
///
/// Measured against Chromium rather than chosen. Gate G2's third criterion allows 30 ms
/// more latency than Chromium's receive path, and a *fixed* depth cannot meet it: 40 ms is
/// within the budget on a clean network and falls apart under 50 ms of jitter -- 17% of
/// frames invented, against Chromium's none -- while 60 ms survives the jitter and is
/// 50 ms adrift on a clean one. Chromium passes both because its buffer grows when it
/// needs to and shrinks when it does not, and the only honest answer was to do the same.
///
/// Ten frames is where growth stops. Past that a conversation is a radio link, and a
/// network that needs more than 200 ms of buffer has a problem no buffer fixes.
pub const MAX_DEPTH: usize = 10;

/// How many clean frames in a row before the buffer gives a packet of depth back.
///
/// Asymmetric on purpose: it deepens on a single gap and shallows only after fifty frames
/// -- a second -- without one. Jitter arrives in bursts, and a buffer that shrank as
/// eagerly as it grew would spend the burst oscillating and inventing audio at every step
/// down.
const CALM_FRAMES_BEFORE_SHALLOWING: u32 = 50;

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
    /// Nothing was available at all.
    ///
    /// The first few of these are concealed rather than silent -- see the branch that
    /// produces them. The variant still says "nothing was there", which is what the
    /// harness measures; what went out of the speaker is a different question from where
    /// it came from.
    Silence,
    /// Inserted on purpose, to fall a frame further behind the network and regain depth.
    ///
    /// Not a gap. The packet it delays is played on the next pop rather than lost, so a
    /// listener hears the same audio slightly later instead of hearing less of it. It is a
    /// separate variant because a harness that counted it as concealment would report a
    /// buffer working correctly as one that is failing -- and one that compared output
    /// frames positionally would see every frame after it as wrong.
    Stretched,
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
    /// Times the buffer grew because a frame it wanted was not there.
    pub deepened: u64,
    /// Times it gave a packet of depth back after a settled second.
    pub shallowed: u64,
    /// Frames inserted to fall further behind the network and regain depth.
    pub stretched: u64,
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
    /// Consecutive frames that came from a real packet, for shallowing.
    calm: u32,
    /// Whether the last frame was a stall, so two never run together.
    stalled_last: bool,
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
            // Clamped rather than trusted. Below `MIN_DEPTH` the recovery is unreachable
            // -- it needs the packet after the gap already in hand -- and above `MAX_DEPTH`
            // a conversation stops being one.
            depth: depth.clamp(MIN_DEPTH, MAX_DEPTH),
            starved: 0,
            calm: 0,
            stalled_last: false,
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
                // This is the only signal that says the buffer is too shallow: a packet
                // that did arrive, after its slot had already played. A packet that never
                // arrives is loss, and no depth recovers it -- an earlier version deepened
                // on every gap and grew to 185 ms under 10% loss, buying nothing and
                // spending the whole latency budget to do it.
                self.deepen();
                return;
            }
        }
        self.packets.insert(sequence, payload.to_vec());
        self.stats.accepted += 1;
    }

    /// Grows the buffer by one packet, up to [`MAX_DEPTH`].
    ///
    /// Called only when a packet arrives after its slot has played, which is the one
    /// observation that means "too shallow". The calm counter is reset with it: a burst of
    /// jitter should not be half-forgiven by the packets that arrived on time between its
    /// late ones.
    fn deepen(&mut self) {
        self.calm = 0;
        if self.depth < MAX_DEPTH {
            self.depth += 1;
            self.stats.deepened += 1;
        }
    }

    /// How many packets the buffer is currently holding before it plays.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
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

        // Growing `depth` mid-stream does nothing on its own: it gates the first fill and
        // nothing after it, so a buffer that decided it was too shallow would carry on
        // playing one frame per pop and stay exactly as shallow as before.
        //
        // Regaining depth means playing something that is not the next packet, once, so
        // the stream falls one frame further behind the network. Opus's concealment is
        // what fills it -- the same extrapolation it uses for a real gap, which is designed
        // not to be noticed.
        //
        // Only when the packet is actually there. Stalling in front of a hole would add
        // latency without adding safety, and the hole is still a hole afterwards.
        if self.packets.len() < self.depth
            && self.packets.contains_key(&sequence)
            && !self.stalled_last
        {
            // Never twice running. One stall buys one frame of depth; repeating without
            // playing anything in between would let the buffer fall arbitrarily far behind
            // a network that is simply slow, which is a different fault with the same
            // symptom.
            self.stalled_last = true;
            self.decoder.conceal(&mut samples)?;
            self.stats.stretched += 1;
            return Ok(Some(Frame {
                samples,
                source: FrameSource::Stretched,
            }));
        }
        self.stalled_last = false;
        let source = if let Some(payload) = self.packets.remove(&sequence) {
            self.decoder.decode(&payload, &mut samples)?;
            self.starved = 0;
            self.calm = self.calm.saturating_add(1);
            if self.calm >= CALM_FRAMES_BEFORE_SHALLOWING && self.depth > MIN_DEPTH {
                self.depth -= 1;
                self.calm = 0;
                self.stats.shallowed += 1;
            }
            self.stats.played += 1;
            FrameSource::Packet
        } else if let Some(next) = self.packets.get(&sequence.wrapping_add(1)).cloned() {
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
            if codec::has_redundancy(&next) {
                self.decoder.decode_lost(&next, &mut samples)?;
                self.stats.recovered += 1;
                FrameSource::Recovered
            } else {
                self.decoder.conceal(&mut samples)?;
                self.stats.concealed += 1;
                FrameSource::Concealed
            }
        } else if self.packets.is_empty() {
            // Nothing at all, and the question is whether that is a gap or an ending.
            //
            // For the first `STARVATION_FRAMES` it is treated as a gap, and concealed.
            // That is the same window `push` uses to decide the sequence number is still
            // meaningful, so the two agree on when a stream has stopped rather than
            // stumbled. Until 2026-08-29 this branch emitted digital zeroes from the very
            // first frame -- a hard edge to silence, which is a click, at exactly the
            // moment somebody starts speaking after a pause and the buffer has run dry.
            // The branch below conceals a hole in the middle of a stream and this one did
            // not conceal the hole at its front.
            //
            // Past the window, silence, and it costs nothing: libopus's concealment decays
            // to silence within a handful of frames of its own accord, so the frames after
            // that are inaudible either way and there is no reason to spend a decode on
            // each of them for a peer who has stopped sending.
            self.starved = self.starved.saturating_add(1);
            self.stats.silent += 1;
            if self.starved <= STARVATION_FRAMES {
                self.decoder.conceal(&mut samples)?;
            }
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
        let packets = stream(8, 0);
        // Pushed shuffled, and one ahead of what is being played so the buffer never runs
        // thin enough to stall for depth -- which is a different behaviour with the same
        // shape, and this test is about ordering.
        for sequence in [2u16, 0, 1, 3, 4, 5] {
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
        // What is left in the buffer now depends on whether it stalled on the way, which
        // is a different behaviour. What this test is about is that the late packet was
        // refused: four were accepted, and the fifth push was the same packet again.
        assert_eq!(
            buffer.stats().accepted,
            4,
            "the late packet was taken into the buffer"
        );
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
    fn an_empty_buffer_conceals_the_gap_before_it_gives_up_on_it() {
        // A peer that has gone quiet under DTX, or one that has left. The output device
        // still asks for a frame every twenty milliseconds either way.
        let mut buffer = JitterBuffer::new(MIN_DEPTH).unwrap();
        let packets = stream(3, 0);
        for (sequence, packet) in packets.iter().enumerate().take(MIN_DEPTH) {
            buffer.push(u16::try_from(sequence).unwrap(), packet);
        }
        // Popped until the buffer is empty. It may insert one stall on the way -- running
        // thin is exactly when it does -- and what this test is about is what happens after
        // there is nothing left at all.
        let mut frame = buffer.pop().unwrap().unwrap();
        for _ in 0..6 {
            frame = buffer.pop().unwrap().unwrap();
            if frame.source == FrameSource::Silence {
                break;
            }
        }
        assert_eq!(frame.source, FrameSource::Silence);

        // Not zeroes. Until 2026-08-29 this branch went straight to digital silence, which
        // is a hard edge and therefore a click -- at exactly the moment a buffer runs dry
        // under somebody who is about to speak. The branch that conceals a hole in the
        // middle of a stream was right there; the hole at its *front* was not concealed.
        assert!(
            frame.samples.iter().any(|sample| *sample != 0.0),
            "the first frame after the buffer empties should be concealed, not zeroed"
        );

        // And it does give up. Past `STARVATION_FRAMES` the stream has stopped rather than
        // stumbled -- the same window `push` uses to decide the sequence number has stopped
        // meaning anything -- so there is nothing to extrapolate towards and no reason to
        // spend a decode per frame on a peer who is not sending.
        let mut last = frame;
        for _ in 0..STARVATION_FRAMES + 2 {
            last = buffer.pop().unwrap().unwrap();
        }
        assert_eq!(last.source, FrameSource::Silence);
        assert!(
            last.samples.iter().all(|sample| *sample == 0.0),
            "a stream that has stopped should be silent rather than concealed for ever"
        );
    }

    #[test]
    fn a_depth_below_the_minimum_is_raised_to_it() {
        // A caller asking for no buffering at all gets the shallowest depth that can still
        // work. One packet cannot: the recovery needs the packet *after* the gap already
        // in hand, so a depth of one makes the redundancy Opus carries unreachable and
        // turns every single loss into concealment.
        for asked in [0, 1] {
            let buffer = JitterBuffer::new(asked).unwrap();
            assert_eq!(buffer.depth(), MIN_DEPTH, "asked for {asked}");
        }
    }

    #[test]
    fn a_depth_above_the_maximum_is_lowered_to_it() {
        let buffer = JitterBuffer::new(MAX_DEPTH + 50).unwrap();
        assert_eq!(buffer.depth(), MAX_DEPTH);
    }

    #[test]
    fn the_buffer_deepens_when_a_frame_is_not_there_and_shallows_when_it_settles() {
        // The property gate G2's third criterion forced. A fixed depth cannot satisfy it:
        // 40 ms is within the 30 ms budget on a clean network and invents 17% of frames
        // under 50 ms of jitter, and 60 ms survives the jitter and is 50 ms adrift on a
        // clean one. Chromium passes both because its buffer moves.
        let mut buffer = JitterBuffer::new(MIN_DEPTH).unwrap();
        let packets = stream(400, 5);

        // Deepening is driven by a packet arriving *after* its slot has played, which is
        // the only observation that means "too shallow". A packet that never arrives is
        // loss, and no depth recovers it -- an earlier version deepened on every gap and
        // grew to 185 ms under 10% loss, spending the whole latency budget for nothing.
        for sequence in 0..4u16 {
            buffer.push(sequence, &packets[sequence as usize]);
        }
        for _ in 0..3 {
            buffer.pop().unwrap();
        }
        let started = buffer.depth();
        buffer.push(0, &packets[0]); // long since played
        assert!(
            buffer.depth() > started,
            "it did not deepen for a missing frame: {} then {}",
            started,
            buffer.depth()
        );

        // Then a settled run gives the depth back.
        let deepened = buffer.depth();
        // From 4, in order: the recovery reads the packet after the gap without consuming
        // it, so 3 is still held and starting at 5 would leave a second hole at 4 -- which
        // deepens the buffer again and the run never settles.
        let mut sequence = 4u16;
        for _ in 0..(CALM_FRAMES_BEFORE_SHALLOWING + 5) {
            buffer.push(sequence, &packets[sequence as usize]);
            buffer.pop().unwrap();
            sequence = sequence.wrapping_add(1);
        }
        assert!(
            buffer.depth() < deepened,
            "it never gave the depth back: still {}",
            buffer.depth()
        );
        assert!(buffer.depth() >= MIN_DEPTH);
        assert!(buffer.stats().deepened > 0 && buffer.stats().shallowed > 0);
    }
}
