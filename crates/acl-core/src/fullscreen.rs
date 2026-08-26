//! Whether something is running in exclusive fullscreen.
//!
//! [`crate::overlay::availability`] takes one bit and turns it into a message. This
//! produces the bit, and it is the only part of that pair that has to touch the platform.
//!
//! # Why this question and not a simpler one
//!
//! The tempting check is geometry: compare the foreground window's rectangle to its
//! monitor's and call a match fullscreen. That answers the wrong question. Borderless
//! fullscreen matches too, and borderless is the case where the overlay **works** — it is
//! composited like any other window. Greying the overlay out for it would break the
//! configuration most players are actually in, in the name of a case they are not.
//!
//! What matters is whether the desktop compositor has been bypassed, because a layered
//! window it is not compositing does not appear at all. Windows already answers exactly
//! that: `SHQueryUserNotificationState` reports `QUNS_RUNNING_D3D_FULL_SCREEN` when a
//! Direct3D application holds the display exclusively, and does not report it when
//! Fullscreen Optimizations have quietly turned the same request into a flip-model
//! borderless window. That distinction is the one §4.7 names, and it is the shell's to
//! make rather than ours to infer.
//!
//! `QUNS_BUSY` — a full-screen application that is not Direct3D — is deliberately **not**
//! counted. A full-screen console or video player does not take the display away from the
//! compositor, so the overlay is fine, and treating "busy" as "hidden" would tell a player
//! to change a game setting that was never the problem.
//!
//! That arm is the one this file gets right or wrong, so it is the one that was measured.
//! With Among Us running full-screen on 2026-08-26 the shell returned `QUNS_BUSY`, not
//! `QUNS_RUNNING_D3D_FULL_SCREEN` — so the common case does **not** take the
//! hidden-overlay path, which is what the mapping above needs to be true. The other half
//! of the pair is still owed: nobody has yet confirmed a `QUNS_RUNNING_D3D_FULL_SCREEN`
//! reading with the overlay genuinely invisible, and until somebody turns Fullscreen
//! Optimizations off and looks, that direction rests on the documentation rather than on
//! a measurement.
//!
//! # What it costs
//!
//! A shell call, not a field read. It belongs on the overlay's own cadence — once when the
//! overlay is about to be shown, and on a slow timer while it is — never on the audio path
//! or on a per-frame game tick.

/// What the shell says about the display right now.
///
/// Three answers rather than two: "nothing is holding the display" and "the question could
/// not be asked" are different, and collapsing them would hide a failing call behind a
/// working overlay for as long as it kept failing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayState {
    /// Nothing has taken the display away from the compositor.
    Composited,
    /// A Direct3D application holds the display exclusively.
    ExclusiveFullscreen,
    /// The shell did not answer.
    ///
    /// Treated as composited by [`Self::is_exclusive_fullscreen`], and on purpose: the
    /// consequence of guessing wrong in that direction is an overlay that is drawn and
    /// cannot be seen, and the consequence of guessing wrong the other way is an overlay
    /// that refuses to appear and tells the player to change a setting that is already
    /// correct. The first is a puzzle; the second is wrong advice.
    Unknown,
}

impl DisplayState {
    /// Whether a layered window would fail to appear.
    #[must_use]
    pub const fn is_exclusive_fullscreen(self) -> bool {
        matches!(self, Self::ExclusiveFullscreen)
    }

    /// What the overlay can do about it.
    #[must_use]
    pub const fn overlay(self) -> crate::overlay::OverlayAvailability {
        crate::overlay::availability(self.is_exclusive_fullscreen())
    }
}

#[cfg(windows)]
mod platform {
    use super::DisplayState;
    use windows_sys::Win32::UI::Shell::{
        QUERY_USER_NOTIFICATION_STATE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
    };

    /// Asks the shell.
    #[must_use]
    pub fn display_state() -> DisplayState {
        let mut state: QUERY_USER_NOTIFICATION_STATE = 0;
        // SAFETY: the out parameter is a live local for the duration of the call, which is
        // the whole of this function's contract with the shell.
        let result = unsafe { SHQueryUserNotificationState(&raw mut state) };
        if result < 0 {
            // A negative HRESULT. The documented one is E_FAIL from a session with no
            // interactive desktop, which this client is never in — so reaching here means
            // something unforeseen, and `Unknown` is the honest name for it.
            return DisplayState::Unknown;
        }
        if state == QUNS_RUNNING_D3D_FULL_SCREEN {
            DisplayState::ExclusiveFullscreen
        } else {
            DisplayState::Composited
        }
    }
}

#[cfg(windows)]
pub use platform::display_state;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::DisplayState;
    use crate::overlay::OverlayAvailability;

    /// The unknown case has to read as composited, or a shell call that started failing
    /// would take the overlay down with it and blame the player's graphics settings.
    #[test]
    fn only_exclusive_fullscreen_hides_the_overlay() {
        assert_eq!(
            DisplayState::ExclusiveFullscreen.overlay(),
            OverlayAvailability::HiddenByExclusiveFullscreen
        );
        assert_eq!(
            DisplayState::Composited.overlay(),
            OverlayAvailability::Available
        );
        assert_eq!(
            DisplayState::Unknown.overlay(),
            OverlayAvailability::Available
        );
    }

    /// The call itself, against whatever this machine is doing. It cannot assert which
    /// answer comes back — that depends on what is on screen — but it can assert that the
    /// call returns one at all, which is what a wrong feature flag or a moved symbol would
    /// break.
    #[cfg(windows)]
    #[test]
    fn the_shell_answers() {
        assert_ne!(
            super::display_state(),
            DisplayState::Unknown,
            "SHQueryUserNotificationState failed, which it should only do without an \
             interactive desktop"
        );
    }
}
