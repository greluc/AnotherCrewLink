#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! Gate G2, first criterion: every DSP node against Chromium's own output.
//!
//! > Every DSP node matches its golden vector to within −80 dBFS RMS error.
//!
//! The vectors come from `scripts/golden-vectors`, rendered inside Electron with
//! `OfflineAudioContext`. Both halves are given: the input the node was driven with and
//! the output it produced. That matters more than it looks — regenerating the input on
//! this side would mean a disagreement about the *input* could present as a disagreement
//! about the node, and the two need different fixes.
//!
//! # What is not measured yet
//!
//! A node with no implementation is named in [`UNIMPLEMENTED`] and its vectors are
//! reported rather than skipped quietly. The list is asserted against, so it shrinks
//! visibly and cannot grow by accident — a node that loses its implementation fails here
//! rather than disappearing from the measurement.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use acl_audio::biquad::{Biquad, FilterKind};
use acl_audio::gain::Gain;
use acl_audio::panner::{Panner, Position};
use acl_audio::wav;
use serde::Deserialize;

/// The gate's tolerance: RMS error, relative to full scale, in decibels.
const TOLERANCE_DBFS: f64 = -80.0;

/// Nodes this port does not implement yet.
///
/// Every vector for one of these is counted and reported. Emptying this list is what
/// finishes the first criterion of gate G2.
const UNIMPLEMENTED: [&str; 2] = ["chain", "convolver"];

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(rename = "sampleRate")]
    sample_rate: u32,
    vectors: Vec<Vector>,
}

#[derive(Debug, Deserialize)]
struct Vector {
    name: String,
    node: String,
    #[allow(dead_code)]
    input: String,
    /// The input vector this one was rendered from. Absent on the inputs themselves.
    #[serde(default)]
    from: Option<String>,
    config: serde_json::Value,
    channels: usize,
    #[allow(dead_code)]
    frames: usize,
    #[allow(dead_code)]
    sha256: String,
}

fn golden_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/golden")
}

fn read(name: &str) -> wav::Wav {
    let path = golden_directory().join(format!("{name}.wav"));
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}. Regenerate the vectors with `npm run golden`.",
            path.display()
        )
    });
    wav::decode(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The RMS of the difference between two signals, in decibels relative to full scale.
///
/// Full scale is 1.0, so a perfect match is negative infinity and the gate's −80 dB is a
/// difference whose RMS is one part in ten thousand. Returns `None` if the two are not
/// the same length, which is a different failure and worth telling apart.
fn rms_error_dbfs(reference: &[f32], measured: &[f32]) -> Option<f64> {
    if reference.len() != measured.len() {
        return None;
    }
    if reference.is_empty() {
        return Some(f64::NEG_INFINITY);
    }
    let sum: f64 = reference
        .iter()
        .zip(measured)
        .map(|(a, b)| {
            let difference = f64::from(*a) - f64::from(*b);
            difference * difference
        })
        .sum();
    #[allow(
        clippy::cast_precision_loss,
        reason = "a vector is at most a few hundred thousand samples"
    )]
    let mean = sum / reference.len() as f64;
    Some(if mean == 0.0 {
        f64::NEG_INFINITY
    } else {
        10.0 * mean.log10()
    })
}

fn number(config: &serde_json::Value, key: &str) -> f32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the generator writes these as ordinary node settings"
    )]
    {
        config
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0) as f32
    }
}

/// Runs one node over an input, or `None` if this port has no implementation for it.
fn run(vector: &Vector, input: &[f32], sample_rate: f32) -> Option<Vec<f32>> {
    match vector.node.as_str() {
        "gain" => {
            let gain = Gain::new(number(&vector.config, "value"));
            let mut out = input.to_vec();
            gain.process_block(&mut out);
            Some(out)
        }
        "biquad" => {
            let kind = match vector
                .config
                .get("type")
                .and_then(serde_json::Value::as_str)
            {
                Some("lowpass") => FilterKind::LowPass,
                Some("highpass") => FilterKind::HighPass,
                other => panic!("{}: filter type {other:?} is not implemented", vector.name),
            };
            let mut filter = Biquad::new(
                kind,
                number(&vector.config, "frequency"),
                number(&vector.config, "Q"),
                sample_rate,
            );
            let mut out = input.to_vec();
            filter.process_block(&mut out);
            Some(out)
        }
        "panner" => {
            // The settings the client builds every peer with; only the position varies
            // between vectors.
            let panner = Panner::default();
            let source = Position {
                x: f64::from(number(&vector.config, "x")),
                y: f64::from(number(&vector.config, "y")),
                z: f64::from(number(&vector.config, "z")),
            };
            Some(panner.process_block(input, source))
        }
        _ => None,
    }
}

#[test]
fn every_dsp_node_matches_chromium() {
    let manifest_path = golden_directory().join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}. Generate the vectors with `npm run golden`.",
            manifest_path.display()
        )
    });
    let manifest: Manifest = serde_json::from_str(&text).expect("the manifest parses");

    #[allow(
        clippy::cast_precision_loss,
        reason = "48000 is exact in an f32 and every vector uses it"
    )]
    let sample_rate = manifest.sample_rate as f32;

    let mut checked = 0usize;
    let mut unchecked: BTreeSet<String> = BTreeSet::new();
    let mut failures = Vec::new();

    for vector in &manifest.vectors {
        // The inputs are vectors too, so the Rust side does not have to reproduce them.
        // Nothing runs over them.
        let Some(from) = vector.from.as_ref() else {
            continue;
        };

        let input = read(from);
        let expected = read(&vector.name);
        assert_eq!(
            input.channels, 1,
            "{}: inputs are mono by construction",
            vector.name
        );

        let Some(produced) = run(vector, &input.samples, sample_rate) else {
            unchecked.insert(vector.node.clone());
            continue;
        };

        // A mono node against a stereo vector would compare the wrong samples, so the
        // shape is checked before the content.
        assert_eq!(
            vector.channels, expected.channels,
            "{}: the manifest and the file disagree about channels",
            vector.name
        );

        match rms_error_dbfs(&expected.samples, &produced) {
            None => failures.push(format!(
                "  {}: {} samples produced against {} expected",
                vector.name,
                produced.len(),
                expected.samples.len()
            )),
            Some(error) if error > TOLERANCE_DBFS => failures.push(format!(
                "  {}: {error:.1} dBFS RMS error, tolerance {TOLERANCE_DBFS:.0}",
                vector.name
            )),
            Some(_) => checked += 1,
        }
    }

    assert!(
        failures.is_empty(),
        "gate G2: {} of {} node vectors differ from Chromium.\n{}",
        failures.len(),
        checked + failures.len(),
        failures.join("\n")
    );

    // The list is asserted against rather than merely reported: a node that quietly
    // stopped being measured would otherwise look like a node that passes.
    let expected_unchecked: BTreeSet<String> = UNIMPLEMENTED
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    assert_eq!(
        unchecked, expected_unchecked,
        "the set of unimplemented nodes has changed; update UNIMPLEMENTED"
    );

    eprintln!(
        "gate G2 (DSP): {checked} vectors within {TOLERANCE_DBFS:.0} dBFS. Not yet implemented: {}",
        UNIMPLEMENTED.join(", ")
    );
}

#[test]
fn the_measurement_notices_a_difference() {
    // A gate that cannot fail is not a gate. A whole signal off by one part in a
    // thousand is -60 dBFS, comfortably above the -80 the criterion allows.
    let reference = vec![0.5f32; 1000];
    let measured: Vec<f32> = reference.iter().map(|sample| sample + 0.001).collect();
    let error = rms_error_dbfs(&reference, &measured).expect("same length");
    assert!(error > TOLERANCE_DBFS, "should be caught, got {error:.1}");
    assert!(
        (error - -60.0).abs() < 0.1,
        "expected -60 dBFS, got {error:.1}"
    );

    // And one sample out of a thousand off by the same amount is -90 dBFS, which the
    // criterion allows. That is the point of an RMS measure rather than a peak one: it
    // asks how much of the signal is wrong, not whether any of it is.
    let mut one_off = reference.clone();
    one_off[0] = 0.5 + 0.001;
    let small = rms_error_dbfs(&reference, &one_off).expect("same length");
    assert!(small < TOLERANCE_DBFS, "should pass, got {small:.1}");

    // And an exact match is not merely small, it is nothing.
    assert_eq!(
        rms_error_dbfs(&reference, &reference),
        Some(f64::NEG_INFINITY)
    );

    // Different lengths are a different failure and are told apart.
    assert_eq!(rms_error_dbfs(&reference, &measured[..999]), None);
}
