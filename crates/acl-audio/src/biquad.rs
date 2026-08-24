//! The muffle filter: `BiquadFilterNode`, for the two shapes the client uses.
//!
//! The coefficient formulas are the Web Audio specification's, and the detail that decides
//! whether this agrees with Chromium is what `Q` means. For `lowpass` and `highpass` the
//! specification reads it **in decibels** —
//!
//! ```text
//! alpha = sin(w0) / (2 * 10^(Q/20))
//! ```
//!
//! — where the RBJ cookbook the formulas otherwise come from reads it linearly. Getting
//! that wrong produces a filter that is plausible, stable and wrong, and it is why the
//! client can set `Q = -15` at all: as a linear Q that is meaningless, and as −15 dB it is
//! a gentle rolloff.

/// Which shape a filter takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    /// Passes below the corner frequency. What a muffle node is created as.
    LowPass,
    /// Passes above it. What the impostor radio switches to.
    HighPass,
}

/// A direct-form-1 biquad, which is the form the specification writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    /// A filter of the given shape.
    ///
    /// `frequency` is in hertz and `q` in decibels, as the specification reads it for
    /// these two shapes.
    #[must_use]
    pub fn new(kind: FilterKind, frequency: f32, q: f32, sample_rate: f32) -> Self {
        let nyquist = f64::from(sample_rate) / 2.0;
        // The specification normalises the frequency against the Nyquist rate and clamps
        // it there. Past it a filter is not merely wrong, its coefficients are not finite.
        let normalised = (f64::from(frequency) / nyquist).clamp(0.0, 1.0);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            LowPass if normalised >= 1.0 => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
            LowPass if normalised <= 0.0 => (0.0, 0.0, 0.0, 1.0, 0.0, 0.0),
            HighPass if normalised >= 1.0 => (0.0, 0.0, 0.0, 1.0, 0.0, 0.0),
            HighPass if normalised <= 0.0 => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
            LowPass => {
                let w0 = std::f64::consts::PI * normalised;
                let alpha = w0.sin() / 2.0 * 10f64.powf(-f64::from(q) / 20.0);
                let cos = w0.cos();
                (
                    f64::midpoint(1.0, -cos),
                    1.0 - cos,
                    f64::midpoint(1.0, -cos),
                    1.0 + alpha,
                    -2.0 * cos,
                    1.0 - alpha,
                )
            }
            HighPass => {
                let w0 = std::f64::consts::PI * normalised;
                let alpha = w0.sin() / 2.0 * 10f64.powf(-f64::from(q) / 20.0);
                let cos = w0.cos();
                (
                    f64::midpoint(1.0, cos),
                    -(1.0 + cos),
                    f64::midpoint(1.0, cos),
                    1.0 + alpha,
                    -2.0 * cos,
                    1.0 - alpha,
                )
            }
        };

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Filters one sample, advancing the state.
    #[must_use]
    pub fn process(&mut self, input: f32) -> f32 {
        let x0 = f64::from(input);
        let y0 = self.b2.mul_add(
            self.x2,
            self.b1
                .mul_add(self.x1, self.b0 * x0)
                .mul_add(1.0, -(self.a1 * self.y1 + self.a2 * self.y2)),
        );
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;
        // The graph carries f32; the state is kept in f64 so a resonant filter does not
        // accumulate its own rounding, which is what the specification's own wording
        // implies and what Chromium does.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "narrowing back to the sample format is the point"
        )]
        {
            y0 as f32
        }
    }

    /// Filters a block in place.
    pub fn process_block(&mut self, samples: &mut [f32]) {
        for sample in samples {
            *sample = self.process(*sample);
        }
    }
}

use FilterKind::{HighPass, LowPass};

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::cast_precision_loss
    )]

    use super::*;

    /// The rate every golden vector is rendered at.
    const RATE: f32 = 48000.0;

    fn response(filter: &mut Biquad, frequency: f32, samples: usize) -> f32 {
        // Drive a sine through and measure what comes out, past the settling transient.
        let mut peak = 0.0f32;
        for index in 0..samples {
            let phase = 2.0 * std::f32::consts::PI * frequency * (index as f32) / RATE;
            let out = filter.process(phase.sin());
            if index > samples / 2 {
                peak = peak.max(out.abs());
            }
        }
        peak
    }

    #[test]
    fn a_low_pass_passes_below_and_stops_above() {
        let mut low = Biquad::new(LowPass, 2000.0, 20.0, RATE);
        let passed = response(&mut low, 200.0, 4800);
        let mut high = Biquad::new(LowPass, 2000.0, 20.0, RATE);
        let stopped = response(&mut high, 12000.0, 4800);
        assert!(passed > 0.5, "200 Hz should get through, got {passed}");
        assert!(stopped < 0.1, "12 kHz should not, got {stopped}");
    }

    #[test]
    fn a_high_pass_does_the_opposite() {
        let mut low = Biquad::new(HighPass, 1000.0, 10.0, RATE);
        let stopped = response(&mut low, 100.0, 4800);
        let mut high = Biquad::new(HighPass, 1000.0, 10.0, RATE);
        let passed = response(&mut high, 8000.0, 4800);
        assert!(stopped < 0.2, "100 Hz should be cut, got {stopped}");
        assert!(passed > 0.5, "8 kHz should get through, got {passed}");
    }

    #[test]
    fn q_is_read_in_decibels() {
        // The detail that decides whether this agrees with Chromium, and it is exact:
        // with `alpha = sin(w0)/2 * 10^(-Q/20)`, the gain at the corner frequency is
        // 10^(Q/20). A Q of 20 therefore peaks at ten, not at twenty — reading Q linearly
        // would put it at twenty, which is a filter that is plausible, stable and wrong.
        for q in [0.0f32, 6.0, 20.0] {
            let mut filter = Biquad::new(LowPass, 2000.0, q, RATE);
            let at_corner = response(&mut filter, 2000.0, 19200);
            let expected = 10f32.powf(q / 20.0);
            assert!(
                (at_corner - expected).abs() < expected * 0.01,
                "Q={q} dB should peak at {expected} at the corner, got {at_corner}"
            );
        }
    }

    #[test]
    fn a_negative_q_is_a_gentler_filter_not_an_unstable_one() {
        // The client sets Q = -15 for the camera muffle. As a linear Q that is
        // meaningless; as decibels it is a wide, gentle rolloff, and the filter stays
        // stable, which is what this checks.
        let mut filter = Biquad::new(LowPass, 2300.0, -15.0, RATE);
        let mut worst = 0.0f32;
        for index in 0..48000 {
            let phase = 2.0 * std::f32::consts::PI * 1000.0 * (index as f32) / RATE;
            worst = worst.max(filter.process(phase.sin()).abs());
        }
        assert!(worst.is_finite() && worst < 4.0, "runs away: {worst}");
    }

    #[test]
    fn a_frequency_past_the_nyquist_rate_is_clamped_rather_than_infinite() {
        // Not reachable from the client's settings, but a filter whose coefficients are
        // not finite turns every later sample into a NaN, and NaN in an audio graph is
        // silence that never comes back.
        let mut filter = Biquad::new(LowPass, 96000.0, 1.0, RATE);
        let out = filter.process(0.5);
        assert!(out.is_finite());
    }

    #[test]
    fn silence_in_is_silence_out() {
        let mut filter = Biquad::new(LowPass, 2000.0, 20.0, RATE);
        let mut block = vec![0.0f32; 128];
        filter.process_block(&mut block);
        assert!(block.iter().all(|sample| *sample == 0.0));
    }
}
