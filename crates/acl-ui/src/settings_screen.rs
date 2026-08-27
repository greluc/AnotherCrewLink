//! What the settings screen contains, as data.
//!
//! §4.8 calls settings "the largest single screen" and gives it three of the phase's
//! eleven and a half weeks. Most of that size is not drawing: it is forty controls, each
//! pointing at a key in [`crate::settings`], several gated on another control, several
//! behind a confirmation, and two that are not settings at all. Written as a paint
//! function that is forty branches nobody can check.
//!
//! So the screen is described here and drawn in `crate::views::settings`, for the reason
//! [`crate::views`] gives: a decision inside a paint function is a decision nobody can
//! test. The tests below are what that buys — every control points at a key the schema
//! has, of a type the schema agrees with; every range contains its own default; every
//! label exists in the shipped English catalogue; and every setting 1.x has is either
//! reachable on this screen or named in [`NOT_SHOWN`] with a reason.
//!
//! That last one is the one worth having. A setting that exists, is written to
//! `config.json`, and has no control left to reach it is not a missing feature anybody
//! reports — it is a preference that quietly stops being adjustable.

use crate::settings::Default_;

/// What a control does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A checkbox.
    Toggle,
    /// A slider over a numeric setting.
    Slider {
        /// The lowest value the slider offers.
        min: f64,
        /// The highest.
        max: f64,
        /// How far one step moves.
        step: f64,
        /// Whether the number shown is `min + max - value` rather than the value.
        ///
        /// True for exactly one setting, and it is not cosmetic: `micSensitivity` is a
        /// noise floor, so a *larger* stored number means the microphone opens *less*
        /// readily. The slider is labelled sensitivity and has to run the other way.
        inverted: bool,
    },
    /// A fixed set of values.
    Choice(&'static [Choice]),
    /// A key or mouse button, captured by pressing it.
    Shortcut,
    /// An audio device, chosen from what the machine currently has.
    Device {
        /// True for a microphone, false for a speaker.
        capture: bool,
    },
    /// Free text.
    Text,
    /// A sentence, drawn where it stands.
    ///
    /// Not a setting and not interactive: some fields need a line beside them that is a
    /// rule rather than a hint, and a tooltip is not where a rule belongs — nobody hovers a
    /// field before typing in it.
    Note,
    /// A button that tries something and changes nothing.
    ///
    /// Deliberately **not** [`Kind::Action`]. An action here means one that alters the
    /// configuration irreversibly, which is why `an_action_is_not_a_setting` requires every
    /// one of them to carry a warning — restoring defaults rewrites every preference, and
    /// resetting the offsets throws away what the reader is using. Playing a sound through
    /// the chosen speaker does neither, and putting it under the same kind would have meant
    /// either a confirmation dialog before a chime or weakening the rule that keeps the
    /// other two behind one.
    Probe,
    /// What the microphone is hearing right now, drawn rather than edited.
    ///
    /// Not a setting: nothing is stored under its key and nothing can be. It is here
    /// because it belongs beside the two settings that decide what the detector does with
    /// that level — `micSensitivity` is a threshold, and a threshold without a reading of
    /// what it is being compared against is a number somebody guesses at.
    ///
    /// `VadFrame::level` has carried it since P3+, documented as "for a meter", and nothing
    /// read it.
    Meter,
    /// A locale, chosen from the tree under `static/locales`.
    ///
    /// Not a [`Kind::Choice`] because the options are not fixed: they are whatever
    /// directories are shipped, and there are thirty-seven of them.
    Language,
    /// A button that does something rather than storing a value.
    Action,
}

/// One option of a [`Kind::Choice`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice {
    /// What is stored when it is picked.
    pub value: Default_,
    /// The i18n key of its label.
    pub label: &'static str,
}

impl Kind {
    /// Whether this control stands for a value in `config.json`.
    ///
    /// Four kinds do not. `Action` runs something, `Probe` tries something, `Note` says
    /// something and `Meter` reads something; each needs a key, because the screen builds a
    /// widget id from it, and none of them is a setting.
    ///
    /// Said here rather than in each rule that needs it. Three tests were listing the kinds
    /// to skip, and a fourth kind added later would have been forgotten by all three at
    /// once — the way `Note` was, until `the_two_scopes_hold_the_two_lists` caught it.
    #[must_use]
    pub const fn is_a_setting(self) -> bool {
        !matches!(self, Self::Action | Self::Probe | Self::Note | Self::Meter)
    }
}

/// One control on the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    /// The settings key it reads and writes, or the action it performs.
    pub key: &'static str,
    /// The i18n key of its label.
    ///
    /// Optional because one control has no label in 1.x: the push-to-talk mode is three
    /// radio buttons under the audio heading, and each button is labelled rather than the
    /// group. Giving it one here would be inventing a string no translator has.
    pub label: Option<&'static str>,
    /// What it does.
    pub kind: Kind,
    /// A setting that must be on for this control to be usable.
    ///
    /// The checkbox beside a slider, and the three overlay controls that mean nothing
    /// with the overlay off. Modelled as a property of the gated control rather than as a
    /// control of its own, because that is what it is: `micSensitivityEnabled` has no
    /// meaning apart from the slider it enables.
    pub gate: Option<&'static str>,
    /// The i18n key of a warning to confirm before the change takes effect.
    ///
    /// Every one of these is a setting that has broken somebody's audio. The Electron
    /// client puts them behind a dialog, and so does this.
    pub warning: Option<&'static str>,
}

impl Control {
    /// A plain checkbox.
    const fn toggle(key: &'static str, label: &'static str) -> Self {
        Self {
            key,
            label: Some(label),
            kind: Kind::Toggle,
            gate: None,
            warning: None,
        }
    }

    /// A control that asks first.
    const fn warning(mut self, key: &'static str) -> Self {
        self.warning = Some(key);
        self
    }

    /// A control that is only usable while another setting is on.
    const fn gated_by(mut self, key: &'static str) -> Self {
        self.gate = Some(key);
        self
    }

    /// A slider over a numeric setting.
    const fn slider(key: &'static str, label: &'static str, min: f64, max: f64, step: f64) -> Self {
        Self {
            key,
            label: Some(label),
            kind: Kind::Slider {
                min,
                max,
                step,
                inverted: false,
            },
            gate: None,
            warning: None,
        }
    }

    /// A control of some other kind.
    const fn of(key: &'static str, label: Option<&'static str>, kind: Kind) -> Self {
        Self {
            key,
            label,
            kind,
            gate: None,
            warning: None,
        }
    }
}

/// Who may change the settings in a section, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// This player's own client. Always editable.
    Client,
    /// The lobby's rules, which the host owns and every peer receives.
    ///
    /// Editable only by the game host, and only while in a lobby. Both halves matter and
    /// both have a string: a player who is not the host is told so, and a host who is not
    /// in a lobby yet is told the other thing.
    Lobby,
}

impl Scope {
    /// Why a [`Scope::Lobby`] control is unavailable, given where the player is.
    ///
    /// The two strings 1.x shows on the same tooltip. Which one is right depends on
    /// whether the player is somewhere a lobby exists at all.
    #[must_use]
    pub const fn unavailable(self, in_menu_or_lobby: bool) -> Option<&'static str> {
        match self {
            Self::Client => None,
            Self::Lobby if in_menu_or_lobby => Some("settings.lobbysettings.gamehostonly"),
            Self::Lobby => Some("settings.lobbysettings.inlobbyonly"),
        }
    }
}

/// A group of controls under one heading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    /// The i18n key of its heading, or `None` for the one group that has none.
    pub title: Option<&'static str>,
    /// Whose settings these are.
    pub scope: Scope,
    /// What is in it.
    pub controls: &'static [Control],
}

/// The lobby rules, which the host sets for everyone.
const LOBBY: &[Control] = &[
    Control::slider(
        "maxDistance",
        "settings.lobbysettings.voicedistance",
        1.0,
        10.0,
        0.1,
    ),
    Control::toggle("wallsBlockAudio", "settings.lobbysettings.wallsblockaudio"),
    Control::toggle("visionHearing", "settings.lobbysettings.visiononly"),
    Control::toggle("haunting", "settings.lobbysettings.impostorshearsghost"),
    Control::toggle(
        "hearImpostorsInVents",
        "settings.lobbysettings.hear_imposters_invents",
    ),
    Control::toggle(
        "impostersHearImpostersInvent",
        "settings.lobbysettings.private_talk_invents",
    ),
    Control::toggle(
        "commsSabotage",
        "settings.lobbysettings.comms_sabotage_audio",
    ),
    Control::toggle(
        "hearThroughCameras",
        "settings.lobbysettings.hear_through_cameras",
    ),
    Control::toggle(
        "impostorRadioEnabled",
        "settings.lobbysettings.impostor_radio",
    ),
    Control::toggle("deadOnly", "settings.lobbysettings.ghost_only")
        .warning("settings.lobbysettings.ghost_only_warning"),
    Control::toggle("meetingGhostOnly", "settings.lobbysettings.meetings_only")
        .warning("settings.lobbysettings.meetings_only_warning"),
];

/// Listing the lobby publicly, and what it is listed as.
const PUBLIC_LOBBY: &[Control] = &[
    Control::toggle(
        "publicLobby_on",
        "settings.lobbysettings.public_lobby.enabled",
    )
    .warning("settings.lobbysettings.public_lobby.enable_warning"),
    Control::of(
        "publicLobby_title",
        Some("settings.lobbysettings.public_lobby.title"),
        Kind::Text,
    )
    .gated_by("publicLobby_on"),
    Control::of(
        "publicLobby_language",
        Some("settings.lobbysettings.public_lobby.language"),
        Kind::Language,
    )
    .gated_by("publicLobby_on"),
    // Under the two fields it is about, because it is about what somebody types into them.
    // `PublicLobbySettings.tsx` shows it in the same place and for the same reason: a title
    // is the one thing here that every other player reads.
    Control::of(
        "publicLobbyBanWarning",
        Some("settings.lobbysettings.public_lobby.ban_warning"),
        Kind::Note,
    )
    .gated_by("publicLobby_on"),
];

/// Devices, modes and volumes.
const AUDIO: &[Control] = &[
    Control::of(
        "microphone",
        Some("settings.audio.microphone"),
        Kind::Device { capture: true },
    ),
    Control::of(
        "speaker",
        Some("settings.audio.speaker"),
        Kind::Device { capture: false },
    ),
    // Between the speaker and the mode, because it is about the speaker: it plays a sound
    // through the one that is selected, which is the only way to find out that the
    // selection is wrong.
    Control::of(
        "testSpeaker",
        Some("settings.audio.test_speaker_start"),
        Kind::Probe,
    ),
    Control::of("microphoneLevel", None, Kind::Meter),
    Control::of(
        "pushToTalkMode",
        None,
        Kind::Choice(&[
            Choice {
                value: Default_::Number(0.0),
                label: "settings.audio.voice_activity",
            },
            Choice {
                value: Default_::Number(1.0),
                label: "settings.audio.push_to_talk",
            },
            Choice {
                value: Default_::Number(2.0),
                label: "settings.audio.push_to_mute",
            },
        ]),
    ),
    Control::slider(
        "microphoneGain",
        "settings.audio.microphone_volume",
        0.0,
        300.0,
        2.0,
    )
    .gated_by("microphoneGainEnabled"),
    Control {
        kind: Kind::Slider {
            min: 0.0,
            max: 1.0,
            step: 0.05,
            inverted: true,
        },
        ..Control::slider(
            "micSensitivity",
            "settings.audio.microphone_sens",
            0.0,
            1.0,
            0.05,
        )
        .gated_by("micSensitivityEnabled")
        .warning("settings.audio.microphone_sens_warning")
    },
    Control::slider(
        "masterVolume",
        "settings.audio.mastervolume",
        0.0,
        200.0,
        1.0,
    ),
    Control::slider(
        "crewVolumeAsGhost",
        "settings.audio.crewvolume",
        0.0,
        100.0,
        1.0,
    ),
    Control::slider(
        "ghostVolumeAsImpostor",
        "settings.audio.ghostvolumeasimpostor",
        0.0,
        100.0,
        1.0,
    ),
];

/// The four shortcuts.
const KEYBOARD: &[Control] = &[
    Control::of(
        "pushToTalkShortcut",
        Some("settings.keyboard.push_to_talk"),
        Kind::Shortcut,
    ),
    Control::of(
        "impostorRadioShortcut",
        Some("settings.keyboard.impostor_radio"),
        Kind::Shortcut,
    ),
    Control::of(
        "muteShortcut",
        Some("settings.keyboard.mute"),
        Kind::Shortcut,
    ),
    Control::of(
        "deafenShortcut",
        Some("settings.keyboard.deafen"),
        Kind::Shortcut,
    ),
];

/// The in-game overlay.
const OVERLAY: &[Control] = &[
    Control::toggle("alwaysOnTop", "settings.overlay.always_on_top"),
    Control::toggle("enableOverlay", "settings.overlay.enabled"),
    Control::toggle("compactOverlay", "settings.overlay.compact").gated_by("enableOverlay"),
    Control::toggle("meetingOverlay", "settings.overlay.meeting").gated_by("enableOverlay"),
    Control::of(
        "overlayPosition",
        Some("settings.overlay.pos"),
        // Seven, and the values are not the labels: `bottom_left` is shown as
        // `locations.bottom`. Renaming either half would move an existing setting to a
        // position the player did not choose.
        Kind::Choice(&[
            Choice {
                value: Default_::Text("hidden"),
                label: "settings.overlay.locations.hidden",
            },
            Choice {
                value: Default_::Text("top"),
                label: "settings.overlay.locations.top",
            },
            Choice {
                value: Default_::Text("bottom_left"),
                label: "settings.overlay.locations.bottom",
            },
            Choice {
                value: Default_::Text("right"),
                label: "settings.overlay.locations.right",
            },
            Choice {
                value: Default_::Text("right1"),
                label: "settings.overlay.locations.right1",
            },
            Choice {
                value: Default_::Text("left"),
                label: "settings.overlay.locations.left",
            },
            Choice {
                value: Default_::Text("left1"),
                label: "settings.overlay.locations.left1",
            },
        ]),
    )
    .gated_by("enableOverlay"),
];

/// Where the game is started from.
///
/// The three the client knows how to start. A custom entry is chosen by its own title and
/// cannot be listed here, because the list is a compile-time constant and those are not —
/// see `waiting_for_the_game`, which starts one that is already stored.
const LAUNCH: &[Control] = &[Control::of(
    "launchPlatform",
    Some("game.open"),
    Kind::Choice(&[
        // The stored values are the shipped client's, upper case and not tidied: they are
        // `Platform::key`, which has a test comparing it against `GamePlatform.ts`. Writing
        // them in lower case would move an existing setting to a platform nobody chose.
        Choice {
            value: Default_::Text("STEAM"),
            label: "platform.steam",
        },
        Choice {
            value: Default_::Text("EPIC"),
            label: "platform.epicgames",
        },
        Choice {
            value: Default_::Text("MICROSOFT"),
            label: "platform.microsoft",
        },
    ]),
)];

/// The network, and the server.
const ADVANCED: &[Control] = &[
    Control::toggle("natFix", "settings.advanced.nat_fix")
        .warning("settings.advanced.nat_fix_warning"),
    Control::of(
        "serverURL",
        Some("settings.advanced.voice_server"),
        Kind::Text,
    )
    .warning("settings.advanced.voice_server_warning"),
];

/// The switches that are still being decided about.
const BETA: &[Control] = &[
    Control::toggle("vadEnabled", "settings.beta.vad_enabled")
        .warning("settings.beta.vad_enabled_warning"),
    Control::toggle(
        "hardware_acceleration",
        "settings.beta.hardware_acceleration",
    )
    .warning("settings.beta.hardware_acceleration_warning"),
    Control::toggle("echoCancellation", "settings.beta.echocancellation"),
    Control::toggle("noiseSuppression", "settings.beta.noiseSuppression"),
    Control::toggle("enableSpatialAudio", "settings.beta.spatial_audio"),
    Control::toggle("oldSampleDebug", "settings.beta.oldsampledebug")
        .warning("settings.beta.oldsampledebug_warning"),
];

/// What to hide from a camera.
const STREAMING: &[Control] = &[Control::toggle("hideCode", "settings.streaming.hidecode")];

/// The two buttons.
///
/// Both are actions rather than settings, and both are behind a warning: one rewrites
/// every preference and the other throws away the offsets the reader is currently using.
const TROUBLESHOOTING: &[Control] = &[
    Control::of(
        "restoreDefaults",
        Some("settings.troubleshooting.restore"),
        Kind::Action,
    )
    .warning("settings.troubleshooting.restore_warning"),
    Control::of(
        "resetOffsets",
        Some("settings.troubleshooting.reset_offsets"),
        Kind::Action,
    )
    .warning("settings.troubleshooting.reset_offsets_warning"),
];

/// The interface language, which has no heading of its own.
const LANGUAGE: &[Control] = &[Control::of(
    "language",
    Some("settings.language"),
    Kind::Language,
)];

/// The whole screen, in the order 1.x shows it.
///
/// The order is kept because it is what somebody who has used the Electron client knows.
/// §4.8 allows the layout to differ; it does not follow that it should differ for no
/// reason.
pub const SECTIONS: &[Section] = &[
    Section {
        title: Some("settings.lobbysettings.title"),
        scope: Scope::Lobby,
        controls: LOBBY,
    },
    Section {
        title: Some("settings.lobbysettings.public_lobby.title"),
        scope: Scope::Lobby,
        controls: PUBLIC_LOBBY,
    },
    Section {
        title: Some("settings.audio.title"),
        scope: Scope::Client,
        controls: AUDIO,
    },
    Section {
        title: Some("settings.keyboard.title"),
        scope: Scope::Client,
        controls: KEYBOARD,
    },
    Section {
        title: Some("settings.overlay.title"),
        scope: Scope::Client,
        controls: OVERLAY,
    },
    Section {
        // `game.open` is "Open via", which is the phrase the launch button uses. The
        // section borrows it rather than inventing a heading no translator has.
        title: Some("game.open"),
        scope: Scope::Client,
        controls: LAUNCH,
    },
    Section {
        title: Some("settings.advanced.title"),
        scope: Scope::Client,
        controls: ADVANCED,
    },
    Section {
        title: Some("settings.beta.title"),
        scope: Scope::Client,
        controls: BETA,
    },
    Section {
        title: None,
        scope: Scope::Client,
        controls: LANGUAGE,
    },
    Section {
        title: Some("settings.streaming.title"),
        scope: Scope::Client,
        controls: STREAMING,
    },
    Section {
        title: Some("settings.troubleshooting.title"),
        scope: Scope::Client,
        controls: TROUBLESHOOTING,
    },
];

/// Settings this screen deliberately does not show, and why.
///
/// The list exists so that `every_setting_is_reachable_or_named_here` can be exhaustive.
/// A setting that is neither on the screen nor here fails that test, which is the point:
/// the failure mode being guarded against is a preference that silently stops being
/// adjustable, and nobody files a report about a control that was never there.
pub const NOT_SHOWN: &[(&str, &str)] = &[
    (
        "microphoneLabel",
        "written as a side effect of choosing a microphone, not chosen. Windows changes a \
		 device's id when it is unplugged and replugged; the label is what the id is \
		 recovered from.",
    ),
    ("speakerLabel", "the same, for the speaker."),
    (
        "launchPlatform",
        "belongs to the launcher, which is its own screen: it is where the game is started \
		 from, not how it is heard.",
    ),
    (
        "publicLobby_mods",
        "dead in 1.x. It is declared in `SettingsStore.tsx` with a default of `NONE`, and \
		 nothing reads it or writes it -- the `mods` field the server is told about comes \
		 from the game state instead. Ported so that a `config.json` round-trips, and given \
		 no control because it would not do anything.",
    ),
];

/// Whether a control can be used, and why not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Availability {
    /// Whether the control takes input.
    pub enabled: bool,
    /// The i18n key of an explanation, when there is one worth showing.
    pub reason: Option<&'static str>,
}

/// Works out whether a control is usable.
///
/// Two things can disable a control and they are not the same. A lobby rule is disabled
/// because of somebody else -- the host, or the absence of a lobby -- and there is nothing
/// the player can do about it from this screen, so it says so. A gated control is disabled
/// because of the checkbox beside it, which the player can see and click, so it says
/// nothing: a tooltip reading "turn on the checkbox next to this" is noise.
///
/// The order matters when both apply. The lobby reason comes first, because it is the one
/// the player cannot resolve.
#[must_use]
pub fn availability(
    control: &Control,
    scope: Scope,
    gate_is_on: bool,
    host_may_change: bool,
    in_menu_or_lobby: bool,
) -> Availability {
    if scope == Scope::Lobby && !host_may_change {
        return Availability {
            enabled: false,
            reason: scope.unavailable(in_menu_or_lobby),
        };
    }
    Availability {
        enabled: control.gate.is_none() || gate_is_on,
        reason: None,
    }
}

/// Every control on the screen, section by section.
pub fn controls() -> impl Iterator<Item = &'static Control> {
    SECTIONS.iter().flat_map(|section| section.controls.iter())
}

/// Whether a gate is itself one of the screen's controls.
///
/// Two of the four are: `enableOverlay` and `publicLobby_on` each have a row of their own,
/// with a label a translator wrote. Drawing a gating checkbox for them as well put the same
/// switch on the screen a second, third and fourth time with nothing beside it — which is
/// what an unlabelled checkbox under "Compact overlay" was.
///
/// The other two, `microphoneGainEnabled` and `micSensitivityEnabled`, exist only to enable
/// their slider. Those are drawn, and they take the slider's own label.
#[must_use]
pub fn gate_is_its_own_control(gate: &str) -> bool {
    controls().any(|control| control.key == gate)
}

/// What a slider shows for a stored value.
///
/// The identity for every slider but one. See [`Kind::Slider`]'s `inverted`.
#[must_use]
pub fn shown(kind: Kind, stored_value: f64) -> f64 {
    match kind {
        Kind::Slider {
            min,
            max,
            inverted: true,
            ..
        } => min + max - stored_value,
        _ => stored_value,
    }
}

/// What to store for a value the player set the slider to.
///
/// Its own function rather than a second call to [`shown`], even though the arithmetic is
/// the same one: the two directions being the same is a property of this particular
/// inversion, not something a caller should have to know.
#[must_use]
pub fn stored(kind: Kind, shown_value: f64) -> f64 {
    match kind {
        Kind::Slider {
            min,
            max,
            inverted: true,
            ..
        } => min + max - shown_value,
        _ => shown_value,
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{
        Choice, Kind, NOT_SHOWN, SECTIONS, Scope, availability, controls, gate_is_its_own_control,
        shown, stored,
    };
    use crate::settings::{Default_, defaults, lobby_defaults};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    /// Every setting, from both lists, with the type its default has.
    fn schema() -> BTreeMap<&'static str, Default_> {
        defaults().into_iter().chain(lobby_defaults()).collect()
    }

    /// A gate with a row of its own is not drawn a second time.
    ///
    /// Two of the four are: `enableOverlay` and `publicLobby_on`. Drawing a gating checkbox
    /// for them as well put the same switch on the screen four times over, three of those
    /// with no label beside it.
    #[test]
    fn a_gate_that_is_a_control_is_recognised_as_one() {
        assert!(gate_is_its_own_control("enableOverlay"));
        assert!(gate_is_its_own_control("publicLobby_on"));
        assert!(!gate_is_its_own_control("microphoneGainEnabled"));
        assert!(!gate_is_its_own_control("micSensitivityEnabled"));
        assert!(!gate_is_its_own_control("no_such_setting"));
    }

    /// Every gate that *is* drawn has a label to take.
    ///
    /// It takes the label of the control it gates, so a gate on a control that has none
    /// would be an unlabelled checkbox again — the exact thing this is here to prevent, and
    /// invisible until somebody opens the screen and looks.
    #[test]
    fn every_drawn_gate_has_a_label_to_borrow() {
        for control in controls() {
            let Some(gate) = control.gate else {
                continue;
            };
            if gate_is_its_own_control(gate) {
                continue;
            }
            assert!(
                control.label.is_some(),
                "{} gates {} and has no label for it",
                gate,
                control.key
            );
        }
    }

    /// A control that writes nothing must not share a key with a setting.
    ///
    /// The same collision `an_action_is_not_a_setting` guards, for the three kinds that
    /// were added beside it: any code that treated controls uniformly would write `testSpeaker`
    /// into `config.json` as though somebody had chosen it.
    #[test]
    fn nothing_that_writes_nothing_shadows_a_setting() {
        let schema = schema();
        for control in controls() {
            if control.kind.is_a_setting() || control.kind == Kind::Action {
                continue;
            }
            assert!(
                !schema.contains_key(control.key),
                "{} is both a {:?} and a setting",
                control.key,
                control.kind
            );
            assert!(
                control.warning.is_none(),
                "{} changes nothing, so there is nothing to confirm",
                control.key
            );
        }
    }

    /// Every key this screen can write: a control's own key, and any gate it names.
    fn reachable() -> BTreeSet<&'static str> {
        let mut keys = BTreeSet::new();
        for control in controls() {
            if control.kind != Kind::Action {
                keys.insert(control.key);
            }
            keys.extend(control.gate);
        }
        keys
    }

    fn locales() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../static/locales")
    }

    /// The load-bearing one.
    ///
    /// A setting that is in the schema, is written to `config.json`, and has no control
    /// left to reach it is not a missing feature anybody reports — it is a preference that
    /// quietly stops being adjustable. So every one of them is either on the screen or in
    /// `NOT_SHOWN` with a reason, and there is no third place to be.
    #[test]
    fn every_setting_is_reachable_or_named_here() {
        let reachable = reachable();
        let excused: BTreeSet<&str> = NOT_SHOWN.iter().map(|(key, _)| *key).collect();
        let mut missing = Vec::new();
        for key in schema().keys() {
            if !reachable.contains(key) && !excused.contains(key) {
                missing.push(*key);
            }
        }
        assert!(
            missing.is_empty(),
            "these settings have no control and no reason: {missing:?}"
        );
    }

    /// And the other direction: a control pointing at a key the schema does not have
    /// writes a setting nothing reads.
    ///
    /// Three kinds write nothing and so have nothing to point at. `Action` runs something,
    /// `Probe` tries something, and `Meter` only reads — each has a key because the screen
    /// needs one to build a widget id from, and none of them is a setting.
    #[test]
    fn every_control_points_at_a_real_setting() {
        let schema = schema();
        for control in controls() {
            if !control.kind.is_a_setting() {
                continue;
            }
            assert!(
                schema.contains_key(control.key),
                "{} is not in the schema",
                control.key
            );
        }
        for control in controls() {
            let Some(gate) = control.gate else {
                continue;
            };
            assert!(
                matches!(schema.get(gate), Some(Default_::Bool(_))),
                "{} is gated by {gate}, which is not a boolean setting",
                control.key
            );
        }
    }

    /// An excuse for a setting that does not exist is an excuse that has outlived its
    /// setting, and it would go on silencing the test above for a key nobody has.
    #[test]
    fn nothing_is_excused_that_the_schema_does_not_have() {
        let schema = schema();
        for (key, _) in NOT_SHOWN {
            assert!(schema.contains_key(key), "{key} is excused but not defined");
        }
    }

    /// A checkbox over a number, or a slider over a string, is a control that writes a
    /// value the rest of the client will not accept.
    #[test]
    fn every_control_agrees_with_the_type_of_its_setting() {
        let schema = schema();
        for control in controls() {
            let Some(default) = schema.get(control.key) else {
                continue;
            };
            let agrees = matches!(
                (control.kind, default),
                (Kind::Toggle, Default_::Bool(_))
					| (Kind::Slider { .. }, Default_::Number(_))
					// A choice may be over either -- the push-to-talk mode is a number and
					// the overlay position is a string.
					| (Kind::Choice(_), Default_::Number(_) | Default_::Text(_))
					| (
						Kind::Shortcut | Kind::Device { .. } | Kind::Text | Kind::Language,
						Default_::Text(_)
					)
            );
            assert!(
                agrees,
                "{} is a {:?} over a {default:?}",
                control.key, control.kind
            );
        }
    }

    /// A slider whose range does not contain its own default opens showing a value the
    /// player never chose, and saves it the moment they touch anything else.
    #[test]
    fn every_slider_contains_its_own_default() {
        let schema = schema();
        for control in controls() {
            let Kind::Slider { min, max, step, .. } = control.kind else {
                continue;
            };
            let Some(Default_::Number(default)) = schema.get(control.key) else {
                panic!(
                    "{} is a slider over something that is not a number",
                    control.key
                );
            };
            assert!(
                (min..=max).contains(default),
                "{} defaults to {default}, outside {min}..={max}",
                control.key
            );
            assert!(min < max, "{} has an empty range", control.key);
            assert!(step > 0.0, "{} has a step of {step}", control.key);
        }
    }

    /// The same for a choice: a default that is not one of the options shows as no
    /// selection at all.
    #[test]
    fn every_choice_offers_its_own_default() {
        let schema = schema();
        for control in controls() {
            let Kind::Choice(options) = control.kind else {
                continue;
            };
            let Some(default) = schema.get(control.key) else {
                continue;
            };
            assert!(
                options.iter().any(|choice| choice.value == *default),
                "{} defaults to {default:?}, which is not one of its options",
                control.key
            );
        }
    }

    /// Every label, warning and heading is a key the shipped catalogue has.
    ///
    /// English rather than all thirty-seven: the other locales are Crowdin's and are
    /// allowed to be incomplete, and `t` falls back to the key. English is the one that
    /// must be whole, because it is the fallback.
    #[test]
    fn every_string_on_the_screen_is_in_the_english_catalogue() {
        let catalogue = acl_i18n::Catalogue::load(&locales(), "en").expect("the shipped English");
        let mut wanted: Vec<&str> = Vec::new();
        for section in SECTIONS {
            wanted.extend(section.title);
            for control in section.controls {
                wanted.extend(control.label);
                wanted.extend(control.warning);
                if let Kind::Choice(options) = control.kind {
                    wanted.extend(options.iter().map(|choice: &Choice| choice.label));
                }
            }
        }
        // The two tooltips a lobby control shows when it cannot be changed. They are not on
        // a control, so nothing above would have collected them.
        wanted.extend(Scope::Lobby.unavailable(true));
        wanted.extend(Scope::Lobby.unavailable(false));

        let missing: Vec<&str> = wanted
            .into_iter()
            .filter(|key| !catalogue.defines(key))
            .collect();
        assert!(
            missing.is_empty(),
            "not in `en/translation.json`: {missing:?}"
        );
    }

    /// One setting per control. Two controls over one key are two places to change it and
    /// one of them will not be refreshed when the other does.
    #[test]
    fn no_setting_has_two_controls() {
        let mut seen = BTreeSet::new();
        for control in controls() {
            assert!(
                seen.insert(control.key),
                "{} has more than one control",
                control.key
            );
        }
    }

    /// The sensitivity slider runs the other way, and the round trip is what proves the
    /// two directions are actually inverse rather than merely both subtracting something.
    #[test]
    fn the_sensitivity_slider_runs_backwards_and_comes_back() {
        let control = controls()
            .find(|control| control.key == "micSensitivity")
            .expect("the sensitivity slider");
        assert!(matches!(control.kind, Kind::Slider { inverted: true, .. }));
        // The shipped default, 0.15, is a fairly open microphone, so it shows near the top.
        assert!((shown(control.kind, 0.15) - 0.85).abs() < 1e-9);
        for stored_value in [0.0, 0.15, 0.5, 1.0] {
            let round_trip = stored(control.kind, shown(control.kind, stored_value));
            assert!(
                (round_trip - stored_value).abs() < 1e-9,
                "{stored_value} came back as {round_trip}"
            );
        }
    }

    /// Every other slider is left alone by the same two functions.
    #[test]
    fn an_ordinary_slider_shows_what_is_stored() {
        let control = controls()
            .find(|control| control.key == "masterVolume")
            .expect("the master volume");
        assert!((shown(control.kind, 137.0) - 137.0).abs() < 1e-9);
        assert!((stored(control.kind, 137.0) - 137.0).abs() < 1e-9);
    }

    /// A lobby control says *why* it cannot be changed, and the two reasons are different:
    /// not being the host is not the same as there being no lobby.
    #[test]
    fn a_lobby_control_says_which_of_the_two_reasons_it_is() {
        assert_eq!(Scope::Client.unavailable(true), None);
        assert_ne!(
            Scope::Lobby.unavailable(true),
            Scope::Lobby.unavailable(false)
        );
        assert!(Scope::Lobby.unavailable(false).is_some());
    }

    /// The lobby rules are all in lobby-scoped sections, and none of the client's own
    /// settings are. Sending a client setting to every peer, or failing to send a lobby
    /// rule, are both the kind of mistake that only shows up with four people in a call.
    #[test]
    fn the_two_scopes_hold_the_two_lists() {
        let lobby: BTreeSet<&str> = lobby_defaults().into_iter().map(|(key, _)| key).collect();
        for section in SECTIONS {
            for control in section.controls {
                if !control.kind.is_a_setting() {
                    continue;
                }
                assert_eq!(
                    lobby.contains(control.key),
                    section.scope == Scope::Lobby,
                    "{} is in the wrong scope",
                    control.key
                );
            }
        }
    }

    /// A section with no controls draws a heading over nothing.
    #[test]
    fn no_section_is_empty() {
        for section in SECTIONS {
            assert!(!section.controls.is_empty(), "{:?} is empty", section.title);
        }
        assert!(controls().count() >= 30, "the screen lost most of itself");
    }

    /// A lobby rule the player cannot change says which of the two reasons it is, and a
    /// gated control says nothing -- the checkbox that would enable it is beside it.
    #[test]
    fn a_disabled_control_explains_itself_only_when_the_player_cannot_fix_it() {
        let gated = controls()
            .find(|control| control.key == "microphoneGain")
            .expect("the gain slider");
        let off = availability(gated, Scope::Client, false, true, false);
        assert!(!off.enabled);
        assert_eq!(off.reason, None, "the checkbox is right there");
        assert!(availability(gated, Scope::Client, true, true, false).enabled);

        let rule = controls()
            .find(|control| control.key == "wallsBlockAudio")
            .expect("a lobby rule");
        let not_host = availability(rule, Scope::Lobby, true, false, true);
        assert!(!not_host.enabled);
        assert_eq!(not_host.reason, Scope::Lobby.unavailable(true));
        assert!(availability(rule, Scope::Lobby, true, true, true).enabled);
    }

    /// When both apply, the reason given is the one the player cannot resolve. A host-only
    /// rule that is also gated would otherwise explain the half that is not the obstacle.
    #[test]
    fn the_lobby_reason_wins_over_the_gate() {
        let gated_rule = controls()
            .find(|control| control.key == "publicLobby_title")
            .expect("a gated lobby control");
        let both = availability(gated_rule, Scope::Lobby, false, false, false);
        assert!(!both.enabled);
        assert_eq!(both.reason, Scope::Lobby.unavailable(false));
    }

    /// Actions are not settings, and an action id that collided with one would be written
    /// into `config.json` by any code that treated the two uniformly.
    #[test]
    fn an_action_is_not_a_setting() {
        let schema = schema();
        for control in controls() {
            if control.kind != Kind::Action {
                continue;
            }
            assert!(
                !schema.contains_key(control.key),
                "{} is both an action and a setting",
                control.key
            );
            assert!(
                control.warning.is_some(),
                "{} does something irreversible without asking",
                control.key
            );
        }
    }
}
