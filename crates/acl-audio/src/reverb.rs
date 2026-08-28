//! The same reverb as [`crate::convolver`], twenty milliseconds at a time.
//!
//! [`crate::convolver::Convolver`] is the correctness implementation: it takes a whole
//! signal, transforms it once against the whole response, and is what gate G2 measures
//! against Chromium. It cannot be used on a live call, for a reason that has nothing to do
//! with speed — it needs the *last* sample of the input before it can produce the first.
//!
//! This is the same convolution rearranged so the first sample comes out one block after it
//! went in: the response is cut into blocks the size of a frame, each transformed once when
//! the response is loaded, and every frame is multiplied against all of them and summed.
//! Uniformly partitioned overlap-save, which is what `fft.rs` says the real-time path would
//! need and what the `ConvolverNode` does underneath.
//!
//! The test that matters is [`tests::block_by_block_agrees_with_the_whole_signal`]: the same
//! noise through both, sample for sample. Everything else here is bookkeeping around a
//! convolution that is already measured.
//!
//! # Why the response is a decoded WAV rather than the Ogg the app ships
//!
//! `static/sounds/reverb.ogx` is what `Voice.tsx` fetches, and what it convolves with is
//! not that file: it is what `decodeAudioData` makes of it — Vorbis decoded and resampled
//! from 44.1 kHz to the context's rate. `assets/reverb-48k.wav` is that result, taken from
//! Chromium itself by `scripts/golden-vectors`, and it is the same file the golden
//! convolver vectors are measured with. So this client convolves with the bytes the shipped
//! client convolves with, and no Ogg decoder or resampler enters the build to do it.
//!
//! # Two ways it can be silent, and only one of them is a bug
//!
//! A `ConvolverNode` with no buffer outputs silence rather than passing audio through, so
//! routing a voice into one that has not loaded makes that player inaudible — which is what
//! the impostors-hear-ghosts setting did in the Electron client whenever the decode had not
//! finished in time. `Voice.tsx` guards it by refusing to connect the effect at all and
//! saying so in the log. The same guard is here as the difference between [`warm`] and
//! [`ready`]: [`ready`] never waits, and a caller that gets `None` is expected to leave the
//! dry path alone rather than route into something that is not there yet.

use std::sync::Arc;
use std::sync::OnceLock;

use crate::codec::{FRAME_SAMPLES, SAMPLE_RATE};
use crate::convolver::normalization_scale;
use crate::fft::{Complex, transform};

/// The block this works in: one frame, so a frame in is a frame out.
///
/// It is also the length of each partition of the response, and those two have to be the
/// same number — partition `p` contributes the input from `p` blocks ago, and that only
/// lines up if a partition is as long as a block.
pub const BLOCK: usize = FRAME_SAMPLES;

/// The transform length: the smallest power of two that holds two blocks.
///
/// Two, because each transform covers the previous block as well as this one. That is what
/// makes the circular convolution the transform actually performs agree with the linear one
/// over the half of it this reads — the wrapped part lands in the half that is thrown away.
const TRANSFORM: usize = (2 * BLOCK).next_power_of_two();

/// How many bins are kept.
///
/// Everything here is real, so each spectrum is conjugate-symmetric and the upper half is
/// the lower half read backwards. Storing it would double both the memory and the
/// multiply-accumulate loop, which is the one loop in this file that costs anything.
const BINS: usize = TRANSFORM / 2 + 1;

/// The response, decoded by Chromium at the rate this client runs at.
const ENCODED: &[u8] = include_bytes!("../assets/reverb-48k.wav");

/// Built once, on whichever thread called [`warm`] first.
static RESPONSE: OnceLock<Option<Arc<Response>>> = OnceLock::new();

/// An impulse response, cut into blocks and transformed, ready to convolve against.
///
/// Shared: this is several megabytes and identical for every peer, so it is built once and
/// each [`Reverb`] holds a handle. What is *not* shared is the delay line of past input
/// spectra, which is per peer because the input is.
#[derive(Debug)]
pub struct Response {
    /// Every partition's spectrum, flat: `[(channel * partitions + partition) * BINS + bin]`.
    ///
    /// One allocation rather than a `Vec` per partition. The inner loop walks it in order,
    /// and three hundred separate allocations would walk three hundred cache lines to find
    /// out where to go next.
    spectra: Vec<Complex>,
    /// How many blocks the response was cut into.
    partitions: usize,
    /// How many channels it has: one is fed to both sides, two are a side each.
    channels: usize,
}

impl Response {
    /// Cuts an interleaved impulse response into partitions and transforms each.
    ///
    /// `sample_rate` is the rate the response is *at*, which is what the normalisation
    /// scale is a function of — the same argument [`crate::convolver::Convolver::new`]
    /// takes, and it has to be the same value there and here or the two disagree by tens of
    /// decibels while agreeing about everything else.
    #[must_use]
    pub fn new(interleaved: &[f32], channels: usize, sample_rate: f64) -> Self {
        if channels == 0 || interleaved.is_empty() {
            return Self {
                spectra: Vec::new(),
                partitions: 0,
                channels: 0,
            };
        }
        // `normalize` is left at its default in the client, and this is what that default
        // means. See `convolver.rs`: skipping it leaves the reverb right in shape and wrong
        // by tens of decibels.
        let scale = normalization_scale(interleaved, channels, sample_rate);

        let frames = interleaved.len() / channels;
        let partitions = frames.div_ceil(BLOCK);
        let mut spectra = vec![Complex::default(); channels * partitions * BINS];
        let mut scratch = vec![Complex::default(); TRANSFORM];

        for channel in 0..channels {
            for partition in 0..partitions {
                scratch.fill(Complex::default());
                for offset in 0..BLOCK {
                    let frame = partition * BLOCK + offset;
                    let Some(sample) = interleaved.get(frame * channels + channel) else {
                        // The last partition is short unless the response divides evenly.
                        // Zeroes are the right tail: the response has ended.
                        break;
                    };
                    if let Some(slot) = scratch.get_mut(offset) {
                        *slot = Complex::real(f64::from(*sample) * scale);
                    }
                }
                transform(&mut scratch, false);

                let start = (channel * partitions + partition) * BINS;
                if let Some(target) = spectra.get_mut(start..start + BINS) {
                    for (slot, value) in target.iter_mut().zip(scratch.iter()) {
                        *slot = *value;
                    }
                }
            }
        }

        Self {
            spectra,
            partitions,
            channels,
        }
    }

    /// How many blocks the response was cut into.
    #[must_use]
    pub fn partitions(&self) -> usize {
        self.partitions
    }

    /// How many channels it has.
    #[must_use]
    pub fn channels(&self) -> usize {
        self.channels
    }
}

/// Decodes and partitions the embedded response, and says whether it worked.
///
/// Blocking, and it transforms three hundred blocks to do it. Call it from a thread that is
/// allowed to take a hundred milliseconds — never from the mixing loop, which has twenty.
/// Calling it twice is free; the second caller gets the first one's answer.
pub fn warm() -> bool {
    RESPONSE.get_or_init(build).is_some()
}

/// The response if [`warm`] has finished with it, and `None` while it has not.
///
/// Never waits, on purpose. A caller on the audio path that finds `None` should leave the
/// player's dry path alone — the way `Voice.tsx` declines to connect a convolver whose
/// buffer has not arrived, rather than routing a voice into one and making that player
/// silent.
#[must_use]
pub fn ready() -> Option<Arc<Response>> {
    RESPONSE.get().and_then(Option::clone)
}

/// Decodes the embedded WAV and partitions it.
fn build() -> Option<Arc<Response>> {
    let decoded = crate::wav::decode(ENCODED).ok()?;
    // The response is resampled to the context's rate before it is convolved with, and this
    // one was resampled to ours when it was decoded. At another rate it would be the right
    // room at the wrong speed, so refuse it rather than convolve with it: an asset that
    // does not match is a build mistake, and a build mistake that produces sound is one
    // nobody looks for.
    if decoded.sample_rate != SAMPLE_RATE || decoded.channels == 0 {
        return None;
    }
    Some(Arc::new(Response::new(
        &decoded.samples,
        decoded.channels,
        f64::from(decoded.sample_rate),
    )))
}

/// One peer's reverb: the response, and everything this peer has said lately.
///
/// Every buffer is allocated here, because [`Reverb::process`] is not allowed to allocate —
/// see `tests/allocations.rs`. The state is what makes it per peer: two people haunting
/// share the response and share nothing else.
#[derive(Debug)]
pub struct Reverb {
    /// The partitioned response, shared with every other peer.
    response: Arc<Response>,
    /// The last `partitions` input spectra, newest at [`Self::head`].
    ///
    /// One delay line, not two: the input is mono, and both output channels multiply the
    /// same spectra by their own half of the response.
    delay_line: Vec<Complex>,
    /// Where the newest spectrum goes. Walks backwards through the partitions from here.
    head: usize,
    /// The block before this one, which each transform needs as its first half.
    previous: Vec<f32>,
    /// The transform of `[previous, current]`.
    scratch: Vec<Complex>,
    /// One output channel's sum, then that channel's samples.
    accumulator: Vec<Complex>,
}

impl Reverb {
    /// A reverb for one peer, with every buffer it will ever need already allocated.
    #[must_use]
    pub fn new(response: Arc<Response>) -> Self {
        let partitions = response.partitions;
        Self {
            response,
            delay_line: vec![Complex::default(); partitions * BINS],
            head: 0,
            previous: vec![0.0; BLOCK],
            scratch: vec![Complex::default(); TRANSFORM],
            accumulator: vec![Complex::default(); TRANSFORM],
        }
    }

    /// Convolves one block of mono input into interleaved stereo.
    ///
    /// `input` must be [`BLOCK`] long and `out` twice that. A call with any other length
    /// writes silence rather than a partial answer: half a frame of reverb is a click, and
    /// silence for one frame is not.
    ///
    /// Allocates nothing.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        out.fill(0.0);
        let partitions = self.response.partitions;
        if input.len() != BLOCK || out.len() != BLOCK * 2 || partitions == 0 {
            return;
        }

        // The window each transform covers: the previous block, then this one, then zeroes
        // out to the transform length. The zeroes are what keep the wrapped part of the
        // circular convolution inside the half that is discarded.
        self.scratch.fill(Complex::default());
        for (slot, sample) in self
            .scratch
            .iter_mut()
            .zip(self.previous.iter().chain(input.iter()))
        {
            *slot = Complex::real(f64::from(*sample));
        }
        transform(&mut self.scratch, false);

        let head = self.head;
        if let Some(slot) = self.delay_line.get_mut(head * BINS..head * BINS + BINS) {
            for (target, value) in slot.iter_mut().zip(self.scratch.iter()) {
                *target = *value;
            }
        }

        for channel in 0..2 {
            // A one-channel response feeds both sides; a two-channel one gives a side each.
            // Summing them, or convolving only the first, both produce something that
            // sounds like reverb and is not this one.
            let source = channel.min(self.response.channels.saturating_sub(1));

            // Borrowed field by field: the accumulator is written while the delay line and
            // the response are read, and those are three different fields of `self`.
            let accumulator = &mut self.accumulator;
            let delay_line = &self.delay_line;
            let spectra = &self.response.spectra;
            {
                let Some(sum) = accumulator.get_mut(..BINS) else {
                    continue;
                };
                for slot in sum.iter_mut() {
                    *slot = Complex::default();
                }
                for partition in 0..partitions {
                    // Partition `p` of the response meets the input from `p` blocks ago:
                    // that is the delay the partitioning takes out of the convolution and
                    // has to put back.
                    let slot = (head + partitions - partition) % partitions;
                    let Some(past) = delay_line.get(slot * BINS..slot * BINS + BINS) else {
                        continue;
                    };
                    let base = (source * partitions + partition) * BINS;
                    let Some(shape) = spectra.get(base..base + BINS) else {
                        continue;
                    };
                    for ((bin, x), h) in sum.iter_mut().zip(past).zip(shape) {
                        bin.re += x.re.mul_add(h.re, -(x.im * h.im));
                        bin.im += x.re.mul_add(h.im, x.im * h.re);
                    }
                }
            }

            // The half that was not computed, which is the half that was not stored. Every
            // signal here is real, so the spectrum is its own conjugate read backwards.
            for bin in 1..TRANSFORM / 2 {
                let Some(&value) = accumulator.get(bin) else {
                    continue;
                };
                if let Some(slot) = accumulator.get_mut(TRANSFORM - bin) {
                    *slot = Complex {
                        re: value.re,
                        im: -value.im,
                    };
                }
            }
            transform(accumulator, true);

            // The second half of the window is the part the wrap did not reach.
            let Some(valid) = accumulator.get(BLOCK..2 * BLOCK) else {
                continue;
            };
            for (frame, value) in valid.iter().enumerate() {
                if let Some(target) = out.get_mut(frame * 2 + channel) {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "narrowing back to the sample format"
                    )]
                    {
                        *target = value.re as f32;
                    }
                }
            }
        }

        self.previous.copy_from_slice(input);
        self.head = (head + 1) % partitions;
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
    use crate::convolver::Convolver;

    /// Deterministic noise, so a failure is the same failure twice.
    fn noise(count: usize, seed: u64) -> Vec<f32> {
        let mut state = seed;
        (0..count)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 33) as f64 / f64::from(u32::MAX) - 0.5) as f32
            })
            .collect()
    }

    /// The worst single-sample disagreement between two signals.
    fn worst(a: &[f32], b: &[f32]) -> f64 {
        a.iter()
            .zip(b)
            .map(|(x, y)| f64::from(*x - *y).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn block_by_block_agrees_with_the_whole_signal() {
        // The whole point of this module: the same convolution, rearranged so it can be
        // done a frame at a time. A short synthetic response, so the arithmetic being
        // checked is the partitioning rather than the size of the file.
        let response_samples = noise(BLOCK * 5 / 2, 7);
        let shared = Arc::new(Response::new(&response_samples, 2, 48000.0));
        assert_eq!(
            shared.partitions(),
            2,
            "1.25 blocks per channel is 2 blocks"
        );

        let input = noise(BLOCK * 4, 11);
        let offline = Convolver::new(&response_samples, 2, 48000.0, true);
        let expected = offline.process_mono_to_stereo(&input);

        let mut reverb = Reverb::new(Arc::clone(&shared));
        let mut produced = Vec::with_capacity(expected.len());
        let mut out = vec![0.0f32; BLOCK * 2];
        for block in input.as_chunks::<BLOCK>().0 {
            reverb.process(block, &mut out);
            produced.extend_from_slice(&out);
        }

        assert_eq!(produced.len(), expected.len());
        let error = worst(&expected, &produced);
        assert!(
            error < 1e-6,
            "block by block differs from all at once by {error}"
        );
    }

    #[test]
    fn a_one_channel_response_feeds_both_sides() {
        let response_samples = noise(BLOCK, 3);
        let shared = Arc::new(Response::new(&response_samples, 1, 48000.0));
        let mut reverb = Reverb::new(shared);
        let mut out = vec![0.0f32; BLOCK * 2];
        reverb.process(&noise(BLOCK, 5), &mut out);
        for frame in out.as_chunks::<2>().0 {
            assert!((frame[0] - frame[1]).abs() < 1e-12);
        }
        assert!(out.iter().any(|sample| sample.abs() > 1e-6), "not silence");
    }

    #[test]
    fn a_block_of_the_wrong_length_is_silence_rather_than_half_an_answer() {
        let shared = Arc::new(Response::new(&noise(BLOCK, 3), 1, 48000.0));
        let mut reverb = Reverb::new(shared);
        let mut out = vec![1.0f32; BLOCK * 2];
        reverb.process(&noise(BLOCK - 1, 5), &mut out);
        assert!(out.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn an_empty_response_convolves_to_silence_rather_than_panicking() {
        let shared = Arc::new(Response::new(&[], 0, 48000.0));
        assert_eq!(shared.partitions(), 0);
        let mut reverb = Reverb::new(shared);
        let mut out = vec![1.0f32; BLOCK * 2];
        reverb.process(&noise(BLOCK, 5), &mut out);
        assert!(out.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn the_embedded_response_is_the_one_chromium_decoded() {
        assert!(warm(), "the embedded impulse response did not load");
        let response = ready().expect("warm said it loaded");
        assert_eq!(response.channels(), 2);
        // 144 892 frames at 48 kHz: three seconds, and it is still 35 dB above silence two
        // seconds in, which is why none of it is thrown away.
        assert_eq!(response.partitions(), 144_892_usize.div_ceil(BLOCK));
    }

    #[test]
    fn the_embedded_response_convolves_the_way_the_measured_one_does() {
        // The same comparison as above against the real response, which is the one players
        // hear. Slower, and worth it: the synthetic case has two partitions and this has a
        // hundred and fifty-one, so it is the only one that exercises the delay line
        // wrapping round.
        assert!(warm());
        let response = ready().expect("warm said it loaded");
        let decoded = crate::wav::decode(ENCODED).expect("the embedded response decodes");

        let input = noise(BLOCK * 3, 23);
        let expected = Convolver::new(
            &decoded.samples,
            decoded.channels,
            f64::from(decoded.sample_rate),
            true,
        )
        .process_mono_to_stereo(&input);

        let mut reverb = Reverb::new(response);
        let mut produced = Vec::with_capacity(expected.len());
        let mut out = vec![0.0f32; BLOCK * 2];
        for block in input.as_chunks::<BLOCK>().0 {
            reverb.process(block, &mut out);
            produced.extend_from_slice(&out);
        }

        let error = worst(&expected, &produced);
        assert!(error < 1e-5, "the real response differs by {error}");
        assert!(
            produced.iter().any(|sample| sample.abs() > 1e-4),
            "a reverb that agrees with the reference by both being silent proves nothing"
        );
    }

    /// One haunting ghost must cost a fraction of a frame, and this says what fraction.
    ///
    /// Not in a debug build: this is the one test here whose answer depends on the
    /// optimiser, and a scalar radix-2 transform run unoptimised would fail a bound that
    /// says nothing about what ships. Measured at 0.84 ms per frame on the machine this was
    /// written on -- four per cent of the twenty a frame has -- so a lobby full of ghosts
    /// fits. The bound is a quarter of a frame, which is loose enough to survive a slow
    /// runner and tight enough that losing the partitioning fails it: the offline convolver
    /// on the same response takes the better part of a second.
    #[cfg(not(debug_assertions))]
    #[test]
    fn one_ghost_costs_a_fraction_of_a_frame() {
        assert!(warm());
        let response = ready().expect("warm said it loaded");
        let mut reverb = Reverb::new(response);
        let input = noise(BLOCK, 1);
        let mut out = vec![0.0f32; BLOCK * 2];
        for _ in 0..10 {
            reverb.process(&input, &mut out);
        }

        const RUNS: u32 = 200;
        let start = std::time::Instant::now();
        for _ in 0..RUNS {
            reverb.process(&input, &mut out);
        }
        let each = start.elapsed() / RUNS;
        let frame = std::time::Duration::from_millis(20);
        println!(
            "reverb: {each:?} per frame, {:.1}% of the budget",
            100.0 * each.as_secs_f64() / frame.as_secs_f64()
        );
        assert!(each < frame / 4, "a frame of reverb took {each:?}");
    }

    /// The room keeps going after the ghost stops, and stops when the response runs out.
    ///
    /// This is what makes the reverb worth the state it costs, and it is also what the
    /// mixing loop has to keep feeding: a peer whose gain has fallen to zero still has three
    /// seconds of themself inside the convolver, and dropping them mid-tail is a cut rather
    /// than a fade. Where the last block falls is not a guess -- the response is cut into
    /// `partitions` blocks, and one block of input convolved with `partitions` blocks of
    /// response is `partitions + 1` blocks long -- so the voice that arrived at call zero is
    /// last heard at call `partitions`, and not at all after it.
    #[test]
    fn the_tail_outlasts_the_voice_that_made_it() {
        assert!(warm());
        let response = ready().expect("warm said it loaded");
        let partitions = response.partitions();
        let mut reverb = Reverb::new(response);

        let mut out = vec![0.0f32; BLOCK * 2];
        reverb.process(&noise(BLOCK, 31), &mut out);

        let silence = vec![0.0f32; BLOCK];
        let mut last_heard = 0;
        for call in 1..=partitions + 1 {
            reverb.process(&silence, &mut out);
            if out.iter().any(|sample| *sample != 0.0) {
                last_heard = call;
            }
        }
        assert_eq!(
            last_heard, partitions,
            "the tail should end with the response and ended at call {last_heard} of {partitions}"
        );
    }

    #[test]
    fn ready_does_not_wait() {
        // Whether it has been warmed or not, this returns. The mixing loop calls it every
        // frame and cannot afford the other answer.
        let _ = ready();
    }
}
