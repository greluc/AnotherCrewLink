//! `AnalyserNode`: the frequency picture the voice-activity detector reads.
//!
//! The fifth node, and the odd one: its output is not audio. It passes its input through
//! untouched and, on demand, hands back a picture of the last `fftSize` samples. The
//! client's detector asks for that picture sixty times a second and decides from a narrow
//! band of it whether somebody is speaking.
//!
//! Three details decide whether this agrees with Chromium, and none of them is the FFT:
//!
//! - **A Blackman window**, applied before the transform. Without one, a tone between two
//!   bins smears across the whole spectrum, and a detector reading a narrow band sees
//!   speech in a door closing.
//! - **Smoothing over time**, and in the right direction: the new magnitude is blended
//!   *into* the previous picture by `1 - smoothingTimeConstant`, so a larger constant
//!   means a slower picture. Getting the sense backwards produces a detector that is
//!   either deaf or permanently triggered, and both look like a threshold problem.
//! - **The decibel window**. `getByteFrequencyData` maps −100..−30 dBFS onto 0..255 and
//!   clamps outside it, so everything quieter than −100 dB is zero rather than negative.

use crate::fft::{Complex, transform};

/// The default decibel floor, which maps to byte 0.
pub const DEFAULT_MIN_DECIBELS: f64 = -100.0;

/// The default decibel ceiling, which maps to byte 255.
pub const DEFAULT_MAX_DECIBELS: f64 = -30.0;

/// The Blackman window's coefficients, as the specification gives them.
const BLACKMAN: (f64, f64, f64) = (0.42, 0.5, 0.08);

/// An analyser, holding the smoothed picture between calls.
#[derive(Debug, Clone, PartialEq)]
pub struct Analyser {
    fft_size: usize,
    smoothing: f64,
    min_decibels: f64,
    max_decibels: f64,
    /// The smoothed magnitude per bin, carried from call to call.
    smoothed: Vec<f64>,
}

impl Analyser {
    /// An analyser with the client's decibel window.
    ///
    /// `fft_size` must be a power of two; the specification requires it and the transform
    /// insists on it. `smoothing` is the specification's `smoothingTimeConstant`.
    #[must_use]
    pub fn new(fft_size: usize, smoothing: f64) -> Self {
        let size = fft_size.next_power_of_two().max(2);
        Self {
            fft_size: size,
            smoothing: smoothing.clamp(0.0, 1.0),
            min_decibels: DEFAULT_MIN_DECIBELS,
            max_decibels: DEFAULT_MAX_DECIBELS,
            smoothed: vec![0.0; size / 2],
        }
    }

    /// How many bins the picture has: half the transform length.
    #[must_use]
    pub fn frequency_bin_count(&self) -> usize {
        self.fft_size / 2
    }

    /// Takes in one block of samples, updating the smoothed picture.
    ///
    /// The specification analyses the most recent `fftSize` samples, so a block shorter
    /// than that is padded with the silence that precedes it — which is what an analyser
    /// asked for a picture before it has heard `fftSize` samples returns.
    pub fn push(&mut self, samples: &[f32]) {
        let size = self.fft_size;
        let mut buffer = vec![Complex::default(); size];

        // The most recent `fftSize` samples, right-aligned: earlier ones are silence.
        let take = samples.len().min(size);
        let offset = size - take;
        let recent = samples.get(samples.len() - take..).unwrap_or(samples);
        for (index, sample) in recent.iter().enumerate() {
            let window = blackman(index + offset, size);
            if let Some(slot) = buffer.get_mut(index + offset) {
                *slot = Complex::real(f64::from(*sample) * window);
            }
        }

        transform(&mut buffer, false);

        #[allow(
            clippy::cast_precision_loss,
            reason = "an FFT size is a small power of two"
        )]
        let normalise = 1.0 / size as f64;
        for bin in 0..self.frequency_bin_count() {
            let Some(value) = buffer.get(bin) else {
                break;
            };
            let magnitude = value.re.hypot(value.im) * normalise;
            if let Some(slot) = self.smoothed.get_mut(bin) {
                // Blended *into* the previous picture: a larger constant is a slower one.
                *slot = self
                    .smoothing
                    .mul_add(*slot, (1.0 - self.smoothing) * magnitude);
            }
        }
    }

    /// The picture, as bytes, the way `getByteFrequencyData` returns it.
    #[must_use]
    pub fn byte_frequency_data(&self) -> Vec<u8> {
        let span = self.max_decibels - self.min_decibels;
        self.smoothed
            .iter()
            .map(|magnitude| {
                if *magnitude <= 0.0 || span <= 0.0 {
                    // log10 of zero is negative infinity, which is below the floor
                    // anyway. Said here rather than left to the clamp, because an
                    // infinity through the arithmetic first is a NaN.
                    return 0;
                }
                let decibels = 20.0 * magnitude.log10();
                let scaled = 255.0 * (decibels - self.min_decibels) / span;
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "clamped to 0..255 first"
                )]
                {
                    scaled.clamp(0.0, 255.0) as u8
                }
            })
            .collect()
    }
}

/// The Blackman window at one position.
fn blackman(index: usize, size: usize) -> f64 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "an FFT size is a small power of two"
    )]
    let position = index as f64 / size as f64;
    let (a0, a1, a2) = BLACKMAN;
    let two_pi = std::f64::consts::TAU;
    a2.mul_add(
        (two_pi * 2.0 * position).cos(),
        a0 - a1 * (two_pi * position).cos(),
    )
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::cast_precision_loss
    )]

    use super::*;

    fn sine(frequency: f64, length: usize, rate: f64) -> Vec<f32> {
        (0..length)
            .map(|index| {
                #[allow(clippy::cast_possible_truncation)]
                {
                    ((std::f64::consts::TAU * frequency * index as f64 / rate).sin() * 0.9) as f32
                }
            })
            .collect()
    }

    #[test]
    fn a_tone_lands_in_its_own_bin() {
        // 48000 / 1024 is 46.875 Hz per bin, so 3000 Hz is bin 64 exactly.
        let mut analyser = Analyser::new(1024, 0.0);
        analyser.push(&sine(3000.0, 1024, 48000.0));
        let bins = analyser.byte_frequency_data();
        // Its own bin is the loudest, and its neighbours fall away either side. Asserted
        // that way rather than by taking the maximum: a Blackman window spreads a tone
        // over three bins, and at full scale the top two quantise to the same byte, so
        // "the index of the largest" is decided by which way the tie is broken.
        assert_eq!(bins[64], 255, "the tone should be at the top of the range");
        assert!(bins[63] < bins[64] || bins[63] == 255);
        assert!(bins[65] < bins[64] || bins[65] == 255);
        assert!(bins[60] < bins[64] / 2, "and away from it, much less");
        assert!(bins[68] < bins[64] / 2);
    }

    #[test]
    fn the_window_stops_a_tone_smearing_across_the_spectrum() {
        // A frequency between two bins. Without a window it leaks into every bin; with
        // one the leakage falls away, and a detector reading a narrow band is the thing
        // that would otherwise hear speech in a door closing.
        let mut analyser = Analyser::new(1024, 0.0);
        analyser.push(&sine(3023.0, 1024, 48000.0));
        let bins = analyser.byte_frequency_data();
        let peak = bins.iter().copied().max().unwrap();
        // Far from the tone, twenty bins away, there should be very little left.
        let far = bins[100];
        assert!(peak > 200, "the tone should be loud, got {peak}");
        assert!(far < peak / 4, "leakage {far} against a peak of {peak}");
    }

    #[test]
    fn smoothing_makes_the_picture_slower_not_quieter() {
        // The direction that is easy to get backwards. A large constant keeps most of the
        // previous picture, so the first block barely moves it.
        let mut slow = Analyser::new(1024, 0.9);
        slow.push(&sine(3000.0, 1024, 48000.0));
        let mut fast = Analyser::new(1024, 0.0);
        fast.push(&sine(3000.0, 1024, 48000.0));
        assert!(
            slow.byte_frequency_data()[64] < fast.byte_frequency_data()[64],
            "a slower picture should not have arrived yet"
        );

        // And it does arrive, given enough blocks.
        for _ in 0..80 {
            slow.push(&sine(3000.0, 1024, 48000.0));
        }
        let settled = slow.byte_frequency_data()[64];
        let immediate = fast.byte_frequency_data()[64];
        assert!(
            settled.abs_diff(immediate) <= 2,
            "settled at {settled} against {immediate}"
        );
    }

    #[test]
    fn silence_is_the_bottom_of_the_range_rather_than_a_negative_number() {
        // log10 of zero is negative infinity, and an infinity through the scaling
        // arithmetic is a NaN rather than a small number.
        let mut analyser = Analyser::new(1024, 0.0);
        analyser.push(&vec![0.0f32; 1024]);
        assert!(analyser.byte_frequency_data().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_short_block_is_padded_rather_than_refused() {
        // An analyser asked for a picture before it has heard `fftSize` samples returns
        // one, and the client asks on its very first frame.
        let mut analyser = Analyser::new(1024, 0.0);
        analyser.push(&sine(3000.0, 64, 48000.0));
        assert_eq!(analyser.byte_frequency_data().len(), 512);
    }

    #[test]
    fn the_bin_count_is_half_the_transform() {
        assert_eq!(Analyser::new(1024, 0.2).frequency_bin_count(), 512);
        assert_eq!(Analyser::new(2048, 0.8).frequency_bin_count(), 1024);
        // A size that is not a power of two is rounded up rather than refused: the
        // transform insists, and refusing would be a panic in an audio callback.
        assert_eq!(Analyser::new(1000, 0.2).frequency_bin_count(), 512);
    }
}
