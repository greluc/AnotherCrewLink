//! Reading the keyboard, which is a poll and deliberately so.
//!
//! [`crate::shortcuts`] turns a stored name into the key to look for. This asks whether
//! that key is down, and turns a sequence of answers into the two events a shortcut
//! actually has: it went down, and it came up.
//!
//! # Why a poll
//!
//! §4.7 sends the port back to `GetAsyncKeyState`, against the direction the Electron
//! client went. 1.x installs `SetWindowsHookEx(WH_KEYBOARD_LL)` through `uiohook-napi`,
//! which puts a callback in front of every keystroke on the machine and is silently
//! unhooked if one exceeds `LowLevelHooksTimeout` — a latency dependency and a failure
//! mode, accepted there to escape an unlicensed dependency, which is not a constraint
//! this port has. `GetAsyncKeyState` is a direct call, needs no crate, and cannot be
//! unhooked because there is nothing hooked.
//!
//! # Why it survives without the helper
//!
//! Push-to-talk is the one helper-side item §4.7 keeps in the unelevated process, and the
//! reason is here rather than there: the async key state is global rather than per-window,
//! so reading it is not what UIPI filters. A player who answers the elevation prompt with
//! No loses the game reader and the overlay, and can still speak — which is the whole
//! point of that paragraph.
//!
//! # What it costs
//!
//! The shortest press this can miss is one interval; 1.x polled at 60 ms. The low bit of
//! `GetAsyncKeyState`, which reports whether the key went down since the previous call and
//! would close that window, is **not** used: it is cleared by whoever reads it, so a second
//! caller anywhere in the process eats presses from the first, and Microsoft documents it
//! as unreliable for exactly that reason. A shortcut that works except when something else
//! happens to poll is worse than one with a known interval.

use crate::shortcuts::{Binding, mouse_button_key};

/// Somewhere to ask whether a key is down.
///
/// A trait so the state machine below can be tested, which is most of what is worth
/// testing here: the platform call is one line and the edge logic is where a shortcut
/// gets stuck on.
pub trait KeyState {
    /// Whether that virtual-key code is down right now.
    fn is_down(&self, virtual_key: u16) -> bool;
}

/// What one poll of one shortcut found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// It was up and is now down.
    Pressed,
    /// It was down and is now up.
    Released,
    /// No change since the previous poll.
    Unchanged,
}

/// One shortcut, and whether it was down when it was last looked at.
///
/// Holds the previous answer because a shortcut is an edge and the platform only offers a
/// level. Push-to-talk needs the level; anything that toggles needs the edge, and getting
/// the edge from a level is remembering one bit.
#[derive(Clone, Debug)]
pub struct Shortcut {
    binding: Binding,
    down: bool,
}

impl Shortcut {
    /// A shortcut for a binding, starting from "not pressed".
    ///
    /// Starting from not-pressed rather than from the current state, and that is a choice:
    /// a client that starts while the player is holding push-to-talk should see the next
    /// press, not conclude the microphone is already open and then never see a release.
    #[must_use]
    pub const fn new(binding: Binding) -> Self {
        Self {
            binding,
            down: false,
        }
    }

    /// Whether it was down at the last poll.
    #[must_use]
    pub const fn is_down(&self) -> bool {
        self.down
    }

    /// What it is bound to.
    #[must_use]
    pub const fn binding(&self) -> &Binding {
        &self.binding
    }

    /// Rebinds it, and treats it as released.
    ///
    /// Released rather than re-read: a shortcut rebound while it was held would otherwise
    /// owe a release for a key nobody is going to let go of, and a push-to-talk stuck open
    /// is the failure that matters here.
    pub fn rebind(&mut self, binding: Binding) {
        self.binding = binding;
        self.down = false;
    }

    /// Looks once, and says what changed.
    pub fn poll(&mut self, keys: &impl KeyState) -> Edge {
        let down = self.currently_down(keys);
        let edge = match (self.down, down) {
            (false, true) => Edge::Pressed,
            (true, false) => Edge::Released,
            _ => Edge::Unchanged,
        };
        self.down = down;
        edge
    }

    /// Whether any key this binding names is down.
    ///
    /// Any, not all: the three unsided names resolve to both codes, so a player who bound
    /// `Shift` meant either shift key. `None` and `Unsupported` are never down — the
    /// second is `NumpadEnter`, which this backend cannot tell from the main Enter, and
    /// firing on both would mean a hot microphone every time somebody sends a chat
    /// message.
    fn currently_down(&self, keys: &impl KeyState) -> bool {
        match &self.binding {
            Binding::Keys(codes) => codes.iter().any(|code| keys.is_down(*code)),
            // Through the same call: the extra mouse buttons have virtual-key codes, so
            // there is one poll rather than a second path that could disagree with it.
            Binding::Mouse(button) => {
                mouse_button_key(*button).is_some_and(|code| keys.is_down(code))
            }
            Binding::None | Binding::Unsupported => false,
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::KeyState;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    /// The real keyboard.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct AsyncKeyState;

    impl KeyState for AsyncKeyState {
        fn is_down(&self, virtual_key: u16) -> bool {
            // SAFETY: a documented call taking an integer and returning one. It cannot
            // fail: an invalid code reads as not-down.
            let state = unsafe { GetAsyncKeyState(i32::from(virtual_key)) };
            // The high bit, and only the high bit. The low one reports "pressed since the
            // last call", which is cleared by whoever reads it — see the module
            // documentation for why this does not take that trade.
            #[expect(
                clippy::cast_sign_loss,
                reason = "a bit test on the returned word, whose sign bit is the flag"
            )]
            {
                state as u16 & 0x8000 != 0
            }
        }
    }
}

#[cfg(windows)]
pub use platform::AsyncKeyState;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Edge, KeyState, Shortcut};
    use crate::shortcuts::{Binding, binding_for};
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    /// A keyboard nobody is typing on, until a test says otherwise.
    #[derive(Default)]
    struct Fake {
        down: RefCell<BTreeSet<u16>>,
    }

    impl Fake {
        fn press(&self, code: u16) {
            self.down.borrow_mut().insert(code);
        }
        fn release(&self, code: u16) {
            self.down.borrow_mut().remove(&code);
        }
    }

    impl KeyState for Fake {
        fn is_down(&self, virtual_key: u16) -> bool {
            self.down.borrow().contains(&virtual_key)
        }
    }

    fn codes(binding: &Binding) -> Vec<u16> {
        match binding {
            Binding::Keys(codes) => codes.clone(),
            other => panic!("expected key codes, got {other:?}"),
        }
    }

    #[test]
    fn a_press_and_a_release_are_one_edge_each() {
        let keyboard = Fake::default();
        let binding = binding_for("V");
        let code = codes(&binding)[0];
        let mut shortcut = Shortcut::new(binding);

        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
        keyboard.press(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);
        // Held is not pressed again. A push-to-talk that reported a press on every poll
        // would open the microphone sixteen times a second.
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
        assert!(shortcut.is_down());
        keyboard.release(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Released);
        assert!(!shortcut.is_down());
    }

    /// The unsided names resolve to both codes and either one satisfies the shortcut,
    /// which is what `GetAsyncKeyState(VK_SHIFT)` used to do for free.
    #[test]
    fn either_side_satisfies_an_unsided_name() {
        let binding = binding_for("Shift");
        let both = codes(&binding);
        assert!(both.len() > 1, "Shift should resolve to more than one code");

        for code in both {
            let keyboard = Fake::default();
            let mut shortcut = Shortcut::new(binding_for("Shift"));
            keyboard.press(code);
            assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);
        }
    }

    /// `NumpadEnter` resolves to `Unsupported` rather than to `VK_RETURN`, and a shortcut
    /// bound to it must stay silent however hard the main Enter is pressed — the whole
    /// reason that variant exists is that firing on both means a hot microphone whenever
    /// somebody sends a chat message.
    #[test]
    fn an_unsupported_binding_never_fires() {
        let keyboard = Fake::default();
        let mut shortcut = Shortcut::new(binding_for("NumpadEnter"));
        assert_eq!(*shortcut.binding(), Binding::Unsupported);

        for code in 0..=0xFFu16 {
            keyboard.press(code);
        }
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
        assert!(!shortcut.is_down());
    }

    #[test]
    fn an_unbound_shortcut_never_fires() {
        let keyboard = Fake::default();
        let mut shortcut = Shortcut::new(binding_for("this is not a key name"));
        keyboard.press(0x41);
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
    }

    /// Rebinding while the old key is held must not leave a release owed for a key the
    /// player has no reason to let go of. The stuck case is push-to-talk, and it is stuck
    /// open.
    #[test]
    fn rebinding_while_held_does_not_owe_a_release() {
        let keyboard = Fake::default();
        let first = binding_for("V");
        let held = codes(&first)[0];
        let mut shortcut = Shortcut::new(first);

        keyboard.press(held);
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);

        shortcut.rebind(binding_for("B"));
        assert!(!shortcut.is_down());
        // The old key is still held and the new one is not, so nothing happens at all.
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);

        let now = codes(shortcut.binding())[0];
        keyboard.press(now);
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);
    }

    /// A client that starts while the key is already held sees the next press, not a
    /// microphone that is already open with no release coming.
    #[test]
    fn a_shortcut_starts_up_even_if_the_key_is_down() {
        let keyboard = Fake::default();
        let binding = binding_for("V");
        let code = codes(&binding)[0];
        keyboard.press(code);

        let mut shortcut = Shortcut::new(binding);
        assert!(!shortcut.is_down());
        // It is down now, so the first poll is a press — an honest one, with a release to
        // follow when the player lets go.
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);
    }

    /// The mouse buttons go through the same poll as the keys, so there is no second path
    /// that could disagree about whether a shortcut is held.
    #[test]
    fn an_extra_mouse_button_polls_like_a_key() {
        let binding = binding_for("MouseButton4");
        let Binding::Mouse(button) = binding else {
            panic!("MouseButton4 should resolve to a mouse binding, got {binding:?}");
        };
        let code = crate::shortcuts::mouse_button_key(button).expect("a virtual-key code");

        let keyboard = Fake::default();
        let mut shortcut = Shortcut::new(binding_for("MouseButton4"));
        keyboard.press(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);
        keyboard.release(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Released);
    }
}
