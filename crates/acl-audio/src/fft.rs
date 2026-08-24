//! A radix-2 FFT, for the convolver.
//!
//! Sixty lines against a dependency, and the trade is not close for this one: the
//! convolver needs a forward and an inverse transform over power-of-two lengths and
//! nothing else — no real-input specialisation, no planner, no SIMD dispatch. A crate
//! would bring all of that and a supply-chain entry with it.
//!
//! This is the correctness implementation. The real-time path in `P3` needs *uniformly
//! partitioned* convolution — a transform per block rather than one over the whole signal,
//! so the first sample is not waiting on the last — and that is a different structure
//! built on the same transform.

use std::f64::consts::PI;

/// One complex number.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl Complex {
    /// A real number.
    #[must_use]
    pub const fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }

    fn mul(self, other: Self) -> Self {
        Self {
            re: self.re.mul_add(other.re, -(self.im * other.im)),
            im: self.re.mul_add(other.im, self.im * other.re),
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }
}

/// The next power of two at or above `n`, which is the only length [`transform`] accepts.
#[must_use]
pub fn next_power_of_two(n: usize) -> usize {
    n.next_power_of_two().max(1)
}

/// An in-place radix-2 FFT. `inverse` runs the transform backwards and scales by `1/N`.
///
/// # Panics
///
/// Panics if the length is not a power of two. Callers pad to one; a length that is not
/// would silently produce a different transform, and finding that out from a wrong reverb
/// tail is worse than finding it out here.
pub fn transform(buffer: &mut [Complex], inverse: bool) {
    let n = buffer.len();
    assert!(
        n.is_power_of_two(),
        "the transform length must be a power of two, got {n}"
    );
    if n <= 1 {
        return;
    }

    // Bit-reversal permutation, iteratively: the standard Cooley-Tukey reordering.
    let mut target = 0usize;
    for source in 1..n {
        let mut bit = n >> 1;
        while target & bit != 0 {
            target ^= bit;
            bit >>= 1;
        }
        target |= bit;
        if source < target {
            buffer.swap(source, target);
        }
    }

    let sign = if inverse { 1.0 } else { -1.0 };
    let mut length = 2usize;
    while length <= n {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a transform length is far inside an f64's exact range"
        )]
        let angle = sign * 2.0 * PI / length as f64;
        let step = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        let mut start = 0usize;
        while start < n {
            let mut twiddle = Complex::real(1.0);
            for offset in 0..length / 2 {
                let Some(&even) = buffer.get(start + offset) else {
                    break;
                };
                let Some(&odd) = buffer.get(start + offset + length / 2) else {
                    break;
                };
                let rotated = odd.mul(twiddle);
                if let Some(slot) = buffer.get_mut(start + offset) {
                    *slot = even.add(rotated);
                }
                if let Some(slot) = buffer.get_mut(start + offset + length / 2) {
                    *slot = even.sub(rotated);
                }
                twiddle = twiddle.mul(step);
            }
            start += length;
        }
        length <<= 1;
    }

    if inverse {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a transform length is far inside an f64's exact range"
        )]
        let scale = 1.0 / n as f64;
        for value in buffer.iter_mut() {
            value.re *= scale;
            value.im *= scale;
        }
    }
}

/// Convolves two real signals, returning `a.len() + b.len() - 1` samples.
///
/// Both are zero-padded to a common power of two, transformed, multiplied and transformed
/// back. Exact rather than partitioned: this is what the golden vectors are measured
/// against, and a partitioned implementation is measured against this in turn.
#[must_use]
pub fn convolve(a: &[f64], b: &[f64]) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let full = a.len() + b.len() - 1;
    let n = next_power_of_two(full);

    let mut left: Vec<Complex> = a.iter().map(|value| Complex::real(*value)).collect();
    left.resize(n, Complex::default());
    let mut right: Vec<Complex> = b.iter().map(|value| Complex::real(*value)).collect();
    right.resize(n, Complex::default());

    transform(&mut left, false);
    transform(&mut right, false);
    for (one, other) in left.iter_mut().zip(&right) {
        *one = one.mul(*other);
    }
    transform(&mut left, true);

    left.into_iter().take(full).map(|value| value.re).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_transform_and_its_inverse_are_the_identity() {
        let original: Vec<Complex> = (0..16)
            .map(|index| Complex {
                re: f64::from(index) * 0.25,
                im: f64::from(index % 3) - 1.0,
            })
            .collect();
        let mut buffer = original.clone();
        transform(&mut buffer, false);
        transform(&mut buffer, true);
        for (before, after) in original.iter().zip(&buffer) {
            assert!((before.re - after.re).abs() < 1e-12);
            assert!((before.im - after.im).abs() < 1e-12);
        }
    }

    #[test]
    fn a_constant_transforms_to_a_single_bin() {
        // The one case that can be checked by hand: a constant is entirely at DC.
        let mut buffer = vec![Complex::real(1.0); 8];
        transform(&mut buffer, false);
        assert!((buffer[0].re - 8.0).abs() < 1e-12);
        for value in &buffer[1..] {
            assert!(value.re.abs() < 1e-12 && value.im.abs() < 1e-12);
        }
    }

    #[test]
    fn convolving_with_an_impulse_returns_the_signal() {
        let signal = [1.0, -0.5, 0.25, 2.0];
        let impulse = [1.0];
        let out = convolve(&signal, &impulse);
        assert_eq!(out.len(), 4);
        for (expected, got) in signal.iter().zip(&out) {
            assert!((expected - got).abs() < 1e-12);
        }
    }

    #[test]
    fn a_delayed_impulse_delays_the_signal() {
        let out = convolve(&[1.0, 2.0, 3.0], &[0.0, 0.0, 1.0]);
        assert_eq!(out.len(), 5);
        assert!(out[0].abs() < 1e-12);
        assert!(out[1].abs() < 1e-12);
        assert!((out[2] - 1.0).abs() < 1e-12);
        assert!((out[4] - 3.0).abs() < 1e-12);
    }

    #[test]
    fn it_agrees_with_the_direct_sum() {
        // The definition, computed the slow way, on a length small enough to be sure of.
        let a: Vec<f64> = (0..37).map(|i| f64::from(i).sin()).collect();
        let b: Vec<f64> = (0..23).map(|i| f64::from(i).cos() * 0.1).collect();
        let fast = convolve(&a, &b);

        let mut slow = vec![0.0; a.len() + b.len() - 1];
        for (i, one) in a.iter().enumerate() {
            for (j, other) in b.iter().enumerate() {
                slow[i + j] += one * other;
            }
        }

        assert_eq!(fast.len(), slow.len());
        for (expected, got) in slow.iter().zip(&fast) {
            assert!(
                (expected - got).abs() < 1e-10,
                "expected {expected}, got {got}"
            );
        }
    }

    #[test]
    fn an_empty_input_convolves_to_nothing() {
        assert!(convolve(&[], &[1.0]).is_empty());
        assert!(convolve(&[1.0], &[]).is_empty());
    }
}
