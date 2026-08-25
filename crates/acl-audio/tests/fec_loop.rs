#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
//! §3e end to end: a loss report turns into audio that survives the loss.
//!
//! The pieces are tested on their own — `fec` decides what to tell the encoder, `codec`
//! passes it to libopus, `jitter` reconstructs a lost frame out of the next one. This is
//! the join, and the join is where §3e says the failure lives:
//!
//! > A Rust client that sets the flag but sends no RR achieves nothing, and because the
//! > Chromium peer then never learns it is losing packets either, it stops emitting FEC
//! > too.
//!
//! What is *not* here is the other end of that sentence — a Chromium peer, and receiver
//! reports off a real network. Both belong to phase 4, and criterion 5 of gate G2 is
//! measured against a Chromium sender, not this. The loss reports below stand in for the
//! ones `ReceiverReportInterceptor` will produce.

use acl_audio::codec::{Encoder, FRAME_SAMPLES};
use acl_audio::fec::{FecController, MAX_APPLIED};
use acl_audio::impairment::{Profile, apply};
use acl_audio::jitter::{DEFAULT_DEPTH, FrameSource, JitterBuffer};

const FRAMES: u16 = 600;
const FRAME_MS: u32 = 20;
const LOSS: u8 = 10;

/// Speech-like material: a pitch that steps every frame, so concealment cannot fake it.
fn source(frame: usize) -> Vec<f32> {
    let hertz = 200.0 + f64::from(u32::try_from(frame % 7).unwrap_or(0)) * 130.0;
    (0..FRAME_SAMPLES)
        .map(|index| ((std::f64::consts::TAU * hertz * index as f64 / 48000.0).sin() * 0.5) as f32)
        .collect()
}

/// How many output frames came from the redundancy, over a run at `LOSS` percent.
///
/// `told_the_encoder` is the only difference between the two runs: whether the loop is
/// closed or the flag is set and idle.
fn recovered_frames(told_the_encoder: bool) -> (usize, usize, u8) {
    let mut encoder = Encoder::new().unwrap();
    let mut controller = FecController::new();

    if told_the_encoder {
        // What phase 4 will do on every receiver report: convert RFC 3550's fixed-point
        // fraction, and only call libopus when the controller says it is worth it.
        let fraction = u8::try_from(u32::from(LOSS) * 256 / 100).unwrap();
        for _ in 0..20 {
            if let Some(percent) = controller.observe_fraction_lost(fraction) {
                encoder.set_packet_loss(percent).unwrap();
            }
        }
    }
    let applied = encoder.packet_loss();

    let mut packet = Vec::new();
    let mut packets = Vec::with_capacity(FRAMES as usize);
    for frame in 0..FRAMES as usize {
        encoder.encode(&source(frame), &mut packet).unwrap();
        packets.push(packet.clone());
    }

    // The same dropped packets in both runs, so the comparison is about the encoder and
    // nothing else.
    let arrivals = apply(
        Profile {
            loss_percent: LOSS,
            ..Profile::perfect()
        },
        FRAMES,
        FRAME_MS,
        20_260_824,
    );

    let mut buffer = JitterBuffer::new(DEFAULT_DEPTH).unwrap();
    let mut recovered = 0;
    let mut concealed = 0;
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
            match frame.source {
                FrameSource::Recovered => recovered += 1,
                FrameSource::Concealed | FrameSource::Silence => concealed += 1,
                FrameSource::Packet | FrameSource::Stretched => {}
            }
        }
        now_ms += FRAME_MS;
    }

    (recovered, concealed, applied)
}

#[test]
fn a_loss_report_becomes_redundancy_that_is_actually_recovered() {
    let (with_reports, gaps_with, applied) = recovered_frames(true);
    let (without_reports, gaps_without, idle) = recovered_frames(false);

    println!(
        "loop closed:  encoder told {applied}%, {with_reports} frames recovered, {gaps_with} gaps"
    );
    println!(
        "loop open:    encoder told {idle}%, {without_reports} frames recovered, {gaps_without} gaps"
    );

    // The controller reached the encoder at all.
    assert!(applied > 0, "the controller told the encoder nothing");
    assert_eq!(idle, 0, "the unreported run should have no protection");

    // And it bought something. This is the measurement §3e is about: the flag on its own
    // is worth nothing, and the difference between these two numbers is what the receiver
    // reports are for.
    assert!(
        with_reports > without_reports * 4,
        "recovery barely moved: {with_reports} against {without_reports}"
    );
    assert!(
        gaps_with < gaps_without,
        "gaps did not improve: {gaps_with} against {gaps_without}"
    );
}

#[test]
fn the_bitrate_stays_above_the_floor_that_makes_redundancy_possible() {
    // §3e: below roughly 16-20 kbps libopus carries no meaningful redundancy, so a change
    // that lowered the bitrate would switch the whole loop off silently -- the flag would
    // still read as set and the loss percentage would still be applied.
    let mut encoder = Encoder::new().unwrap();
    let bitrate = encoder.bitrate().unwrap();
    assert!(
        bitrate >= 20_000,
        "libopus settled on {bitrate} bps, below the floor where LBRR carries anything"
    );
}

#[test]
fn a_hostile_report_cannot_take_the_encoder_past_the_clamp() {
    // The end-to-end version of the clamp: the byte comes off the network, and the peer
    // that sends it is not the one whose audio suffers.
    let mut encoder = Encoder::new().unwrap();
    let mut controller = FecController::new();
    for _ in 0..500 {
        if let Some(percent) = controller.observe_fraction_lost(255) {
            encoder.set_packet_loss(percent).unwrap();
        }
    }
    assert_eq!(encoder.packet_loss(), MAX_APPLIED);
}

#[test]
fn a_peer_that_falls_silent_stops_costing_bitrate() {
    // A peer that left, or whose reports stopped arriving. Without this the encoder would
    // carry redundancy for the rest of the call for a correspondent who is not there.
    let mut encoder = Encoder::new().unwrap();
    let mut controller = FecController::new();
    for _ in 0..30 {
        if let Some(percent) = controller.observe_fraction_lost(31) {
            encoder.set_packet_loss(percent).unwrap();
        }
    }
    assert!(encoder.packet_loss() > 0, "no protection was ever applied");

    for _ in 0..300 {
        if let Some(percent) = controller.idle() {
            encoder.set_packet_loss(percent).unwrap();
        }
    }
    assert_eq!(encoder.packet_loss(), 0);
}
