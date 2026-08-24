//! Getting a microphone to 48 kHz, and back out to whatever the speakers want.
//!
//! Most devices are already at 48 kHz and this does nothing at all. The ones that are not
//! — a headset that opens at 44.1, a virtual cable at 16 — need converting, and the codec
//! and the whole graph downstream assume one rate.
//!
//! # Why `process_into_buffer` and nothing else
//!
//! `rubato`'s `process` returns a freshly allocated `Vec` per call. In a capture callback
//! that is an allocation every ten milliseconds on a thread that must not block, and an
//! allocator that decides to ask the kernel for memory at the wrong moment is a click in
//! somebody's ear. `process_into_buffer` writes into buffers this type owns and reuses, so
//! the steady state allocates nothing. Gate G2's fourth criterion is that the render path
//! allocates nothing under the CI allocator, and this is one of the two places that would
//! otherwise fail it.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Resampler as _, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

/// The rate the graph, the codec and the wire all run at.
pub const TARGET_RATE: u32 = 48000;

/// What went wrong.
#[derive(Debug)]
pub enum ResampleError {
    /// `rubato` refused the configuration or the block.
    Rubato(String),
}

impl std::fmt::Display for ResampleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rubato(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for ResampleError {}

/// Converts one mono stream to [`TARGET_RATE`].
///
/// A rate that already matches costs nothing: the samples are copied straight through,
/// which is both the fast path and the common one.
pub struct Resampler {
    /// `None` when the input is already at the target rate.
    inner: Option<Inner>,
    input_rate: u32,
}

struct Inner {
    resampler: Async<f32>,
    /// The block `rubato` insists on being handed, filled a piece at a time.
    pending: Vec<f32>,
    /// Owned scratch, so `process_into_buffer` never allocates. Mono, so interleaved
    /// and sequential are the same thing and a plain slice serves as either.
    input: Vec<f32>,
    output: Vec<f32>,
    chunk: usize,
}

impl Resampler {
    /// A resampler from `input_rate` to [`TARGET_RATE`].
    ///
    /// `chunk` is how many input frames it converts at a time. It is fixed because
    /// `SincFixedIn` needs it fixed, and because a fixed block is what lets the buffers be
    /// allocated once.
    ///
    /// # Errors
    ///
    /// Returns [`ResampleError`] if `rubato` refuses the ratio.
    pub fn new(input_rate: u32, chunk: usize) -> Result<Self, ResampleError> {
        if input_rate == TARGET_RATE || input_rate == 0 {
            return Ok(Self {
                inner: None,
                input_rate,
            });
        }

        // The parameters the plan settles on: a sinc filter long enough that the
        // stopband is well below anything a voice codec will carry, and a Blackman-Harris
        // window, which trades a slightly wider transition for far less leakage.
        let parameters = SincInterpolationParameters {
            sinc_len: 256,
            // `None` asks rubato to derive the cutoff from the window and the length,
            // which is the value it recommends and one fewer number to get wrong.
            f_cutoff: None,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: WindowFunction::BlackmanHarris2,
        };
        let ratio = f64::from(TARGET_RATE) / f64::from(input_rate);
        // `FixedAsync::Input`: a fixed number of frames in, however many come out. That is
        // the shape a capture callback has — the device decides how much it hands over,
        // and the resampler decides how much that becomes.
        let resampler =
            Async::<f32>::new_sinc(ratio, 2.0, &parameters, chunk, 1, FixedAsync::Input)
                .map_err(|error| ResampleError::Rubato(error.to_string()))?;

        // Sized once, from what the resampler says it can produce, and reused for every
        // block afterwards.
        let input = vec![0.0f32; chunk];
        let output = vec![0.0f32; resampler.output_frames_max()];

        Ok(Self {
            inner: Some(Inner {
                resampler,
                pending: Vec::with_capacity(chunk),
                input,
                output,
                chunk,
            }),
            input_rate,
        })
    }

    /// The rate this converts from.
    #[must_use]
    pub const fn input_rate(&self) -> u32 {
        self.input_rate
    }

    /// Whether any conversion happens at all.
    #[must_use]
    pub const fn passthrough(&self) -> bool {
        self.inner.is_none()
    }

    /// Feeds samples in and appends whatever full blocks come out.
    ///
    /// Input that does not fill a block is held until it does, so a caller may hand over
    /// whatever the device gave it. Nothing is allocated in the steady state: `into` is
    /// the caller's and grows once, and the scratch buffers belong to this type.
    ///
    /// # Errors
    ///
    /// Returns [`ResampleError`] if `rubato` refuses a block.
    pub fn push(&mut self, samples: &[f32], into: &mut Vec<f32>) -> Result<(), ResampleError> {
        let Some(inner) = self.inner.as_mut() else {
            // Already at the right rate. The common case, and it costs a copy.
            into.extend_from_slice(samples);
            return Ok(());
        };

        let mut offset = 0;
        while offset < samples.len() {
            let wanted = inner.chunk - inner.pending.len();
            let take = wanted.min(samples.len() - offset);
            inner
                .pending
                .extend_from_slice(samples.get(offset..offset + take).unwrap_or_default());
            offset += take;

            if inner.pending.len() < inner.chunk {
                break;
            }

            inner.input.copy_from_slice(&inner.pending);
            inner.pending.clear();

            // The adapters are views over buffers this type already owns; constructing
            // one borrows, it does not allocate.
            let source = InterleavedSlice::new(inner.input.as_slice(), 1, inner.chunk)
                .map_err(|error| ResampleError::Rubato(error.to_string()))?;
            let frames = inner.output.len();
            let mut sink = InterleavedSlice::new_mut(inner.output.as_mut_slice(), 1, frames)
                .map_err(|error| ResampleError::Rubato(error.to_string()))?;

            let (_, written) = inner
                .resampler
                .process_into_buffer(&source, &mut sink, None)
                .map_err(|error| ResampleError::Rubato(error.to_string()))?;

            into.extend_from_slice(inner.output.get(..written).unwrap_or_default());
        }
        Ok(())
    }

    /// How many input frames are held back waiting for a full block.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.inner.as_ref().map_or(0, |inner| inner.pending.len())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation
    )]

    use super::*;

    fn tone(hertz: f64, frames: usize, rate: u32) -> Vec<f32> {
        (0..frames)
            .map(|index| {
                ((std::f64::consts::TAU * hertz * index as f64 / f64::from(rate)).sin() * 0.5)
                    as f32
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        (sum / samples.len() as f64).sqrt()
    }

    #[test]
    fn a_matching_rate_is_a_copy() {
        // The common case: most devices open at 48 kHz, and paying for a filter to
        // convert 48 to 48 would be a filter every microphone runs through for nothing.
        let mut resampler = Resampler::new(TARGET_RATE, 480).unwrap();
        assert!(resampler.passthrough());
        let input = tone(440.0, 1000, TARGET_RATE);
        let mut out = Vec::new();
        resampler.push(&input, &mut out).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn a_rate_of_zero_does_not_divide_by_it() {
        // A device that reports nothing. The ratio would be infinite and `rubato` would
        // refuse, which in a capture callback is a failure at the worst moment.
        let resampler = Resampler::new(0, 480).unwrap();
        assert!(resampler.passthrough());
    }

    #[test]
    fn forty_four_one_becomes_forty_eight() {
        // The rate a great many headsets open at.
        let mut resampler = Resampler::new(44100, 441).unwrap();
        assert!(!resampler.passthrough());
        let input = tone(440.0, 44100, 44100);
        let mut out = Vec::new();
        resampler.push(&input, &mut out).unwrap();

        // A second in becomes a second out, within the block the resampler is still
        // holding and its own filter delay.
        let expected = 48000;
        assert!(
            out.len().abs_diff(expected) < 1000,
            "expected about {expected} frames, got {}",
            out.len()
        );
    }

    #[test]
    fn the_tone_survives_the_conversion() {
        // Level rather than samples: a resampler is not supposed to reproduce the input,
        // it is supposed to reproduce the sound. The filter's own start-up is skipped.
        let mut resampler = Resampler::new(44100, 441).unwrap();
        let input = tone(440.0, 44100, 44100);
        let mut out = Vec::new();
        resampler.push(&input, &mut out).unwrap();
        let settled = &out[4800..out.len().saturating_sub(480)];
        let expected = rms(&tone(440.0, 1000, 44100));
        let got = rms(settled);
        assert!(
            (got - expected).abs() < expected * 0.05,
            "expected around {expected}, got {got}"
        );
    }

    #[test]
    fn input_is_held_until_a_block_is_full() {
        // A device hands over whatever it has, which is rarely the block size. Holding
        // the remainder is what lets the caller not care.
        let mut resampler = Resampler::new(44100, 441).unwrap();
        let mut out = Vec::new();
        resampler.push(&tone(440.0, 100, 44100), &mut out).unwrap();
        assert!(out.is_empty(), "not a full block yet");
        assert_eq!(resampler.pending(), 100);

        resampler.push(&tone(440.0, 341, 44100), &mut out).unwrap();
        assert!(!out.is_empty(), "now it is");
        assert_eq!(resampler.pending(), 0);
    }

    #[test]
    fn a_block_larger_than_the_chunk_is_split() {
        // The other direction: a device that hands over five blocks at once.
        let mut resampler = Resampler::new(44100, 441).unwrap();
        let mut out = Vec::new();
        resampler
            .push(&tone(440.0, 441 * 5, 44100), &mut out)
            .unwrap();
        assert_eq!(resampler.pending(), 0);
        assert!(out.len() > 441 * 5, "upsampling produces more than it took");
    }

    #[test]
    fn sixteen_kilohertz_upsamples_too() {
        // A virtual cable, or a headset in its telephony mode. Three times the rate.
        let mut resampler = Resampler::new(16000, 160).unwrap();
        let mut out = Vec::new();
        resampler
            .push(&tone(440.0, 16000, 16000), &mut out)
            .unwrap();
        assert!(
            out.len().abs_diff(48000) < 1000,
            "expected about 48000, got {}",
            out.len()
        );
    }

    #[test]
    fn the_steady_state_does_not_grow_its_own_buffers() {
        // The property `process_into_buffer` is chosen for. `rubato`'s `process` returns
        // a fresh Vec per call, which in a capture callback is an allocation every ten
        // milliseconds on a thread that must not block. Checked by capacity: after the
        // first block the scratch is the size it will stay.
        let mut resampler = Resampler::new(44100, 441).unwrap();
        let mut out = Vec::with_capacity(48000);
        let block = tone(440.0, 441, 44100);

        resampler.push(&block, &mut out).unwrap();
        let capacity = out.capacity();
        for _ in 0..50 {
            out.clear();
            resampler.push(&block, &mut out).unwrap();
        }
        assert_eq!(out.capacity(), capacity, "the output buffer grew");
    }
}
