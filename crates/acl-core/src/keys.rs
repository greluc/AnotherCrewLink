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
    /// What the last poll saw, or `None` before there has been one.
    ///
    /// Three states rather than two, and the third is a fix from 2026-08-29. It used to
    /// start at "not pressed", which is a claim rather than an observation -- and it is
    /// wrong in exactly the case that matters. A player binds mute by *pressing the key*,
    /// so at the moment the new shortcut is built that key is held down. The next poll
    /// then read false-to-true and called it a press: the player bound mute and was
    /// instantly muted, bound deafen and was instantly deafened. `Voice.tsx` carries a
    /// comment about this same bug.
    ///
    /// `None` means "look, and believe what you see, without calling it a change". The
    /// release that follows is still seen -- it is a genuine true-to-false -- so nothing
    /// is left owing an edge, which was the fear the old comment named and did not
    /// actually avoid.
    down: Option<bool>,
}

impl Shortcut {
    /// A shortcut for a binding, which has not been looked at yet.
    #[must_use]
    pub const fn new(binding: Binding) -> Self {
        Self {
            binding,
            down: None,
        }
    }

    /// Whether it was down at the last poll.
    ///
    /// `false` before the first one, which is what a level-triggered caller wants:
    /// push-to-talk is closed until something has actually looked.
    #[must_use]
    pub const fn is_down(&self) -> bool {
        matches!(self.down, Some(true))
    }

    /// What it is bound to.
    #[must_use]
    pub const fn binding(&self) -> &Binding {
        &self.binding
    }

    /// Rebinds it, and forgets what the old key was doing.
    ///
    /// Forgets rather than assumes released: a shortcut is rebound at the moment somebody
    /// finishes pressing the new key, so "released" is the one thing it reliably is not.
    /// The next poll adopts whatever the new key is actually doing, without calling it a
    /// change.
    pub fn rebind(&mut self, binding: Binding) {
        self.binding = binding;
        self.down = None;
    }

    /// Looks once, and says what changed.
    pub fn poll(&mut self, keys: &impl KeyState) -> Edge {
        let down = self.currently_down(keys);
        let edge = match self.down {
            // The first look. Whatever it finds is the starting position, not a change:
            // a key that is already held was not just pressed.
            None => Edge::Unchanged,
            Some(was) => match (was, down) {
                (false, true) => Edge::Pressed,
                (true, false) => Edge::Released,
                _ => Edge::Unchanged,
            },
        };
        self.down = Some(down);
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
            // One poll to establish that nothing is held. A shortcut's first look is an
            // observation rather than a change -- see
            // `a_key_already_held_is_adopted_rather_than_read_as_a_press`.
            assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
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

        // Established first, as the thirty-millisecond poll does in practice.
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
        keyboard.press(held);
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);

        shortcut.rebind(binding_for("B"));
        assert!(!shortcut.is_down());
        // The old key is still held and the new one is not, so nothing happens at all.
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
        // And once more, because the first poll after a rebind is now the establishing
        // one: the press below has to be a press against a known starting position.
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);

        let now = codes(shortcut.binding())[0];
        keyboard.press(now);
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);
    }

    /// A key that was already held when the shortcut was made was not just pressed.
    ///
    /// This asserted the opposite until 2026-08-29, and the opposite muted people. A
    /// shortcut is bound by *pressing the key*, so at the instant the new `Shortcut` is
    /// built that key is down; reading the first poll as a press meant binding mute muted
    /// you and binding deafen deafened you, on the spot, every time. `Voice.tsx` carries a
    /// comment about the same bug.
    #[test]
    fn a_key_already_held_is_adopted_rather_than_read_as_a_press() {
        let keyboard = Fake::default();
        let binding = binding_for("V");
        let code = codes(&binding)[0];
        keyboard.press(code);

        let mut shortcut = Shortcut::new(binding);
        assert!(!shortcut.is_down(), "nothing has looked yet");

        // The first look adopts. It is a starting position, not a change.
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
        assert!(shortcut.is_down(), "and it is honest about what it found");

        // Nothing is left owing an edge: the release is a genuine true-to-false, and the
        // press after it is a genuine press.
        keyboard.release(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Released);
        keyboard.press(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);
    }

    /// The same rule after a rebind, which is where it actually bites.
    #[test]
    fn rebinding_onto_a_held_key_does_not_fire_it() {
        let keyboard = Fake::default();
        let mut shortcut = Shortcut::new(binding_for("V"));

        // The player opens the settings and presses `B` to bind it. `B` is held at the
        // moment the rebind happens, because pressing it is how it was chosen.
        let held = binding_for("B");
        let code = codes(&held)[0];
        keyboard.press(code);
        shortcut.rebind(held);

        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
        keyboard.release(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Released);
        keyboard.press(code);
        assert_eq!(
            shortcut.poll(&keyboard),
            Edge::Pressed,
            "the first deliberate press after the rebind is the first press"
        );
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
        // One poll to establish that it is up, which is what a shortcut bound while its
        // button is *not* held sees.
        assert_eq!(shortcut.poll(&keyboard), Edge::Unchanged);
        keyboard.press(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Pressed);
        keyboard.release(code);
        assert_eq!(shortcut.poll(&keyboard), Edge::Released);
    }
}
