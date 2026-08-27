//! Whether somebody is speaking: the port of `src/renderer/vad.ts`.
//!
//! Not a general voice-activity detector, and not trying to be. It is the one this client
//! ships: the energy in a narrow band around speech fundamentals, measured against a noise
//! floor the detector learns from the first second of audio, with a counter that has to
//! fill before it will say "talking" and drain before it will take it back.
//!
//! That counter is the whole design. A threshold on its own flickers on every consonant
//! and every chair creak, which on the other end is a name that blinks. Requiring several
//! consecutive frames each way trades a little latency for a decision that holds still.
//!
//! # The noise floor is learned, not configured
//!
//! For the first second the detector records levels and decides nothing. What it settles
//! on is the *minimum* it saw, times a margin — not the mean, which one cough during
//! calibration would drag up far enough to make the microphone useless for the session.

use crate::analyser::Analyser;

/// How long the detector listens before it will decide anything, in milliseconds.
pub const NOISE_CAPTURE_MS: u64 = 1000;

/// The transform size the detector's thresholds were tuned against.
///
/// `src/renderer/vad.ts` line 57. Here rather than at the call site because it is not the
/// caller's choice: the bin widths decide which frequencies fall in the capture band, so an
/// analyser built at a different size measures a different thing and the thresholds below
/// stop meaning what they say.
pub const FFT_SIZE: usize = 1024;

/// How much of the previous frame each bin keeps.
///
/// `src/renderer/vad.ts` line 59, and low on purpose. Web Audio defaults to 0.8, which is
/// most of a second of history and turns a decision about *this* frame into one about the
/// last twenty — a speaking indicator that lags a sentence behind the sentence.
pub const SMOOTHING: f64 = 0.2;

/// The band the decision is made in: speech fundamentals, in hertz.
pub const MIN_CAPTURE_HZ: f64 = 85.0;

/// The top of that band.
pub const MAX_CAPTURE_HZ: f64 = 255.0;

/// The learned floor is multiplied by this, so ordinary room noise sits below it.
pub const AVG_NOISE_MULTIPLIER: f64 = 1.2;

/// The floor is never allowed below this, however quiet the room was.
pub const MIN_NOISE_LEVEL: f64 = 0.15;

/// Nor above this, however loud it was.
pub const MAX_NOISE_LEVEL: f64 = 0.7;

/// How high the counter may go, which is how long it takes to fall silent.
const ACTIVITY_MAX: i32 = 30;

/// How high it has to be before the answer is "talking".
const ACTIVITY_THRESHOLD: i32 = 5;

/// The settings the client uses, gathered so a caller cannot set half of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadSettings {
    /// The band the decision is made in.
    pub min_capture_hz: f64,
    /// The top of it.
    pub max_capture_hz: f64,
    /// The margin over the learned floor.
    pub avg_noise_multiplier: f64,
    /// The lowest the floor may be.
    pub min_noise_level: f64,
    /// The highest.
    pub max_noise_level: f64,
}

impl Default for VadSettings {
    fn default() -> Self {
        Self {
            min_capture_hz: MIN_CAPTURE_HZ,
            max_capture_hz: MAX_CAPTURE_HZ,
            avg_noise_multiplier: AVG_NOISE_MULTIPLIER,
            min_noise_level: MIN_NOISE_LEVEL,
            max_noise_level: MAX_NOISE_LEVEL,
        }
    }
}

/// What one frame's decision came to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadFrame {
    /// Whether the detector thinks somebody is speaking.
    pub talking: bool,
    /// Whether that changed with this frame, which is what drives the wire message.
    pub changed: bool,
    /// How far above the floor this frame is, scaled to roughly 0..1, for a meter.
    pub level: f64,
}

/// The detector.
#[derive(Debug, Clone)]
pub struct Vad {
    settings: VadSettings,
    sample_rate: f64,
    /// Levels seen while calibrating, before the floor is decided.
    calibration: Vec<f64>,
    /// The learned floor, once calibration has finished.
    base_level: Option<f64>,
    /// What is left of the range above the floor, for the meter.
    voice_scale: f64,
    counter: i32,
    talking: bool,
}

impl Vad {
    /// A detector for one microphone.
    #[must_use]
    pub fn new(sample_rate: f64, settings: VadSettings) -> Self {
        Self {
            settings,
            sample_rate,
            calibration: Vec::new(),
            base_level: None,
            voice_scale: 1.0,
            counter: 0,
            talking: false,
        }
    }

    /// Changes the settings and learns the room again.
    ///
    /// `micSensitivity` is a live setting on the shipped client -- it writes
    /// `audioListener.options.minNoiseLevel` and then calls `init()`, which starts the
    /// measurement over. Keeping the old floor would be worse than either: it was decided
    /// against a threshold that is no longer the one in force.
    ///
    /// Allocation-free, which matters because the caller is an audio callback: the only
    /// heap in here is the calibration buffer, and clearing a `Vec` does not touch it.
    pub fn retune(&mut self, settings: VadSettings) {
        self.settings = settings;
        self.calibration.clear();
        self.base_level = None;
        self.voice_scale = 1.0;
        self.counter = 0;
        self.talking = false;
    }

    /// Whether the floor has been decided yet.
    #[must_use]
    pub fn calibrated(&self) -> bool {
        self.base_level.is_some()
    }

    /// The learned floor, once there is one.
    #[must_use]
    pub fn base_level(&self) -> Option<f64> {
        self.base_level
    }

    /// Ends calibration and settles on a floor.
    ///
    /// The client calls this on a timer, a second after it starts listening. The floor is
    /// the **minimum** level seen, not the mean: one cough during calibration drags a mean
    /// up far enough to make the microphone useless for the rest of the session, and a
    /// minimum is what the room actually sounds like when nothing is happening.
    pub fn finish_calibration(&mut self) {
        let quietest = self
            .calibration
            .iter()
            .copied()
            .filter(|level| *level > 0.0)
            .fold(f64::INFINITY, f64::min);

        let measured = if quietest.is_finite() {
            quietest
        } else {
            // Nothing usable was heard. The client's own fallback, which is the floor's
            // own minimum rather than silence: a detector with a floor of zero calls
            // everything speech.
            self.settings.min_noise_level
        };

        let base = (measured * self.settings.avg_noise_multiplier)
            .clamp(self.settings.min_noise_level, self.settings.max_noise_level);
        self.base_level = Some(base);
        self.voice_scale = 1.0 - base;
        self.calibration.clear();
    }

    /// Takes one frame from the analyser and decides.
    ///
    /// Before calibration has finished, the level is recorded and the answer is always
    /// "not talking" — which is what the client does, and is why the microphone is quiet
    /// for the first second.
    pub fn push(&mut self, analyser: &Analyser) -> VadFrame {
        self.push_bins(&analyser.byte_frequency_data())
    }

    /// As [`Self::push`], from the analyser's bytes directly.
    ///
    /// The bytes are all the detector ever sees, so this is the real interface and the
    /// method above is the convenience. It is also what makes the detector testable
    /// without synthesising audio to drive an analyser with.
    pub fn push_bins(&mut self, bins: &[u8]) -> VadFrame {
        let level = band_average(
            bins,
            self.sample_rate,
            self.settings.min_capture_hz,
            self.settings.max_capture_hz,
        );

        let Some(base) = self.base_level else {
            self.calibration.push(level);
            return VadFrame {
                talking: false,
                changed: false,
                level: 0.0,
            };
        };

        // The counter fills while the level is above the floor and drains while it is
        // below, and the answer is whether it is past the threshold. Several frames each
        // way, so a consonant does not make a name blink.
        if level >= base && self.counter < ACTIVITY_MAX {
            self.counter += 1;
        } else if level < base && self.counter > 0 {
            self.counter -= 1;
        }

        let talking = self.counter > ACTIVITY_THRESHOLD;
        let changed = talking != self.talking;
        self.talking = talking;

        VadFrame {
            talking,
            changed,
            level: (level - base).max(0.0) / self.voice_scale.max(f64::EPSILON),
        }
    }
}

/// The average of one frequency band, as a fraction of full scale.
///
/// The bytes are the analyser's, so each is 0..255 across the decibel window. Bins are
/// picked by rounding the band edges to the nearest bin, which is what the client does —
/// at 48 kHz and 1024 bins, 85 Hz and 255 Hz both round to bins 2 and 5, so the decision
/// rests on three bins.
#[must_use]
pub fn band_average(bins: &[u8], sample_rate: f64, min_hz: f64, max_hz: f64) -> f64 {
    let count = bins.len();
    if count == 0 {
        return 0.0;
    }
    let start = frequency_to_index(min_hz, sample_rate, count);
    let end = frequency_to_index(max_hz, sample_rate, count);
    if end <= start {
        return 0.0;
    }
    let sum: f64 = bins
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .map(|byte| f64::from(*byte) / 255.0)
        .sum();
    #[allow(
        clippy::cast_precision_loss,
        reason = "a bin count is at most a few thousand"
    )]
    let span = (end - start) as f64;
    sum / span
}

/// Which bin a frequency falls in, rounded, and clamped to the range that exists.
fn frequency_to_index(frequency: f64, sample_rate: f64, bin_count: usize) -> usize {
    let nyquist = sample_rate / 2.0;
    if nyquist <= 0.0 {
        return 0;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "a bin count is at most a few thousand"
    )]
    let count = bin_count as f64;
    let scaled = (frequency / nyquist) * count;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped into the bin range first"
    )]
    {
        scaled.round().clamp(0.0, count) as usize
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::cast_precision_loss,
        clippy::float_cmp
    )]

    use super::*;

    const RATE: f64 = 48000.0;

    /// An analyser holding a given level across the whole spectrum.
    fn flat(level: u8, bins: usize) -> Vec<u8> {
        vec![level; bins]
    }

    #[test]
    fn the_band_is_the_one_speech_lives_in() {
        // 48000 / 2 / 512 is 46.875 Hz per bin, so 85 Hz rounds to bin 2 and 255 Hz to
        // bin 5. Three bins, and the decision rests on them.
        assert_eq!(frequency_to_index(85.0, RATE, 512), 2);
        assert_eq!(frequency_to_index(255.0, RATE, 512), 5);
    }

    #[test]
    fn a_band_average_is_a_fraction_of_full_scale() {
        assert_eq!(band_average(&flat(255, 512), RATE, 85.0, 255.0), 1.0);
        assert_eq!(band_average(&flat(0, 512), RATE, 85.0, 255.0), 0.0);
        // Only the band counts: loud everywhere else, silent in it.
        let mut bins = flat(255, 512);
        for bin in bins.iter_mut().take(5).skip(2) {
            *bin = 0;
        }
        assert_eq!(band_average(&bins, RATE, 85.0, 255.0), 0.0);
    }

    #[test]
    fn an_empty_or_inverted_band_is_zero_rather_than_a_division_by_nothing() {
        assert_eq!(band_average(&[], RATE, 85.0, 255.0), 0.0);
        assert_eq!(band_average(&flat(255, 512), RATE, 255.0, 85.0), 0.0);
        assert_eq!(band_average(&flat(255, 512), 0.0, 85.0, 255.0), 0.0);
    }

    /// Drives a detector with a constant level for a number of frames.
    fn run(vad: &mut Vad, level: u8, frames: usize) -> VadFrame {
        // A flat picture is easier to reason about than a synthesised tone, and the
        // detector only ever sees the analyser's bytes. 512 bins is an fftSize of 1024,
        // which is what the client sets.
        let bins = flat(level, 512);
        let mut last = VadFrame {
            talking: false,
            changed: false,
            level: 0.0,
        };
        for _ in 0..frames {
            last = vad.push_bins(&bins);
        }
        last
    }

    #[test]
    fn nothing_is_decided_before_the_floor_is_learned() {
        // The first second is quiet by design, and a detector that answered during it
        // would answer from a floor of zero — which calls everything speech.
        let mut vad = Vad::new(RATE, VadSettings::default());
        let frame = run(&mut vad, 255, 60);
        assert!(!frame.talking);
        assert!(!vad.calibrated());
    }

    #[test]
    fn the_floor_is_the_quietest_moment_not_the_average() {
        // One cough during calibration drags a mean up far enough to make the microphone
        // useless for the session. The minimum is what the room sounds like.
        let mut vad = Vad::new(RATE, VadSettings::default());
        run(&mut vad, 20, 30); // quiet room
        run(&mut vad, 250, 5); // a cough
        vad.finish_calibration();
        let base = vad.base_level().unwrap();
        // 20/255 is about 0.078; times the 1.2 margin it is still under the floor's own
        // minimum, so the minimum wins.
        assert_eq!(base, MIN_NOISE_LEVEL);
    }

    #[test]
    fn the_floor_is_clamped_at_both_ends() {
        let mut quiet = Vad::new(RATE, VadSettings::default());
        run(&mut quiet, 1, 10);
        quiet.finish_calibration();
        assert_eq!(quiet.base_level().unwrap(), MIN_NOISE_LEVEL);

        let mut loud = Vad::new(RATE, VadSettings::default());
        run(&mut loud, 255, 10);
        loud.finish_calibration();
        // 1.0 * 1.2 is above the ceiling, so the ceiling wins — a room that was loud
        // throughout calibration does not get a floor nothing can cross.
        assert_eq!(loud.base_level().unwrap(), MAX_NOISE_LEVEL);
    }

    #[test]
    fn calibrating_on_silence_falls_back_rather_than_to_zero() {
        // A muted or absent microphone. A floor of zero calls everything speech.
        let mut vad = Vad::new(RATE, VadSettings::default());
        run(&mut vad, 0, 30);
        vad.finish_calibration();
        assert!(vad.base_level().unwrap() >= MIN_NOISE_LEVEL);
    }

    #[test]
    fn it_takes_several_frames_to_start_and_to_stop() {
        // The counter is the design. A threshold on its own flickers on every consonant,
        // and on the other end that is a name that blinks.
        let mut vad = Vad::new(RATE, VadSettings::default());
        run(&mut vad, 10, 20);
        vad.finish_calibration();

        // Loud, but not yet: the counter has to pass the threshold.
        for _ in 0..ACTIVITY_THRESHOLD {
            assert!(!run(&mut vad, 255, 1).talking);
        }
        assert!(
            run(&mut vad, 255, 1).talking,
            "should start after six frames"
        );

        // Stopping costs whatever the counter has climbed to. Right at the threshold that
        // is one frame — but a sentence drives the counter to its ceiling, and from there
        // it takes twenty-five, which is the asymmetry the design is for: quick to start,
        // slow to let go, so a pause for breath is not a gap in the audio.
        assert!(
            !run(&mut vad, 0, 1).talking,
            "at the threshold, one frame drops it"
        );

        run(&mut vad, 255, ACTIVITY_MAX as usize);
        for _ in 0..(ACTIVITY_MAX - ACTIVITY_THRESHOLD - 1) {
            assert!(
                run(&mut vad, 0, 1).talking,
                "a pause for breath is not a stop"
            );
        }
        assert!(!run(&mut vad, 0, 1).talking, "and then it does stop");
    }

    #[test]
    fn a_change_is_reported_once() {
        // The wire message goes out on the change, not on every frame: an event per frame
        // would be five a second per player for as long as they speak.
        let mut vad = Vad::new(RATE, VadSettings::default());
        run(&mut vad, 10, 20);
        vad.finish_calibration();

        run(&mut vad, 255, 6);
        let steady = run(&mut vad, 255, 5);
        assert!(
            steady.talking && !steady.changed,
            "no change while it holds"
        );
    }

    #[test]
    fn the_counter_does_not_run_away() {
        // Bounded above, so a long sentence does not take thirty frames of silence to
        // fall back from — it takes the same as a short one.
        let mut vad = Vad::new(RATE, VadSettings::default());
        run(&mut vad, 10, 20);
        vad.finish_calibration();
        run(&mut vad, 255, 500);
        let quiet = run(&mut vad, 0, ACTIVITY_MAX as usize);
        assert!(!quiet.talking);
    }

    #[test]
    fn the_meter_is_zero_below_the_floor_and_rises_above_it() {
        let mut vad = Vad::new(RATE, VadSettings::default());
        run(&mut vad, 10, 20);
        vad.finish_calibration();
        assert_eq!(run(&mut vad, 0, 1).level, 0.0);
        assert!(run(&mut vad, 255, 1).level > 0.5);
    }
}
