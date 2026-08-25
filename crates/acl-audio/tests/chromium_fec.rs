#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]
//! Gate G2, fifth criterion, against packets Chromium actually encoded.
//!
//! > Under a 5% loss profile with a Chromium sender, the Rust receive path recovers Opus
//! > in-band FEC — `decode(..., fec: true)` on the following packet, driven by the jitter
//! > buffer's loss signal — and `getStats()` on the Electron peer shows `fecPacketsSent`
//! > climbing in both directions.
//!
//! # Why this does not need the transport layer
//!
//! The obvious reading of "a Chromium sender" is a Chromium peer on the other end of a
//! connection, which would put the whole criterion behind P4. But what the receive path
//! needs from Chromium is *packets encoded the way Chromium encodes them* — with the
//! redundant copy libwebrtc asks libopus for when it believes there is loss. Chromium's
//! `AudioEncoder` is that encoder and it is reachable from a page.
//!
//! `scripts/opus-vectors` captures 1000 frames of it, and `test/opus/chromium-fec.bin` is
//! what it wrote. This is the same move `scripts/golden-vectors` makes for the DSP: let
//! Chromium be the reference rather than a specification read carefully.
//!
//! # The other direction
//!
//! Whether a *Chromium* receiver recovers **our** redundancy is the fourth leg, and it is
//! the one thing here that cannot be settled by looking at bytes: what is in question is
//! not our packets but what Chromium's decoder does with them.
//!
//! `scripts/our-fec` settles it. An encoded transform can replace a frame's payload rather
//! than only drop it, so Chromium packetises our Opus, sends it to itself, and its own
//! receive path decodes it — with the loss injected on the *receiving* side, after
//! depacketisation, because loss injected before packetisation closes the sequence numbers
//! up and nothing downstream learns a frame is missing.
//!
//! `the_chromium_receiver_recovers_ours` below reads what it measured.

use std::fs;
use std::path::{Path, PathBuf};

use acl_audio::codec::{FRAME_SAMPLES, has_redundancy};
use acl_audio::impairment::{Profile, apply};
use acl_audio::jitter::{DEFAULT_DEPTH, FrameSource, JitterBuffer};

const FRAME_MS: u32 = 20;
const SEED: u32 = 20_260_824;

fn vector_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/opus/chromium-fec.bin")
}

/// Reads the length-prefixed packet file `scripts/opus-vectors` writes.
fn chromium_packets() -> Vec<Vec<u8>> {
    let raw = fs::read(vector_path()).unwrap_or_else(|error| {
        panic!(
            "{} is missing ({error}). Run `npm run opus-vectors` to capture it from Chromium.",
            vector_path().display()
        )
    });
    let count = u32::from_le_bytes(raw[0..4].try_into().expect("a count")) as usize;
    let mut packets = Vec::with_capacity(count);
    let mut at = 4usize;
    for _ in 0..count {
        let length = u32::from_le_bytes(raw[at..at + 4].try_into().expect("a length")) as usize;
        at += 4;
        packets.push(raw[at..at + length].to_vec());
        at += length;
    }
    packets
}

#[test]
fn chromium_actually_put_redundancy_in_the_packets() {
    // Everything below rests on this, and it is not a given: `useinbandfec` is a request,
    // and libopus only spends bits on a redundant copy when the loss it has been told
    // about makes one worth carrying. Below about 5% it emits none at all -- measured, and
    // recorded in §4.5 -- which is exactly why the capture tells it 5%.
    //
    // Asked with `opus_packet_has_lbrr` rather than by decoding, because a decode with
    // `fec: true` succeeds either way: given no redundancy it produces concealment and
    // returns the same frame size. That is the trap this project already fell into once.
    let packets = chromium_packets();
    assert!(
        packets.len() > 900,
        "only {} packets captured",
        packets.len()
    );

    let carrying = packets.iter().filter(|p| has_redundancy(p)).count();
    let share = carrying as f64 / packets.len() as f64;
    println!(
        "Chromium put redundancy in {carrying} of {} packets ({:.0}%)",
        packets.len(),
        share * 100.0
    );
    assert!(
        carrying > 0,
        "Chromium emitted no in-band FEC at all -- the capture's encoder configuration is not doing what it claims"
    );
}

/// Runs the captured packets through one profile and reports where the audio came from.
fn receive(profile: Profile, packets: &[Vec<u8>]) -> (usize, usize, usize) {
    let count = u16::try_from(packets.len().min(1000)).expect("a thousand packets");
    let arrivals = apply(profile, count, FRAME_MS, SEED);
    let mut buffer = JitterBuffer::new(DEFAULT_DEPTH).unwrap();

    let mut next_arrival = 0usize;
    let mut now_ms = 0u32;
    let horizon = arrivals.last().map_or(0, |a| a.at_ms) + FRAME_MS * 10;
    let (mut from_packet, mut recovered, mut gaps) = (0usize, 0usize, 0usize);

    while now_ms <= horizon {
        while next_arrival < arrivals.len() && arrivals[next_arrival].at_ms <= now_ms {
            let arrival = &arrivals[next_arrival];
            buffer.push(arrival.sequence, &packets[arrival.sequence as usize]);
            next_arrival += 1;
        }
        if let Some(frame) = buffer.pop().unwrap() {
            assert_eq!(frame.samples.len(), FRAME_SAMPLES);
            match frame.source {
                FrameSource::Packet => from_packet += 1,
                FrameSource::Recovered => recovered += 1,
                FrameSource::Concealed | FrameSource::Silence => gaps += 1,
                // A deliberate delay, not a hole: the packet it holds back is played on
                // the next pop rather than lost.
                FrameSource::Stretched => {}
            }
        }
        now_ms += FRAME_MS;
    }
    (from_packet, recovered, gaps)
}

#[test]
fn the_receive_path_recovers_chromiums_redundancy_at_five_percent_loss() {
    // The criterion itself, in the direction this crate can observe.
    let packets = chromium_packets();
    let (from_packet, recovered, gaps) = receive(
        Profile {
            loss_percent: 5,
            ..Profile::perfect()
        },
        &packets,
    );
    println!("5% loss: {from_packet} from packets, {recovered} recovered, {gaps} gaps");

    assert!(
        recovered > 0,
        "nothing was recovered from a Chromium sender that emitted redundancy"
    );
    // Most of what was lost should come back. The rest is where two consecutive packets
    // went, which no single redundant copy can help with.
    assert!(
        recovered > gaps,
        "recovered {recovered} against {gaps} gaps -- the redundancy is barely being used"
    );
}

#[test]
fn the_recovery_is_the_redundancys_and_not_concealment_wearing_its_name() {
    // The check that makes the one above mean something. Concealment also produces a frame
    // of the right length and also reports success; this project counted those as
    // recoveries once and reported identical numbers for a sender that had been told about
    // loss and one that never had.
    //
    // Here the two senders are Chromium with redundancy, and the same packets with the
    // redundancy filtered out by keeping only the ones that carry none.
    let packets = chromium_packets();
    let without: Vec<Vec<u8>> = packets
        .iter()
        .map(|packet| {
            if has_redundancy(packet) {
                // Not a real Opus packet any more, which is the point: the buffer must not
                // be able to recover from it. Kept the same length so the profile applies
                // identically.
                packets
                    .iter()
                    .find(|other| !has_redundancy(other))
                    .cloned()
                    .unwrap_or_else(|| packet.clone())
            } else {
                packet.clone()
            }
        })
        .collect();

    let profile = Profile {
        loss_percent: 5,
        ..Profile::perfect()
    };
    let (_, with_redundancy, _) = receive(profile, &packets);
    let (_, without_redundancy, _) = receive(profile, &without);
    println!("recovered with redundancy: {with_redundancy}, without: {without_redundancy}");

    assert!(
        with_redundancy > without_redundancy,
        "the same number came back either way ({with_redundancy} against {without_redundancy}), which means the counter is measuring concealment"
    );
}

/// What `scripts/our-fec` measured, parsed by hand for the same reason the rest of this
/// crate parses fixtures by hand: two numbers do not justify a JSON dependency in the
/// audio path's tree.
fn concealed_share(block: &str) -> Option<f64> {
    let at = block.find("\"concealedShare\":")? + "\"concealedShare\":".len();
    let rest = block.get(at..)?;
    let start = rest.find(|c: char| c.is_ascii_digit())?;
    let tail = rest.get(start..)?;
    let end = tail
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == '-'))
        .unwrap_or(tail.len());
    tail.get(..end)?.parse().ok()
}

#[test]
fn the_chromium_receiver_recovers_ours() {
    // Criterion 5's fourth leg, and the only one that is about Chromium's behaviour rather
    // than about bytes. Two runs of the same audio through Chromium's receive path, losing
    // the same frames: one encoded with the redundancy, one without.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/receive/our-fec.json");
    let raw = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} is missing ({error}). Run `npm run our-fec` to measure it.",
            path.display()
        )
    });

    let with = raw
        .split("\"withRedundancy\":")
        .nth(1)
        .and_then(concealed_share)
        .expect("a concealed share for the protected run");
    let without = raw
        .split("\"withoutRedundancy\":")
        .nth(1)
        .and_then(concealed_share)
        .expect("a concealed share for the bare run");

    println!("Chromium concealed {with:.2}% of ours with redundancy, {without:.2}% without");
    assert!(
        without > with,
        "Chromium concealed no less from a stream carrying redundancy ({with:.2}% against {without:.2}%)"
    );
    // A margin, so a run that differed by measurement noise could not read as a recovery.
    assert!(
        without - with > 0.5,
        "the difference is {:.2} points, small enough to be noise",
        without - with
    );
}
