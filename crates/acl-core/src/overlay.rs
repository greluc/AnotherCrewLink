//! Whether the in-game overlay can appear, and the honest answer when it cannot.
//!
//! **Windows only since 2026-08-25.** This module used to hold two decisions, and the
//! larger one was Linux's: a `Backend` enum of `Windows`, `X11` and `Wayland`, gated
//! on the live winit backend rather than on `XDG_SESSION_TYPE`, so that an `XWayland`
//! session — which has a working overlay — was not greyed out by a variable that
//! describes the session instead of the backend the process actually got. That was the
//! subtle half, and it went with the client's Linux support.
//!
//! What is left is one bit: with Fullscreen Optimizations off, a layered window does not
//! appear at all. The alternative is a swapchain hook, which this project must not ship —
//! it is the technique anti-cheat systems exist to detect, in a client that already asks
//! players to run it beside a game. So the overlay says it cannot appear, and names the
//! setting the player can change.
//!
//! One bit does not need a module, and this one keeps its name anyway. A `bool` at the
//! call site says nothing about *which* way round it runs or what to tell the player; the
//! enum below is the message, and `availability` is where the commitment not to hook a
//! swapchain is written down. Whether the game is in exclusive fullscreen is a question
//! for the platform layer; what to do with the answer is this.

/// Whether the overlay can be shown, and if not, what to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayAvailability {
    /// It can be shown.
    Available,
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
}

/// What the overlay can do, given what the game is doing.
///
/// There was a `backend` argument here until Linux went. `is_recoverable_by_the_player`
/// went with it: it distinguished the fullscreen case, which a setting fixes, from
/// Wayland, which a setting does not, and with one unavailable case left it was only
/// `!is_available()` under another name.
#[must_use]
pub const fn availability(exclusive_fullscreen: bool) -> OverlayAvailability {
    if exclusive_fullscreen {
        OverlayAvailability::HiddenByExclusiveFullscreen
    } else {
        OverlayAvailability::Available
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn windows_has_the_overlay_the_probe_measured() {
        // `experiments/overlay-probe`: layered, transparent, topmost, exstyle 0x000c0138.
        assert_eq!(availability(false), OverlayAvailability::Available);
        assert!(availability(false).is_available());
    }

    #[test]
    fn exclusive_fullscreen_hides_it() {
        // With Fullscreen Optimizations off, a layered window is not composited. The
        // alternative is a swapchain hook, which this client will not ship.
        assert_eq!(
            availability(true),
            OverlayAvailability::HiddenByExclusiveFullscreen
        );
        assert!(!availability(true).is_available());
    }

    #[test]
    fn the_unavailable_case_is_named_rather_than_being_a_bare_false() {
        // The point of keeping an enum for one bit. A player told only "the overlay is
        // unavailable" has nowhere to go; this variant names Fullscreen Optimizations,
        // and a future second cause has somewhere to be added without changing callers
        // from `bool`.
        assert_ne!(
            OverlayAvailability::HiddenByExclusiveFullscreen,
            OverlayAvailability::Available
        );
    }
}
