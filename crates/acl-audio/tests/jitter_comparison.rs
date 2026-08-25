#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
//! `neteq` against the fixed buffer, under the same impairment.
//!
//! §4.5 item 3d asks for exactly this and says why: *"without it the gate has no baseline
//! to judge `NetEQ` against — and no fallback short of porting the reference
//! implementation, which is a multi-week job."* "`NetEQ` is better" is an assumption
//! everybody in this field shares, this project included, and it had never been tested
//! here against the thing it would replace.
//!
//! # What the two are
//!
//! `acl_audio::jitter` is a fixed buffer three frames deep. It holds packet *N+1* before
//! giving up on *N*, which is what lets it recover the redundant copy Opus put there, and
//! it resynchronises after a stall rather than treating the backlog as late.
//!
//! `neteq` adapts its depth to what the network is doing, which is the thing a fixed
//! buffer cannot: it can be shallow on a good connection and deepen on a bad one. What it
//! cannot do is in-band error correction, because its decoder trait has no way to say
//! "recover the previous packet from this one" — see `acl_audio::neteq_bridge`.
//!
//! So this is not a like-for-like race. It is the question the gate actually has to
//! answer: is an adaptive delay worth more than error correction on the networks this
//! app's players are on?
//!
//! # What this measurement can and cannot say
//!
//! **It cannot pronounce on `NetEQ`, and the numbers below must not be read as doing so.**
//!
//! `neteq` 0.9.1's delay manager ignores the `arrival_time` on the packet it is given and
//! calls `Instant::now()` itself (`delay_manager.rs:245`), measuring the interval between
//! consecutive `insert_packet` calls. There is no clock to inject. So the network `NetEQ`
//! believes it is on is *the timing of this test process*, and the only way to give it
//! something resembling the intended one is to run in real time — which this does, at five
//! seconds per profile, which is why it is `#[ignore]`d.
//!
//! Real time from a test is not accurate time. Windows' timer granularity is about 15 ms
//! against a 20 ms packet interval, so the harness contributes jitter of the same order as
//! the thing being simulated. The `clean` profile is the control that shows it: `NetEQ`
//! reports around 11% of frames as `Expand` on a network with **no impairment at all**,
//! which is not a property of `NetEQ` but the sound of this harness's own scheduling.
//!
//! Two earlier versions of this file were worse and looked better. Both drove `NetEQ` from
//! simulated time, its estimator saturated at `base_maximum_delay_ms`, and every frame came
//! back as concealment — reported, in one run, as a tidy table of `NetEQ` under packet loss.
//!
//! # So what is it for
//!
//! Three things it does establish:
//!
//! 1. **The bridge works.** `neteq` decodes through the same libopus as everything else,
//!    which is what item 3d asked for and what keeps a second Opus out of the binary.
//! 2. **The fixed buffer's own numbers**, which are measured under the same real-time
//!    conditions and are therefore comparable *to each other*: how it degrades from a
//!    clean network to 10% loss to a half-second freeze.
//! 3. **That evaluating `NetEQ` properly needs something this crate does not have** — either
//!    a version whose delay manager takes a clock, or a harness that is not a test.

use std::time::{Duration, Instant};

use acl_audio::codec::{Encoder, FRAME_SAMPLES};
use acl_audio::impairment::{Profile, apply};
use acl_audio::jitter::{DEFAULT_DEPTH, FrameSource, JitterBuffer};
use acl_audio::neteq_bridge::{OPUS_PAYLOAD_TYPE, OpusForNetEq};
use neteq::neteq::{NetEq, NetEqConfig, SpeechType};
use neteq::packet::{AudioPacket, RtpHeader};

/// Five seconds. Short because the `NetEQ` half runs in real time -- see below.
const FRAMES: u16 = 250;
const FRAME_MS: u32 = 20;
/// What `NetEQ` returns per `get_audio`, whatever the packets carry.
const NETEQ_BLOCK_MS: u32 = 10;
const SEED: u32 = 20_260_824;

/// One buffer's behaviour under one profile.
struct Result_ {
    played: usize,
    gaps: usize,
    latency_ms: u32,
}

impl Result_ {
    fn gap_share(&self) -> f64 {
        let total = self.played + self.gaps;
        if total == 0 {
            return 1.0;
        }
        self.gaps as f64 / total as f64
    }
}

/// Speech-like material: a pitch that steps every frame, so concealment cannot fake it.
fn source(frame: usize) -> Vec<f32> {
    let hertz = 200.0 + f64::from(u32::try_from(frame % 7).unwrap_or(0)) * 130.0;
    (0..FRAME_SAMPLES)
        .map(|index| ((std::f64::consts::TAU * hertz * index as f64 / 48000.0).sin() * 0.5) as f32)
        .collect()
}

fn encode(loss_percent: u8) -> Vec<Vec<u8>> {
    let mut encoder = Encoder::new().unwrap();
    // Told about the loss it will meet, which is what makes it emit the redundancy the
    // fixed buffer recovers from. A sender that is never told achieves nothing by having
    // the flag set.
    encoder.set_packet_loss(loss_percent.max(1)).unwrap();
    let mut packet = Vec::new();
    let mut packets = Vec::with_capacity(FRAMES as usize);
    for frame in 0..FRAMES as usize {
        encoder.encode(&source(frame), &mut packet).unwrap();
        packets.push(packet.clone());
    }
    packets
}

fn through_fixed(profile: Profile, packets: &[Vec<u8>]) -> Result_ {
    let arrivals = apply(profile, FRAMES, FRAME_MS, SEED);
    let mut buffer = JitterBuffer::new(DEFAULT_DEPTH).unwrap();

    let mut next_arrival = 0usize;
    let mut now_ms = 0u32;
    let horizon = arrivals.last().map_or(0, |a| a.at_ms) + FRAME_MS * 10;
    let (mut played, mut gaps) = (0usize, 0usize);

    while now_ms <= horizon {
        while next_arrival < arrivals.len() && arrivals[next_arrival].at_ms <= now_ms {
            let arrival = &arrivals[next_arrival];
            buffer.push(arrival.sequence, &packets[arrival.sequence as usize]);
            next_arrival += 1;
        }
        if let Some(frame) = buffer.pop().unwrap() {
            match frame.source {
                FrameSource::Packet | FrameSource::Recovered => played += 1,
                FrameSource::Concealed | FrameSource::Silence => gaps += 1,
                // A delay rather than a hole, and NetEQ's own time-stretching is not
                // counted as concealment either -- counting ours would tilt the comparison.
                FrameSource::Stretched => {}
            }
        }
        now_ms += FRAME_MS;
    }

    Result_ {
        played,
        gaps,
        latency_ms: DEFAULT_DEPTH as u32 * FRAME_MS,
    }
}

fn through_neteq(profile: Profile, packets: &[Vec<u8>]) -> Result_ {
    let arrivals = apply(profile, FRAMES, FRAME_MS, SEED);

    let mut neteq = NetEq::new(NetEqConfig {
        sample_rate: 48000,
        channels: 1,
        ..NetEqConfig::default()
    })
    .expect("neteq should build for 48 kHz mono");
    neteq.register_decoder(OPUS_PAYLOAD_TYPE, Box::new(OpusForNetEq::new().unwrap()));

    // This half runs in real time, and it has to.
    //
    // `AudioPacket` carries an `arrival_time`, and the delay manager ignores it:
    // `delay_manager.rs:245` calls `Instant::now()` itself and measures the interval
    // between that and the previous call. So the estimate is driven by when
    // `insert_packet` is *called*, not by when the packet is said to have arrived, and
    // there is no clock to inject -- the crate has no such seam.
    //
    // Feeding it a whole stream in a tight loop therefore reads as a network delivering
    // twenty milliseconds of audio every few microseconds. The estimator saturates at
    // `base_maximum_delay_ms`, NetEQ stretches every frame trying to reach a two-second
    // target it will never see, and every frame comes back classified `Expand`. Two
    // earlier versions of this test measured exactly that and reported it as NetEQ's
    // behaviour under loss, which it is not.
    let base = Instant::now();
    let mut next_arrival = 0usize;
    let mut now_ms = 0u32;
    let horizon = arrivals.last().map_or(0, |a| a.at_ms) + FRAME_MS * 10;
    let (mut played, mut gaps) = (0usize, 0usize);
    let mut depth_sum = 0u64;
    let mut depth_count = 0u64;

    // NetEQ hands back 10 ms per call -- its own source says so, next to the field that
    // holds what is left of a 20 ms packet. Pulling once per 20 ms, as the fixed buffer is
    // driven, fills it at twice the rate it drains: the first version of this measured a
    // 2.65 second buffer and no gaps at all under any profile, which is what a queue that
    // never runs dry looks like from the outside.
    while now_ms <= horizon {
        // Wait until the wall clock reaches this step, because the delay manager is
        // reading it. This is what makes the run take its full five seconds per profile.
        let due = base + Duration::from_millis(u64::from(now_ms));
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        }

        while next_arrival < arrivals.len() && arrivals[next_arrival].at_ms <= now_ms {
            let arrival = &arrivals[next_arrival];
            let packet = AudioPacket {
                header: RtpHeader::new(
                    arrival.sequence,
                    u32::from(arrival.sequence) * FRAME_SAMPLES as u32,
                    // One sender, so any stream identifier does; it only has to be stable.
                    1,
                    OPUS_PAYLOAD_TYPE,
                    false,
                ),
                payload: packets[arrival.sequence as usize].clone(),
                arrival_time: base + Duration::from_millis(u64::from(arrival.at_ms)),
                sample_rate: 48000,
                channels: 1,
                duration_ms: FRAME_MS,
            };
            let _ = neteq.insert_packet(packet);
            next_arrival += 1;
        }

        if let Ok(frame) = neteq.get_audio() {
            assert_eq!(
                frame.duration_ms(),
                NETEQ_BLOCK_MS,
                "neteq's block size changed; the pull rate above is now wrong"
            );
            // NetEQ's own classification, not the energy in the block. Concealment here is
            // extrapolation -- `expand.rs` continues the waveform rather than going quiet
            // -- so a version of this that asked "is there sound in it" reported no gaps at
            // 10% loss, which is not a thing any buffer can do.
            match frame.speech_type {
                SpeechType::Expand | SpeechType::Cng => gaps += 1,
                SpeechType::Normal | SpeechType::Music => played += 1,
            }
        }
        let stats = neteq.get_statistics();
        depth_sum += u64::from(stats.network.current_buffer_size_ms);
        depth_count += 1;
        now_ms += NETEQ_BLOCK_MS;
    }

    Result_ {
        played,
        gaps,
        latency_ms: u32::try_from(depth_sum / depth_count.max(1)).unwrap_or(u32::MAX),
    }
}

#[test]
#[ignore = "runs in real time: NetEQ's delay manager reads the wall clock, so this takes about a minute"]
fn neteq_against_the_fixed_buffer() {
    println!(
        "\n{:<12} {:>18} {:>18} {:>12} {:>12}",
        "profile", "fixed gaps", "neteq gaps", "fixed ms", "neteq ms"
    );

    let mut worst_fixed = 0.0f64;
    for (name, profile) in Profile::suite() {
        let packets = encode(profile.loss_percent);
        let fixed = through_fixed(profile, &packets);
        let adaptive = through_neteq(profile, &packets);

        println!(
            "{:<12} {:>17.1}% {:>17.1}% {:>12} {:>12}",
            name,
            fixed.gap_share() * 100.0,
            adaptive.gap_share() * 100.0,
            fixed.latency_ms,
            adaptive.latency_ms
        );

        // Neither may fall over. A buffer that produced nothing at all would be a stop,
        // not a comparison.
        assert!(fixed.played > 0, "{name}: the fixed buffer played nothing");
        assert!(adaptive.played > 0, "{name}: neteq played nothing");

        worst_fixed = worst_fixed.max(fixed.gap_share());
    }

    println!(
        "
fixed depth: {DEFAULT_DEPTH} frames. NetEQ's is what its delay manager settled on.
"
    );

    // Only the fixed buffer is held to a bar here. NetEQ's column is printed and not
    // asserted on, for the reason the header gives at length: this harness cannot give it
    // the network it is being asked about, so a threshold on its numbers would be a
    // threshold on Windows' timer granularity.
    assert!(
        worst_fixed < 0.25,
        "the fixed buffer left {:.1}% of frames as gaps on its worst profile",
        worst_fixed * 100.0
    );
}
