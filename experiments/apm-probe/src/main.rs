//! P1+ experiment 2: does the chosen echo canceller link, and does it cancel echo?
//!
//! It began as gate G2's precondition (a) — does sonora build for 32-bit Windows, which
//! the plan called genuinely unproven. The answer was yes, and the question then became
//! moot: the injection path was removed on 2026-08-24, the `i686-pc-windows-msvc` target
//! went with it, and there is no 32-bit build left to worry about.
//!
//! What replaced it is the question that actually matters. `experiments/README.md` records
//! sonora as the default because `webrtc-audio-processing` does not build on Windows, and
//! the plan wants an A/B echo-return-loss measurement before that default becomes a
//! decision. That A/B needs a speaker and a microphone in a room; this does not, and it
//! answers the prior question the A/B assumes: **does sonora cancel anything at all?**
//!
//! A crate that links and runs and removes no echo would have passed every check this
//! project had until now.
//!
//! # How the echo is made
//!
//! No microphone. The far end is fed to the render path, and the capture path is given
//! what a microphone in a room would pick up: the same signal, delayed by the time it takes
//! to leave a speaker and come back, attenuated, and smeared by a couple of reflections.
//! That is not a real room -- a real one has hundreds of reflections and a moving talker --
//! but it is the shape an echo canceller has to model, and a canceller that cannot remove
//! this one cannot remove a real one either.
//!
//! ERLE is echo return loss enhancement: how much quieter the echo is on the way out than
//! it was on the way in, in decibels. Ten is audible improvement, twenty is good, and zero
//! means the canceller did nothing.

use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};

/// What the client runs at.
const SAMPLE_RATE: u32 = 48_000;

/// The APM's own frame. Everything here is fed in these.
const FRAME: usize = (SAMPLE_RATE as usize) / 100;

/// How far the loudspeaker is from the microphone, in time.
///
/// 60 ms is a normal figure for laptop speakers plus the buffering on either side of them,
/// and it is comfortably inside AEC3's search window.
const ECHO_DELAY_MS: usize = 60;

/// How much quieter the echo is than what was played.
const ECHO_GAIN: f32 = 0.4;

/// Twelve seconds. AEC3 needs a few seconds to converge, and a measurement taken over the
/// convergence is a measurement of the convergence.
const SECONDS: usize = 12;

/// Far-end material: a talker, roughly. Pitch and level both move, because a steady tone is
/// the one thing an adaptive filter finds easy.
fn far_end(sample: usize) -> f32 {
    let t = sample as f64 / f64::from(SAMPLE_RATE);
    let syllable = (t * 3.7).sin().mul_add(0.5, 0.5);
    let pitch = 140.0 + 40.0 * (t * 1.3).sin();
    let harmonics = (std::f64::consts::TAU * pitch * t).sin()
        + 0.5 * (std::f64::consts::TAU * pitch * 2.0 * t).sin()
        + 0.25 * (std::f64::consts::TAU * pitch * 3.0 * t).sin();
    (harmonics * syllable * 0.3) as f32
}

/// Energy of a block, as a sum of squares.
fn energy(samples: &[f32]) -> f64 {
    samples.iter().map(|s| f64::from(*s).powi(2)).sum()
}

/// Builds the far-end signal and what a microphone in the room would hear.
fn scene() -> (Vec<f32>, Vec<f32>) {
    let total = SECONDS * SAMPLE_RATE as usize;
    let delay = ECHO_DELAY_MS * SAMPLE_RATE as usize / 1000;
    let played: Vec<f32> = (0..total).map(far_end).collect();

    let mut heard = vec![0.0f32; total];
    for (index, sample) in heard.iter_mut().enumerate() {
        let mut echo = 0.0f32;
        for (offset, gain) in [(0usize, 1.0f32), (313, 0.45), (877, 0.2)] {
            if let Some(source) = index.checked_sub(delay + offset) {
                echo += played[source] * gain;
            }
        }
        *sample = echo * ECHO_GAIN;
    }
    (played, heard)
}

fn main() {
    println!("sonora linked for {}", std::env::consts::ARCH);

    let stream = StreamConfig::new(SAMPLE_RATE, 1);
    let mut apm = AudioProcessing::builder()
        .config(Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Config::default()
        })
        .capture_config(stream)
        .render_config(stream)
        .build();

    let (played, heard) = scene();
    let total = played.len();

    // AEC3 needs to be told roughly how late the echo is; it refines from there.
    let _ = apm.set_stream_delay_ms(ECHO_DELAY_MS as i32);

    let mut render_out = vec![0.0f32; FRAME];
    let mut capture_out = vec![0.0f32; FRAME];

    // Measured over the last two seconds only. The first ten are the filter converging, and
    // averaging over them measures how long it took rather than how well it did.
    let measure_from = total.saturating_sub(2 * SAMPLE_RATE as usize);
    let mut echo_in = 0.0f64;
    let mut echo_out = 0.0f64;

    let mut at = 0;
    while at + FRAME <= total {
        let render_frame = &played[at..at + FRAME];
        let capture_frame = &heard[at..at + FRAME];

        // Render first, always. It is what tells the canceller what to look for; doing it
        // after the capture frame would have it cancelling an echo it has not heard yet.
        let _ = apm.process_render_f32(&[render_frame], &mut [&mut render_out[..]]);
        let _ = apm.process_capture_f32(&[capture_frame], &mut [&mut capture_out[..]]);

        if at >= measure_from {
            echo_in += energy(capture_frame);
            echo_out += energy(&capture_out);
        }
        at += FRAME;
    }

    let erle_db = if echo_out <= f64::EPSILON {
        f64::INFINITY
    } else {
        10.0 * (echo_in / echo_out).log10()
    };

    println!("echo delay:  {ECHO_DELAY_MS} ms, three reflections, {SECONDS} s of speech");
    println!("measured over the last 2 s, after convergence");
    println!("ERLE:        {erle_db:.1} dB");

    // Ten decibels is the line between "audible improvement" and "did something you would
    // have to measure to notice". A canceller that links, runs, and returns its input
    // unchanged would pass every other check this project has.
    assert!(
        erle_db >= 10.0,
        "sonora removed only {erle_db:.1} dB of echo; a canceller that does nothing looks \
         exactly like this and passes every build check"
    );
    println!("verdict:     the echo canceller works");
}
