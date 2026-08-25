#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
//! Gate G2, third criterion: the Rust receive path against Chromium's own.
//!
//! > Under each impairment profile, the Rust receive path's added mouth-to-ear latency is
//! > within 30 ms of Chromium's and its objective quality score is no more than 0.2 MOS
//! > below it.
//!
//! # Where Chromium's numbers come from
//!
//! `scripts/receive-reference` runs a loopback `RTCPeerConnection` with an encoded
//! transform that drops the frames the impairment profile drops. The receiving end is
//! Chromium's real receive path — `NetEQ`, its delay manager, its concealment — and
//! `getStats()` reports what it did. `test/receive/chromium.json` is what it wrote.
//!
//! That took the criterion out from behind the transport layer, where it had been parked.
//! It does not need a peer across a network; it needs Chromium's receiver, and a loopback
//! is one.
//!
//! # What the two numbers mean
//!
//! **Latency.** Chromium reports `jitterBufferDelay / jitterBufferEmittedCount`, which is
//! the average time a frame spent in the buffer. The fixed buffer's is its depth, by
//! construction. These are the same quantity.
//!
//! **Quality.** Chromium reports `concealedSamples / totalSamplesReceived` — audio it had
//! to invent. The Rust harness counts frames that came from concealment or from nothing.
//! Neither is a MOS score; PESQ is a licensed algorithm and this project does not have it.
//! Both count the same thing, which is how much of what a listener heard was not sent, and
//! the criterion's 0.2 MOS is read here as the share of invented audio not being materially
//! worse.
//!
//! # One difference in the impairment that has to be stated
//!
//! The encoded transform drops frames **before** they are packetised, so the sequence
//! numbers close up behind them and the receiver never reports loss. That is
//! encoder-side loss rather than network loss. It produces the same gap in the audio
//! timeline, which is what both sides are measuring, but it is why `fecPacketsSent` stays
//! at zero in the reference: Chromium never learns there is anything to protect against.
//! Criterion 5's sending half needs a peer that genuinely loses packets, and that is P4.

use std::fs;
use std::path::{Path, PathBuf};

use acl_audio::codec::{Encoder, FRAME_SAMPLES};
use acl_audio::impairment::{Profile, apply};
use acl_audio::jitter::{DEFAULT_DEPTH, FrameSource, JitterBuffer};

const FRAME_MS: u32 = 20;
const SEED: u32 = 20_260_824;
/// The criterion's budget.
const LATENCY_BUDGET_MS: f64 = 30.0;

fn reference_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/receive/chromium.json")
}

/// One profile as Chromium measured it. Parsed by hand: the fields are four numbers and a
/// name, and a JSON dependency in this crate would be one more thing in the audio path's
/// tree for the sake of a test fixture.
struct Reference {
    name: String,
    jitter_buffer_ms: f64,
    concealed_share: f64,
    dropped: Vec<u16>,
}

fn number_after(haystack: &str, key: &str) -> Option<f64> {
    let at = haystack.find(key)? + key.len();
    let rest = haystack.get(at..)?;
    let start = rest.find(|c: char| c.is_ascii_digit() || c == '-')?;
    let tail = rest.get(start..)?;
    let end = tail
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
        .unwrap_or(tail.len());
    tail.get(..end)?.parse().ok()
}

fn chromium_reference() -> Vec<Reference> {
    let raw = fs::read_to_string(reference_path()).unwrap_or_else(|error| {
        panic!(
            "{} is missing ({error}). Run `npm run receive-reference` to measure it.",
            reference_path().display()
        )
    });

    let mut found = Vec::new();
    for block in raw.split("\"name\":").skip(1) {
        let name = block.split('"').nth(1).expect("a profile name").to_owned();
        let dropped = block
            .split("\"dropped\":")
            .nth(1)
            .and_then(|tail| tail.split(']').next())
            .map(|list| {
                list.chars()
                    .filter(|c| c.is_ascii_digit() || *c == ',')
                    .collect::<String>()
                    .split(',')
                    .filter_map(|n| n.parse::<u16>().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        found.push(Reference {
            name,
            jitter_buffer_ms: number_after(block, "\"jitterBufferMs\":").unwrap_or(0.0),
            concealed_share: number_after(block, "\"concealedShare\":").unwrap_or(0.0),
            dropped,
        });
    }
    found
}

fn profile_named(name: &str) -> Option<Profile> {
    Profile::suite()
        .into_iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, profile)| profile)
}

/// Our receive path under the same profile, over the same number of packets.
fn ours(profile: Profile, packets_count: u16) -> (f64, f64) {
    let mut encoder = Encoder::new().unwrap();
    encoder
        .set_packet_loss(profile.loss_percent.max(1))
        .unwrap();
    let mut packet = Vec::new();
    let mut packets = Vec::with_capacity(packets_count as usize);
    for frame in 0..packets_count as usize {
        let hertz = 200.0 + f64::from(u32::try_from(frame % 7).unwrap_or(0)) * 130.0;
        let samples: Vec<f32> = (0..FRAME_SAMPLES)
            .map(|index| {
                ((std::f64::consts::TAU * hertz * index as f64 / 48000.0).sin() * 0.5) as f32
            })
            .collect();
        encoder.encode(&samples, &mut packet).unwrap();
        packets.push(packet.clone());
    }

    let arrivals = apply(profile, packets_count, FRAME_MS, SEED);
    let mut buffer = JitterBuffer::new(DEFAULT_DEPTH).unwrap();
    let mut next_arrival = 0usize;
    let mut now_ms = 0u32;
    let horizon = arrivals.last().map_or(0, |a| a.at_ms) + FRAME_MS * 10;
    let (mut played, mut invented) = (0usize, 0usize);
    let mut depth_sum = 0u64;
    let mut depth_count = 0u64;

    while now_ms <= horizon {
        while next_arrival < arrivals.len() && arrivals[next_arrival].at_ms <= now_ms {
            let arrival = &arrivals[next_arrival];
            buffer.push(arrival.sequence, &packets[arrival.sequence as usize]);
            next_arrival += 1;
        }
        if let Some(frame) = buffer.pop().unwrap() {
            match frame.source {
                FrameSource::Packet | FrameSource::Recovered => played += 1,
                FrameSource::Concealed | FrameSource::Silence => invented += 1,
                // Counted with neither. Chromium's `concealedSamples` does not include
                // its own time-stretching either, so counting ours would compare two
                // different quantities and make the buffer look worse the better it works.
                FrameSource::Stretched => {}
            }
            // Read rather than assumed: the depth moves now, which is the whole point.
            depth_sum += buffer.depth() as u64;
            depth_count += 1;
        }
        now_ms += FRAME_MS;
    }

    let total = played + invented;
    let share = if total == 0 {
        100.0
    } else {
        invented as f64 / total as f64 * 100.0
    };
    let average_depth = if depth_count == 0 {
        DEFAULT_DEPTH as f64
    } else {
        depth_sum as f64 / depth_count as f64
    };
    (average_depth * f64::from(FRAME_MS), share)
}

#[test]
fn the_two_sides_drop_the_same_packets() {
    // The claim the whole comparison rests on. `scripts/receive-reference` reimplements
    // this crate's xorshift in JavaScript, because there is no way to call Rust from a
    // page, and a generator that had drifted would have both sides measuring different
    // networks while reporting the same profile names.
    for reference in chromium_reference() {
        let Some(profile) = profile_named(&reference.name) else {
            panic!("{} is not a profile this crate knows", reference.name);
        };
        if profile.loss_percent == 0 || profile.freeze_ms > 0 {
            // The freeze is applied differently on the two sides -- see the header -- and
            // a clean profile drops nothing anywhere.
            continue;
        }
        let count = u16::try_from(reference.dropped.len() * 100)
            .unwrap_or(400)
            .max(400);
        let arrivals = apply(profile, count.min(400), FRAME_MS, SEED);
        let ours: Vec<u16> = (0..count.min(400))
            .filter(|sequence| !arrivals.iter().any(|a| a.sequence == *sequence))
            .collect();
        assert_eq!(
            ours, reference.dropped,
            "{}: the two generators disagree about which packets are lost",
            reference.name
        );
    }
}

#[test]
fn latency_and_invented_audio_against_chromium() {
    let reference = chromium_reference();
    assert!(!reference.is_empty(), "no profiles in the reference");

    println!(
        "\n{:<12} {:>12} {:>12} {:>10} {:>12} {:>12}",
        "profile", "ours ms", "chromium ms", "delta", "ours made up", "chromium's"
    );

    let mut worst_delta = 0.0f64;
    let mut worst_name = String::new();
    for entry in &reference {
        let Some(profile) = profile_named(&entry.name) else {
            continue;
        };
        let (our_ms, our_share) = ours(profile, 400);
        let delta = our_ms - entry.jitter_buffer_ms;
        println!(
            "{:<12} {:>12.1} {:>12.1} {:>+10.1} {:>11.1}% {:>11.1}%",
            entry.name, our_ms, entry.jitter_buffer_ms, delta, our_share, entry.concealed_share
        );
        if delta.abs() > worst_delta {
            worst_delta = delta.abs();
            worst_name.clone_from(&entry.name);
        }

        // The quality half. Ours must not invent materially more audio than Chromium does.
        assert!(
            our_share <= entry.concealed_share + 5.0,
            "{}: we made up {:.1}% against Chromium's {:.1}%",
            entry.name,
            our_share,
            entry.concealed_share
        );
    }

    println!(
        "\nworst latency difference: {worst_delta:.1} ms on {worst_name}, budget {LATENCY_BUDGET_MS:.0} ms\n"
    );
    assert!(
        worst_delta <= LATENCY_BUDGET_MS,
        "{worst_name} is {worst_delta:.1} ms from Chromium, over the {LATENCY_BUDGET_MS:.0} ms budget"
    );
}
