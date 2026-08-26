//! Finding the game's window, and whether this process is allowed to follow it.
//!
//! [`crate::overlay`] decides what to do; [`crate::fullscreen`] answers whether a layered
//! window would be composited at all. This answers the other half: whether the overlay may
//! attach to the game in the first place.
//!
//! # Why the question is not obvious
//!
//! UIPI blocks window manipulation across integrity levels, so an unelevated process
//! cannot drive a window belonging to an elevated one — and the README tells players to run
//! the game elevated. §4.7 puts the overlay in the elevated helper for exactly this reason,
//! and then asks for the check as well: "it is the difference between 'the overlay is
//! broken' and an accurate message about elevation."
//!
//! There is no API that asks it directly. What `native/electron-overlay-window`'s
//! `windows.c` does, and what this ports, is to try: post a registered message to the
//! window and see whether the attempt was refused.
//!
//! ```c
//! static bool has_uipi_access(HWND hwnd) {
//!   SetLastError(ERROR_SUCCESS);
//!   PostMessage(hwnd, WM_OVERLAY_UIPI_TEST, 0, 0);
//!   return GetLastError() != ERROR_ACCESS_DENIED;
//! }
//! ```
//!
//! **And the quirk beside it, which that file records and this reproduces**: a window whose
//! thread has stopped pumping messages refuses the post with the same error. Reading that
//! as "the game is elevated" would tell a player to restart the client as administrator
//! when what is actually happening is that their game is briefly busy. So a hung window is
//! its own answer, and `IsHungAppWindow` is what distinguishes them.
//!
//! The message is registered rather than invented. `RegisterWindowMessage` returns a value
//! in the range reserved for exactly this, so nothing in the game can be listening for it
//! and nothing this posts can mean anything to the receiver.

/// What was found, and what it means for the overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attachment {
    /// A window this process can follow.
    Available,
    /// A window this process may not touch, because the game is at a higher integrity
    /// level than this process.
    ///
    /// The case the elevated helper exists for. Recoverable, and the message says how.
    BlockedByElevation,
    /// A window that is not answering, so the question cannot be settled right now.
    ///
    /// Not a failure and not a refusal. A game that is loading a map, or that the graphics
    /// driver has stalled, looks exactly like this for a second or two, and the right
    /// response is to ask again rather than to tell the player anything.
    NotResponding,
    /// The game has no top-level window.
    ///
    /// It is starting, or it has just gone.
    NotFound,
}

impl Attachment {
    /// Whether the overlay can attach right now.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// Whether it is worth asking again in a moment.
    ///
    /// Elevation is the one answer that will not change on its own: a player has to act.
    /// The other two resolve by themselves, so a client that stopped asking would stay
    /// wrong for the rest of the session.
    #[must_use]
    pub const fn worth_retrying(self) -> bool {
        matches!(self, Self::NotResponding | Self::NotFound)
    }

    /// What to tell the player, or nothing when there is nothing useful to say.
    ///
    /// `None` for the two transient answers, and that is the point of them being separate
    /// variants: a message that appears while a map loads and disappears again is worse
    /// than no message, because the player reads it and acts on it.
    #[must_use]
    pub const fn message(self) -> Option<&'static str> {
        match self {
            Self::BlockedByElevation => Some(
                "The overlay cannot follow the game, because the game is running with more \
                 privileges than this client. Allow the elevation prompt, or start the game \
                 without running it as administrator.",
            ),
            Self::Available | Self::NotResponding | Self::NotFound => None,
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::Attachment;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_SUCCESS, GetLastError, HWND, LPARAM, SetLastError, WPARAM,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsHungAppWindow, IsWindowVisible, PostMessageW,
        RegisterWindowMessageW,
    };

    /// The message posted at the window to see whether the post is refused.
    ///
    /// Named after this client rather than reusing the Electron module's string, so that a
    /// 1.x and a 2.x testing the same window are not confused for each other by anything
    /// watching. Registered once: `RegisterWindowMessage` returns the same value for the
    /// same string for the lifetime of the session, so calling it repeatedly is waste
    /// rather than error, but the value is wanted on a hot-ish path.
    fn probe_message() -> u32 {
        static MESSAGE: OnceLock<u32> = OnceLock::new();
        *MESSAGE.get_or_init(|| {
            let name: Vec<u16> = OsStr::new("ANOTHERCREWLINK_OVERLAY_UIPI_TEST")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            // SAFETY: a documented call taking a null-terminated string that outlives it.
            unsafe { RegisterWindowMessageW(name.as_ptr()) }
        })
    }

    /// What `EnumWindows` is looking for and what it found.
    struct Search {
        process_id: u32,
        found: HWND,
    }

    /// The callback. Stops at the first visible top-level window of the process.
    ///
    /// Visible, because a process has several windows that are not the one on screen —
    /// message-only windows, `IME`, and the hidden ones a UI toolkit keeps for itself. The
    /// first visible top-level one is the game.
    unsafe extern "system" fn consider(window: HWND, state: LPARAM) -> i32 {
        // SAFETY: `state` is the pointer passed to EnumWindows below, which outlives the
        // enumeration because that call blocks until it is finished.
        let search = unsafe { &mut *(state as *mut Search) };
        let mut owner = 0u32;
        // SAFETY: a valid window handle from the enumeration and a live local.
        unsafe { GetWindowThreadProcessId(window, &raw mut owner) };
        // SAFETY: a valid window handle.
        if owner == search.process_id && unsafe { IsWindowVisible(window) } != 0 {
            search.found = window;
            return 0;
        }
        1
    }

    /// The game's main window, if it has one on screen.
    #[must_use]
    pub fn find_window(process_id: u32) -> Option<HWND> {
        let mut search = Search {
            process_id,
            found: std::ptr::null_mut(),
        };
        // SAFETY: the callback matches the documented signature, and the pointer refers to
        // a local that outlives the call because EnumWindows is synchronous.
        unsafe { EnumWindows(Some(consider), (&raw mut search) as LPARAM) };
        if search.found.is_null() {
            None
        } else {
            Some(search.found)
        }
    }

    /// Whether the overlay could attach to the game right now.
    #[must_use]
    pub fn attachment(process_id: u32) -> Attachment {
        let Some(window) = find_window(process_id) else {
            return Attachment::NotFound;
        };
        // Before the post, not after. A window whose thread has stopped pumping refuses
        // the post with the same error an integrity mismatch gives, and the two are
        // different things to tell somebody.
        // SAFETY: a valid window handle from the enumeration above.
        if unsafe { IsHungAppWindow(window) } != 0 {
            return Attachment::NotResponding;
        }

        // SAFETY: documented calls; the message is a registered id and the parameters are
        // ignored by anything that might receive it.
        let refused = unsafe {
            SetLastError(ERROR_SUCCESS);
            PostMessageW(
                window,
                probe_message(),
                WPARAM::default(),
                LPARAM::default(),
            );
            GetLastError() == ERROR_ACCESS_DENIED
        };
        if refused {
            Attachment::BlockedByElevation
        } else {
            Attachment::Available
        }
    }
}

#[cfg(windows)]
pub use platform::{attachment, find_window};

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::Attachment;

    /// Only one of the four is worth interrupting somebody about, and only one of the four
    /// will not fix itself.
    #[test]
    fn only_elevation_is_worth_saying_and_only_it_needs_a_person() {
        assert!(
            Attachment::BlockedByElevation.message().is_some(),
            "the one answer a player can act on has to say so"
        );
        for transient in [Attachment::NotResponding, Attachment::NotFound] {
            assert!(transient.message().is_none());
            assert!(transient.worth_retrying());
        }
        assert!(Attachment::Available.is_available());
        assert!(!Attachment::Available.worth_retrying());
        assert!(!Attachment::BlockedByElevation.worth_retrying());
    }

    /// A process id nothing owns has no window, and asking must not be a failure — the
    /// game not running is the state this spends most of its time in.
    #[cfg(windows)]
    #[test]
    fn a_process_that_does_not_exist_simply_has_no_window() {
        // Zero is the System Idle Process, which owns no windows and always exists, so
        // this asks the real question rather than one about a stale id.
        assert_eq!(super::attachment(0), Attachment::NotFound);
    }

    /// Against the real game, which is the only thing that can confirm the positive half.
    ///
    /// Ignored, because it needs Among Us to be running. Everything above tests the
    /// negative answers and the decisions; what it cannot test is whether `find_window`
    /// picks the game's window out of the several a Unity process owns, and a wrong pick
    /// there fails silently -- the overlay would follow a hidden window and simply never
    /// appear. Start the game, then:
    ///
    /// ```text
    /// cargo test -p acl-core -- --ignored the_game
    /// ```
    #[cfg(windows)]
    #[test]
    #[ignore = "needs Among Us to be running"]
    fn the_game_has_a_window_this_process_may_follow() {
        let game = acl_game::windows::find_process("Among Us.exe").expect("the game is running");
        let attachment = super::attachment(game);
        assert_ne!(
            attachment,
            Attachment::NotFound,
            "the game is running but no visible top-level window was found for it"
        );
        // Not asserted as `Available`: a player who started the game elevated should get
        // `BlockedByElevation`, and that is the right answer rather than a failure. What
        // must not happen is `NotFound`, which is the silent one.
        eprintln!("attachment to Among Us: {attachment:?}");
    }

    /// This process's own window, for the positive half.
    ///
    /// A console test binary has no visible top-level window, so this cannot assert
    /// `Available` — what it does assert is that asking about a process that certainly is
    /// not elevated relative to itself never comes back claiming elevation.
    #[cfg(windows)]
    #[test]
    fn this_process_is_never_blocked_from_itself() {
        assert_ne!(
            super::attachment(std::process::id()),
            Attachment::BlockedByElevation
        );
    }
}
