//! Whether the in-game overlay can appear, and the honest answer when it cannot.
//!
//! Two of §4.7's items, and both are named there because both have an obvious wrong
//! implementation:
//!
//! **Wayland detection gated on the live backend, not on `XDG_SESSION_TYPE`.** That
//! variable describes the session, not the backend the process actually got. A player on
//! a Wayland session whose window system handed them `XWayland` has a working overlay
//! today, and reading the variable would grey it out for them — a regression delivered as
//! a feature, to the users least able to argue with it.
//!
//! **Exclusive fullscreen.** With Fullscreen Optimizations off, a layered window does not
//! appear at all. The alternative is a swapchain hook, which this project must not ship:
//! it is the technique anti-cheat systems exist to detect, in a client that already asks
//! players to run it beside a game. So the overlay says it cannot appear and why, and the
//! setting the player can change is named.
//!
//! Neither decision touches a platform API here. What the backend is and whether the game
//! is in exclusive fullscreen are questions for the platform layer; what to do with the
//! answers is this.

/// The windowing backend the process actually got.
///
/// Deliberately not "the session type". `winit` reports which backend it initialised, and
/// that is the only thing that decides whether a layered, click-through, always-on-top
/// window is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// Win32. `experiments/overlay-probe` measured `layered=true transparent=true
    /// topmost=true` here.
    Windows,
    /// X11, including `XWayland`.
    X11,
    /// Wayland proper.
    Wayland,
}

/// Whether the overlay can be shown, and if not, what to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayAvailability {
    /// It can be shown.
    Available,
    /// Wayland gives no protocol for an always-on-top, click-through window that follows
    /// another application's.
    ///
    /// Not a bug and not something a setting fixes. An `XWayland` session works, which is
    /// worth saying, because for most players it is one login-screen choice away.
    UnsupportedCompositor,
    /// The game is in exclusive fullscreen, where a layered window is not composited.
    ///
    /// Recoverable, and by the player: turning Fullscreen Optimizations on, or running
    /// the game borderless, brings it back. That is the message, because the alternative
    /// this client will not implement is a swapchain hook.
    HiddenByExclusiveFullscreen,
}

impl OverlayAvailability {
    /// Whether the overlay should be drawn at all.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// Whether the player can do something about it.
    ///
    /// The two unavailable cases are not the same and must not share a message: one is a
    /// setting away and the other is a session away.
    #[must_use]
    pub const fn is_recoverable_by_the_player(self) -> bool {
        matches!(self, Self::HiddenByExclusiveFullscreen)
    }
}

/// What the overlay can do, given the backend and what the game is doing.
///
/// The compositor is checked first. On Wayland the overlay cannot appear whatever the
/// game does, so reporting a fullscreen problem there would send the player to change a
/// setting that changes nothing.
#[must_use]
pub const fn availability(backend: Backend, exclusive_fullscreen: bool) -> OverlayAvailability {
    match backend {
        Backend::Wayland => OverlayAvailability::UnsupportedCompositor,
        Backend::Windows | Backend::X11 if exclusive_fullscreen => {
            OverlayAvailability::HiddenByExclusiveFullscreen
        }
        Backend::Windows | Backend::X11 => OverlayAvailability::Available,
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn xwayland_keeps_the_overlay_it_has_today() {
        // The trap §4.7 names. `XDG_SESSION_TYPE=wayland` with an X11 backend is an
        // ordinary XWayland session, and the overlay works there. A check on the variable
        // would grey it out — a regression shipped as a feature.
        //
        // This function cannot make that mistake, because the session type is not one of
        // its arguments. The test says so, so that adding it later is a visible change.
        assert_eq!(
            availability(Backend::X11, false),
            OverlayAvailability::Available
        );
    }

    #[test]
    fn wayland_proper_is_unsupported_and_says_so_as_a_compositor_problem() {
        assert_eq!(
            availability(Backend::Wayland, false),
            OverlayAvailability::UnsupportedCompositor
        );
        assert!(!availability(Backend::Wayland, false).is_available());
    }

    #[test]
    fn wayland_does_not_blame_the_game_for_the_compositor() {
        // Reporting a fullscreen problem on Wayland sends the player to change a setting
        // that changes nothing, and they conclude the client is broken rather than
        // unsupported.
        assert_eq!(
            availability(Backend::Wayland, true),
            OverlayAvailability::UnsupportedCompositor
        );
    }

    #[test]
    fn exclusive_fullscreen_hides_the_overlay_on_a_backend_that_otherwise_works() {
        // With Fullscreen Optimizations off, a layered window is not composited. The
        // alternative is a swapchain hook, which this client will not ship.
        for backend in [Backend::Windows, Backend::X11] {
            assert_eq!(
                availability(backend, true),
                OverlayAvailability::HiddenByExclusiveFullscreen,
                "{backend:?}"
            );
        }
    }

    #[test]
    fn windows_has_the_overlay_the_probe_measured() {
        // `experiments/overlay-probe`: layered, transparent, topmost, exstyle 0x000c0138.
        assert_eq!(
            availability(Backend::Windows, false),
            OverlayAvailability::Available
        );
    }

    #[test]
    fn the_two_unavailable_cases_do_not_share_a_message() {
        // One is a setting away, the other is a session away. A single "the overlay is
        // unavailable" would send half the affected players looking in the wrong place.
        assert!(OverlayAvailability::HiddenByExclusiveFullscreen.is_recoverable_by_the_player());
        assert!(!OverlayAvailability::UnsupportedCompositor.is_recoverable_by_the_player());
        assert_ne!(
            OverlayAvailability::HiddenByExclusiveFullscreen,
            OverlayAvailability::UnsupportedCompositor
        );
    }

    #[test]
    fn every_combination_is_decided() {
        for backend in [Backend::Windows, Backend::X11, Backend::Wayland] {
            for fullscreen in [true, false] {
                let answer = availability(backend, fullscreen);
                // Nothing falls through to a default, and `Available` is only ever the
                // answer when it is really available.
                assert_eq!(
                    answer.is_available(),
                    backend != Backend::Wayland && !fullscreen,
                    "{backend:?} fullscreen={fullscreen}"
                );
            }
        }
    }
}
