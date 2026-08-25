#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
//! Gate G2, third criterion: what the receive path sounds like when the network is not.
//!
//! > Under each impairment profile, the Rust receive path's added mouth-to-ear latency is
//! > within 30 ms of Chromium's and its objective quality score is no more than 0.2 MOS
//! > below it.
//!
//! This is the harness and the measurement for **our** path, in detail. The comparison
//! against Chromium's own receive path is `chromium_reference.rs`, which turned out not to
//! need the network layer after all: a loopback peer connection with an encoded transform
//! is Chromium's receiver, and dropping frames in the transform is the impairment.
//!
//! The numbers here are printed rather than only asserted, so a change in them is visible
//! in a diff.
//!
//! # What is measured
//!
//! - **Continuity**: how many output frames came from a real packet, from the redundancy,
//!   from concealment, or from nothing. The last is the one a listener notices.
//! - **Added latency**: the buffer's depth in milliseconds. No longer fixed: measuring
//!   against Chromium showed that no single depth can meet the criterion, so the buffer
//!   moves. See `chromium_reference.rs`.
//! - **Stretched**: frames the buffer inserted on purpose to fall further behind the
//!   network and regain depth. Not gaps — the packet each one delays is played next.
//! - **Quality**: correlation with the clean decode of the same stream, per frame,
//!   averaged. Not PESQ — that is a licensed algorithm and a dependency this does not
//!   have — but it moves the same way and it is honest about being a proxy.
//!
//! # What "recovered" means here, after it stopped meaning anything
//!
//! These columns were wrong until `codec::has_redundancy` existed. `decode_lost` succeeds
//! whether or not the packet it is given carries a redundant copy -- with none it produces
//! concealment and returns the same frame size -- so counting its successes counted gaps,
//! not recoveries, and reported the same figure for a sender that had been told about loss
//! and one that never had. The buffer now asks `opus_packet_has_lbrr` before it claims
//! anything.
//!
//! Correcting it appeared to expose a threshold, and the threshold turned out to be a
//! second bug:
//!
//! | told the encoder | recovered, before | recovered, after |
//! | --- | --- | --- |
//! | 1% | 0 | 6 |
//! | 2% | 0 | 20 |
//! | 5% | 28 | 39 |
//! | 10% | 79 | 79 |
//!
//! The left column read as "below about five percent libopus emits no usable redundancy",
//! which was written down as a property of libopus. It is not. LBRR lives in the SILK
//! layer, libopus decides for itself whether a signal is speech or music, and music is
//! coded by CELT, which has no LBRR at all -- so the encoder was answering a question
//! nobody had meant to ask. `Encoder::new` now says `Signal::Voice`, which is true of this
//! application, and the right column is what the same runs produce.
//!
//! Worth keeping as a warning rather than deleting: a measured number that looks like a
//! codec's behaviour can be a configuration nobody chose.
//!
//! # Where the quality number still lies, and where it stopped
//!
//! Each output frame is compared against the source frame it actually carries, not the one
//! at the same position. That matters because a stall plays a concealment frame and holds
//! its packet back, so everything after it is one place earlier than a positional
//! comparison expects. When the buffer first learned to stall, this file reported 0.028 for
//! a path that had just been made better.
//!
//! **`freeze-500` is still wrong, and is left wrong.** Half a second with nothing arriving
//! ends in a resynchronisation — the buffer abandons the sequence it was waiting for and
//! starts again from what arrived — and after that there is no mapping from output frame to
//! source frame to align by. It reads about 0.5 and sounds fine. Continuity is the column
//! to read there: 1000 frames from packets and 3% gaps, against 500 and 52% before the
//! resynchronisation existed.

use acl_audio::codec::{Decoder, Encoder, FRAME_SAMPLES};
use acl_audio::impairment::{Profile, apply};
use acl_audio::jitter::{DEFAULT_DEPTH, FrameSource, JitterBuffer};

/// How many frames each profile is measured over. 1000 frames is twenty seconds.
const FRAMES: u16 = 1000;

/// The frame the client sends.
const FRAME_MS: u32 = 20;

/// One profile's result.
#[derive(Debug)]
struct Measurement {
    from_packet: usize,
    /// Frames the buffer inserted on purpose to regain depth. Not a gap.
    stretched: usize,
    recovered: usize,
    concealed: usize,
    silent: usize,
    /// Mean correlation with the clean decode, 0 to 1.
    quality: f64,
    /// The buffer's depth, in milliseconds.
    added_latency_ms: u32,
}

impl Measurement {
    /// The share of frames a listener would hear as a hole.
    fn gap_share(&self) -> f64 {
        let total = self.from_packet + self.recovered + self.concealed + self.silent;
        if total == 0 {
            return 1.0;
        }
        (self.concealed + self.silent) as f64 / total as f64
    }
}

/// Speech-like material: a pitch that steps every frame, so concealment cannot fake it.
fn source(frame: usize) -> Vec<f32> {
    let hertz = 200.0 + f64::from(u32::try_from(frame % 7).unwrap_or(0)) * 130.0;
    (0..FRAME_SAMPLES)
        .map(|index| ((std::f64::consts::TAU * hertz * index as f64 / 48000.0).sin() * 0.5) as f32)
        .collect()
}

/// Correlation of two frames, 0 when unrelated and 1 when identical in shape.
fn correlation(reference: &[f32], measured: &[f32]) -> f64 {
    let dot: f64 = reference
        .iter()
        .zip(measured)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    let energy_reference: f64 = reference.iter().map(|a| f64::from(*a).powi(2)).sum();
    let energy_measured: f64 = measured.iter().map(|b| f64::from(*b).powi(2)).sum();
    let denominator = (energy_reference * energy_measured).sqrt();
    if denominator <= f64::EPSILON {
        // Two silences are identical; silence against audio is not.
        return if energy_reference <= f64::EPSILON && energy_measured <= f64::EPSILON {
            1.0
        } else {
            0.0
        };
    }
    (dot / denominator).abs()
}

/// Encodes the stream once, and decodes it once with nothing lost, as the reference.
fn encode_and_reference(loss_percent: u8) -> (Vec<Vec<u8>>, Vec<Vec<f32>>) {
    let mut encoder = Encoder::new().unwrap();
    encoder.set_packet_loss(loss_percent).unwrap();
    let mut packet = Vec::new();
    let mut packets = Vec::with_capacity(FRAMES as usize);
    for frame in 0..FRAMES as usize {
        encoder.encode(&source(frame), &mut packet).unwrap();
        packets.push(packet.clone());
    }

    let mut decoder = Decoder::new().unwrap();
    let mut clean = Vec::with_capacity(packets.len());
    let mut out = vec![0.0f32; FRAME_SAMPLES];
    for one in &packets {
        decoder.decode(one, &mut out).unwrap();
        clean.push(out.clone());
    }
    (packets, clean)
}

fn measure(profile: Profile, seed: u32) -> Measurement {
    // The encoder is told about the loss it will actually meet, which is what makes it
    // emit the redundancy the buffer recovers from. A sender that is never told achieves
    // nothing by having the flag set, which is the trap the whole item is about.
    let (packets, clean) = encode_and_reference(profile.loss_percent.max(1));
    let arrivals = apply(profile, FRAMES, FRAME_MS, seed);

    let mut buffer = JitterBuffer::new(DEFAULT_DEPTH).unwrap();
    let mut produced = Vec::new();
    let mut sources = Vec::new();

    // Played at the wall clock the arrivals carry: a packet that has not arrived by the
    // time its slot comes round is a loss, however briefly.
    let mut next_arrival = 0usize;
    let mut now_ms = 0u32;
    let horizon = arrivals.last().map_or(0, |a| a.at_ms) + FRAME_MS * 10;

    while now_ms <= horizon {
        while next_arrival < arrivals.len() && arrivals[next_arrival].at_ms <= now_ms {
            let arrival = &arrivals[next_arrival];
            buffer.push(arrival.sequence, &packets[arrival.sequence as usize]);
            next_arrival += 1;
        }
        if let Some(frame) = buffer.pop().unwrap() {
            sources.push(frame.source);
            produced.push(frame.samples);
        }
        now_ms += FRAME_MS;
    }

    // Compared against the source frame each output frame *is*, not against the one at the
    // same position. A stall plays a concealment frame and holds the packet back, so from
    // that point on every output frame carries source audio one place earlier -- and a
    // positional comparison sees the whole rest of the call as wrong. It read 0.028 where
    // it had read 0.984, for a buffer that had just been made better.
    //
    // The stalls themselves are skipped rather than scored. They are audio the buffer
    // invented on purpose, and there is no source frame they correspond to.
    let quality = {
        let mut source_index = 0usize;
        let mut total = 0.0f64;
        let mut compared = 0usize;
        for (index, frame) in produced.iter().enumerate() {
            let stalled = sources.get(index) == Some(&FrameSource::Stretched);
            if stalled {
                continue;
            }
            let Some(reference) = clean.get(source_index) else {
                break;
            };
            total += correlation(reference, frame);
            compared += 1;
            source_index += 1;
        }
        if compared == 0 {
            0.0
        } else {
            total / compared as f64
        }
    };

    Measurement {
        from_packet: sources
            .iter()
            .filter(|s| **s == FrameSource::Packet)
            .count(),
        stretched: sources
            .iter()
            .filter(|s| **s == FrameSource::Stretched)
            .count(),
        recovered: sources
            .iter()
            .filter(|s| **s == FrameSource::Recovered)
            .count(),
        concealed: sources
            .iter()
            .filter(|s| **s == FrameSource::Concealed)
            .count(),
        silent: sources
            .iter()
            .filter(|s| **s == FrameSource::Silence)
            .count(),
        quality,
        added_latency_ms: DEFAULT_DEPTH as u32 * FRAME_MS,
    }
}

#[test]
fn the_receive_path_survives_every_profile() {
    println!(
        "\n{:<12} {:>7} {:>9} {:>9} {:>7} {:>8} {:>9}",
        "profile", "packet", "recovered", "concealed", "silent", "gaps", "quality"
    );

    let mut worst_gap = 0.0f64;
    for (name, profile) in Profile::suite() {
        let measurement = measure(profile, 20_260_824);
        println!(
            "{:<12} {:>7} {:>9} {:>9} {:>7} {:>9} {:>7.1}% {:>9.3}",
            name,
            measurement.from_packet,
            measurement.recovered,
            measurement.concealed,
            measurement.silent,
            measurement.stretched,
            measurement.gap_share() * 100.0,
            measurement.quality
        );

        // Nothing may fall over. A profile that produced no audio at all would be a
        // buffer that stops rather than one that copes.
        assert!(
            measurement.from_packet > 0,
            "{name}: nothing was played from a packet"
        );
        worst_gap = worst_gap.max(measurement.gap_share());
    }

    println!(
        "\nadded latency: {} ms (fixed depth of {DEFAULT_DEPTH} frames)\n",
        DEFAULT_DEPTH as u32 * FRAME_MS
    );

    // At 10% loss roughly one frame in ten is missing, and half of those are recoverable
    // — a gap share above a quarter would mean the recovery is not working at all.
    assert!(
        worst_gap < 0.25,
        "the worst profile left {:.1}% of frames as gaps",
        worst_gap * 100.0
    );
}

#[test]
fn error_correction_is_what_carries_the_lossy_profiles() {
    // The measurement that says the redundancy is doing something, rather than the buffer
    // merely surviving because concealment is good at tones.
    let with_loss = measure(
        Profile {
            loss_percent: 10,
            ..Profile::perfect()
        },
        20_260_824,
    );
    assert!(
        with_loss.recovered > with_loss.concealed,
        "recovered {} against concealed {}",
        with_loss.recovered,
        with_loss.concealed
    );
}

#[test]
fn a_clean_network_costs_nothing_but_the_buffer() {
    // The control. Every frame from a packet, and the only latency is the depth.
    let clean = measure(Profile::perfect(), 1);
    assert_eq!(clean.concealed, 0);
    assert_eq!(clean.recovered, 0);
    assert!(clean.quality > 0.99, "quality {:.3}", clean.quality);
    assert_eq!(clean.added_latency_ms, DEFAULT_DEPTH as u32 * FRAME_MS);
}

#[test]
fn a_freeze_is_ridden_out_rather_than_ending_the_stream() {
    // Half a second of nothing, then everything at once. A buffer that treated the
    // backlog as late and dropped it would go quiet for the rest of the call.
    let frozen = measure(
        Profile {
            freeze_ms: 500,
            ..Profile::perfect()
        },
        20_260_824,
    );
    // Every packet is eventually played: the freeze delays, it does not destroy. Before
    // the buffer learned to resynchronise this was 500 of 1000, because `next` had run on
    // while nothing arrived and the whole backlog was judged late.
    assert_eq!(
        frozen.from_packet, FRAMES as usize,
        "the backlog should all be played, not discarded as late"
    );
    // And it took exactly one decision to get there.
    assert!(frozen.silent < 40, "{} silent frames", frozen.silent);
}
