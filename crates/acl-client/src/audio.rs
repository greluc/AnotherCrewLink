//! The devices, and the loop between them.
//!
//! `acl-audio` has every piece of this and none of the joining: the encoder, the decoder,
//! the jitter buffer, the mixer, the panner, the gain law and `voice_params` are all there
//! and all tested against golden vectors. What did not exist was the real-time loop that
//! puts a microphone at one end and a speaker at the other.
//!
//! # Two callbacks and a thread, and the callbacks may not think
//!
//! `cpal` calls into this from the operating system's audio threads. Those callbacks have a
//! deadline measured in milliseconds and no way to report missing it except by producing a
//! click, so what happens inside them is: copy, and hand over. Every decision — which peers
//! are audible, how loud, which side — is made on the frame loop and reaches the callbacks
//! as numbers that are already decided.
//!
//! That is why the gain and pan for each peer arrive through a lock the callback only ever
//! *tries*: a callback that blocked on the frame loop would be a callback that missed its
//! deadline because the window was busy drawing hats.
//!
//! # The reference signal, and why it flows the way it does
//!
//! The echo canceller has to be told what the speakers are about to play *before* it is
//! asked to clean what the microphone heard — the other order asks it to remove an echo of
//! something it has not been told about yet. So the output callback copies every frame it
//! plays into a queue, and the capture side drains that queue into `Apm::render` before
//! each `Apm::capture`.
//!
//! Both queues are behind locks the callbacks only ever *try*. A callback that blocked
//! would miss a deadline measured in milliseconds, and the worst case of not blocking is a
//! frame of reference the canceller did not get — which costs some cancellation for one
//! frame. Blocking costs a click.
//!
//! # Rates
//!
//! The pipeline works at 48 kHz because Opus does. A device that offers it is opened at it;
//! one that does not is opened at its own rate and resampled by `acl_audio::resample`,
//! which is `rubato` underneath. The alternative — refusing the device — is a client with
//! no microphone on hardware that has one.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use acl_audio::codec::{Encoder, FRAME_SAMPLES};
use acl_audio::jitter::JitterBuffer;
use acl_core::peers::Incoming;

/// How loud a peer is, and where.
///
/// The capture settings that are fixed when the microphone is opened.
///
/// Fixed, because that is what they are on the shipped client: `echoCancellation`,
/// `noiseSuppression` and `vadEnabled` are `getUserMedia` constraints there, given once and
/// changed by reopening the stream. `Settings.tsx` puts all three in the list that raises
/// its "unsaved" count, which is how it asks for the reconnect that applies them.
///
/// Here the reload button reopens the audio, which is the same bargain.
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one field per setting, for the reason `settings_screen` gives about its own:               packing them would hide which switch on the page each one is"
)]
pub(crate) struct Capture {
    /// Whether the echo canceller runs.
    pub(crate) echo_cancellation: bool,
    /// Whether the noise suppressor runs.
    pub(crate) noise_suppression: bool,
    /// Whether the voice detector runs at all.
    ///
    /// With it off nothing is reported to the lobby and the microphone is governed by the
    /// talk mode alone, which is what somebody switching it off is asking for.
    pub(crate) voice_detection: bool,
    /// Open the device at 48 kHz or not at all.
    ///
    /// `oldSampleDebug`, which asks `getUserMedia` for `sampleRate: 48000` outright rather
    /// than accepting what the device offers. The point of a debug switch like this is to
    /// take the resampler out of the picture when somebody is chasing a sound, so a device
    /// that cannot do 48 kHz is refused rather than quietly resampled -- which would leave
    /// the switch on and the thing it removes still there.
    pub(crate) fixed_rate: bool,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            echo_cancellation: true,
            noise_suppression: true,
            voice_detection: true,
            fixed_rate: false,
        }
    }
}

/// Where the voice detector's noise floor should sit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Floor {
    /// Whatever `VadSettings::default` says, which is what an unticked box means.
    Default,
    /// The level the player chose.
    At(f64),
}

impl Floor {
    /// The settings a detector should be rebuilt with.
    fn settings(self) -> acl_audio::vad::VadSettings {
        let mut settings = acl_audio::vad::VadSettings::default();
        if let Self::At(level) = self {
            settings.min_noise_level = level;
        }
        settings
    }
}

/// The capture settings that change while the microphone is open.
///
/// Atomics rather than a lock. These are read inside a real-time callback, and a callback
/// that waits on a mutex the settings screen is holding is a callback that misses its
/// deadline — which is heard as a click. The two `f32`/`f64` values are stored as their
/// bit patterns because there is no atomic float.
#[derive(Debug)]
pub(crate) struct Tuning {
    /// Input gain, as a multiplier. One is unchanged.
    gain: std::sync::atomic::AtomicU32,
    /// The voice detector's noise floor, or negative for "leave it alone".
    noise_floor: std::sync::atomic::AtomicU64,
    /// Bumped whenever `noise_floor` changes, so the callback knows to re-tune the
    /// detector rather than comparing floats every frame.
    generation: std::sync::atomic::AtomicU64,
    /// What the microphone last heard, nought to one, for the settings screen's meter.
    ///
    /// Written by the capture callback and read by the paint. `VadFrame::level` has carried
    /// this since P3+, documented as "for a meter", and nothing had ever read it.
    ///
    /// Negative means no microphone has reported anything, which draws an empty bar rather
    /// than a full one — a meter that reads maximum when nothing is listening is worse than
    /// one that reads nothing.
    level: std::sync::atomic::AtomicU32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            gain: std::sync::atomic::AtomicU32::new(1.0_f32.to_bits()),
            noise_floor: std::sync::atomic::AtomicU64::new((-1.0_f64).to_bits()),
            generation: std::sync::atomic::AtomicU64::new(0),
            level: std::sync::atomic::AtomicU32::new((-1.0_f32).to_bits()),
        }
    }
}

impl Tuning {
    /// Sets both, and says so if the floor moved.
    fn set(&self, gain: f32, noise_floor: Option<f64>) {
        use std::sync::atomic::Ordering;
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
        let wanted = noise_floor.unwrap_or(-1.0).to_bits();
        if self.noise_floor.swap(wanted, Ordering::Relaxed) != wanted {
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records what the microphone just heard.
    fn heard(&self, level: f32) {
        self.level
            .store(level.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// What it last heard, if anything has.
    fn level(&self) -> Option<f32> {
        let held = f32::from_bits(self.level.load(std::sync::atomic::Ordering::Relaxed));
        (held >= 0.0).then_some(held)
    }

    /// The gain to apply to the next frame.
    fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// The floor to re-tune to, if it has moved since `seen`.
    ///
    /// Three answers, so a named one: nothing changed, changed to a number, or changed back
    /// to the detector's own default.
    fn floor_if_moved(&self, seen: &mut u64) -> Option<Floor> {
        use std::sync::atomic::Ordering;
        let now = self.generation.load(Ordering::Relaxed);
        if now == *seen {
            return None;
        }
        *seen = now;
        let stored = f64::from_bits(self.noise_floor.load(Ordering::Relaxed));
        Some(if stored >= 0.0 {
            Floor::At(stored)
        } else {
            Floor::Default
        })
    }
}

/// The filter `voice_params` asked for, as one this crate can run.
///
/// Two `FilterKind`s exist and they are not the same type on purpose: one is a decision
/// about what a player should sound like, the other is a biquad. This is the one place they
/// meet.
#[cfg(feature = "audio")]
fn biquad_for(muffle: acl_audio::voice::Muffle) -> acl_audio::biquad::Biquad {
    let kind = match muffle.kind {
        acl_audio::voice::FilterKind::LowPass => acl_audio::biquad::FilterKind::LowPass,
        acl_audio::voice::FilterKind::HighPass => acl_audio::biquad::FilterKind::HighPass,
    };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a sample rate of 48 000, which is exact in an f32"
    )]
    acl_audio::biquad::Biquad::new(
        kind,
        muffle.frequency,
        muffle.q,
        acl_audio::stream::WANTED_RATE as f32,
    )
}

/// One per audible peer, replaced wholesale each frame. The gain is
/// `acl_audio::voice::voice_params`' answer — every rule about distance, walls, vision, the
/// dead and the vents is already in it — and the position is where the panner should put
/// them.
///
/// The position is kept as a position rather than as a left/right pair on purpose:
/// `acl_audio::panner::Panner` turns one into the other, with the distance model and the
/// equal-power law the Electron client's `PannerNode` uses, and it is tested against those.
/// Reducing it to two weights here would be reimplementing that badly.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Placement {
    /// Overall gain, after every rule that is not distance.
    pub(crate) gain: f32,
    /// Where they are, relative to the listener.
    pub(crate) source: acl_audio::panner::Position,
    /// The distance model, whose `max_distance` is the hearing range this frame.
    pub(crate) panner: acl_audio::panner::Panner,
    /// Whether to pan at all.
    ///
    /// `enableSpatialAudio` off means centred: the gain still falls with distance, because
    /// that is a different setting and turning off panning is not turning off distance.
    pub(crate) spatial: bool,
    /// The filter to put in this peer's path, if any.
    ///
    /// `voice_params` has decided this since the port began -- the vent's low pass, the
    /// camera's, the impostor radio's high pass -- and nothing carried it here, so nothing
    /// applied it. The *gain* half of those rules worked, which is what made it hard to
    /// notice: a player in a vent was quieter, and no more muffled than one standing next
    /// to you.
    pub(crate) muffle: Option<acl_audio::voice::Muffle>,
}

/// How deep the jitter buffer is, in packets.
///
/// Three, which is 60 ms at this frame size. `acl-audio`'s own tests measure what each
/// depth costs against recorded impairment; this is the shipped default and the number to
/// change if a real lobby says otherwise.
const JITTER_DEPTH: usize = 3;

/// The pipeline, as the window holds it.
pub(crate) struct Audio {
    /// Packets from the mesh, on their way to a decoder.
    incoming: Sender<Incoming>,
    /// Packets from the microphone, on their way to the mesh.
    outgoing: Receiver<Vec<u8>>,
    /// Transitions from the voice detector on the capture thread. See
    /// [`Self::take_voice_activity`].
    activity: Receiver<bool>,
    /// What each peer sounds like, read by the mixing thread.
    placements: Arc<Mutex<std::collections::BTreeMap<String, Placement>>>,
    /// The capture settings that change while it runs, read by the capture callback.
    tuning: Arc<Tuning>,
    /// What the speaker callback drains, so a test tone can be put into it.
    playing: Arc<Mutex<std::collections::VecDeque<f32>>>,
    /// Why there is no audio, when there is none.
    trouble: Option<String>,
    /// Kept alive: dropping a `cpal` stream stops it.
    _streams: Vec<Box<dyn std::any::Any + Send>>,
}

impl Audio {
    /// Opens the devices and starts the loop.
    ///
    /// A failure is not fatal and not silent. A client with no microphone is a client that
    /// can still hear, and one with no speaker can still be heard; both are worth saying
    /// out loud and neither is worth refusing to start over.
    pub(crate) fn start(capture: Capture) -> Self {
        let (incoming, packets) = std::sync::mpsc::channel::<Incoming>();
        let (encoded, outgoing) = std::sync::mpsc::channel::<Vec<u8>>();
        // Transitions only, which is what makes an unbounded channel safe here: somebody
        // talking produces two of these a sentence, not fifty a second.
        let (voice, activity) = std::sync::mpsc::channel::<bool>();
        let placements = Arc::new(Mutex::new(std::collections::BTreeMap::new()));
        let tuning = Arc::new(Tuning::default());
        // Made here rather than inside `open`, because the handle keeps a share of it: the
        // speaker test puts samples into the queue the mixer fills, which is what makes the
        // test go through the device the client is actually playing on.
        let playing: Arc<Mutex<std::collections::VecDeque<f32>>> =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));

        match Self::open(
            packets,
            &encoded,
            &voice,
            &placements,
            capture,
            &tuning,
            &playing,
        ) {
            Ok(streams) => Self {
                incoming,
                outgoing,
                activity,
                placements,
                tuning,
                playing,
                trouble: None,
                _streams: streams,
            },
            Err(why) => Self {
                incoming,
                outgoing,
                activity,
                placements,
                tuning,
                playing,
                trouble: Some(why),
                _streams: Vec::new(),
            },
        }
    }

    /// The settings that can change while the microphone is open.
    ///
    /// `gain` is `microphoneGain / 100` when the player enabled it and one when they did
    /// not. `noise_floor` is `micSensitivity` when *that* is enabled and `None` for the
    /// detector's own default.
    ///
    /// **The shipped client couples these and this does not.** `Voice.tsx` sets the gain
    /// only `if (!micSensitivityEnabled)`, so switching sensitivity on silently discards
    /// the gain the player set — two independent settings on the same page, one quietly
    /// disabling the other. Each is applied here for what its own label says it does.
    pub(crate) fn tune(&self, gain: f32, noise_floor: Option<f64>) {
        self.tuning.set(gain, noise_floor);
    }

    /// Hands one arrived packet to the decoder.
    pub(crate) fn receive(&self, packet: Incoming) {
        // A failed send means the mixing thread is gone, which is a client on its way out.
        let _ = self.incoming.send(packet);
    }

    /// Whether the detector changed its mind since the last call, and to what.
    ///
    /// `None` when nothing changed, which is almost every frame. Only the last transition
    /// is returned: if speech started and stopped between two paints, what the peers need is
    /// where it ended up — telling them it started would leave an indicator lit with nothing
    /// behind it.
    pub(crate) fn take_voice_activity(&self) -> Option<bool> {
        let mut last = None;
        while let Ok(speaking) = self.activity.try_recv() {
            last = Some(speaking);
        }
        last
    }

    /// Takes whatever the microphone has produced since the last call.
    ///
    /// Drained rather than blocked on: this is called from the frame loop, which has a
    /// window to draw.
    pub(crate) fn take_encoded(&self) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        while let Ok(packet) = self.outgoing.try_recv() {
            packets.push(packet);
        }
        packets
    }

    /// Replaces what every peer sounds like.
    ///
    /// Wholesale, once a frame. A peer who is not in the map is not mixed, which is how
    /// somebody who has gone out of range stops being heard without anything having to
    /// notice they went.
    pub(crate) fn place(&self, placements: std::collections::BTreeMap<String, Placement>) {
        if let Ok(mut held) = self.placements.lock() {
            *held = placements;
        }
    }

    /// What the microphone is hearing, for the settings screen's meter.
    ///
    /// `None` until a frame has been through the detector, which is also the answer when
    /// there is no microphone at all or the detector is switched off.
    pub(crate) fn input_level(&self) -> Option<f32> {
        self.tuning.level()
    }

    /// Plays a short sound through whichever speaker is in use.
    ///
    /// Into the same queue the mixer fills, so it goes through the device the client is
    /// actually playing on — which is the whole question the button answers. A separate
    /// stream opened for the test could succeed on a device the client is not using.
    ///
    /// Two notes and a fade, generated rather than shipped: an asset would be a file, a
    /// decoder and a licence for something the ear only has to recognise as "that worked".
    /// Whether a test tone is still playing.
    ///
    /// The queue the mixer fills is the same one the tone went into, so "still playing" is
    /// "there is more of it than the mixer has produced". Approximate on purpose: a queue
    /// with anything in it while nothing is being received is the tone, and the button only
    /// has to know whether to say start or stop.
    pub(crate) fn testing_speaker(&self) -> bool {
        self.playing.lock().is_ok_and(|playing| !playing.is_empty())
    }

    /// Stops one, by dropping what has not been played.
    pub(crate) fn stop_testing_speaker(&self) {
        if let Ok(mut playing) = self.playing.lock() {
            playing.clear();
        }
    }

    pub(crate) fn test_speaker(&self) {
        const A: f32 = 660.0;
        const E: f32 = 880.0;
        // 48 000 is exact in an `f32`, and a note of 8 640 samples is exact as a `usize`.
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a fixed sample rate and a fixed fraction of a second"
        )]
        let (rate, note) = {
            let rate = acl_audio::stream::WANTED_RATE as f32;
            (rate, (rate * 0.18) as usize)
        };

        let Ok(mut ready) = self.playing.lock() else {
            return;
        };
        for frequency in [A, E] {
            for sample in 0..note {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a sample index inside one short note"
                )]
                let time = sample as f32 / rate;
                // Down over the note, so it ends rather than stopping. A tone cut off at
                // full amplitude is a click, and a click through a speaker somebody is
                // testing reads as a fault.
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a sample index inside one short note"
                )]
                let fade = 1.0 - (sample as f32 / note as f32);
                ready.push_back(0.25 * fade * (std::f32::consts::TAU * frequency * time).sin());
            }
        }
    }

    /// Why there is no audio, if there is none.
    pub(crate) fn trouble(&self) -> Option<&str> {
        self.trouble.as_deref()
    }

    /// Opens both devices and starts the mixing thread.
    ///
    /// The order matters in one way: the mixing thread is started first, so a packet that
    /// arrives while a device is still opening has somewhere to go.
    #[cfg(feature = "audio")]
    fn open(
        packets: Receiver<Incoming>,
        encoded: &Sender<Vec<u8>>,
        voice: &Sender<bool>,
        placements: &Arc<Mutex<std::collections::BTreeMap<String, Placement>>>,
        capture: Capture,
        tuning: &Arc<Tuning>,
        ready: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    ) -> Result<Vec<Box<dyn std::any::Any + Send>>, String> {
        // What the mixing thread produces and the output callback consumes. A mutex the
        // callback only ever *tries*: blocking there would miss a deadline measured in
        // milliseconds because the mixer was busy, and a click is worse than a gap.
        let ready = Arc::clone(ready);
        // What the speakers played, on its way to the echo canceller.
        let played: Arc<Mutex<std::collections::VecDeque<f32>>> =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));

        let mixing = Arc::clone(&ready);
        let placements = Arc::clone(placements);
        std::thread::Builder::new()
            .name("mixer".to_owned())
            .spawn(move || mix(&packets, &mixing, &placements))
            .map_err(|error| format!("the mixer could not be started: {error}"))?;

        let host = cpal::default_host();
        // The speaker first, because a client that can hear is useful on its own and a
        // microphone failure should not cost it.
        let speaker = open_speaker(&host, &ready, &played)?;
        let microphone = open_microphone(&host, encoded, voice, &played, capture, tuning)?;
        Ok(vec![speaker, microphone])
    }

    /// Off Windows, or in a build without the audio feature, there are no devices.
    #[cfg(not(feature = "audio"))]
    fn open(
        _packets: Receiver<Incoming>,
        _encoded: &Sender<Vec<u8>>,
        _voice: &Sender<bool>,
        _placements: &Arc<Mutex<std::collections::BTreeMap<String, Placement>>>,
    ) -> Result<Vec<Box<dyn std::any::Any + Send>>, String> {
        Err("this build has no audio devices; enable the `audio` feature".to_owned())
    }
}

/// Opens the speaker: mixed frames out, and a copy of them for the canceller.
#[cfg(feature = "audio")]
fn open_speaker(
    host: &cpal::Host,
    ready: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    played: &Arc<Mutex<std::collections::VecDeque<f32>>>,
) -> Result<Box<dyn std::any::Any + Send>, String> {
    use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

    let output = host
        .default_output_device()
        .ok_or_else(|| "no output device".to_owned())?;
    let config = at_any_rate(&output, false)?;
    let channels = config.channels.max(1) as usize;
    let playing = Arc::clone(ready);
    let recording = Arc::clone(played);

    let stream = output
        .build_output_stream(
            config,
            move |buffer: &mut [f32], _: &_| {
                // Silence first, so every path out of here leaves a defined buffer -- an
                // untouched output buffer is whatever was in it last time, which is the
                // previous frame played again.
                buffer.fill(0.0);
                if let Ok(mut ready) = playing.try_lock() {
                    for slot in buffer.iter_mut() {
                        let Some(sample) = ready.pop_front() else {
                            break;
                        };
                        *slot = sample;
                    }
                }
                // A copy for the canceller, averaged to mono. Tried rather than waited on:
                // a frame of reference it did not get costs some cancellation for one
                // frame, and blocking here costs a click.
                if let Ok(mut played) = recording.try_lock() {
                    // Capped for the same reason the playback queue is: a capture stream
                    // that has stopped draining must not turn this into a queue that grows.
                    const CAP: usize = FRAME_SAMPLES * 25;
                    if played.len() < CAP {
                        for frame in buffer.chunks(channels) {
                            let sum: f32 = frame.iter().sum();
                            #[expect(
                                clippy::cast_precision_loss,
                                reason = "a channel count, which is one or two"
                            )]
                            played.push_back(sum / channels as f32);
                        }
                    }
                }
            },
            |error| acl_core::log_warn!("audio", "output stream: {error}"),
            None,
        )
        .map_err(|error| format!("the speaker could not be opened: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("the speaker would not start: {error}"))?;
    Ok(Box::new(stream))
}

/// Opens the microphone: resample, cancel the echo, encode, hand over.
#[cfg(feature = "audio")]
#[expect(
    clippy::too_many_lines,
    reason = "one function because it is one device: the encoder, the resampler, the               canceller and the detector are built in the order the callback uses them,               and the callback closes over all four. Splitting it would move the captures               into a struct and the reading order into two places"
)]
fn open_microphone(
    host: &cpal::Host,
    encoded: &Sender<Vec<u8>>,
    voice: &Sender<bool>,
    played: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    capture: Capture,
    tuning: &Arc<Tuning>,
) -> Result<Box<dyn std::any::Any + Send>, String> {
    use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

    let input = host
        .default_input_device()
        .ok_or_else(|| "no input device".to_owned())?;
    let config = at_any_rate(&input, true)?;
    if capture.fixed_rate && config.sample_rate != acl_audio::stream::WANTED_RATE {
        return Err(format!(
            "the microphone runs at {} Hz and the 48 kHz debug setting is on",
            config.sample_rate
        ));
    }
    let channels = config.channels.max(1) as usize;

    let mut opus = Encoder::new().map_err(|error| format!("the encoder refused: {error}"))?;
    // `None` when the device already runs at 48 kHz, which is the common case and the one
    // that should cost nothing.
    let mut rates = if config.sample_rate == acl_audio::stream::WANTED_RATE {
        None
    } else {
        Some(
            acl_audio::resample::Resampler::new(config.sample_rate, FRAME_SAMPLES)
                .map_err(|error| format!("no resampler for this device: {error}"))?,
        )
    };
    // The two stages the player can switch off. Both were hard-coded until 2026-08-27 --
    // cancellation on, suppression off -- and neither setting reached this line.
    let mut apm: Box<dyn acl_audio::apm::Apm> = Box::new(acl_audio::apm::Sonora::configured(
        capture.echo_cancellation,
        capture.noise_suppression,
    ));
    // A starting point rather than a measurement: the canceller refines it, and what it
    // needs is the right region. Two device buffers is what a default-sized `cpal` stream
    // comes to on Windows.
    apm.set_delay_ms(40);

    // The detector, reading frames the canceller has already cleaned. Before it, the
    // detector would hear the speakers and report the lobby talking as this player speaking
    // -- an indicator that lights up whenever anybody else does.
    //
    // `acl_audio::vad` and `acl_audio::analyser` have both existed since P3+ and neither had
    // a caller. The client emitted no `VAD` at all, so every other client saw this one as
    // permanently silent, and this one saw them the same way: `Link` parsed the event and
    // dropped it.
    let mut analyser =
        acl_audio::analyser::Analyser::new(acl_audio::vad::FFT_SIZE, acl_audio::vad::SMOOTHING);
    let mut detector = acl_audio::vad::Vad::new(
        f64::from(acl_audio::stream::WANTED_RATE),
        acl_audio::vad::VadSettings::default(),
    );
    // The detector learns the room's noise floor before it decides anything, and reports
    // nothing until it has. A second of frames, counted rather than timed: the callback has
    // no clock and every frame is the same length.
    let calibration_frames =
        (acl_audio::vad::NOISE_CAPTURE_MS / u64::from(acl_audio::codec::FRAME_MS)).max(1);
    let mut heard_frames = 0_u64;

    let reference = Arc::clone(played);
    let tuning = Arc::clone(tuning);
    // What generation of the sensitivity setting the detector was last built for.
    let mut tuned_for = 0_u64;
    let detecting = capture.voice_detection;
    let voice = voice.clone();
    let encoded = encoded.clone();
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME_SAMPLES * 2);
    let mut converted: Vec<f32> = Vec::new();
    let mut packet = Vec::new();

    let stream = input
        .build_input_stream(
            config,
            move |buffer: &[f32], _: &_| {
                // Down to mono here rather than later: the encoder is mono, and carrying
                // two channels as far as the encoder only to average them there is twice
                // the memory for the same answer.
                let mut mono: Vec<f32> = Vec::with_capacity(buffer.len());
                for frame in buffer.chunks(channels) {
                    let sum: f32 = frame.iter().sum();
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a channel count, which is one or two"
                    )]
                    mono.push(sum / channels as f32);
                }

                // `microphoneGain`, which is a `GainNode` on the shipped client and sits in
                // the same place: after the source and before everything that reads it, so
                // the canceller, the detector and the encoder all see one signal. Clamped,
                // because the setting goes to 300 per cent and three times a loud sample is
                // not a sample.
                let gain = tuning.gain();
                if (gain - 1.0).abs() > f32::EPSILON {
                    for sample in &mut mono {
                        *sample = (*sample * gain).clamp(-1.0, 1.0);
                    }
                }

                match rates.as_mut() {
                    Some(rates) => {
                        converted.clear();
                        if rates.push(&mono, &mut converted).is_ok() {
                            pending.extend_from_slice(&converted);
                        }
                    }
                    None => pending.extend_from_slice(&mono),
                }

                while pending.len() >= FRAME_SAMPLES {
                    let mut frame: Vec<f32> = pending.drain(..FRAME_SAMPLES).collect();

                    // What the speakers played, before what the microphone heard. The other
                    // order asks the canceller to remove an echo of something it has not
                    // been told about yet.
                    if let Ok(mut played) = reference.try_lock() {
                        while played.len() >= FRAME_SAMPLES {
                            let render: Vec<f32> = played.drain(..FRAME_SAMPLES).collect();
                            let _ = apm.render(&render);
                        }
                    }
                    let _ = apm.capture(&mut frame);

                    // On the same frame the encoder gets, before it gets it. The
                    // detector consumes nothing, and reading it here is what guarantees the
                    // two are looking at identical samples.
                    //
                    // Skipped entirely when the player switched the detector off: with
                    // `vadEnabled` false nothing should be measuring them, and reporting a
                    // level from a detector nobody asked for is the opposite of off.
                    if detecting {
                        // `micSensitivity`, which the shipped client applies live and then
                        // re-runs its calibration. Same here: a floor learned against a
                        // threshold that is no longer in force is worse than no floor.
                        if let Some(floor) = tuning.floor_if_moved(&mut tuned_for) {
                            detector.retune(floor.settings());
                            heard_frames = 0;
                        }
                        analyser.push(&frame);
                        let heard = detector.push(&analyser);
                        heard_frames += 1;
                        if heard_frames == calibration_frames {
                            detector.finish_calibration();
                        }
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "a meter reading between nought and one"
                        )]
                        tuning.heard(heard.level as f32);
                        if heard.changed && voice.send(heard.talking).is_err() {
                            return;
                        }
                    }

                    packet.clear();
                    if encode_frame(&mut opus, &frame, &mut packet).is_ok()
                        && encoded.send(packet.clone()).is_err()
                    {
                        // Nobody is listening any more, which is a client on its way out.
                        // Stop trying rather than filling a queue nothing drains.
                        return;
                    }
                }
            },
            // Printed rather than swallowed. Measured on a real machine: exactly one
            // underrun, at start-up, and none in the twenty seconds after -- the first
            // callback pays for the encoder's and the canceller's warm-up while the device
            // is already running. Worth knowing about, and worth not mistaking for the
            // recurring kind, which would sound like a stutter rather than like nothing.
            |error| acl_core::log_warn!("audio", "input stream: {error}"),
            None,
        )
        .map_err(|error| format!("the microphone could not be opened: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("the microphone would not start: {error}"))?;
    Ok(Box::new(stream))
}

/// A configuration for a device, at 48 kHz if it offers it and at its own rate if not.
///
/// `acl_audio::stream::choose` makes the choice — it is tested without a sound card — and
/// this translates `cpal`'s types into what it takes and back. A device that cannot give
/// 48 kHz is opened at what it has and resampled, because refusing it would be a client
/// with no microphone on hardware that has one.
#[cfg(feature = "audio")]
fn at_any_rate(device: &cpal::Device, input: bool) -> Result<cpal::StreamConfig, String> {
    use cpal::traits::DeviceTrait as _;

    let configs: Vec<cpal::SupportedStreamConfigRange> = if input {
        device
            .supported_input_configs()
            .map_err(|error| error.to_string())?
            .collect()
    } else {
        device
            .supported_output_configs()
            .map_err(|error| error.to_string())?
            .collect()
    };

    // The choosing itself is `acl_audio::stream::choose`, which is tested without a sound
    // card. This translates `cpal`'s types into what it takes and back.
    let supported: Vec<acl_audio::stream::Supported> = configs
        .iter()
        .filter(|range| range.sample_format() == cpal::SampleFormat::F32)
        .map(|range| acl_audio::stream::Supported {
            min_rate: range.min_sample_rate(),
            max_rate: range.max_sample_rate(),
            channels: range.channels(),
            buffer_frames: match range.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
                cpal::SupportedBufferSize::Unknown => None,
            },
        })
        .collect();

    let chosen = acl_audio::stream::choose(&supported, if input { 1 } else { 2 })
        .map_err(|error| error.to_string())?;
    Ok(cpal::StreamConfig {
        channels: chosen.channels,
        sample_rate: chosen.rate,
        buffer_size: cpal::BufferSize::Default,
    })
}

/// The mixing thread: packets in, stereo frames out.
///
/// Everything expensive happens here rather than in a callback. It keeps one [`Listener`]
/// per peer, pulls a frame from each buffer that has one, places it with the numbers the
/// frame loop last handed over, and sums them.
///
/// It runs until the channel closes, which is when the window drops the pipeline.
#[cfg(feature = "audio")]
fn mix(
    packets: &Receiver<Incoming>,
    ready: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    placements: &Arc<Mutex<std::collections::BTreeMap<String, Placement>>>,
) {
    use acl_audio::mixer::Mixer;

    let mut listeners: std::collections::BTreeMap<String, Listener> =
        std::collections::BTreeMap::new();
    let mut mixer = Mixer::new(FRAME_SAMPLES);
    let mut mono = vec![0.0_f32; FRAME_SAMPLES];
    // One filter per peer that has one, with the settings it was built from so a change
    // can be told from a repeat.
    let mut muffles: std::collections::BTreeMap<
        String,
        (acl_audio::voice::Muffle, acl_audio::biquad::Biquad),
    > = std::collections::BTreeMap::new();
    let mut stereo = vec![0.0_f32; FRAME_SAMPLES * 2];

    loop {
        // Blocks until something arrives, then takes everything else that has. A frame is
        // produced per round, which is what keeps this in step with the packets rather
        // than with a timer that would drift against them.
        let Ok(first) = packets.recv() else {
            return;
        };
        let mut arrived = vec![first];
        while let Ok(next) = packets.try_recv() {
            arrived.push(next);
        }
        for packet in arrived {
            let listener = match listeners.entry(packet.peer.clone()) {
                std::collections::btree_map::Entry::Occupied(held) => held.into_mut(),
                std::collections::btree_map::Entry::Vacant(empty) => {
                    // libopus refusing a decoder for a configuration this fixed would be
                    // remarkable, and it is still not a reason to stop the thread that
                    // every other peer's audio goes through.
                    let Ok(listener) = Listener::new() else {
                        continue;
                    };
                    empty.insert(listener)
                }
            };
            listener.push(&packet);
        }

        let placed = placements
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default();
        // A filter is state: it remembers the last two samples. So it lives here, beside
        // the decoder it filters, rather than in `Placement` -- which is rebuilt from the
        // window's thread every frame and would reset the filter with it, turning a
        // continuous low pass into a click every twenty milliseconds.
        muffles.retain(|peer, _| placed.get(peer).is_some_and(|p| p.muffle.is_some()));
        mixer.begin();
        let mut anything = false;
        for (peer, listener) in &mut listeners {
            let Ok(true) = listener.next_frame(&mut mono) else {
                continue;
            };
            let placement = placed.get(peer).copied().unwrap_or_default();
            if placement.gain <= 0.0 {
                // Out of range, dead, behind a wall -- whatever the rule was, it was
                // applied on the frame loop and the answer is silence. Not mixing is
                // cheaper than mixing zero and sounds the same.
                continue;
            }
            // The gain first, then the panner, which is the order the Electron graph has:
            // a `GainNode` into a `PannerNode`. Reversing them is audible, because the
            // distance model is not linear in the gain.
            for sample in &mut mono {
                *sample *= placement.gain;
            }
            // After the gain and before the panner, which is where `Voice.tsx` puts it:
            // `applyEffect(gain, muffle, destination)` inserts it between the two. The
            // panner here runs after the gain rather than before it, and that changes
            // nothing -- a gain is a scalar, and so is each side of an equal-power pan, so
            // filtering the mono signal is the same signal either way.
            if let Some(wanted) = placement.muffle {
                let filter = match muffles.entry(peer.clone()) {
                    std::collections::btree_map::Entry::Occupied(held) => {
                        let held = held.into_mut();
                        // Rebuilt only when the shape changes. Rebuilding every frame
                        // would throw away the two samples of history that make it a
                        // filter rather than a gain.
                        if held.0 != wanted {
                            *held = (wanted, biquad_for(wanted));
                        }
                        &mut held.1
                    }
                    std::collections::btree_map::Entry::Vacant(empty) => {
                        &mut empty.insert((wanted, biquad_for(wanted))).1
                    }
                };
                filter.process_block(&mut mono);
            }
            let source = if placement.spatial {
                placement.source
            } else {
                // Centred, and still at its distance: turning off panning is not turning
                // off the distance model.
                acl_audio::panner::Position {
                    x: 0.0,
                    y: 0.0,
                    z: -placement.source.length(),
                }
            };
            let panned = placement.panner.process_block(&mono, source);
            // Copied rather than sliced: `process_block` returns two samples per input and
            // `stereo` is sized for exactly that, so these agree -- but a length that is
            // asserted by construction is one a later change can break silently, and this
            // does not panic when it does.
            stereo.fill(0.0);
            for (slot, sample) in stereo.iter_mut().zip(panned.iter()) {
                *slot = *sample;
            }
            mixer.add(&stereo);
            anything = true;
        }
        if !anything {
            continue;
        }
        let finished = mixer.finish();
        if let Ok(mut ready) = ready.lock() {
            // A cap, because a speaker that has stopped consuming must not turn into a
            // queue that grows without limit. Two hundred milliseconds is well past what
            // any device buffers and well short of a memory problem.
            const CAP: usize = FRAME_SAMPLES * 2 * 10;
            if ready.len() < CAP {
                ready.extend(finished.iter().copied());
            }
        }
    }
}

/// One peer's receiving end.
///
/// A jitter buffer and nothing else: `acl_audio::jitter::JitterBuffer` owns the decoder,
/// because ordering and decoding are one decision — a packet's redundancy can rebuild the
/// frame *before* it, so what to decode depends on what arrived, and splitting them puts
/// that choice in two places.
///
/// A wrapper rather than a bare `JitterBuffer` so the two calls this makes have names that
/// say what they are for, and so the depth is decided once.
pub(crate) struct Listener {
    buffer: JitterBuffer,
}

impl Listener {
    /// A listener for one peer.
    ///
    /// # Errors
    ///
    /// If libopus refuses a decoder, which it does not for a configuration this fixed.
    pub(crate) fn new() -> Result<Self, acl_audio::codec::CodecError> {
        Ok(Self {
            buffer: JitterBuffer::new(JITTER_DEPTH)?,
        })
    }

    /// Takes one arrived packet.
    pub(crate) fn push(&mut self, packet: &Incoming) {
        self.buffer.push(packet.sequence, &packet.payload);
    }

    /// The next frame of samples, if the buffer has one ready.
    ///
    /// `None` means the buffer is still filling, which is what it is for: a frame played
    /// early is a frame played from a packet that had not arrived.
    ///
    /// # Errors
    ///
    /// If the decoder refuses a packet, which means the packet is not Opus.
    pub(crate) fn next_frame(
        &mut self,
        into: &mut [f32],
    ) -> Result<bool, acl_audio::codec::CodecError> {
        let Some(frame) = self.buffer.pop()? else {
            return Ok(false);
        };
        into.fill(0.0);
        for (slot, sample) in into.iter_mut().zip(frame.samples.iter()) {
            *slot = *sample;
        }
        Ok(true)
    }
}

/// One frame of microphone audio, encoded.
///
/// Free-standing so the capture callback stays a copy and a hand-over: everything that can
/// fail or take time happens here, on a thread that is allowed to.
///
/// # Errors
///
/// If libopus refuses the frame, which it does for a frame that is not exactly
/// [`FRAME_SAMPLES`] long.
pub(crate) fn encode_frame(
    encoder: &mut Encoder,
    samples: &[f32],
    into: &mut Vec<u8>,
) -> Result<usize, acl_audio::codec::CodecError> {
    debug_assert_eq!(
        samples.len(),
        FRAME_SAMPLES,
        "the encoder takes exactly one frame"
    );
    encoder.encode(samples, into)
}

#[cfg(test)]
mod tests {

    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    /// The floor only counts as moved when it moves.
    ///
    /// The callback rebuilds the detector on every reported move, and rebuilding restarts
    /// its calibration — so a generation that ticked on every frame would leave the
    /// microphone permanently in its first second, never deciding anything.
    #[test]
    fn re_tuning_is_reported_once_per_change() {
        let tuning = super::Tuning::default();
        let mut seen = 0;
        // The first read is a change: the default is "leave it alone", and the callback has
        // not been told anything yet.
        tuning.set(1.0, Some(0.2));
        assert_eq!(
            tuning.floor_if_moved(&mut seen),
            Some(super::Floor::At(0.2))
        );
        assert_eq!(tuning.floor_if_moved(&mut seen), None, "nothing moved");

        // The same value again is not a move, even though it was stored again.
        tuning.set(2.0, Some(0.2));
        assert_eq!(tuning.floor_if_moved(&mut seen), None);

        // Turning the setting off is a move, back to the detector's own default.
        tuning.set(2.0, None);
        assert_eq!(
            tuning.floor_if_moved(&mut seen),
            Some(super::Floor::Default)
        );
    }

    /// The gain survives the round trip through its bit pattern.
    #[test]
    fn the_gain_is_the_gain() {
        let tuning = super::Tuning::default();
        assert!((tuning.gain() - 1.0).abs() < f32::EPSILON, "one by default");
        tuning.set(3.0, None);
        assert!((tuning.gain() - 3.0).abs() < f32::EPSILON);
    }

    /// Only `Floor::At` moves the threshold; the default leaves it where the detector puts
    /// it.
    #[test]
    fn a_default_floor_changes_nothing() {
        let plain = acl_audio::vad::VadSettings::default();
        assert_eq!(super::Floor::Default.settings(), plain);
        let moved = super::Floor::At(0.42).settings();
        assert!((moved.min_noise_level - 0.42).abs() < f64::EPSILON);
        assert!((moved.max_noise_level - plain.max_noise_level).abs() < f64::EPSILON);
    }

    use super::{Audio, Listener, Placement, encode_frame};
    use acl_audio::codec::{Encoder, FRAME_SAMPLES};
    use acl_core::peers::Incoming;

    fn packet(sequence: u16, payload: Vec<u8>) -> Incoming {
        Incoming {
            peer: "somebody".to_owned(),
            sequence,
            timestamp: u32::from(sequence) * 960,
            payload,
        }
    }

    fn one_encoded_frame() -> Vec<u8> {
        let mut encoder = Encoder::new().expect("an encoder");
        let mut packet = Vec::new();
        encode_frame(&mut encoder, &vec![0.0; FRAME_SAMPLES], &mut packet).expect("a packet");
        packet
    }

    /// A packet in, a frame out -- once the buffer has enough to start. The buffer filling
    /// first is what it is for: a frame played early is one played from a packet that had
    /// not arrived.
    #[test]
    fn packets_become_frames_once_the_buffer_has_filled() {
        let mut listener = Listener::new().expect("a listener");
        let encoded = one_encoded_frame();
        let mut samples = vec![0.0_f32; FRAME_SAMPLES];

        assert!(
            !listener.next_frame(&mut samples).expect("no decode error"),
            "a frame came out of an empty buffer"
        );

        for sequence in 0..8 {
            listener.push(&packet(sequence, encoded.clone()));
        }
        let mut produced = 0;
        for _ in 0..8 {
            if listener.next_frame(&mut samples).expect("no decode error") {
                produced += 1;
            }
        }
        assert!(produced > 0, "nothing came out of a full buffer");
        assert_eq!(samples.len(), FRAME_SAMPLES, "the frame changed length");
    }

    /// Out-of-order arrival is the jitter buffer's whole job, and it must not reach the
    /// decoder as such: Opus decoded out of order sounds like a bad connection rather than
    /// like the packets it was given.
    #[test]
    fn packets_arriving_out_of_order_still_produce_frames() {
        let mut listener = Listener::new().expect("a listener");
        let encoded = one_encoded_frame();
        for sequence in [3_u16, 1, 0, 2, 5, 4] {
            listener.push(&packet(sequence, encoded.clone()));
        }
        let mut samples = vec![0.0_f32; FRAME_SAMPLES];
        let mut produced = 0;
        for _ in 0..6 {
            if listener.next_frame(&mut samples).expect("no decode error") {
                produced += 1;
            }
        }
        assert!(produced > 0, "shuffled packets produced nothing");
    }

    /// The encoder takes exactly one frame, and a caller that hands it anything else is a
    /// caller whose buffering is wrong -- which is worth catching in a debug build rather
    /// than turning into a silent `WrongFrameSize` at run time.
    #[test]
    fn a_frame_is_exactly_one_frame() {
        let mut encoder = Encoder::new().expect("an encoder");
        let mut into = Vec::new();
        assert!(encode_frame(&mut encoder, &vec![0.0; FRAME_SAMPLES], &mut into).is_ok());
        assert!(!into.is_empty(), "the encoder produced nothing");
    }

    /// A client starts either way, and says which way it went.
    ///
    /// On a machine with a sound card the devices open and there is nothing to report; on
    /// one without -- a CI runner, a remote session -- there is a reason. Both are working
    /// clients: one with no microphone can still hear, one with no speaker can still be
    /// heard, and neither is worth refusing to start over.
    ///
    /// This was written against a build that opened nothing, and failed the moment the
    /// devices were real. What it should assert is the invariant rather than the outcome.
    #[test]
    fn a_client_starts_with_devices_or_without_and_says_which() {
        let audio = Audio::start(super::Capture::default());
        // `None` means the devices opened, and nothing here plays anything -- that would
        // put a noise on the machine of whoever ran the tests.
        if let Some(why) = audio.trouble() {
            assert!(!why.is_empty(), "a reason with nothing in it");
        }
        // Either way the channels work, so nothing above has to special-case silence.
        audio.receive(packet(0, vec![1, 2, 3]));
        let _ = audio.take_encoded();
        audio.place(
            [(
                "somebody".to_owned(),
                Placement {
                    gain: 0.5,
                    ..Placement::default()
                },
            )]
            .into_iter()
            .collect(),
        );
    }

    /// The whole chain, without a sound card: encode, carry, order, decode, place, mix.
    ///
    /// Every piece of this is tested on its own in `acl-audio`. What this checks is that
    /// they are joined the right way round -- which is the only thing this module adds and
    /// the only thing those tests cannot see.
    #[test]
    fn a_frame_survives_the_whole_chain() {
        use acl_audio::mixer::Mixer;

        // A tone rather than silence: silence would survive a pipeline that dropped
        // everything, which is the failure most worth catching here.
        let mut encoder = Encoder::new().expect("an encoder");
        let tone: Vec<f32> = (0..FRAME_SAMPLES)
            .map(|at| {
                #[expect(clippy::cast_precision_loss, reason = "a sample index under 1000")]
                let phase = at as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU;
                phase.sin() * 0.5
            })
            .collect();

        let mut listener = Listener::new().expect("a listener");
        for sequence in 0..8_u16 {
            let mut opus = Vec::new();
            encode_frame(&mut encoder, &tone, &mut opus).expect("a packet");
            listener.push(&Incoming {
                peer: "somebody".to_owned(),
                sequence,
                timestamp: u32::from(sequence) * 960,
                payload: opus,
            });
        }

        let mut mono = vec![0.0_f32; FRAME_SAMPLES];
        let mut got_one = false;
        for _ in 0..8 {
            if listener.next_frame(&mut mono).expect("no decode error") {
                got_one = true;
                break;
            }
        }
        assert!(got_one, "nothing came out of the jitter buffer");
        let loudest = mono.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
        assert!(
            loudest > 0.05,
            "the decoded frame is silent: peak {loudest}"
        );

        // Placed to the right of the listener, so the right channel should be the louder.
        let placement = Placement {
            gain: 1.0,
            source: acl_audio::panner::Position {
                x: 2.0,
                y: 0.0,
                z: 0.0,
            },
            panner: acl_audio::panner::Panner::default(),
            spatial: true,
            muffle: None,
        };
        let panned = placement.panner.process_block(&mono, placement.source);
        let (mut left, mut right) = (0.0_f32, 0.0_f32);
        for pair in panned.as_chunks::<2>().0 {
            left = left.max(pair[0].abs());
            right = right.max(pair[1].abs());
        }
        assert!(
            right > left,
            "a source on the right came out louder on the left: {left} against {right}"
        );

        let mut mixer = Mixer::new(FRAME_SAMPLES);
        mixer.begin();
        mixer.add(&panned);
        let summed = mixer.finish();
        let peak = summed.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
        assert!(peak > 0.0, "the mixer produced silence from a tone");
    }
}
