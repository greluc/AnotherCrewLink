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
#[derive(Clone, Debug)]
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
    /// The microphone the player picked, by id, or empty for the system default.
    ///
    /// Stored, translated, shown in a picker, and read by nothing until 2026-08-29: both
    /// devices were always the Windows default. A player on a USB headset picked it, saw
    /// it selected, and talked into their laptop's array microphone.
    pub(crate) microphone: String,
    /// The speaker, the same way.
    pub(crate) speaker: String,
    /// What the picker showed when the microphone was chosen.
    ///
    /// Windows changes a device's id when the same headset moves to a different port, and
    /// the name survives it. `microphoneLabel` exists for that and was written by nothing,
    /// so the recovery 1.x has did not exist here: the id stopped matching and the client
    /// silently fell back to the default.
    pub(crate) microphone_label: String,
    /// The same, for the speaker.
    pub(crate) speaker_label: String,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            echo_cancellation: true,
            noise_suppression: true,
            voice_detection: true,
            microphone: String::new(),
            speaker: String::new(),
            microphone_label: String::new(),
            speaker_label: String::new(),
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
        let mut settings = acl_audio::vad::VadSettings {
            // Unconditionally, because `Voice.tsx:1092` does it unconditionally: it
            // writes `maxNoiseLevel = 1` on the line after it writes the floor, whether
            // or not the player enabled the slider. `MAX_NOISE_LEVEL = 0.7` is `vad.ts`'s
            // own default and 1.x overrides it every time, so the port matched a number
            // the client it copies never uses -- and the mismatch was fatal rather than
            // cosmetic, because the slider stores up to 1.0 and a floor above the ceiling
            // used to abort the process from inside the capture callback.
            max_noise_level: 1.0,
            ..acl_audio::vad::VadSettings::default()
        };
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
    /// Whether the microphone should be sending at all.
    ///
    /// Read inside the capture callback, one frame before the packet is made, and that is
    /// the whole of a fix from 2026-08-29. The gate used to be applied in the window's
    /// paint, over a batch of frames that had already been encoded and queued -- and the
    /// paint runs at five hertz whenever the pointer is not over the window, which is the
    /// whole time anybody is playing.
    ///
    /// So releasing push-to-talk cut up to two hundred milliseconds off the end of the
    /// word, and pressing it sent up to two hundred milliseconds of whatever was in the
    /// buffer *before* the press -- the room, the game, the sentence not meant for the
    /// lobby. Twenty milliseconds is the smallest that error can be, and this makes it
    /// that.
    transmitting: std::sync::atomic::AtomicBool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            gain: std::sync::atomic::AtomicU32::new(1.0_f32.to_bits()),
            noise_floor: std::sync::atomic::AtomicU64::new((-1.0_f64).to_bits()),
            generation: std::sync::atomic::AtomicU64::new(0),
            level: std::sync::atomic::AtomicU32::new((-1.0_f32).to_bits()),
            // Silent until something says otherwise. A client that transmitted while the
            // switches were still being read would send the first frames of every session
            // from a muted microphone.
            transmitting: std::sync::atomic::AtomicBool::new(false),
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

    /// Opens or closes the microphone.
    ///
    /// Called from whatever polls the keys, at whatever rate it polls them; read once per
    /// twenty-millisecond frame by the capture callback.
    pub(crate) fn transmit(&self, on: bool) {
        self.transmitting
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether the capture callback should be handing frames over.
    fn transmitting(&self) -> bool {
        self.transmitting.load(std::sync::atomic::Ordering::Relaxed)
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

/// Whether the peer's unaltered voice still reaches the mix.
///
/// `Voice.tsx` inserts an effect with `applyEffect`, which connects the effect to the
/// destination *and* disconnects `gain -> destination` -- and tolerates the disconnect
/// failing, because another effect may have done it already. What comes out of that is a
/// graph of parallel branches: every connected effect reaches the destination, and the
/// direct path reaches it only while no effect has taken it away.
///
/// So the reverb does not replace the muffle and the muffle does not replace the reverb. A
/// dead impostor holding the radio is high-passed *and* haunting, and is heard through both
/// at once. Reading it as a chain, or as one-or-the-other, are the two ways to get this
/// wrong, and both produce sound.
#[cfg(feature = "audio")]
const fn direct_path_survives(muffled: bool, reverb_applied: bool) -> bool {
    muffled || !reverb_applied
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
    /// Whether this peer is a ghost an impostor is haunted by.
    ///
    /// `voice_params` has decided this for as long as it has decided the muffle, and it was
    /// thrown away for the same reason: nothing carried it. The gain half of the rule worked
    /// -- `ghostVolumeAsImpostor` was applied, and walls stopped blocking the ghost -- so a
    /// haunting ghost was audible at the right volume, in the same dry room as everybody
    /// else, and the setting looked like it was doing its job.
    pub(crate) reverb: bool,
}

/// How much audio the mixer tries to keep in front of the speaker.
///
/// Three frames, which is sixty milliseconds. Enough that a late burst of packets does not
/// leave the speaker with nothing to play, and short enough that it is not heard as delay --
/// it sits on top of the jitter buffer's own sixty, and the two together are what a player
/// experiences as the lag between somebody speaking and being heard.
#[cfg(feature = "audio")]
const TARGET_DEPTH: usize = FRAME_SAMPLES * 2 * 3;

/// The most frames one round will produce before going back for more packets.
///
/// A bound rather than a target. If the speaker has fallen a long way behind -- the machine
/// was asleep, the device stalled -- catching up in one go would mean a long burst of mixing
/// with the packet channel unattended. Ten frames is two hundred milliseconds of catching
/// up per round, which closes any real gap in a few rounds.
#[cfg(feature = "audio")]
const MOST_AT_ONCE: usize = 10;

/// How deep the jitter buffer is, in packets.
///
/// Three, which is 60 ms at this frame size. `acl-audio`'s own tests measure what each
/// depth costs against recorded impairment; this is the shipped default and the number to
/// change if a real lobby says otherwise.
const JITTER_DEPTH: usize = 3;

/// The pipeline, as the window holds it.
pub(crate) struct Audio {
    /// Transitions from the voice detector on the capture thread. See
    /// [`Self::take_voice_activity`].
    activity: Receiver<bool>,
    /// What each peer sounds like, read by the mixing thread.
    placements: Arc<Mutex<std::collections::BTreeMap<String, Placement>>>,
    /// The capture settings that change while it runs, read by the capture callback.
    tuning: Arc<Tuning>,
    /// The test tone, on its own.
    ///
    /// It used to go into `playing`, and "is a tone playing" was "is that queue not empty" --
    /// which is also true whenever anybody is *talking*, because the mixer fills the same
    /// queue. So the button's label flickered between start and stop several times a second
    /// as the queue filled and drained, and pressing stop threw away everybody's audio
    /// rather than the tone.
    tone: Arc<Mutex<std::collections::VecDeque<f32>>>,
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
    ///
    /// `packets` and `sink` are the two ends of the media path, and neither belongs to the
    /// window. Arriving audio comes straight from the signalling worker; encoded frames go
    /// straight back to it from the capture callback. Until 2026-08-29 both travelled
    /// through `eframe`'s `update`, whose floor is two hundred milliseconds when the
    /// pointer is not over the window -- fifty packets a second delivered ten at a time,
    /// five times a second, in each direction. `receive` and `take_encoded` are gone rather
    /// than merely unused: a method that exists is one something can start calling again.
    pub(crate) fn start(
        capture: &Capture,
        packets: Receiver<Incoming>,
        sink: &crate::net::AudioSink,
    ) -> Self {
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
        let tone: Arc<Mutex<std::collections::VecDeque<f32>>> =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));
        // Three hundred transforms to cut the reverb's impulse response into blocks, which
        // is a tenth of a second the mixing loop does not have -- it has twenty
        // milliseconds. Done here, on a thread of its own, so it is ready long before the
        // first ghost; until it is, `reverb::ready` says no and the mixer leaves the dry
        // path alone, which is what `Voice.tsx` does with a convolver whose buffer has not
        // arrived.
        std::thread::spawn(|| {
            if !acl_audio::reverb::warm() {
                acl_core::log_warn!(
                    "audio",
                    "the reverb impulse response did not load; haunting ghosts will be dry"
                );
            }
        });

        match Self::open(
            packets,
            sink,
            &voice,
            &placements,
            capture,
            &tuning,
            &playing,
            &tone,
        ) {
            // `trouble` now carries a *partial* failure as well as a total one: the
            // speaker is open and the microphone is not, so there is something to say and
            // something still to hear.
            Ok((streams, trouble)) => Self {
                activity,
                placements,
                tuning,
                tone,
                trouble,
                _streams: streams,
            },
            Err(why) => Self {
                activity,
                placements,
                tuning,
                tone,
                trouble: Some(why),
                _streams: Vec::new(),
            },
        }
    }

    /// The knobs the capture callback reads.
    ///
    /// Handed out so the switch watcher can write the microphone gate straight into the
    /// callback rather than through the window's paint. See `Tuning::transmitting`.
    pub(crate) fn tuning(&self) -> &Arc<Tuning> {
        &self.tuning
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
        self.tone.lock().is_ok_and(|tone| !tone.is_empty())
    }

    /// Stops one, by dropping what has not been played.
    ///
    /// Only the tone. It shared a queue with the mixer until 2026-08-28, and stopping the
    /// test used to clear that -- taking every peer's audio with it.
    pub(crate) fn stop_testing_speaker(&self) {
        if let Ok(mut tone) = self.tone.lock() {
            tone.clear();
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

        let Ok(mut ready) = self.tone.lock() else {
            return;
        };
        ready.clear();
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
                // Twice, because the queue the speaker reads is interleaved stereo. Pushed
                // once, the tone played across the two channels at double speed and half
                // the length -- audible as a blip rather than as the two notes it is.
                let value = 0.25 * fade * (std::f32::consts::TAU * frequency * time).sin();
                ready.push_back(value);
                ready.push_back(value);
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
    #[expect(
        clippy::too_many_arguments,
        reason = "one function because it opens one machine's audio: the two devices share             the reference queue between them, and splitting it would put the order they             are opened in -- speaker first, so a microphone failure costs nothing -- in             two places"
    )]
    fn open(
        packets: Receiver<Incoming>,
        sink: &crate::net::AudioSink,
        voice: &Sender<bool>,
        placements: &Arc<Mutex<std::collections::BTreeMap<String, Placement>>>,
        capture: &Capture,
        tuning: &Arc<Tuning>,
        ready: &Arc<Mutex<std::collections::VecDeque<f32>>>,
        tone: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    ) -> Result<Opened, String> {
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
        //
        // That was written here from the start and was not true until 2026-08-29: the
        // speaker was opened first and then `?` on the microphone threw the whole `Vec`
        // away, so the stream dropped and stopped. A player whose microphone was busy,
        // missing, or at a rate `choose` could not use heard *nobody*, when 1.x leaves
        // them listening and merely unable to speak.
        let speaker = open_speaker(&host, &ready, tone, &played, capture)?;
        match open_microphone(&host, sink, voice, &played, capture, tuning) {
            Ok(microphone) => Ok((vec![speaker, microphone], None)),
            Err(why) => {
                acl_core::log_warn!("audio", "no microphone: {why}");
                Ok((vec![speaker], Some(why)))
            }
        }
    }

    /// Off Windows, or in a build without the audio feature, there are no devices.
    #[cfg(not(feature = "audio"))]
    fn open(
        _packets: Receiver<Incoming>,
        _sink: &crate::net::AudioSink,
        _voice: &Sender<bool>,
        _placements: &Arc<Mutex<std::collections::BTreeMap<String, Placement>>>,
    ) -> Result<Opened, String> {
        Err("this build has no audio devices; enable the `audio` feature".to_owned())
    }
}

/// Opens the speaker: mixed frames out, and a copy of them for the canceller.
#[cfg(feature = "audio")]
fn open_speaker(
    host: &cpal::Host,
    ready: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    tone: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    played: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    capture: &Capture,
) -> Result<Box<dyn std::any::Any + Send>, String> {
    use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

    // The device the player picked, falling back to the system default. `find` matches on
    // the stored id first and on the stored name second, which is the recovery Windows
    // needs: it changes a device's id when the same headset moves to another port.
    let output = acl_audio::device::system::Cpal::new()
        .find(
            acl_audio::device::Direction::Output,
            &capture.speaker,
            &capture.speaker_label,
        )
        .or_else(|| host.default_output_device())
        .ok_or_else(|| "no output device".to_owned())?;
    let config = at_any_rate(&output, false)?;
    let channels = config.channels.max(1) as usize;
    let playing = Arc::clone(ready);
    let testing = Arc::clone(tone);
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
                    lay_out(&mut ready, buffer, channels);
                }
                // On top of whatever is being said, not instead of it. The point of the
                // button is to prove this device makes a sound, and a test that silenced
                // the lobby to do it would be answering a different question.
                if let Ok(mut tone) = testing.try_lock() {
                    lay_out(&mut tone, buffer, channels);
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

/// What opening the devices produced: the streams to hold open, and what went wrong.
///
/// A pair rather than a `Result`, because the two halves are independent. The speaker
/// failing is fatal and comes back as the `Err`; the microphone failing is not, and comes
/// back here beside a speaker that is still playing.
type Opened = (Vec<Box<dyn std::any::Any + Send>>, Option<String>);

/// Lays interleaved-stereo samples into a device buffer, whatever the device's layout.
///
/// Added rather than assigned, because the buffer is zeroed first and the test tone plays
/// on top of the mix; for the mix itself the two are the same thing.
///
/// **This is the whole of a fix made on 2026-08-29.** The callback used to write one
/// queued sample per slot and never consult `channels`:
///
/// ```text
/// for slot in buffer.iter_mut() { *slot = ready.pop_front()...; }
/// ```
///
/// `cpal`'s WASAPI backend reports exactly one channel count per device -- the endpoint's
/// mix format -- so `at_any_rate` returns 6 on a 5.1 endpoint, 8 on 7.1, and 1 on a
/// Bluetooth hands-free one. The queue was then consumed at three, four or half the rate
/// the mixer fills it, and every voice played at that speed. The repository already had
/// this failure written down for the test tone a few hundred lines up -- *"pushed once,
/// the tone played across the two channels at double speed and half the length"* -- and
/// the voice path had the same mistake.
///
/// Two at a time or none, because the queue is interleaved stereo: taking a single sample
/// out of a pair would swap left and right for every frame after it, permanently.
///
/// A device with more than two channels gets the pair in slots 0 and 1 and silence
/// elsewhere. Under WASAPI those two are front-left and front-right for every layout
/// there is, which is where stereo content belongs.
#[cfg(feature = "audio")]
fn lay_out(queue: &mut std::collections::VecDeque<f32>, buffer: &mut [f32], channels: usize) {
    if channels == 0 {
        return;
    }
    for frame in buffer.chunks_mut(channels) {
        if queue.len() < 2 {
            break;
        }
        let (Some(left), Some(right)) = (queue.pop_front(), queue.pop_front()) else {
            break;
        };
        if channels == 1 {
            // The average, not one side: a mono headset that played only the left channel
            // would drop half of a spatialised lobby, and the pan is what puts a player
            // there in the first place.
            if let Some(slot) = frame.first_mut() {
                *slot += f32::midpoint(left, right);
            }
            continue;
        }
        if let Some(slot) = frame.first_mut() {
            *slot += left;
        }
        if let Some(slot) = frame.get_mut(1) {
            *slot += right;
        }
    }
}

/// Opens the microphone: resample, cancel the echo, encode, hand over.
#[cfg(feature = "audio")]
#[expect(
    clippy::too_many_lines,
    reason = "one function because it is one device: the encoder, the resampler, the               canceller and the detector are built in the order the callback uses them,               and the callback closes over all four. Splitting it would move the captures               into a struct and the reading order into two places"
)]
fn open_microphone(
    host: &cpal::Host,
    sink: &crate::net::AudioSink,
    voice: &Sender<bool>,
    played: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    capture: &Capture,
    tuning: &Arc<Tuning>,
) -> Result<Box<dyn std::any::Any + Send>, String> {
    use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

    let input = acl_audio::device::system::Cpal::new()
        .find(
            acl_audio::device::Direction::Input,
            &capture.microphone,
            &capture.microphone_label,
        )
        .or_else(|| host.default_input_device())
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
        // Twenty milliseconds *at the device's rate*, not 960 frames regardless of it.
        // `FRAME_SAMPLES` is twenty milliseconds at 48 kHz and is a different duration at
        // any other: at 16 kHz it is sixty milliseconds, so one chunk in produced three
        // frames out and the loop below emitted all three in a single burst. Only the
        // first got a far-end reference, because the reference queue is drained once per
        // frame -- so the echo canceller ran blind for two frames out of three, and the
        // other end heard their own voice come back.
        //
        // `stream::Chosen::buffer_frames` computes the same figure for the device buffer,
        // and the two agreeing is what makes one callback one frame.
        let chunk = usize::try_from(
            u64::from(config.sample_rate) * u64::from(acl_audio::codec::FRAME_MS) / 1000,
        )
        .unwrap_or(FRAME_SAMPLES)
        .max(1);
        Some(
            acl_audio::resample::Resampler::new(config.sample_rate, chunk)
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
    let sink = sink.clone();
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME_SAMPLES * 2);
    let mut converted: Vec<f32> = Vec::new();
    // Reused, so draining the reference queue allocates nothing after the first frame.
    let mut reference_frames: Vec<f32> = Vec::with_capacity(FRAME_SAMPLES * 4);
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
                    //
                    // Drained under the lock and rendered outside it, which is a fix of
                    // 2026-08-29. The lock used to be held across `apm.render` -- a
                    // multi-millisecond adaptive filter -- and the *output* callback only
                    // ever `try_lock`s it, so every buffer the speaker produced while the
                    // canceller was working was thrown away rather than waited for. The
                    // canceller was starved of exactly the reference it was busy using.
                    // A lock is held for a copy here and for nothing else.
                    reference_frames.clear();
                    if let Ok(mut played) = reference.try_lock() {
                        let whole = (played.len() / FRAME_SAMPLES) * FRAME_SAMPLES;
                        reference_frames.extend(played.drain(..whole));
                    }
                    for render in reference_frames.as_chunks::<FRAME_SAMPLES>().0 {
                        let _ = apm.render(render);
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

                    // `microphoneGain`, which is a `GainNode` on the shipped client --
                    // and it sits *here*, not further up. `Voice.tsx:1070` builds the
                    // detector on `source`, while `source.connect(microphoneGain)` at
                    // :1061 sends the gained copy on to the destination: the detector
                    // reads the microphone before the gain and the encoder reads it after.
                    //
                    // This applied it before both until 2026-08-29, which quietly coupled
                    // two settings that have nothing to do with each other. The detector
                    // learns the room's noise floor from what it hears, so turning the
                    // input gain up raised the floor with it and the threshold moved out
                    // from under the player -- a microphone that opened at a whisper
                    // before the change needing a raised voice after it, for a setting
                    // whose label says nothing about sensitivity.
                    //
                    // Clamped, because the setting goes to 300 per cent and three times a
                    // loud sample is not a sample. After the canceller too, which is the
                    // better place for it either way: an adaptive filter has an easier job
                    // on the signal its microphone actually produced.
                    let gain = tuning.gain();
                    if (gain - 1.0).abs() > f32::EPSILON {
                        for sample in &mut frame {
                            *sample = (*sample * gain).clamp(-1.0, 1.0);
                        }
                    }

                    // The gate, on the frame it applies to. Everything above it still
                    // runs while the microphone is closed -- the canceller keeps its
                    // convergence, the detector keeps its floor, and the meter on the
                    // settings page keeps moving so a muted player can still see that
                    // their microphone works. What stops is the sending.
                    //
                    // It used to be applied in the window's paint, over a batch of frames
                    // already encoded and queued, and the paint runs at five hertz when
                    // the pointer is not over the window. So a push-to-talk release cut up
                    // to two hundred milliseconds off the end of the word and a press sent
                    // up to two hundred milliseconds of what came before it.
                    if !tuning.transmitting() {
                        continue;
                    }

                    packet.clear();
                    if encode_frame(&mut opus, &frame, &mut packet).is_ok()
                        && !sink.send(packet.clone())
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
    // One convolver per haunted peer, and one buffer for what comes out of it. Both are
    // state for the same reason the filters above are: a convolver is three seconds of
    // history, and rebuilding it every frame would be rebuilding the room every frame.
    let mut reverbs: std::collections::BTreeMap<String, acl_audio::reverb::Reverb> =
        std::collections::BTreeMap::new();
    let mut wet = vec![0.0_f32; FRAME_SAMPLES * 2];
    let mut said_it_was_not_loaded = false;
    // Where the delay is, once a second.
    //
    // Two testers measured this client two to three seconds behind TeamSpeak running beside
    // it. Every buffer in this path can be named and its size argued from the code, and that
    // arithmetic comes to about seven hundred milliseconds -- so something holds the rest,
    // and an evening of reasoning has not found it. These numbers say which queue it is, for
    // the cost of one line a second.
    let mut reported = std::time::Instant::now();
    let mut frames_made = 0_u32;

    loop {
        if !take_packets(packets, &mut listeners) {
            return;
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
        // Dropped the moment the decision stops asking for it, which is what cuts the tail.
        // That is `restoreEffect`: a ghost who stops haunting -- the round ends, the
        // impostor dies, the ghost is revived -- has their reverb disconnected in the
        // Electron client too, and the tail goes with it. What does *not* cut it is the gain
        // reaching zero, which is why a peer out of range keeps a placement for as long as
        // the reverb is connected. See where the placements are built.
        //
        // Two cases here are a tail rather than the same tail. `Voice.tsx` restores on
        // `!other.isDead || state !== TASKS || !me.isImpostor || me.isDead`, which is *not*
        // the negation of the four conditions that connected it -- `haunting` is missing
        // from the list. So a host who switches haunting off mid-round leaves the Electron
        // client with a convolver that is still connected and now fed a gain of zero, and it
        // rings out; this drops it and stops it. The same goes for deafening, which never
        // reaches a placement at all. Both are silent either way -- the rule that turns the
        // reverb off is also the rule that sets the gain to zero -- so what differs is three
        // seconds of decay after a switch is flicked, and modelling it would mean carrying a
        // "still connected" flag that no longer answers to any rule.
        reverbs.retain(|peer, _| placed.get(peer).is_some_and(|p| p.reverb));

        // How far behind the speaker is, in frames. Everything below runs that many times
        // rather than once: the decision above is the same for all of them -- it is
        // recomputed five times a second and describes where people are, not what the next
        // twenty milliseconds sound like -- so it is read once and the mixing repeats.
        for _ in 0..frames_wanted(ready) {
            mixer.begin();
            // Two questions, and they were one variable until 2026-08-28.
            //
            // `heard` is whether any peer handed over a frame. `mixed` is whether any of
            // those frames went into the mix. They differ exactly when somebody is speaking
            // and inaudible -- out of range, behind a wall, in a menu, between rounds -- and
            // `next_frame` has already *taken* their packet by the time the gain says so.
            //
            // Ending a round on the second therefore stopped it after one packet per peer,
            // while fifty a second kept arriving. Nothing in the jitter buffer evicts, so it
            // grew by about a third of a second of audio for every second nobody was
            // audible, and then froze there once somebody was: at the right speed, at the
            // right pitch, and two seconds late for the rest of the session. Two testers
            // measured two to three seconds and that is where it was.
            let mut heard = false;
            let mut anything_to_hear = false;
            for (peer, listener) in &mut listeners {
                let Ok(true) = listener.next_frame(&mut mono) else {
                    continue;
                };
                // Before the gain is consulted, because the packet is already out of the
                // buffer. What follows decides whether it is *played*, never whether it was
                // taken.
                heard = true;
                let placement = placed.get(peer).copied().unwrap_or_default();
                if placement.gain <= 0.0 && !placement.reverb {
                    // Out of range, dead, behind a wall -- whatever the rule was, it was
                    // applied on the frame loop and the answer is silence. Not mixing is
                    // cheaper than mixing zero and sounds the same.
                    //
                    // Unless a reverb is connected, in which case zero is not silence: the
                    // convolver still has three seconds of this peer in it, and feeding it the
                    // zeroes is what lets the tail ring out rather than stop dead.
                    continue;
                }
                // The gain first, then the panner, which is the order the Electron graph has:
                // a `GainNode` into a `PannerNode`. Reversing them is audible, because the
                // distance model is not linear in the gain.
                for sample in &mut mono {
                    *sample *= placement.gain;
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

                // The wet branch is taken here, from the gain and *before* the muffle. In
                // `Voice.tsx` both effects hang off the same gain node and each connects to the
                // destination, so the two are branches that get summed -- not a chain. A dead
                // impostor holding the radio is heard through both at once.
                let heard_wet = placement.reverb
                    && haunt(
                        peer,
                        &mono,
                        &placement,
                        source,
                        &mut reverbs,
                        &mut wet,
                        &mut said_it_was_not_loaded,
                    );
                if heard_wet {
                    mixer.add(&wet);
                    anything_to_hear = true;
                }

                // The direct path, and it is only there when nothing replaced it: `applyEffect`
                // disconnects `gain -> destination` as it connects an effect. So a peer with a
                // muffle is heard through the muffle, a peer with only the reverb is heard
                // through the reverb alone, and a peer with neither is heard as they are.
                if direct_path_survives(placement.muffle.is_some(), heard_wet) {
                    // After the gain and before the panner, which is where `Voice.tsx` puts it:
                    // `applyEffect(gain, muffle, destination)` inserts it between the two. The
                    // panner here runs after the gain rather than before it, and that changes
                    // nothing -- a gain is a scalar, and so is each side of an equal-power pan,
                    // so filtering the mono signal is the same signal either way.
                    if let Some(wanted) = placement.muffle {
                        muffle_for(peer, wanted, &mut muffles).process_block(&mut mono);
                    }
                    let panned = placement.panner.process_block(&mono, source);
                    // Copied rather than sliced: `process_block` returns two samples per input
                    // and `stereo` is sized for exactly that, so these agree -- but a length
                    // that is asserted by construction is one a later change can break
                    // silently, and this does not panic when it does.
                    stereo.fill(0.0);
                    for (slot, sample) in stereo.iter_mut().zip(panned.iter()) {
                        *slot = *sample;
                    }
                    mixer.add(&stereo);
                    anything_to_hear = true;
                }
            }
            if anything_to_hear {
                frames_made += 1;
            }
            if !heard {
                // Nobody had a frame to give, so there is nothing left to catch up on and
                // another round would only ask the same question again. On `heard` and not
                // and not on the other: a round that took packets and played none has still
                // done work, and stopping there is what let the backlog build.
                break;
            }
            if !anything_to_hear {
                // Every peer was silent to us this round. Their packets are consumed -- that
                // is the point -- but there is nothing to hand the speaker, and a frame of
                // digital silence in the queue would be a frame of latency for the next
                // person who does speak.
                continue;
            }
            hand_over(ready, mixer.finish());
        }

        if reported.elapsed() >= std::time::Duration::from_secs(1) {
            reported = std::time::Instant::now();
            report_depth(&listeners, ready, frames_made);
            frames_made = 0;
        }
    }
}

/// Puts one finished frame where the speaker will find it.
///
/// Capped, because a speaker that has stopped consuming must not turn its queue into one
/// that grows without limit. Two hundred milliseconds is well past what any device buffers
/// and well short of a memory problem.
#[cfg(feature = "audio")]
fn hand_over(ready: &Arc<Mutex<std::collections::VecDeque<f32>>>, finished: &[f32]) {
    const CAP: usize = FRAME_SAMPLES * 2 * 10;
    if let Ok(mut ready) = ready.lock()
        && ready.len() < CAP
    {
        ready.extend(finished.iter().copied());
    }
}

/// How many frames the speaker still wants.
///
/// The speaker is the clock: it drains its queue at real time whether or not a packet
/// arrived, so this is the difference between what it holds and what it should. At least
/// one, so a round always does something, and never more than [`MOST_AT_ONCE`] -- catching
/// up a long gap in a single round would mix for a quarter of a second with the packet
/// channel unattended.
#[cfg(feature = "audio")]
fn frames_wanted(ready: &Arc<Mutex<std::collections::VecDeque<f32>>>) -> usize {
    let depth = ready.lock().map_or(0, |queue| queue.len());
    (TARGET_DEPTH.saturating_sub(depth) / (FRAME_SAMPLES * 2)).clamp(1, MOST_AT_ONCE)
}

/// Says how far behind the pipeline is, once a second.
///
/// Two testers measured this client two to three seconds behind `TeamSpeak`, running
/// beside it as a reference.
/// Every buffer in the path can be named and its size argued from the code, and that
/// arithmetic comes to about seven hundred milliseconds -- so something holds the rest, and
/// an evening of reasoning has not found it.
///
/// `held` is the one queue in the path with no capacity limit: the jitter buffers' map keeps
/// whatever arrives, and `depth` decides only when playback starts. `queued` is what is
/// waiting for the speaker. `made` should be fifty a second -- fewer is a mixer falling
/// behind, more is one catching up -- and between the three of them the next two-machine
/// test settles in one run what deduction could not.
#[cfg(feature = "audio")]
fn report_depth(
    listeners: &std::collections::BTreeMap<String, Listener>,
    ready: &Arc<Mutex<std::collections::VecDeque<f32>>>,
    made: u32,
) {
    let held: usize = listeners.values().map(Listener::waiting).sum();
    // Interleaved stereo, so half the samples are one ear's worth of time.
    let queued = ready.lock().map_or(0, |queue| queue.len() / 2);
    // Only while there is something to say. A quiet client would otherwise write a line a
    // second for as long as it runs, and a log nobody can page through is a log nobody reads.
    if held == 0 && made == 0 {
        return;
    }
    let frame_ms = acl_audio::codec::FRAME_MS as usize;
    let rate = (acl_audio::codec::SAMPLE_RATE as usize).max(1);
    acl_core::log_info!(
        "audio",
        "behind by {} ms in the jitter buffers ({held} packets) and {} ms at the speaker ({queued} samples); {made} frames made",
        held * frame_ms,
        queued * 1000 / rate
    );
}

/// This peer's filter, built if they had none and rebuilt if the shape changed.
///
/// Kept between frames because a biquad is two samples of history, and rebuilding one every
/// frame turns a continuous low pass into a click every twenty milliseconds.
#[cfg(feature = "audio")]
fn muffle_for<'a>(
    peer: &str,
    wanted: acl_audio::voice::Muffle,
    muffles: &'a mut std::collections::BTreeMap<
        String,
        (acl_audio::voice::Muffle, acl_audio::biquad::Biquad),
    >,
) -> &'a mut acl_audio::biquad::Biquad {
    match muffles.entry(peer.to_owned()) {
        std::collections::btree_map::Entry::Occupied(held) => {
            let held = held.into_mut();
            if held.0 != wanted {
                *held = (wanted, biquad_for(wanted));
            }
            &mut held.1
        }
        std::collections::btree_map::Entry::Vacant(empty) => {
            &mut empty.insert((wanted, biquad_for(wanted))).1
        }
    }
}

/// One haunting ghost through the reverb, into `into`, and whether anything came out.
///
/// `false` means the impulse response has not finished loading, and the caller should leave
/// this peer's direct path where it is. That is `Voice.tsx`'s own answer to a convolver with
/// no buffer: it declines to connect the effect and says so, because a `ConvolverNode`
/// without a response emits silence rather than passing audio through, and routing a voice
/// into one makes that player inaudible.
#[cfg(feature = "audio")]
fn haunt(
    peer: &str,
    mono: &[f32],
    placement: &Placement,
    source: acl_audio::panner::Position,
    reverbs: &mut std::collections::BTreeMap<String, acl_audio::reverb::Reverb>,
    into: &mut [f32],
    said_it_was_not_loaded: &mut bool,
) -> bool {
    let Some(response) = acl_audio::reverb::ready() else {
        if !*said_it_was_not_loaded {
            // Once. `Voice.tsx` warns per player per frame, and this thread would say it
            // fifty times a second.
            *said_it_was_not_loaded = true;
            acl_core::log_warn!(
                "audio",
                "a ghost is haunting before the impulse response finished loading, so they are dry for now"
            );
        }
        return false;
    };

    let convolver = match reverbs.entry(peer.to_owned()) {
        std::collections::btree_map::Entry::Occupied(held) => held.into_mut(),
        std::collections::btree_map::Entry::Vacant(empty) => {
            empty.insert(acl_audio::reverb::Reverb::new(response))
        }
    };
    convolver.process(mono, into);

    // The convolver sits after the panner in the Electron graph, so what it returns is
    // already placed. Panning a mono source is one scalar per side, so applying the two
    // afterwards is the same signal -- and it has to be afterwards here, because the
    // convolver's two sides came through different halves of the response and are no longer
    // the same signal to pan.
    let (left, right) = placement.panner.gains(source);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "narrowing a pair of gains back to the sample format"
    )]
    let (left, right) = (left as f32, right as f32);
    for [on_the_left, on_the_right] in into.as_chunks_mut::<2>().0 {
        *on_the_left *= left;
        *on_the_right *= right;
    }
    true
}

/// Puts everything that has arrived into its peer's jitter buffer.
///
/// Waits up to one frame for the first packet and then takes the rest without waiting.
/// `false` means the channel has closed, which is the window dropping the pipeline.
///
/// The wait has a limit because **the packets are not a clock**. They arrive in bursts --
/// the window hands them over five times a second -- and the mixer used to block here and
/// then produce exactly one frame per burst. Ten packets in, one frame out, is a tenth of
/// real time: two people heard each other slowed down, stuttering, and further behind with
/// every second. The speaker is the clock, and the caller tops its queue up whether or not
/// anything arrived, so this must come back either way.
#[cfg(feature = "audio")]
fn take_packets(
    packets: &Receiver<Incoming>,
    listeners: &mut std::collections::BTreeMap<String, Listener>,
) -> bool {
    let frame = std::time::Duration::from_millis(u64::from(acl_audio::codec::FRAME_MS));
    let mut arrived = Vec::new();
    match packets.recv_timeout(frame) {
        Ok(first) => arrived.push(first),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return false,
    }
    while let Ok(next) = packets.try_recv() {
        arrived.push(next);
    }

    for packet in arrived {
        let listener = match listeners.entry(packet.peer.clone()) {
            std::collections::btree_map::Entry::Occupied(held) => held.into_mut(),
            std::collections::btree_map::Entry::Vacant(empty) => {
                // libopus refusing a decoder for a configuration this fixed would be
                // remarkable, and it is still not a reason to stop the thread that every
                // other peer's audio goes through.
                let Ok(listener) = Listener::new() else {
                    continue;
                };
                empty.insert(listener)
            }
        };
        listener.push(&packet);
    }
    true
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
    /// How many packets are waiting to be played.
    ///
    /// The jitter buffer's map has no capacity limit -- `depth` decides when playback starts,
    /// not how much is retained -- so this is the one queue in the path that can grow with
    /// nothing stopping it. Reported once a second by the mixing loop, because a delay
    /// somebody can hear ought to be a delay somebody can read.
    pub(crate) fn waiting(&self) -> usize {
        self.buffer.held()
    }
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

    /// The two `FilterKind`s do not get crossed on the way between them.
    ///
    /// A transposition here is silent: both shapes are filters, both change the sound, and
    /// a vent that high-passes still sounds like *something* happened. The last time two
    /// enums were mapped across a boundary in this project it was red and blue, and that
    /// shipped.
    #[test]
    fn the_decisions_filter_shape_survives_the_crossing() {
        use acl_audio::voice::{FilterKind, Muffle};

        // A low pass keeps a low tone and takes a high one away; a high pass does the
        // reverse. Asserted on what the filter *does*, because the kinds are different
        // types and there is nothing to compare directly.
        for (kind, keeps, removes) in [
            (FilterKind::LowPass, 300.0_f32, 9000.0_f32),
            (FilterKind::HighPass, 9000.0, 300.0),
        ] {
            let mut filter = super::biquad_for(Muffle {
                kind,
                frequency: 2000.0,
                q: 0.7,
            });
            let mut kept = tone(keeps);
            filter.process_block(&mut kept);
            let mut filter = super::biquad_for(Muffle {
                kind,
                frequency: 2000.0,
                q: 0.7,
            });
            let mut gone = tone(removes);
            filter.process_block(&mut gone);

            assert!(
                peak(&kept) > peak(&gone) * 3.0,
                "{kind:?} kept {:.4} of {keeps} Hz and {:.4} of {removes} Hz",
                peak(&kept),
                peak(&gone)
            );
        }
    }

    /// The four ways a peer's path can be wired, and which of them keep the dry signal.
    ///
    /// The whole table, because the interesting rows are the ones nobody pictures. Two
    /// effects at once is a real state -- a dead impostor holding the radio is haunting and
    /// high-passed -- and the direct path being gone while *neither* effect is audible is
    /// how a player goes silent for no visible reason.
    #[test]
    fn an_effect_takes_the_direct_path_and_two_effects_do_not_take_it_twice() {
        use super::direct_path_survives;

        // Nothing in the way: the voice is heard as it is.
        assert!(direct_path_survives(false, false));
        // The muffle carries it, so the direct path is gone and nothing is lost.
        assert!(direct_path_survives(true, false));
        // The reverb carries it, and only the reverb: a haunting ghost is heard wet, not
        // wet *and* dry, which would be a ghost mixed with themself.
        assert!(!direct_path_survives(false, true));
        // Both, in parallel. The direct path is gone once, not twice, and both branches
        // reach the mix -- reading this as a chain would put the reverb through the radio's
        // high pass, and reading it as one-or-the-other would silence one of them.
        assert!(direct_path_survives(true, true));
    }

    /// A sine at a frequency, one frame long, after the filter has settled.
    fn tone(hertz: f32) -> Vec<f32> {
        #[expect(clippy::cast_precision_loss, reason = "48 000 is exact in an f32")]
        let rate = acl_audio::stream::WANTED_RATE as f32;
        // Three frames, so the measurement below is taken after the filter's own transient
        // rather than during it.
        (0..FRAME_SAMPLES * 3)
            .map(|at| {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a sample index inside three frames"
                )]
                let time = at as f32 / rate;
                (std::f32::consts::TAU * hertz * time).sin()
            })
            .collect()
    }

    /// The loudest sample in the last third, which is after the transient.
    fn peak(samples: &[f32]) -> f32 {
        samples[samples.len() * 2 / 3..]
            .iter()
            .fold(0.0_f32, |so_far, s| so_far.max(s.abs()))
    }

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

    /// Only `Floor::At` moves the threshold, and the ceiling is 1.0 either way.
    #[test]
    fn the_ceiling_is_raised_whatever_the_floor_is() {
        let plain = acl_audio::vad::VadSettings::default();
        let default = super::Floor::Default.settings();
        let moved = super::Floor::At(0.42).settings();

        assert!((moved.min_noise_level - 0.42).abs() < f64::EPSILON);
        assert!((default.min_noise_level - plain.min_noise_level).abs() < f64::EPSILON);

        // Both, and 1.0 rather than the detector's own 0.7. `Voice.tsx:1092` writes
        // `maxNoiseLevel = 1` unconditionally, so `MAX_NOISE_LEVEL` is a default that 1.x
        // overrides every time and never actually applies. Asserted for `Default` too,
        // because the slider stores a floor of up to 1.0 and a ceiling below it used to
        // abort the process from inside the capture callback.
        assert!((default.max_noise_level - 1.0).abs() < f64::EPSILON);
        assert!((moved.max_noise_level - 1.0).abs() < f64::EPSILON);
        assert!(moved.max_noise_level > moved.min_noise_level);
    }

    /// Every device layout `at_any_rate` can return, against the queue it is fed.
    ///
    /// The four rows are the four things `cpal` reports on real Windows hardware: an
    /// ordinary stereo endpoint, a Bluetooth hands-free one at a single channel, and 5.1
    /// and 7.1, which is what a machine plugged into a television or an AV receiver
    /// reports. Before 2026-08-29 the callback ignored the count and wrote one queued
    /// sample per slot, so the last two consumed the mix three and four times too fast
    /// and the first one consumed it at half speed.
    #[cfg(feature = "audio")]
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "every value is an exact binary fraction written in this test, so the
                  sums are exact and an approximate comparison would hide the very
                  off-by-one-slot mistake being checked for"
    )]
    fn a_stereo_pair_reaches_every_layout_at_the_right_speed() {
        use std::collections::VecDeque;

        // One second of "audio" is unnecessary; four stereo frames say everything.
        let source: Vec<f32> = vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0];

        // Stereo: straight through, and four device frames consume all four.
        let mut queue: VecDeque<f32> = source.iter().copied().collect();
        let mut buffer = vec![0.0f32; 8];
        super::lay_out(&mut queue, &mut buffer, 2);
        assert_eq!(buffer, source);
        assert!(
            queue.is_empty(),
            "eight slots at two channels is four frames"
        );

        // Mono: the average of the pair, and four device frames still consume all four.
        let mut queue: VecDeque<f32> = source.iter().copied().collect();
        let mut buffer = vec![0.0f32; 4];
        super::lay_out(&mut queue, &mut buffer, 1);
        assert_eq!(
            buffer,
            vec![0.0, 0.0, 0.0, 0.0],
            "equal and opposite averages to nil"
        );
        assert!(queue.is_empty(), "four slots at one channel is four frames");

        // 5.1: the pair in front left and front right, silence in the other four. Four
        // device frames is twenty-four slots, and it must still take exactly four pairs.
        let mut queue: VecDeque<f32> = source.iter().copied().collect();
        let mut buffer = vec![0.0f32; 24];
        super::lay_out(&mut queue, &mut buffer, 6);
        assert_eq!(buffer[0], 1.0);
        assert_eq!(buffer[1], -1.0);
        assert_eq!(&buffer[2..6], &[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(buffer[18], 4.0);
        assert_eq!(buffer[19], -4.0);
        assert!(
            queue.is_empty(),
            "twenty-four slots at six channels is four frames"
        );

        // 7.1, which is where the old code was worst: it drained the whole queue into the
        // first frame.
        let mut queue: VecDeque<f32> = source.iter().copied().collect();
        let mut buffer = vec![0.0f32; 32];
        super::lay_out(&mut queue, &mut buffer, 8);
        assert_eq!(buffer[0], 1.0);
        assert_eq!(buffer[24], 4.0);
        assert!(
            queue.is_empty(),
            "thirty-two slots at eight channels is four frames"
        );
    }

    /// A queue with one sample left is left alone rather than half-consumed.
    #[cfg(feature = "audio")]
    #[test]
    fn an_odd_sample_never_swaps_the_channels() {
        use std::collections::VecDeque;

        // The mixer always hands over whole stereo frames, so this cannot arise today.
        // It is asserted anyway because the consequence is silent and permanent: taking
        // the left of a pair and leaving the right makes every later frame play right
        // where left should be, for the rest of the session, with nothing to show for it.
        let mut queue: VecDeque<f32> = [1.0, -1.0, 9.0].into_iter().collect();
        let mut buffer = vec![0.0f32; 8];
        super::lay_out(&mut queue, &mut buffer, 2);
        assert_eq!(&buffer[..2], &[1.0, -1.0]);
        assert_eq!(queue.len(), 1, "the lone sample waits for its partner");
        assert_eq!(queue.front().copied(), Some(9.0));
    }

    /// The test tone is added to the mix, not substituted for it.
    #[cfg(feature = "audio")]
    #[test]
    fn the_tone_plays_over_the_lobby_rather_than_instead_of_it() {
        use std::collections::VecDeque;

        // Every value here is an exact binary fraction, so the sums are exact too and
        // comparing them is a comparison rather than an approximation.
        let mut buffer = vec![0.0f32; 4];
        let mut voices: VecDeque<f32> = [0.5, 0.25, 0.5, 0.25].into_iter().collect();
        let mut tone: VecDeque<f32> = [0.125, 0.0625, 0.125, 0.0625].into_iter().collect();
        super::lay_out(&mut voices, &mut buffer, 2);
        super::lay_out(&mut tone, &mut buffer, 2);
        assert_eq!(buffer, vec![0.625, 0.3125, 0.625, 0.3125]);
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
        // The two ends of the media path belong to the signalling worker, not to `Audio`.
        // A `Link` is not started here -- this is a unit test and there is nothing to
        // signal to -- so the ends are made directly, which is what `Link::start` does.
        let (link, packets) = crate::net::Link::start();
        let audio = Audio::start(&super::Capture::default(), packets, &link.audio_sink());
        // `None` means the devices opened, and nothing here plays anything -- that would
        // put a noise on the machine of whoever ran the tests.
        if let Some(why) = audio.trouble() {
            assert!(!why.is_empty(), "a reason with nothing in it");
        }
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
            reverb: false,
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
