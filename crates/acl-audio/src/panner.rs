//! `PannerNode`: where a voice comes from, and how far away it is.
//!
//! Only the configuration the client builds every peer with — `equalpower` panning and a
//! `linear` distance model, with the listener at the origin facing −Z. The HRTF model and
//! the cone parameters are not implemented, because nothing sets them and a node that
//! guessed at them would have no golden vector to be wrong against.
//!
//! Two gains are applied and they are separate things. Distance decides how loud, panning
//! decides where — the specification applies distance and cone gain to the input, then
//! spreads the result across the two channels, and doing it the other way round changes
//! nothing for a mono source but would for anything else.

/// A point in the game's space, as the panner sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Position {
    /// Right of the listener.
    pub x: f64,
    /// Above the listener.
    pub y: f64,
    /// In front is negative, which is the specification's convention and the client's.
    pub z: f64,
}

impl Position {
    /// How far away it is.
    ///
    /// The same distance the model uses, exposed because a caller that wants the distance
    /// without the direction -- panning switched off, say -- should not be recomputing it
    /// with a different rounding.
    #[must_use]
    pub fn length(self) -> f64 {
        self.x.hypot(self.y).hypot(self.z)
    }
}

/// The distance model settings the client uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panner {
    /// Distance at which the gain is 1.
    pub reference_distance: f64,
    /// Distance past which the gain stops falling.
    pub max_distance: f64,
    /// How quickly it falls between the two.
    pub rolloff_factor: f64,
}

impl Default for Panner {
    /// What `Voice.tsx` sets on every peer's panner.
    ///
    /// `maxDistance` is rewritten per frame from the hearing range; the value here is the
    /// client's default lobby setting so the type has an honest default rather than a
    /// zero that would divide by nothing.
    fn default() -> Self {
        Self {
            reference_distance: 0.1,
            max_distance: 5.32,
            rolloff_factor: 1.0,
        }
    }
}

/// One frame's worth of stereo output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stereo {
    /// Left.
    pub left: f32,
    /// Right.
    pub right: f32,
}

impl Panner {
    /// The gain distance alone applies, for the `linear` model.
    ///
    /// ```text
    /// 1 - rolloffFactor * (clamp(d, ref, max) - ref) / (max - ref)
    /// ```
    ///
    /// The clamp is what stops a peer standing on top of the listener being louder than
    /// full scale, and a peer past `maxDistance` being negative — which would invert the
    /// signal rather than silence it.
    #[must_use]
    pub fn distance_gain(self, distance: f64) -> f64 {
        let span = self.max_distance - self.reference_distance;
        if span <= 0.0 {
            // Degenerate settings. The specification divides by this, and a panner whose
            // range is zero should be silent rather than infinite.
            return if distance <= self.reference_distance {
                1.0
            } else {
                0.0
            };
        }
        let clamped = distance.clamp(self.reference_distance, self.max_distance);
        self.rolloff_factor
            .mul_add(-((clamped - self.reference_distance) / span), 1.0)
    }

    /// Where the source sits, in degrees, with 0 straight ahead and positive to the right.
    ///
    /// The specification derives this from the listener's basis. With the listener at the
    /// origin facing −Z with +Y up — which is what the client leaves it at, and what the
    /// vectors were rendered with — the basis is the identity, and the whole derivation
    /// collapses to the angle in the horizontal plane.
    #[must_use]
    pub fn azimuth(source: Position) -> f64 {
        let length = source
            .z
            .mul_add(source.z, source.x.mul_add(source.x, source.y * source.y))
            .sqrt();
        if length == 0.0 {
            // A source at the listener's exact position has no direction. The
            // specification leaves this to the implementation; centred is the only answer
            // that does not favour a side.
            return 0.0;
        }

        // Projected onto the horizontal plane, which is what removes elevation from the
        // azimuth. A source directly overhead projects to nothing and is centred.
        let horizontal = source.x.hypot(source.z);
        if horizontal == 0.0 {
            return 0.0;
        }

        // atan2 against the forward axis: forward is −Z, right is +X.
        let degrees = source.x.atan2(-source.z).to_degrees();
        degrees.clamp(-180.0, 180.0)
    }

    /// The two channel gains for one azimuth, by the equal-power law.
    ///
    /// Azimuth is folded into the front half first: `equalpower` has no way to say
    /// "behind", so a source at 135° is placed where one at 45° would be. That is the
    /// specification's own fold, not an approximation of it.
    #[must_use]
    pub fn equal_power(azimuth: f64) -> (f64, f64) {
        let folded = if azimuth < -90.0 {
            -180.0 - azimuth
        } else if azimuth > 90.0 {
            180.0 - azimuth
        } else {
            azimuth
        };

        // 0 at hard left, 1 at hard right.
        let position = (folded + 90.0) / 180.0;
        let angle = position * std::f64::consts::FRAC_PI_2;
        (angle.cos(), angle.sin())
    }

    /// What a mono source at `source` is multiplied by, per channel.
    ///
    /// Both gains in one number each: distance decides how loud and panning decides where,
    /// and for a mono source the two collapse into a scalar per side. Exposed because a
    /// caller that has already turned mono into stereo -- the reverb does, and its two sides
    /// are not the same signal -- cannot go back through [`Self::process`] and still needs
    /// the same two numbers. Recomputing them at the call site is how they drift.
    #[must_use]
    pub fn gains(self, source: Position) -> (f64, f64) {
        let distance = source
            .z
            .mul_add(source.z, source.x.mul_add(source.x, source.y * source.y))
            .sqrt();
        let gain = self.distance_gain(distance);
        let (left, right) = Self::equal_power(Self::azimuth(source));
        (gain * left, gain * right)
    }

    /// Pans one mono sample to stereo.
    #[must_use]
    pub fn process(self, input: f32, source: Position) -> Stereo {
        let (left, right) = self.gains(source);
        let input = f64::from(input);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "narrowing back to the sample format"
        )]
        Stereo {
            left: (input * left) as f32,
            right: (input * right) as f32,
        }
    }

    /// Pans a mono block into an interleaved stereo one.
    #[must_use]
    pub fn process_block(self, input: &[f32], source: Position) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len() * 2);
        for sample in input {
            let stereo = self.process(*sample, source);
            out.push(stereo.left);
            out.push(stereo.right);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use super::*;

    fn at(x: f64, y: f64, z: f64) -> Position {
        Position { x, y, z }
    }

    #[test]
    fn straight_ahead_is_centred() {
        assert!(Panner::azimuth(at(0.0, 0.0, -1.0)).abs() < 1e-9);
        let (left, right) = Panner::equal_power(0.0);
        assert!(
            (left - right).abs() < 1e-12,
            "centred is equal on both sides"
        );
        // Equal power, not equal amplitude: each side carries 1/sqrt(2), so the two
        // together carry the same power as the mono source did.
        assert!((left - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    }

    #[test]
    fn right_is_right_and_left_is_left() {
        assert!((Panner::azimuth(at(1.0, 0.0, 0.0)) - 90.0).abs() < 1e-9);
        assert!((Panner::azimuth(at(-1.0, 0.0, 0.0)) + 90.0).abs() < 1e-9);

        let (left, right) = Panner::equal_power(90.0);
        assert!(left.abs() < 1e-12 && (right - 1.0).abs() < 1e-12);
        let (left, right) = Panner::equal_power(-90.0);
        assert!((left - 1.0).abs() < 1e-12 && right.abs() < 1e-12);
    }

    #[test]
    fn behind_is_folded_into_the_front() {
        // `equalpower` cannot say "behind", so it places a source at 135° where one at
        // 45° would be. Reproduced rather than approximated: it is the specification's
        // own fold, and it is why a player walking past you does not sweep through the
        // back of the stereo field.
        let front = Panner::equal_power(45.0);
        let behind = Panner::equal_power(135.0);
        assert!((front.0 - behind.0).abs() < 1e-12);
        assert!((front.1 - behind.1).abs() < 1e-12);
    }

    #[test]
    fn distance_falls_off_linearly_between_the_two_bounds() {
        let panner = Panner::default();
        // At the reference distance, full gain.
        assert!((panner.distance_gain(0.1) - 1.0).abs() < 1e-12);
        // Halfway along the span, half the way down.
        let midpoint = f64::midpoint(0.1, 5.32);
        assert!((panner.distance_gain(midpoint) - 0.5).abs() < 1e-12);
        // At the far bound, nothing.
        assert!(panner.distance_gain(5.32).abs() < 1e-12);
    }

    #[test]
    fn the_clamp_stops_a_close_peer_being_louder_than_full_scale() {
        // Without it, a peer standing on top of the listener would exceed unity and a
        // peer past the range would go negative — which inverts the signal rather than
        // silencing it, and sounds like a phase problem rather than a distance one.
        let panner = Panner::default();
        assert!((panner.distance_gain(0.0) - 1.0).abs() < 1e-12);
        assert!(panner.distance_gain(1000.0).abs() < 1e-12);
        assert!(panner.distance_gain(1000.0) >= 0.0);
    }

    #[test]
    fn a_degenerate_range_is_silent_rather_than_infinite() {
        // `maxDistance` is rewritten from the hearing range every frame, and the hearing
        // range can collapse. Dividing by the span would be a NaN in the graph, and a NaN
        // is silence that never comes back.
        let broken = Panner {
            reference_distance: 1.0,
            max_distance: 1.0,
            rolloff_factor: 1.0,
        };
        assert_eq!(broken.distance_gain(0.5), 1.0);
        assert_eq!(broken.distance_gain(2.0), 0.0);
        assert!(broken.distance_gain(2.0).is_finite());
    }

    #[test]
    fn a_source_at_the_listener_is_centred_rather_than_undefined() {
        // atan2(0, 0) is defined but the direction is not, and the specification leaves
        // it to the implementation. Centred is the only answer that does not favour a
        // side, and the client does put a peer exactly here.
        assert_eq!(Panner::azimuth(at(0.0, 0.0, 0.0)), 0.0);
        // Directly overhead has no horizontal direction either.
        assert_eq!(Panner::azimuth(at(0.0, 5.0, 0.0)), 0.0);
    }

    #[test]
    fn panning_and_distance_are_applied_together() {
        let panner = Panner::default();
        let out = panner.process(1.0, at(1.0, 0.0, 0.0));
        let expected = panner.distance_gain(1.0);
        #[allow(clippy::cast_possible_truncation)]
        let expected = expected as f32;
        assert!((out.right - expected).abs() < 1e-6, "{out:?}");
        assert!(out.left.abs() < 1e-6);
    }
}
