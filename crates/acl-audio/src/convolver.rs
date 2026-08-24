//! `ConvolverNode`: the reverb an impostor hears a haunting ghost through.
//!
//! The node the plan singles out as the one that is not a formula. Four of the five DSP
//! nodes are an equation you can check by reading; this one is a convolution whose failure
//! modes are quiet — a tail slightly late or slightly smeared produces no crash, no test
//! failure and no bug report anyone can articulate. Which is why it is measured against
//! Chromium's own output rather than reasoned about.
//!
//! Two things decide whether it agrees, and neither is the convolution:
//!
//! - **The normalisation scale.** With `normalize` left at its default — which is what the
//!   client leaves it at — the specification scales the response by an RMS-derived factor
//!   with two calibration constants in it. Skip it and the reverb is right in shape and
//!   wrong by tens of decibels.
//! - **The channel rule.** A mono source through a two-channel response is stereo, one
//!   channel convolved with each. Summing them to mono, or convolving only the first,
//!   both produce something that sounds like reverb.

use crate::fft::convolve;

/// The calibration constants from the specification's `calculateNormalizationScale`.
///
/// They are not derived from anything in the signal; they are the values Chromium picked
/// so a normalised response is at a comfortable level, and they are part of the contract
/// rather than a tuning choice this port gets to make.
const GAIN_CALIBRATION: f64 = 0.001_25;

/// The rate the calibration above was chosen at. A response at another rate is scaled by
/// the ratio, which is the specification's own compensation.
const GAIN_CALIBRATION_SAMPLE_RATE: f64 = 44100.0;

/// The floor on the measured power, so a near-silent response does not scale to infinity.
const MIN_POWER: f64 = 0.000_125;

/// An impulse response, ready to convolve with.
#[derive(Debug, Clone, PartialEq)]
pub struct Convolver {
    /// One response per channel, already scaled by the normalisation factor.
    channels: Vec<Vec<f64>>,
}

impl Convolver {
    /// Builds a convolver from an interleaved impulse response.
    ///
    /// `normalize` mirrors the node's attribute. The client never sets it, so it is left
    /// at its default of true, and the scale below is what that default means.
    #[must_use]
    pub fn new(interleaved: &[f32], channels: usize, sample_rate: f64, normalize: bool) -> Self {
        if channels == 0 {
            return Self {
                channels: Vec::new(),
            };
        }
        let scale = if normalize {
            normalization_scale(interleaved, channels, sample_rate)
        } else {
            1.0
        };

        let split = (0..channels)
            .map(|channel| {
                interleaved
                    .iter()
                    .skip(channel)
                    .step_by(channels)
                    .map(|sample| f64::from(*sample) * scale)
                    .collect()
            })
            .collect();

        Self { channels: split }
    }

    /// How many channels the response has.
    #[must_use]
    pub fn channels(&self) -> usize {
        self.channels.len()
    }

    /// Convolves a mono input, returning interleaved stereo of the same length.
    ///
    /// The output is truncated to the input's length, which is what an
    /// `OfflineAudioContext` of that length produces: the tail past the end of the render
    /// is simply never asked for. A convolver that returned the full tail would disagree
    /// with every golden vector by being longer than it.
    #[must_use]
    pub fn process_mono_to_stereo(&self, input: &[f32]) -> Vec<f32> {
        let wide: Vec<f64> = input.iter().map(|sample| f64::from(*sample)).collect();

        // A mono source through a two-channel response is stereo, one channel convolved
        // with each. A one-channel response feeds both sides.
        let left = self.channels.first().map(|ir| convolve(&wide, ir));
        let right = self
            .channels
            .get(1)
            .map(|ir| convolve(&wide, ir))
            .or_else(|| left.clone());

        let mut out = Vec::with_capacity(input.len() * 2);
        for frame in 0..input.len() {
            let l = left
                .as_ref()
                .and_then(|side| side.get(frame))
                .unwrap_or(&0.0);
            let r = right
                .as_ref()
                .and_then(|side| side.get(frame))
                .unwrap_or(&0.0);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "narrowing back to the sample format"
            )]
            {
                out.push(*l as f32);
                out.push(*r as f32);
            }
        }
        out
    }
}

/// The specification's `calculateNormalizationScale`, transcribed.
///
/// Power is measured across every channel and every sample together, not per channel, and
/// the floor is applied to the power rather than to the scale — an order that matters for
/// a response that is nearly silent, which is the case the floor exists for.
#[must_use]
pub fn normalization_scale(interleaved: &[f32], channels: usize, sample_rate: f64) -> f64 {
    if channels == 0 || interleaved.is_empty() {
        return 1.0;
    }
    let power: f64 = interleaved
        .iter()
        .map(|sample| {
            let value = f64::from(*sample);
            value * value
        })
        .sum();

    #[allow(
        clippy::cast_precision_loss,
        reason = "an impulse response is a few hundred thousand samples"
    )]
    let mut rms = (power / interleaved.len() as f64).sqrt();
    if !rms.is_finite() || rms < MIN_POWER {
        rms = MIN_POWER;
    }

    let mut scale = GAIN_CALIBRATION / rms;
    if sample_rate > 0.0 {
        scale *= GAIN_CALIBRATION_SAMPLE_RATE / sample_rate;
    }
    // True-stereo compensation: a four-channel response is two stereo responses.
    if channels == 4 {
        scale *= 0.5;
    }
    scale
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::float_cmp
    )]

    use super::*;

    #[test]
    fn an_impulse_response_of_one_passes_the_signal_through() {
        // Without normalisation, so the arithmetic is visible: a single-tap response is
        // the identity.
        let convolver = Convolver::new(&[1.0, 1.0], 2, 48000.0, false);
        let out = convolver.process_mono_to_stereo(&[0.5, -0.25, 0.75]);
        assert_eq!(out, vec![0.5, 0.5, -0.25, -0.25, 0.75, 0.75]);
    }

    #[test]
    fn each_side_gets_its_own_response() {
        // The channel rule. Summing to mono, or convolving only the first channel, both
        // produce something that sounds like reverb and is not this.
        let convolver = Convolver::new(&[1.0, 0.5], 2, 48000.0, false);
        let out = convolver.process_mono_to_stereo(&[1.0]);
        assert_eq!(out, vec![1.0, 0.5]);
    }

    #[test]
    fn a_mono_response_feeds_both_sides() {
        let convolver = Convolver::new(&[1.0], 1, 48000.0, false);
        let out = convolver.process_mono_to_stereo(&[0.25]);
        assert_eq!(out, vec![0.25, 0.25]);
    }

    #[test]
    fn the_output_is_as_long_as_the_input() {
        // An `OfflineAudioContext` of N frames renders N frames; the tail past the end is
        // never asked for. Returning the full convolution would be longer than every
        // golden vector.
        let convolver = Convolver::new(&[1.0, 1.0, 1.0, 1.0], 2, 48000.0, false);
        let out = convolver.process_mono_to_stereo(&[1.0, 0.0, 0.0]);
        assert_eq!(out.len(), 6);
    }

    #[test]
    fn the_normalisation_scale_follows_the_specification() {
        // A response whose samples are all 0.5 has an RMS of 0.5, so the scale is the
        // calibration constant divided by that, times the rate ratio.
        let response = vec![0.5f32; 100];
        let scale = normalization_scale(&response, 1, 44100.0);
        assert!((scale - GAIN_CALIBRATION / 0.5).abs() < 1e-12);

        // At twice the calibration rate it is halved.
        let doubled = normalization_scale(&response, 1, 88200.0);
        assert!((doubled - scale / 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_silent_response_does_not_scale_to_infinity() {
        // The floor is on the power, not on the scale. Without it a response of digital
        // silence divides by zero, and every peer routed through it goes to NaN.
        let scale = normalization_scale(&[0.0f32; 100], 1, 44100.0);
        assert!(scale.is_finite());
        assert!((scale - GAIN_CALIBRATION / MIN_POWER).abs() < 1e-9);
    }

    #[test]
    fn a_four_channel_response_is_halved() {
        let response = vec![0.5f32; 100];
        let stereo = normalization_scale(&response, 2, 44100.0);
        let true_stereo = normalization_scale(&response, 4, 44100.0);
        assert!((true_stereo - stereo / 2.0).abs() < 1e-12);
    }

    #[test]
    fn an_empty_response_is_not_a_division_by_nothing() {
        assert_eq!(normalization_scale(&[], 2, 48000.0), 1.0);
        let convolver = Convolver::new(&[], 0, 48000.0, true);
        assert_eq!(convolver.channels(), 0);
        assert_eq!(convolver.process_mono_to_stereo(&[1.0]), vec![0.0, 0.0]);
    }
}
