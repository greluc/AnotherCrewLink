//! `GainNode`: a multiplication, and the one node whose correctness is not in doubt.
//!
//! It is here anyway, for two reasons. It is what the voice decision's output is applied
//! through, so the graph needs it; and it is the vector that proves the golden-vector
//! harness itself is honest. If the measurement says a multiplication by 0.5 disagrees
//! with Chromium, the measurement is wrong, and it is better to find that out on the
//! simplest node in the set than on the convolver.

/// A constant gain.
///
/// The Web Audio node's gain is an `AudioParam` and can be automated. Nothing in this
/// client automates it — `gain.gain.value = x` is the only form used, written once per
/// frame — so this is a value rather than a schedule. Automation would need the parameter
/// machinery, and building that for a setter nobody calls would be inventing work.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gain {
    value: f32,
}

impl Gain {
    /// A gain of the given value.
    #[must_use]
    pub const fn new(value: f32) -> Self {
        Self { value }
    }

    /// What the gain is set to.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.value
    }

    /// Applies it to one sample.
    #[must_use]
    pub fn process(self, input: f32) -> f32 {
        input * self.value
    }

    /// Applies it to a block in place.
    pub fn process_block(self, samples: &mut [f32]) {
        for sample in samples {
            *sample *= self.value;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use super::*;

    #[test]
    fn silence_is_a_gain_of_zero() {
        // The value a muted peer is set to, and the one the node is created with.
        let mut block = [1.0f32, -1.0, 0.5];
        Gain::new(0.0).process_block(&mut block);
        assert_eq!(block, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn unity_leaves_the_signal_alone() {
        let mut block = [0.25f32, -0.75];
        Gain::new(1.0).process_block(&mut block);
        assert_eq!(block, [0.25, -0.75]);
    }

    #[test]
    fn the_vent_and_camera_gains_scale() {
        assert_eq!(Gain::new(0.5).process(1.0), 0.5);
        assert_eq!(Gain::new(0.8).process(-1.0), -0.8);
    }

    #[test]
    fn a_gain_above_one_is_not_clamped() {
        // Nothing in the voice decision produces one, but the node does not clamp and
        // neither does Chromium's: clipping is the output device's business, and a node
        // that clamped here would disagree with the golden vector if one ever appeared.
        assert_eq!(Gain::new(2.0).process(0.75), 1.5);
    }
}
