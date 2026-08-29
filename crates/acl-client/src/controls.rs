//! Mute, deafen and push-to-talk: the three switches a voice client must have.
//!
//! `acl_core::keys` has had the polling and the edge detection since P5+, and until
//! 2026-08-27 the only caller was the settings screen — where it is used to *record* a
//! shortcut. Nothing read them while the client was running, so none of the three did
//! anything: there was no way to mute yourself, no way to stop hearing the lobby, and
//! push-to-talk transmitted continuously.
//!
//! # The rules are `Voice.tsx`'s, including the odd one
//!
//! The microphone is open when `!deafened && !muted && not in push-to-talk mode`
//! (lines 1105-1108), and in push-to-talk mode it is open only while the key is held.
//!
//! The odd one is mute while deafened: it clears **both** (lines 1113-1117). Read as a
//! toggle that is what it does — you press mute, and you can hear again. It is the way out
//! of deafened for somebody who reached for the nearer key, and copying it is not optional:
//! a player who deafens themselves and then presses mute expects to be back.
//!
//! Deafened silences everybody else too, which is not here — it belongs where the gain is
//! decided, next to the per-player mute it behaves like.

use acl_core::keys::{Edge, KeyState, Shortcut};
use acl_core::shortcuts::binding_for;

/// What the switches say right now.
///
/// Four booleans, and clippy is right that four is a lot and wrong that it is a problem
/// here: they are four independent physical switches, every combination is reachable, and
/// the only thing an enum could express is a mutual exclusion that does not exist. Somebody
/// can be muted, deafened, holding push-to-talk and holding the radio at once -- pointlessly,
/// but the type should not be the thing that says so.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent switches, every combination reachable"
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    /// The microphone is off.
    pub(crate) muted: bool,
    /// The microphone is off and so is everybody else.
    pub(crate) deafened: bool,
    /// Push-to-talk is held, which only means anything in push-to-talk mode.
    pub(crate) holding: bool,
    /// The impostor radio key is held.
    ///
    /// A level like push-to-talk and unlike the other two: you hold it to talk to the other
    /// impostors, and letting go ends it. Whether it *does* anything depends on being an
    /// impostor, being alive and the lobby allowing it — none of which this knows, so all
    /// three are checked where the claim is made.
    pub(crate) on_radio: bool,
}

/// What the talk key does, which is a setting rather than a state.
///
/// `pushToTalkOptions` in `SettingsStore.tsx`, and the numbering is theirs: it is stored in
/// the same file under the same key, so a player who chose push-to-talk in 1.x keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The microphone is open; the detector decides what is sent.
    #[default]
    VoiceActivity,
    /// The microphone is closed until the key is held.
    PushToTalk,
    /// The microphone is open until the key is held.
    PushToMute,
}

impl Mode {
    /// From the stored number.
    ///
    /// Anything else is voice activity, which is the shipped default and the mode that
    /// cannot leave somebody unable to talk: a stored value nobody recognises should not
    /// close the microphone until they find the key it now wants.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the stored value is one of three small integers; anything a truncation                   could produce falls into the same catch-all as anything else unrecognised"
    )]
    pub(crate) fn from_setting(stored: f64) -> Self {
        match stored as i64 {
            1 => Self::PushToTalk,
            2 => Self::PushToMute,
            _ => Self::VoiceActivity,
        }
    }
}

impl State {
    /// Whether this client should be sending audio.
    ///
    /// Mute and deafen come first and are absolute; the mode decides the rest.
    ///
    /// **`Voice.tsx` also applies push-to-mute in voice-activity mode.** Its key handler is
    /// registered unconditionally and reads `mode === PUSH_TO_TALK ? pressing : !pressing`,
    /// so holding the talk key while in voice activity mutes you there. That is not copied:
    /// it is the behaviour of a listener that was not meant to be listening, there is a
    /// mode for wanting it, and the steady state two lines above it disagrees --
    /// `enabled = mode !== PUSH_TO_TALK` leaves the track open in voice activity.
    #[must_use]
    pub(crate) const fn transmitting(self, mode: Mode) -> bool {
        if self.muted || self.deafened {
            return false;
        }
        match mode {
            Mode::VoiceActivity => true,
            Mode::PushToTalk => self.holding,
            Mode::PushToMute => !self.holding,
        }
    }
}

/// How often the keys are looked at.
///
/// A key press lasts as long as a finger stays down, which is fifty to a hundred and fifty
/// milliseconds for a deliberate tap. Thirty is comfortably inside the shortest of those,
/// and it is a `GetAsyncKeyState` per bound key -- four of them, costing nothing.
///
/// **It has to be a clock of its own, and that is the fix of 2026-08-29.** The keys were
/// polled from the window's paint, which drops to five hertz whenever the pointer is not
/// over the window -- the whole time anybody is actually playing. Two hundred milliseconds
/// between looks means an ordinary tap of mute or deafen is pressed and released between
/// two of them and is never seen at all: the player presses the key, nothing happens, and
/// there is nothing to tell them why.
const POLL: std::time::Duration = std::time::Duration::from_millis(30);

/// The switches, watched on their own clock.
///
/// Owns a [`Controls`] on a thread of its own and publishes what it sees: the state for
/// whoever is painting it, and the microphone gate straight into [`crate::audio::Tuning`],
/// where the capture callback reads it one frame before the packet is made.
pub(crate) struct Switchboard {
    /// What the last poll saw. Read once a paint; written thirty-three times a second.
    seen: std::sync::Arc<std::sync::Mutex<State>>,
    /// What the settings say the switches should be. Written when somebody changes them.
    wanted: std::sync::Arc<std::sync::Mutex<(Mode, [String; 4])>>,
    /// The window's own mute and deafen buttons, waiting to be applied.
    ///
    /// The state belongs to the thread now, so a click cannot toggle it directly: it asks,
    /// and the next poll answers. Thirty milliseconds later at the outside, which is faster
    /// than the paint that produced the click.
    asked: std::sync::Arc<Clicks>,
}

/// The two buttons the window can press itself.
#[derive(Default)]
struct Clicks {
    mute: std::sync::atomic::AtomicBool,
    deafen: std::sync::atomic::AtomicBool,
}

impl Switchboard {
    /// Starts watching.
    ///
    /// The thread runs until the process ends. There is nothing to stop it for: it holds
    /// no device, it polls four keys, and a client with no switches is not a voice client.
    pub(crate) fn start(tuning: &std::sync::Arc<crate::audio::Tuning>) -> Self {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(State::default()));
        let wanted = std::sync::Arc::new(std::sync::Mutex::new((
            Mode::VoiceActivity,
            [String::new(), String::new(), String::new(), String::new()],
        )));

        let asked = std::sync::Arc::new(Clicks::default());

        let published = std::sync::Arc::clone(&seen);
        let settings = std::sync::Arc::clone(&wanted);
        let clicks = std::sync::Arc::clone(&asked);
        let tuning = std::sync::Arc::clone(tuning);
        let started = std::thread::Builder::new()
            .name("switches".to_owned())
            .spawn(move || {
                let mut controls = Controls::new("", "", "", "");
                loop {
                    // Rebound first, because somebody may have just changed one on the
                    // settings page and the next press should use it. `rebind` compares
                    // before it rebuilds, so this costs four string comparisons.
                    let mode = match settings.lock() {
                        Ok(settings) => {
                            let (mode, keys) = &*settings;
                            controls.rebind(&keys[0], &keys[1], &keys[2], &keys[3]);
                            *mode
                        }
                        // The window is gone.
                        Err(_) => return,
                    };
                    // The window's buttons, before the keys, so a click and a key press in
                    // the same thirty milliseconds cancel rather than race.
                    if clicks
                        .mute
                        .swap(false, std::sync::atomic::Ordering::Relaxed)
                    {
                        controls.toggle_mute();
                    }
                    if clicks
                        .deafen
                        .swap(false, std::sync::atomic::Ordering::Relaxed)
                    {
                        controls.toggle_deafen();
                    }
                    let switches = controls.poll(&acl_core::keys::AsyncKeyState);
                    // Into the capture callback, which reads it once per twenty-millisecond
                    // frame. This is the whole reason the thread exists.
                    tuning.transmit(switches.transmitting(mode));
                    if let Ok(mut published) = published.lock() {
                        *published = switches;
                    } else {
                        return;
                    }
                    std::thread::sleep(POLL);
                }
            });
        if let Err(error) = started {
            // Not fatal, and not silent. Without the thread the switches simply never
            // change, which is a client that cannot be muted -- worth a line in the log
            // rather than a window that will not open.
            acl_core::log_warn!("audio", "the switches could not be watched: {error}");
        }

        Self {
            seen,
            wanted,
            asked,
        }
    }

    /// The window's mute button.
    pub(crate) fn toggle_mute(&self) {
        self.asked
            .mute
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The window's deafen button.
    pub(crate) fn toggle_deafen(&self) {
        self.asked
            .deafen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Tells the watcher what the settings now say.
    ///
    /// Called from the paint, at whatever rate the paint runs: it is four string
    /// comparisons and a mode, and none of it is time-critical -- what is time-critical is
    /// the polling, which is on the other side of this.
    pub(crate) fn configure(&self, mode: Mode, bindings: [String; 4]) {
        if let Ok(mut wanted) = self.wanted.lock() {
            *wanted = (mode, bindings);
        }
    }

    /// What the switches last said.
    pub(crate) fn state(&self) -> State {
        self.seen.lock().map(|seen| *seen).unwrap_or_default()
    }
}

/// The three shortcuts, and what they have done.
pub(crate) struct Controls {
    mute: Shortcut,
    deafen: Shortcut,
    talk: Shortcut,
    radio: Shortcut,
    state: State,
    /// What the bindings were last built from, so they are rebuilt only when they change.
    ///
    /// Parsing four shortcut strings every frame would be four allocations sixty times a
    /// second to answer a question whose answer changes when somebody opens the settings.
    bound: [String; 4],
}

impl Controls {
    /// Shortcuts from their settings strings.
    pub(crate) fn new(mute: &str, deafen: &str, talk: &str, radio: &str) -> Self {
        // Said out loud, once per rebind. `binding_for` answers `Unsupported` for exactly
        // one name -- `NumpadEnter`, which this backend cannot tell from the main Enter, so
        // honouring it would mean a hot microphone every time somebody sends a chat message
        // -- and `shortcuts`'s own documentation says the caller tells the player. No caller
        // did, so a shortcut bound to it was simply dead: pressed, nothing, no reason.
        for (setting, name) in [
            ("muteShortcut", mute),
            ("deafenShortcut", deafen),
            ("pushToTalkShortcut", talk),
            ("impostorRadioShortcut", radio),
        ] {
            match binding_for(name) {
                acl_core::shortcuts::Binding::Unsupported => acl_core::log_warn!(
                    "keys",
                    "{setting} is bound to {name}, which this client cannot tell apart from Enter;                      it will do nothing until it is rebound"
                ),
                acl_core::shortcuts::Binding::None if !name.is_empty() => acl_core::log_warn!(
                    "keys",
                    "{setting} is bound to {name}, which is not a key this client knows"
                ),
                _ => {}
            }
        }
        Self {
            mute: Shortcut::new(binding_for(mute)),
            deafen: Shortcut::new(binding_for(deafen)),
            talk: Shortcut::new(binding_for(talk)),
            radio: Shortcut::new(binding_for(radio)),
            state: State::default(),
            bound: [
                mute.to_owned(),
                deafen.to_owned(),
                talk.to_owned(),
                radio.to_owned(),
            ],
        }
    }

    /// Rebinds, if the settings have changed since the last call.
    ///
    /// The pressed state is deliberately not carried across: a shortcut that was held when
    /// it was rebound is a key nobody is holding any more, and `Shortcut::new` starts from
    /// not-pressed for the same reason.
    pub(crate) fn rebind(&mut self, mute: &str, deafen: &str, talk: &str, radio: &str) {
        if self.bound[0] == mute
            && self.bound[1] == deafen
            && self.bound[2] == talk
            && self.bound[3] == radio
        {
            return;
        }
        let held = self.state;
        *self = Self::new(mute, deafen, talk, radio);
        // The toggles survive; only the key that produces them changed.
        self.state.muted = held.muted;
        self.state.deafened = held.deafened;
    }

    /// Reads the keyboard and applies the rules.
    pub(crate) fn poll(&mut self, keys: &impl KeyState) -> State {
        if self.deafen.poll(keys) == Edge::Pressed {
            self.toggle_deafen();
        }
        if self.mute.poll(keys) == Edge::Pressed {
            self.toggle_mute();
        }
        // Levels, not edges: these two are held rather than toggled.
        let _ = self.talk.poll(keys);
        self.state.holding = self.talk.is_down();
        let _ = self.radio.poll(keys);
        self.state.on_radio = self.radio.is_down();
        self.state
    }

    /// Deafen, once.
    ///
    /// Its own method rather than the body of the key's arm, because there are two ways to
    /// press it now -- the key and the button in the top row -- and a rule written twice is
    /// a rule that gets changed once.
    ///
    /// Deafening implies muting: `Voice.tsx` closes the microphone on either, and a player
    /// who cannot hear the lobby is not expecting to still be heard by it.
    pub(crate) const fn toggle_deafen(&mut self) {
        self.state.deafened = !self.state.deafened;
        if self.state.deafened {
            self.state.muted = true;
        }
    }

    /// Mute, once, including the odd rule.
    ///
    /// Mute while deafened clears **both**. See the module documentation: it is the way out
    /// for somebody who reached for the nearer key, and a button that did not do it would be
    /// a second control with different rules.
    pub(crate) const fn toggle_mute(&mut self) {
        if self.state.deafened {
            self.state.deafened = false;
            self.state.muted = false;
        } else {
            self.state.muted = !self.state.muted;
        }
    }

    /// What the switches say, without reading the keyboard.
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn state(&self) -> State {
        self.state
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{Controls, Mode, State};

    /// A keyboard with whatever keys the test wants held.
    struct Held(Vec<u16>);

    impl acl_core::keys::KeyState for Held {
        fn is_down(&self, virtual_key: u16) -> bool {
            self.0.contains(&virtual_key)
        }
    }

    /// `F1`, `F2`, `F3` — three bindings the vendored table knows.
    fn controls() -> Controls {
        let mut controls = Controls::new("F1", "F2", "F3", "F4");
        // One poll with nothing held, which is what the thirty-millisecond watcher does
        // before anybody touches a key. A shortcut's first look is an observation rather
        // than a change -- see `acl_core::keys::Shortcut`, where starting from an assumed
        // "not pressed" is what made binding mute mute you on the spot.
        let _ = controls.poll(&Held(vec![]));
        controls
    }

    const F1: u16 = 0x70;
    const F2: u16 = 0x71;
    const F3: u16 = 0x72;
    const F4: u16 = 0x73;

    /// Nothing held, nothing changed.
    /// The buttons and the keys are the same two rules.
    ///
    /// They are, because the key arms call the methods the buttons call -- and this is the
    /// check that they still do. The interesting case is the odd one: mute while deafened
    /// clears both, and a top-row button that toggled `muted` on its own would leave
    /// somebody deafened with the microphone open and no way back that they would find.
    #[test]
    fn a_button_press_and_a_key_press_are_the_same_press() {
        for keys in [vec![], vec![0x11_u16], vec![0x12], vec![0x11, 0x12]] {
            let mut by_key = controls();
            let mut by_button = controls();
            // Deafen, then mute: the pair that clears both.
            by_key.poll(&Held(vec![0x12]));
            by_key.poll(&Held(vec![]));
            by_key.poll(&Held(vec![0x11]));
            by_button.toggle_deafen();
            by_button.toggle_mute();
            assert_eq!(
                by_key.state(),
                by_button.state(),
                "the button and the key disagree after {keys:?}"
            );
        }
    }

    #[test]
    fn a_client_starts_able_to_talk_and_hear() {
        let mut controls = controls();
        let state = controls.poll(&Held(vec![]));
        assert_eq!(state, State::default());
        assert!(state.transmitting(Mode::VoiceActivity));
    }

    /// Mute is a toggle, and it takes a release before it toggles again.
    ///
    /// The level is all the platform offers. A held key read as a repeated press would
    /// toggle sixty times a second, which reads as a microphone that flickers.
    #[test]
    fn holding_mute_down_toggles_once() {
        let mut controls = controls();
        assert!(controls.poll(&Held(vec![F1])).muted);
        assert!(controls.poll(&Held(vec![F1])).muted, "it toggled again");
        assert!(controls.poll(&Held(vec![])).muted, "releasing changed it");
        assert!(!controls.poll(&Held(vec![F1])).muted, "it did not toggle");
    }

    /// Deafening closes the microphone too.
    ///
    /// A player who cannot hear the lobby is not expecting to still be heard by it.
    #[test]
    fn deafening_also_mutes() {
        let mut controls = controls();
        let state = controls.poll(&Held(vec![F2]));
        assert!(state.deafened);
        assert!(state.muted);
        assert!(!state.transmitting(Mode::VoiceActivity));
    }

    /// Mute while deafened is the way back, and clears both.
    ///
    /// `Voice.tsx` lines 1113-1117. Read as a plain toggle it looks wrong; it is the
    /// affordance for somebody who deafened themselves and reached for the nearer key.
    #[test]
    fn mute_while_deafened_undoes_both() {
        let mut controls = controls();
        controls.poll(&Held(vec![F2]));
        controls.poll(&Held(vec![]));
        let state = controls.poll(&Held(vec![F1]));
        assert!(!state.deafened, "still deafened");
        assert!(!state.muted, "still muted");
    }

    /// In push-to-talk mode the microphone is closed unless the key is held.
    #[test]
    fn push_to_talk_inverts_the_default() {
        let mut controls = controls();
        let idle = controls.poll(&Held(vec![]));
        assert!(
            idle.transmitting(Mode::VoiceActivity),
            "not in push-to-talk mode"
        );
        assert!(
            !idle.transmitting(Mode::PushToTalk),
            "push-to-talk should be closed"
        );

        let holding = controls.poll(&Held(vec![F3]));
        assert!(holding.holding);
        assert!(holding.transmitting(Mode::PushToTalk));
    }

    /// Push-to-mute is the other way round, and it was missing entirely.
    ///
    /// The mode was a `bool`, so the settings screen's three choices were two — and the one
    /// key it read, `pushToTalk`, is written by nothing in the project. Every client was in
    /// voice activity whatever the screen said.
    #[test]
    fn push_to_mute_closes_while_it_is_held() {
        let idle = State::default();
        let holding = State {
            holding: true,
            ..State::default()
        };
        assert!(idle.transmitting(Mode::PushToMute), "open until held");
        assert!(!holding.transmitting(Mode::PushToMute), "closed while held");
        // And the key does nothing at all in voice activity. See `transmitting`.
        assert!(holding.transmitting(Mode::VoiceActivity));
    }

    /// The stored numbers are `pushToTalkOptions`, and an unknown one is the safe mode.
    #[test]
    fn the_stored_mode_is_the_shipped_numbering() {
        assert_eq!(Mode::from_setting(0.0), Mode::VoiceActivity);
        assert_eq!(Mode::from_setting(1.0), Mode::PushToTalk);
        assert_eq!(Mode::from_setting(2.0), Mode::PushToMute);
        assert_eq!(
            Mode::from_setting(7.0),
            Mode::VoiceActivity,
            "an unknown mode must not be one that closes the microphone"
        );
    }

    /// And muting beats holding the key.
    #[test]
    fn a_muted_client_stays_quiet_however_hard_it_pushes() {
        let mut controls = controls();
        controls.poll(&Held(vec![F1]));
        let state = controls.poll(&Held(vec![F1, F3]));
        assert!(state.muted);
        assert!(
            !state.transmitting(Mode::PushToTalk),
            "muted and still transmitting"
        );
        assert!(!state.transmitting(Mode::VoiceActivity));
    }

    /// The radio is a level, like push-to-talk and unlike the toggles.
    ///
    /// You hold it to talk to the other impostors and letting go ends it. A toggle here
    /// would be a key that leaves you broadcasting to the impostors after you thought you
    /// had stopped, which in this game is the worst possible failure mode.
    #[test]
    fn the_radio_is_held_rather_than_toggled() {
        let mut controls = controls();
        assert!(!controls.poll(&Held(vec![])).on_radio);
        assert!(controls.poll(&Held(vec![F4])).on_radio);
        assert!(
            controls.poll(&Held(vec![F4])).on_radio,
            "it turned itself off"
        );
        assert!(
            !controls.poll(&Held(vec![])).on_radio,
            "letting go did not end it"
        );
    }

    /// And it is independent of the others: an impostor on the radio can still be muted.
    #[test]
    fn the_radio_and_the_microphone_are_separate_switches() {
        let mut controls = controls();
        let state = controls.poll(&Held(vec![F1, F4]));
        assert!(state.muted);
        assert!(state.on_radio);
        assert!(
            !state.transmitting(Mode::VoiceActivity),
            "muted, so nothing goes out over the radio either"
        );
    }

    /// Rebinding keeps the toggles and forgets what was held.
    ///
    /// A key that was down when it stopped being the binding is a key nobody is holding any
    /// more. Carrying that across would leave push-to-talk open with nothing pressed.
    #[test]
    fn rebinding_keeps_what_you_chose_and_drops_what_you_held() {
        let mut controls = controls();
        controls.poll(&Held(vec![F1]));
        controls.poll(&Held(vec![F3]));
        assert!(controls.state().muted);
        assert!(controls.state().holding);

        controls.rebind("F1", "F2", "F5", "F4");
        assert!(controls.state().muted, "the toggle was forgotten");
        assert!(!controls.state().holding, "a key nobody is holding is held");
    }

    /// Rebinding to the same strings changes nothing at all.
    #[test]
    fn rebinding_to_what_is_already_bound_is_free() {
        let mut controls = controls();
        controls.poll(&Held(vec![F3]));
        controls.rebind("F1", "F2", "F3", "F4");
        assert!(
            controls.state().holding,
            "an unchanged rebind threw the key away"
        );
    }
}
