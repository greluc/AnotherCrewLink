#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
//! Writes this client's own Opus packets out, for a Chromium receiver to decode.
//!
//! Gate G2's fifth criterion has four legs. Three are met by inspecting packets — Chromium
//! emits redundancy, our receiver recovers it, we emit redundancy. The fourth is whether a
//! *Chromium* receiver recovers **ours**, and that needs Chromium to be handed our bytes.
//!
//! `scripts/our-fec` does the handing: a loopback peer connection whose sender transform
//! replaces every encoded frame's payload with one of these, so what arrives at Chromium's
//! receive path — `NetEQ`, libopus, its FEC recovery — is this client's stream rather than
//! Chromium's own.
//!
//! Two files, because a measurement with nothing to compare against says nothing:
//!
//! - `ours-fec.bin`, encoded after the controller reported 5% loss, so the packets carry
//!   the redundant copy.
//! - `ours-nofec.bin`, the same audio with the encoder never told about loss, so they carry
//!   none.
//!
//! If Chromium conceals less from the first than from the second, it recovered ours.
//!
//! This is a test rather than a binary so it needs no new target and runs with everything
//! else; it is `#[ignore]`d because it writes files rather than checking anything.

use std::fs;
use std::path::{Path, PathBuf};

use acl_audio::codec::{Encoder, FRAME_SAMPLES, SAMPLE_RATE, has_redundancy};

/// Ten seconds, which is long enough for a peer connection to settle and be measured.
const FRAMES: usize = 500;

fn output_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/opus")
}

/// The same stepped pitch every other harness here uses, so concealment cannot imitate it.
fn source(frame: usize) -> Vec<f32> {
    let hertz = 200.0 + f64::from(u32::try_from(frame % 7).unwrap_or(0)) * 130.0;
    (0..FRAME_SAMPLES)
        .map(|index| {
            ((std::f64::consts::TAU * hertz * index as f64 / f64::from(SAMPLE_RATE)).sin() * 0.5)
                as f32
        })
        .collect()
}

/// The same length-prefixed shape `scripts/opus-vectors` writes.
fn write(path: &Path, packets: &[Vec<u8>]) {
    let mut out = Vec::new();
    out.extend_from_slice(&u32::try_from(packets.len()).unwrap().to_le_bytes());
    for packet in packets {
        out.extend_from_slice(&u32::try_from(packet.len()).unwrap().to_le_bytes());
        out.extend_from_slice(packet);
    }
    fs::write(path, &out).expect("the vectors should be writable");
}

fn encode(told_about_loss: bool) -> Vec<Vec<u8>> {
    let mut encoder = Encoder::new().unwrap();
    if told_about_loss {
        encoder.set_packet_loss(5).unwrap();
    }
    let mut packet = Vec::new();
    let mut packets = Vec::with_capacity(FRAMES);
    for frame in 0..FRAMES {
        encoder.encode(&source(frame), &mut packet).unwrap();
        packets.push(packet.clone());
    }
    packets
}

#[test]
#[ignore = "writes test/opus/ours-*.bin rather than checking anything"]
fn write_the_vectors_a_chromium_receiver_will_be_given() {
    fs::create_dir_all(output_directory()).expect("the directory should be creatable");

    let protected = encode(true);
    let bare = encode(false);

    let with = protected.iter().filter(|p| has_redundancy(p)).count();
    let without = bare.iter().filter(|p| has_redundancy(p)).count();
    println!("redundancy: protected {with} of {FRAMES}, bare {without} of {FRAMES}");

    // The vectors are only worth writing if they differ in the one way the measurement is
    // about. Two identical streams would produce two identical concealment figures and
    // read as "Chromium recovered nothing".
    assert!(
        with > FRAMES / 2,
        "the protected stream carries almost no redundancy"
    );
    assert_eq!(
        without, 0,
        "the bare stream carries redundancy it should not"
    );

    write(&output_directory().join("ours-fec.bin"), &protected);
    write(&output_directory().join("ours-nofec.bin"), &bare);
    println!("wrote ours-fec.bin and ours-nofec.bin");
}
