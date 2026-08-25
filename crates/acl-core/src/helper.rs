//! Whether there is an elevated helper, what that costs, and when it may be asked for.
//!
//! §4.7 makes three commitments about the helper, and all three are easy to honour in
//! prose and easy to break in code:
//!
//! 1. **"The user clicked No" is an ordinary state, not a startup failure.**
//! 2. **Push-to-talk survives it.** The key poll is `GetAsyncKeyState`, which needs no
//!    elevation, so it is the one helper-side item that falls back into this process
//!    rather than disappearing with the helper. Losing the ability to speak because of a
//!    dialog is not a degradation anybody would accept.
//! 3. **The prompt fires at a moment the user can connect to something they did** — never
//!    from a background timer several minutes after launch, which reads as malware and
//!    gets answered No for that reason alone.
//!
//! The first two are [`Capabilities`]; the third is [`may_prompt`]. Neither touches a
//! platform API, which is why both are here and tested rather than discovered.

/// Where the helper is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HelperState {
    /// Nothing has needed it yet, so nothing has been asked.
    #[default]
    NotRequested,
    /// The elevation prompt is up, or the process is starting.
    Starting,
    /// Connected and answering.
    Running,
    /// The user answered No.
    ///
    /// Ordinary. The client runs without it and says so accurately.
    Refused,
    /// It was running and stopped — a crash, or the user killed it.
    Lost,
}

impl HelperState {
    /// Whether the helper can serve a request right now.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Running)
    }

    /// Whether asking again would be asking a second time.
    ///
    /// A refusal counts: the user has answered, and the answer was no.
    #[must_use]
    pub const fn has_been_asked(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::Refused | Self::Lost
        )
    }
}

/// What the client can do, given where the helper is.
///
/// Every field is answered rather than inferred, so that a new capability has to be
/// placed on one side of the line deliberately.
// Four named capabilities, and naming them is the point: a bitflag or a "degraded" enum
// would let a new one be added without anybody deciding which side of the elevation line
// it falls on, which is the mistake this type exists to prevent.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// Talking and hearing. Never depends on the helper: signalling, WebRTC and audio all
    /// live in this process.
    pub voice: bool,
    /// Push-to-talk, mute and deafen shortcuts.
    ///
    /// **Never false.** The hook lives in the helper for latency and for the elevated
    /// game's sake, but the fallback is a `GetAsyncKeyState` poll in this process, which
    /// needs no elevation of its own.
    pub push_to_talk: bool,
    /// Reading the game: who is where, who is dead, which map.
    pub game_reader: bool,
    /// The in-game overlay.
    pub overlay: bool,
}

impl Capabilities {
    /// What works, given the helper's state.
    #[must_use]
    pub const fn of(helper: HelperState) -> Self {
        let elevated = helper.is_available();
        Self {
            voice: true,
            push_to_talk: true,
            game_reader: elevated,
            overlay: elevated,
        }
    }

    /// Whether anything is missing, for the one line the user should see.
    #[must_use]
    pub const fn is_degraded(self) -> bool {
        !(self.game_reader && self.overlay)
    }
}

/// What made the client consider asking for elevation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The user opened the application.
    AppOpened,
    /// The user joined a lobby.
    LobbyJoined,
    /// The user asked for the game features explicitly, having done without them.
    UserAskedForIt,
    /// A timer, a retry, or anything else the user did not just do.
    ///
    /// Never prompts. A UAC dialog several minutes after launch, with nothing on screen
    /// that explains it, reads as malware — and is answered No for that reason rather
    /// than on its merits, which spends the one answer that matters.
    Background,
}

impl Trigger {
    /// Whether the user did this, and would recognise a prompt as following from it.
    #[must_use]
    pub const fn is_user_initiated(self) -> bool {
        matches!(
            self,
            Self::AppOpened | Self::LobbyJoined | Self::UserAskedForIt
        )
    }
}

/// Whether the elevation prompt may be shown.
///
/// Two rules, and a third that is a judgement rather than a quotation.
///
/// The prompt must follow something the user did, and there must be no helper already.
/// Both are §4.7's.
///
/// The third: **after a refusal, only an explicit request prompts again.** §4.7 says the
/// prompt fires at a moment the user can connect to something they did, and someone who
/// has just declined does not connect a second dialog to having joined a lobby — they
/// connect it to the first dialog, and read it as not taking no for an answer. So a
/// refusal is remembered, and the way back is a control the user reaches for, not a
/// lobby they happened to join. This is the one rule here the plan does not state
/// outright, and it is written down so that changing it is a decision.
#[must_use]
pub fn may_prompt(helper: HelperState, trigger: Trigger) -> bool {
    if !trigger.is_user_initiated() {
        return false;
    }
    match helper {
        HelperState::Running | HelperState::Starting => false,
        HelperState::Refused => matches!(trigger, Trigger::UserAskedForIt),
        HelperState::NotRequested | HelperState::Lost => true,
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    const EVERY_STATE: [HelperState; 5] = [
        HelperState::NotRequested,
        HelperState::Starting,
        HelperState::Running,
        HelperState::Refused,
        HelperState::Lost,
    ];

    const EVERY_TRIGGER: [Trigger; 4] = [
        Trigger::AppOpened,
        Trigger::LobbyJoined,
        Trigger::UserAskedForIt,
        Trigger::Background,
    ];

    #[test]
    fn voice_never_depends_on_the_helper() {
        // Signalling, WebRTC and audio are all in this process. If this ever goes false
        // the split has been drawn in the wrong place.
        for state in EVERY_STATE {
            assert!(Capabilities::of(state).voice, "{state:?}");
        }
    }

    #[test]
    fn push_to_talk_never_depends_on_the_helper() {
        // §4.7's second commitment, and the one worth a test of its own: the hook lives
        // in the helper, but the fallback is a `GetAsyncKeyState` poll here, which needs
        // no elevation. Losing the ability to speak because of a dialog is not a
        // degradation anybody would accept.
        for state in EVERY_STATE {
            assert!(Capabilities::of(state).push_to_talk, "{state:?}");
        }
    }

    #[test]
    fn the_game_reader_and_the_overlay_need_the_helper() {
        let with = Capabilities::of(HelperState::Running);
        assert!(with.game_reader);
        assert!(with.overlay);
        assert!(!with.is_degraded());

        for state in EVERY_STATE {
            if state == HelperState::Running {
                continue;
            }
            let without = Capabilities::of(state);
            assert!(!without.game_reader, "{state:?}");
            assert!(!without.overlay, "{state:?}");
            assert!(without.is_degraded(), "{state:?}");
        }
    }

    #[test]
    fn a_refusal_is_an_ordinary_state_and_not_a_failure() {
        // §4.7's first commitment. What distinguishes it from a startup failure is that
        // the client keeps working: everything that does not need elevation still does.
        let refused = Capabilities::of(HelperState::Refused);
        assert!(refused.voice);
        assert!(refused.push_to_talk);
        assert!(refused.is_degraded());
    }

    #[test]
    fn a_background_trigger_never_prompts() {
        // A UAC dialog minutes after launch, with nothing on screen explaining it, reads
        // as malware -- and gets answered No for that reason rather than on its merits,
        // which spends the one answer that matters.
        for state in EVERY_STATE {
            assert!(!may_prompt(state, Trigger::Background), "{state:?}");
        }
    }

    #[test]
    fn a_user_action_prompts_when_there_is_no_helper() {
        for trigger in [
            Trigger::AppOpened,
            Trigger::LobbyJoined,
            Trigger::UserAskedForIt,
        ] {
            assert!(
                may_prompt(HelperState::NotRequested, trigger),
                "{trigger:?}"
            );
        }
    }

    #[test]
    fn nothing_prompts_while_one_is_already_running_or_starting() {
        // Two prompts for one helper is the shape of a loop, and the second one arrives
        // while the user is still answering the first.
        for trigger in EVERY_TRIGGER {
            assert!(!may_prompt(HelperState::Running, trigger), "{trigger:?}");
            assert!(!may_prompt(HelperState::Starting, trigger), "{trigger:?}");
        }
    }

    #[test]
    fn a_refusal_is_not_reopened_by_joining_a_lobby() {
        // Someone who has just declined does not connect a second dialog to having
        // joined a lobby. They connect it to the first dialog, and read it as not taking
        // no for an answer.
        assert!(!may_prompt(HelperState::Refused, Trigger::LobbyJoined));
        assert!(!may_prompt(HelperState::Refused, Trigger::AppOpened));
    }

    #[test]
    fn a_refusal_is_reopened_by_asking_for_it() {
        // There has to be a way back that does not involve restarting the client.
        assert!(may_prompt(HelperState::Refused, Trigger::UserAskedForIt));
    }

    #[test]
    fn a_helper_that_died_may_be_restarted_by_an_ordinary_action() {
        // Unlike a refusal, this is not an answer the user gave. Rejoining a lobby is a
        // moment they would connect a prompt to.
        assert!(may_prompt(HelperState::Lost, Trigger::LobbyJoined));
    }

    #[test]
    fn every_state_and_trigger_is_decided() {
        // The matrix is small enough to enumerate, and a new variant that nobody placed
        // deliberately shows up here as a surprise rather than in the field.
        let allowed: Vec<(HelperState, Trigger)> = EVERY_STATE
            .into_iter()
            .flat_map(|state| EVERY_TRIGGER.map(move |trigger| (state, trigger)))
            .filter(|(state, trigger)| may_prompt(*state, *trigger))
            .collect();
        assert_eq!(
            allowed,
            [
                (HelperState::NotRequested, Trigger::AppOpened),
                (HelperState::NotRequested, Trigger::LobbyJoined),
                (HelperState::NotRequested, Trigger::UserAskedForIt),
                (HelperState::Refused, Trigger::UserAskedForIt),
                (HelperState::Lost, Trigger::AppOpened),
                (HelperState::Lost, Trigger::LobbyJoined),
                (HelperState::Lost, Trigger::UserAskedForIt),
            ]
        );
    }
}
