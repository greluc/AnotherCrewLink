//! Echo cancellation, noise suppression and gain control, behind a trait.
//!
//! The trait is not decoration. §3.3 put the audio processing module behind one so the
//! gate could change the answer without changing the graph, and the gate very nearly did:
//! `libwebrtc`'s AEC3 became reachable again when the `i686` target was struck, and the
//! A/B came out 11.6 dB against 11.3 dB — no difference worth having. `sonora` stayed
//! because AEC3 arrives as a prebuilt 86 MB library somebody else compiled. If that ever
//! stops being the deciding factor, this file changes and nothing else does.
//!
//! # The far-end reference is a real path
//!
//! An echo canceller needs to know what the speakers played in order to subtract it from
//! what the microphone heard. The reference has to be **the buffer handed to the output
//! device** — after mixing, after panning, filtering and reverb — because that is what
//! actually left the speakers and came back. Not one peer's decoded audio, and not the mix
//! before the graph.
//!
//! Getting that wrong does not fail: the canceller runs, returns success, and removes
//! nothing. §3.3 calls it the most common way an echo canceller is broken, and it is the
//! reason `the_reference_is_what_makes_it_work` exists below — a test that passes whether
//! or not this module is wired correctly would be worse than no test.
//!
//! # Block sizes
//!
//! The processing module works in 10 ms blocks, which is what WebRTC has always used. This
//! client's frame is 20 ms, because that is Opus's. So each call splits into two, and the
//! split is here rather than in the caller: everything upstream and downstream of this
//! speaks in `FRAME_SAMPLES`, and one module knowing about two block sizes is better than
//! every module knowing about one it does not use.

use sonora::config::{EchoCanceller, NoiseSuppression};
use sonora::{AudioProcessing, Config, StreamConfig};

use crate::codec::{FRAME_SAMPLES, SAMPLE_RATE};

/// The block the processing module works in, in samples. 10 ms at 48 kHz.
pub const BLOCK_SAMPLES: usize = (SAMPLE_RATE as usize) / 100;

/// How many of those make up one frame from the rest of this crate.
const BLOCKS_PER_FRAME: usize = FRAME_SAMPLES / BLOCK_SAMPLES;

/// What can go wrong.
#[derive(Debug, PartialEq, Eq)]
pub enum ApmError {
    /// A frame that is not `FRAME_SAMPLES` long.
    WrongFrameLength {
        /// What the module works in.
        expected: usize,
        /// What it was handed.
        got: usize,
    },
}

impl std::fmt::Display for ApmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongFrameLength { expected, got } => {
                write!(f, "expected a frame of {expected} samples, got {got}")
            }
        }
    }
}

impl std::error::Error for ApmError {}

/// What the graph needs from an audio processing module.
///
/// Deliberately small. Everything WebRTC's own interface offers beyond this — the
/// statistics, the runtime setting changes, the multi-channel paths — is surface this
/// client does not use, and surface a replacement would have to reimplement for nothing.
pub trait Apm: Send {
    /// Hands over what the speakers are about to play.
    ///
    /// Call this **before** `capture` for the same instant in time. The other order asks
    /// the canceller to remove an echo of something it has not been told about yet.
    ///
    /// # Errors
    ///
    /// [`ApmError::WrongFrameLength`] if the frame is not `FRAME_SAMPLES` long.
    fn render(&mut self, frame: &[f32]) -> Result<(), ApmError>;

    /// Cleans one captured frame in place.
    ///
    /// # Errors
    ///
    /// [`ApmError::WrongFrameLength`] if the frame is not `FRAME_SAMPLES` long.
    fn capture(&mut self, frame: &mut [f32]) -> Result<(), ApmError>;

    /// Roughly how long it takes for a sample to leave the speakers and reach the
    /// microphone, in milliseconds. The canceller refines it from there; it needs a
    /// starting point that is in the right region rather than a precise figure.
    fn set_delay_ms(&mut self, delay_ms: u32);
}

/// The shipping implementation.
pub struct Sonora {
    inner: AudioProcessing,
    /// Scratch for the module's output, reused. `process_*_f32` writes into a caller
    /// buffer, and allocating one per frame would be an allocation per 20 ms per stream on
    /// a thread that must not allocate at all.
    scratch: Vec<f32>,
}

impl Sonora {
    /// Builds one configured for a voice call.
    ///
    /// Mono, 48 kHz, echo cancellation on. The other stages are left at their defaults:
    /// this client's DSP graph does its own gain and the microphone signal goes through a
    /// VAD, so overriding what WebRTC's own tuning does is a change that would need
    /// listening tests to justify rather than an opinion.
    #[must_use]
    pub fn new() -> Self {
        Self::configured(true, true)
    }

    /// The same, with the two stages the player can switch off.
    ///
    /// `echoCancellation` and `noiseSuppression` are settings on the shipped client's audio
    /// page, and they are given to `getUserMedia` when the microphone is opened rather than
    /// changed while it runs — so they belong here, at construction, and take effect when
    /// the capture is next opened. Neither reached this constructor until 2026-08-27:
    /// cancellation was hard-coded on and suppression hard-coded *off*, which is not even
    /// the setting's own default.
    ///
    /// The other stages stay at their defaults: this client's DSP graph does its own gain
    /// and the microphone signal goes through a VAD, so overriding what WebRTC's own tuning
    /// does is a change that would need listening tests to justify rather than an opinion.
    #[must_use]
    pub fn configured(echo_cancellation: bool, noise_suppression: bool) -> Self {
        let stream = StreamConfig::new(SAMPLE_RATE, 1);
        let inner = AudioProcessing::builder()
            .config(Config {
                echo_canceller: echo_cancellation.then(EchoCanceller::default),
                noise_suppression: noise_suppression.then(NoiseSuppression::default),
                ..Config::default()
            })
            .capture_config(stream)
            .render_config(stream)
            .build();
        Self {
            inner,
            scratch: vec![0.0; BLOCK_SAMPLES],
        }
    }
}

impl Default for Sonora {
    fn default() -> Self {
        Self::new()
    }
}

fn check(frame: &[f32]) -> Result<(), ApmError> {
    if frame.len() == FRAME_SAMPLES {
        Ok(())
    } else {
        Err(ApmError::WrongFrameLength {
            expected: FRAME_SAMPLES,
            got: frame.len(),
        })
    }
}

impl Apm for Sonora {
    fn render(&mut self, frame: &[f32]) -> Result<(), ApmError> {
        check(frame)?;
        for block in 0..BLOCKS_PER_FRAME {
            let from = block * BLOCK_SAMPLES;
            let Some(input) = frame.get(from..from + BLOCK_SAMPLES) else {
                continue;
            };
            // The output of the render path is discarded on purpose. What matters is that
            // the module has seen the signal; nothing downstream wants a processed copy of
            // what is already on its way to the speakers.
            let _ = self
                .inner
                .process_render_f32(&[input], &mut [&mut self.scratch[..]]);
        }
        Ok(())
    }

    fn capture(&mut self, frame: &mut [f32]) -> Result<(), ApmError> {
        check(frame)?;
        for block in 0..BLOCKS_PER_FRAME {
            let from = block * BLOCK_SAMPLES;
            let Some(input) = frame.get(from..from + BLOCK_SAMPLES) else {
                continue;
            };
            // Copied out, processed, copied back: the module will not read and write the
            // same slice, and the borrow checker agrees with it.
            let processed = {
                let _ = self
                    .inner
                    .process_capture_f32(&[input], &mut [&mut self.scratch[..]]);
                &self.scratch
            };
            let Some(target) = frame.get_mut(from..from + BLOCK_SAMPLES) else {
                continue;
            };
            target.copy_from_slice(processed);
        }
        Ok(())
    }

    fn set_delay_ms(&mut self, delay_ms: u32) {
        let _ = self
            .inner
            .set_stream_delay_ms(i32::try_from(delay_ms).unwrap_or(i32::MAX));
    }
}

/// An audio processing module that does nothing, for tests and for a headless build.
///
/// Not a fallback for a canceller that fails to build. It exists so a harness that only
/// wants to exercise the graph does not have to carry a real one, and so the trait has a
/// second implementation — a trait boundary with one implementation is a claim rather than
/// a fact.
#[derive(Debug, Default)]
pub struct Passthrough;

impl Apm for Passthrough {
    fn render(&mut self, frame: &[f32]) -> Result<(), ApmError> {
        check(frame)
    }

    fn capture(&mut self, frame: &mut [f32]) -> Result<(), ApmError> {
        check(frame)
    }

    fn set_delay_ms(&mut self, _delay_ms: u32) {}
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::indexing_slicing
    )]

    use super::*;

    /// How late the echo is in the scene below, and what the canceller is told.
    const ECHO_DELAY_MS: usize = 60;
    const ECHO_DELAY: usize = (SAMPLE_RATE as usize) * ECHO_DELAY_MS / 1000;
    const SECONDS: usize = 12;

    /// Speech-like far end, and what a microphone in the same room would hear of it.
    ///
    /// Three reflections rather than one: a single delayed copy is a filter the canceller
    /// solves almost immediately, and it would flatter any implementation.
    fn scene() -> (Vec<f32>, Vec<f32>) {
        let total = SAMPLE_RATE as usize * SECONDS;
        let mut played = Vec::with_capacity(total);
        for index in 0..total {
            let t = index as f64 / f64::from(SAMPLE_RATE);
            // A pitch that steps every 250 ms, so the canceller cannot settle on one tone.
            let step = (index / (SAMPLE_RATE as usize / 4)) % 5;
            let hertz = 180.0 + (step as f64) * 90.0;
            let envelope = 0.5 + 0.5 * (std::f64::consts::TAU * 3.0 * t).sin();
            played.push(((std::f64::consts::TAU * hertz * t).sin() * 0.4 * envelope) as f32);
        }

        let mut heard = vec![0.0f32; total];
        for (gain, delay) in [
            (0.55f32, ECHO_DELAY),
            (0.25, ECHO_DELAY + 900),
            (0.12, ECHO_DELAY + 2300),
        ] {
            for index in delay..total {
                heard[index] += played[index - delay] * gain;
            }
        }
        (played, heard)
    }

    /// Echo return loss enhancement: how much quieter the echo is on the way out.
    fn erle(apm: &mut dyn Apm, feed_reference: bool) -> f64 {
        let (played, heard) = scene();
        let total = played.len();
        apm.set_delay_ms(ECHO_DELAY_MS as u32);

        // The last two seconds only. The first ten are the adaptive filter converging, and
        // averaging over them measures how long it took rather than how well it did.
        let measure_from = total.saturating_sub(2 * SAMPLE_RATE as usize);
        let (mut before, mut after) = (0.0f64, 0.0f64);

        let mut frame = vec![0.0f32; FRAME_SAMPLES];
        let mut at = 0;
        while at + FRAME_SAMPLES <= total {
            if feed_reference {
                apm.render(&played[at..at + FRAME_SAMPLES]).unwrap();
            }
            frame.copy_from_slice(&heard[at..at + FRAME_SAMPLES]);
            let energy_in: f64 = frame.iter().map(|s| f64::from(*s).powi(2)).sum();
            apm.capture(&mut frame).unwrap();
            let energy_out: f64 = frame.iter().map(|s| f64::from(*s).powi(2)).sum();

            if at >= measure_from {
                before += energy_in;
                after += energy_out;
            }
            at += FRAME_SAMPLES;
        }
        if after <= f64::EPSILON {
            return f64::INFINITY;
        }
        10.0 * (before / after).log10()
    }

    #[test]
    fn the_reference_is_what_makes_it_work() {
        // The test this module exists for. §3.3: getting the far-end reference wrong does
        // not fail -- the canceller runs, reports success, and removes nothing. A test
        // that passed either way would certify exactly that mistake.
        //
        // Same canceller, same scene, same delay. The only difference is whether it was
        // told what the speakers played.
        let with = erle(&mut Sonora::new(), true);
        let without = erle(&mut Sonora::new(), false);
        println!("ERLE with the reference: {with:.1} dB, without it: {without:.1} dB");

        assert!(with > 6.0, "cancelled almost nothing: {with:.1} dB");
        // Without the reference there is nothing to subtract, so anything it removes is
        // the other stages working on a signal they were not asked about.
        assert!(
            without < 3.0,
            "cancelled {without:.1} dB with no reference at all"
        );
        assert!(
            with > without + 5.0,
            "the reference made almost no difference: {with:.1} dB against {without:.1} dB"
        );
    }

    #[test]
    fn a_frame_of_the_wrong_length_is_refused() {
        // Rather than processing part of it and leaving the rest untouched, which is a
        // microphone that works for 10 ms in every 20.
        let mut apm = Sonora::new();
        let short = vec![0.0f32; FRAME_SAMPLES - 1];
        assert_eq!(
            apm.render(&short),
            Err(ApmError::WrongFrameLength {
                expected: FRAME_SAMPLES,
                got: FRAME_SAMPLES - 1
            })
        );
        let mut short = vec![0.0f32; 480];
        assert!(apm.capture(&mut short).is_err());
    }

    #[test]
    fn the_frame_splits_into_whole_blocks() {
        // If this ever stops being true the loops above would silently drop the remainder.
        assert_eq!(BLOCKS_PER_FRAME * BLOCK_SAMPLES, FRAME_SAMPLES);
        assert_eq!(BLOCKS_PER_FRAME, 2);
    }

    #[test]
    fn passthrough_changes_nothing_and_still_checks_the_length() {
        let mut apm = Passthrough;
        let original: Vec<f32> = (0..FRAME_SAMPLES)
            .map(|i| (i as f32 / 100.0).sin())
            .collect();
        let mut frame = original.clone();
        apm.render(&original).unwrap();
        apm.capture(&mut frame).unwrap();
        assert_eq!(frame, original);
        assert!(apm.capture(&mut [0.0; 7]).is_err());
    }

    #[test]
    fn silence_in_is_silence_out() {
        // A canceller that injects anything of its own into a quiet capture would be heard
        // as a hiss by every player on a quiet microphone.
        let mut apm = Sonora::new();
        let quiet = vec![0.0f32; FRAME_SAMPLES];
        let mut frame = vec![0.0f32; FRAME_SAMPLES];
        for _ in 0..50 {
            apm.render(&quiet).unwrap();
            frame.copy_from_slice(&quiet);
            apm.capture(&mut frame).unwrap();
        }
        let peak = frame.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(peak < 1e-3, "peak {peak} out of silence");
    }
}
