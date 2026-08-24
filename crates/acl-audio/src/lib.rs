//! The audio engine: the DSP graph, the voice decision, capture, codec and playback.
//!
//! Phase 3 of the Rust port, and the plan calls it the phase that decides the project.
//! No UI and no network here — a library plus harnesses that read WAV in and write WAV
//! out, so every part of it can be measured against the Electron client's own output
//! rather than against an opinion.

pub mod analyser;
pub mod biquad;
pub mod codec;
pub mod convolver;
pub mod device;
pub mod fft;
pub mod gain;
pub mod jitter;
pub mod panner;
pub mod resample;
pub mod vad;
pub mod voice;
pub mod wav;
