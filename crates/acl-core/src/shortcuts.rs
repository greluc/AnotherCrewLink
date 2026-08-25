//! Turning a stored shortcut name into the key the poll will look for.
//!
//! This looks like a port of `src/main/keyBindings.ts` and is not one, in the way that
//! matters most.
//!
//! The **names** must survive exactly. They are what is written in 1.x's `config.json`,
//! and §4.10's rollout has the 2.0 build read that file while 1.x is still using it. A
//! name this side does not recognise is a shortcut a player finds simply dead after
//! upgrading, with nothing in any log to say why, and push-to-talk is one of the four.
//! `every_name_the_electron_client_accepts_is_accepted_here` reads `keyBindings.ts` and
//! fails if the two sets ever diverge.
//!
//! The **codes** must not. `keyBindings.ts` speaks libuiohook scancodes because the
//! Electron client installs `SetWindowsHookEx(WH_KEYBOARD_LL)`. §4.7 sends the port back
//! to `GetAsyncKeyState` — "a desktop-wide hook is a latency dependency in front of every
//! keystroke on the machine and is silently unhooked if a callback exceeds
//! `LowLevelHooksTimeout`; the Electron client accepted that to escape an unlicensed
//! dependency, which is not a constraint the port has" — and `GetAsyncKeyState` speaks
//! Windows virtual-key codes. Copying the table across would compile, pass a shape test,
//! and bind every shortcut to the wrong key.
//!
//! # What the poll cannot do
//!
//! One name does not survive the change of backend, and it is recorded rather than
//! guessed at: see [`Binding::Unsupported`].

/// What one stored name resolves to.
///
/// `Keys` holds a list because of the three unsided names. `GetAsyncKeyState(VK_SHIFT)`
/// was true for either shift key, so a player who bound `Shift` meant either one; the
/// sided names bind one each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    /// One or more virtual-key codes, any of which satisfies the shortcut.
    Keys(Vec<u16>),
    /// An extra mouse button, which arrives on a different event.
    Mouse(u8),
    /// Deliberately unbound, or an unrecognised name.
    ///
    /// Unrecognised is not an error: this runs while the client is starting, and a bad
    /// value in a settings file should cost one shortcut rather than the whole client.
    None,
    /// A name the polling backend cannot reproduce.
    ///
    /// Exactly one: `NumpadEnter`. libuiohook reports it as its own scancode, and
    /// `GetAsyncKeyState` cannot — the numeric keypad's Enter is `VK_RETURN` with the
    /// extended-key flag, and the flag is in the message, not in the key state.
    ///
    /// Mapping it to `VK_RETURN` would make the shortcut fire on the main Enter as well,
    /// which for push-to-talk means a hot microphone every time somebody sends a chat
    /// message. Silently binding nothing would leave a player with a dead shortcut and no
    /// explanation. So it resolves to this, and the caller tells them to rebind — which
    /// is the only one of the three that does not surprise anybody.
    ///
    /// A real fix exists and is not this module's: a raw-input or hook backend
    /// distinguishes the two, and taking one is the decision §4.7 declined for the whole
    /// keyboard path.
    Unsupported,
}

impl Binding {
    /// Whether a key the poll saw satisfies this shortcut.
    #[must_use]
    pub fn matches_key(&self, virtual_key: u16) -> bool {
        matches!(self, Self::Keys(codes) if codes.contains(&virtual_key))
    }

    /// Whether a mouse button satisfies it.
    #[must_use]
    pub fn matches_mouse(&self, button: u8) -> bool {
        matches!(self, Self::Mouse(bound) if *bound == button)
    }
}

/// Windows virtual-key codes, for the keys this client can bind.
mod vk {
    pub(super) const BACK: u16 = 0x08;
    pub(super) const RETURN: u16 = 0x0D;
    pub(super) const CAPITAL: u16 = 0x14;
    pub(super) const ESCAPE: u16 = 0x1B;
    pub(super) const SPACE: u16 = 0x20;
    pub(super) const PRIOR: u16 = 0x21;
    pub(super) const NEXT: u16 = 0x22;
    pub(super) const END: u16 = 0x23;
    pub(super) const HOME: u16 = 0x24;
    pub(super) const LEFT: u16 = 0x25;
    pub(super) const UP: u16 = 0x26;
    pub(super) const RIGHT: u16 = 0x27;
    pub(super) const DOWN: u16 = 0x28;
    pub(super) const DELETE: u16 = 0x2E;
    pub(super) const NUMPAD0: u16 = 0x60;
    pub(super) const MULTIPLY: u16 = 0x6A;
    pub(super) const ADD: u16 = 0x6B;
    pub(super) const SUBTRACT: u16 = 0x6D;
    pub(super) const DECIMAL: u16 = 0x6E;
    pub(super) const DIVIDE: u16 = 0x6F;
    pub(super) const F1: u16 = 0x70;
    pub(super) const LSHIFT: u16 = 0xA0;
    pub(super) const RSHIFT: u16 = 0xA1;
    pub(super) const LCONTROL: u16 = 0xA2;
    pub(super) const RCONTROL: u16 = 0xA3;
    pub(super) const LMENU: u16 = 0xA4;
    pub(super) const RMENU: u16 = 0xA5;
}

/// The extra mouse buttons, as `GetAsyncKeyState` reports them.
const XBUTTON1: u16 = 0x05;
const XBUTTON2: u16 = 0x06;

/// The virtual-key code an extra mouse button polls as.
///
/// The buttons are named 4 and 5 in the settings and stored that way; the poll asks for
/// `VK_XBUTTON1` and `VK_XBUTTON2`.
#[must_use]
pub const fn mouse_button_key(button: u8) -> Option<u16> {
    match button {
        4 => Some(XBUTTON1),
        5 => Some(XBUTTON2),
        _ => None,
    }
}

/// Resolves a stored shortcut name.
///
/// Single characters are uppercased first. The pre-1.0.4 code compared a character code
/// against a virtual-key code, and those are the uppercase values, so a lowercase letter
/// never matched anything — a shortcut that looked set and did nothing. Accepting either
/// is strictly more forgiving, and it is behaviour players already have.
#[must_use]
pub fn binding_for(name: &str) -> Binding {
    if name.is_empty() {
        return Binding::None;
    }

    if let Some(binding) = named(name) {
        return binding;
    }

    // A single character: a letter or a digit, by whichever case it was typed in.
    let mut characters = name.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        let upper = character.to_ascii_uppercase();
        if upper.is_ascii_uppercase() || upper.is_ascii_digit() {
            // 'A'..'Z' and '0'..'9' are their own virtual-key codes on Windows.
            return Binding::Keys(vec![upper as u16]);
        }
    }

    Binding::None
}

/// The names the settings store may hold.
///
/// Taken from `keyBindings.ts`'s `NAMED`, name for name. Renaming one needs a migration:
/// an unrecognised name binds nothing, and a player would find push-to-talk dead.
fn named(name: &str) -> Option<Binding> {
    let keys = |codes: &[u16]| Some(Binding::Keys(codes.to_vec()));
    Some(match name {
        "Disabled" => Binding::None,
        "Space" => return keys(&[vk::SPACE]),
        "Backspace" => return keys(&[vk::BACK]),
        "Delete" => return keys(&[vk::DELETE]),
        "Enter" => return keys(&[vk::RETURN]),
        "Up" => return keys(&[vk::UP]),
        "Down" => return keys(&[vk::DOWN]),
        "Left" => return keys(&[vk::LEFT]),
        "Right" => return keys(&[vk::RIGHT]),
        "Home" => return keys(&[vk::HOME]),
        "CapsLock" => return keys(&[vk::CAPITAL]),
        "End" => return keys(&[vk::END]),
        "PageUp" => return keys(&[vk::PRIOR]),
        "PageDown" => return keys(&[vk::NEXT]),
        "Escape" => return keys(&[vk::ESCAPE]),
        // The three unsided ones: either side satisfies them.
        "Control" => return keys(&[vk::LCONTROL, vk::RCONTROL]),
        "Shift" => return keys(&[vk::LSHIFT, vk::RSHIFT]),
        "Alt" => return keys(&[vk::LMENU, vk::RMENU]),
        "LShift" => return keys(&[vk::LSHIFT]),
        "RShift" => return keys(&[vk::RSHIFT]),
        "LAlt" => return keys(&[vk::LMENU]),
        "RAlt" => return keys(&[vk::RMENU]),
        "LControl" => return keys(&[vk::LCONTROL]),
        "RControl" => return keys(&[vk::RCONTROL]),
        "MouseButton4" => Binding::Mouse(4),
        "MouseButton5" => Binding::Mouse(5),
        "NumpadMultiply" => return keys(&[vk::MULTIPLY]),
        "NumpadAdd" => return keys(&[vk::ADD]),
        "NumpadSubtract" => return keys(&[vk::SUBTRACT]),
        "NumpadDecimal" => return keys(&[vk::DECIMAL]),
        "NumpadDivide" => return keys(&[vk::DIVIDE]),
        // See `Binding::Unsupported`.
        "NumpadEnter" => Binding::Unsupported,
        _ => {
            if let Some(digit) = name
                .strip_prefix("Numpad")
                .and_then(|d| d.parse::<u16>().ok())
                && digit <= 9
            {
                return keys(&[vk::NUMPAD0 + digit]);
            }
            if let Some(number) = name.strip_prefix('F').and_then(|n| n.parse::<u16>().ok())
                && (1..=12).contains(&number)
            {
                return keys(&[vk::F1 + number - 1]);
            }
            return None;
        }
    })
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_lowercase_letter_binds_the_same_key_as_an_uppercase_one() {
        // The 1.0.4 fix. The code before it compared a character code against a
        // virtual-key code, and those are the uppercase values, so a lowercase letter
        // looked set and did nothing.
        assert_eq!(binding_for("v"), binding_for("V"));
        assert_eq!(binding_for("V"), Binding::Keys(vec![0x56]));
    }

    #[test]
    fn a_digit_binds_its_own_key() {
        assert_eq!(binding_for("5"), Binding::Keys(vec![0x35]));
    }

    #[test]
    fn an_unsided_modifier_matches_either_side() {
        // `GetAsyncKeyState(VK_SHIFT)` was true for either, so a player who bound `Shift`
        // meant either. The other 1.0.4 fix.
        let shift = binding_for("Shift");
        assert!(shift.matches_key(vk::LSHIFT));
        assert!(shift.matches_key(vk::RSHIFT));
        for name in ["Control", "Alt"] {
            let binding = binding_for(name);
            assert!(
                matches!(&binding, Binding::Keys(codes) if codes.len() == 2),
                "{name}"
            );
        }
    }

    #[test]
    fn a_sided_modifier_matches_only_that_side() {
        let left = binding_for("LShift");
        assert!(left.matches_key(vk::LSHIFT));
        assert!(!left.matches_key(vk::RSHIFT));
    }

    #[test]
    fn the_function_and_numpad_ranges_resolve() {
        assert_eq!(binding_for("F1"), Binding::Keys(vec![0x70]));
        assert_eq!(binding_for("F12"), Binding::Keys(vec![0x7B]));
        assert_eq!(binding_for("Numpad0"), Binding::Keys(vec![0x60]));
        assert_eq!(binding_for("Numpad9"), Binding::Keys(vec![0x69]));
        // The operator keys, which were saved and resolved to nothing until 1.0.4 listed
        // them.
        assert_eq!(binding_for("NumpadAdd"), Binding::Keys(vec![0x6B]));
        assert_eq!(binding_for("NumpadDivide"), Binding::Keys(vec![0x6F]));
    }

    #[test]
    fn out_of_range_lookalikes_bind_nothing() {
        // `F13` and `Numpad10` are names nothing writes, and a prefix match that accepted
        // them would resolve to whatever came after the range in the table.
        assert_eq!(binding_for("F0"), Binding::None);
        assert_eq!(binding_for("F13"), Binding::None);
        assert_eq!(binding_for("Numpad10"), Binding::None);
    }

    #[test]
    fn the_mouse_buttons_are_a_different_event_and_a_different_code() {
        assert_eq!(binding_for("MouseButton4"), Binding::Mouse(4));
        assert!(binding_for("MouseButton4").matches_mouse(4));
        assert!(!binding_for("MouseButton4").matches_mouse(5));
        assert!(!binding_for("MouseButton4").matches_key(4));
        assert_eq!(mouse_button_key(4), Some(XBUTTON1));
        assert_eq!(mouse_button_key(5), Some(XBUTTON2));
        assert_eq!(mouse_button_key(3), None);
    }

    #[test]
    fn numpad_enter_is_refused_rather_than_bound_to_the_main_enter() {
        // `GetAsyncKeyState` cannot tell them apart: the numeric keypad's Enter is
        // `VK_RETURN` with the extended-key flag, and the flag is in the message, not in
        // the key state. Binding it to `VK_RETURN` would make push-to-talk open the
        // microphone every time somebody sent a chat message.
        assert_eq!(binding_for("NumpadEnter"), Binding::Unsupported);
        assert!(!binding_for("NumpadEnter").matches_key(vk::RETURN));
        // And it is distinguishable from a name nobody recognises, so the client can say
        // "rebind this" rather than "unknown shortcut".
        assert_ne!(binding_for("NumpadEnter"), Binding::None);
    }

    #[test]
    fn an_unknown_name_costs_one_shortcut_and_not_the_client() {
        assert_eq!(binding_for("Nonsense"), Binding::None);
        assert_eq!(binding_for(""), Binding::None);
        assert_eq!(binding_for("Disabled"), Binding::None);
    }

    #[test]
    fn every_name_the_electron_client_accepts_is_accepted_here() {
        // The migration surface. These names are in 1.x's `config.json` and §4.10 has the
        // 2.0 build read that file while 1.x still uses it. A name this side does not
        // know is a shortcut a player finds dead after upgrading, with nothing in any log
        // to say why -- and push-to-talk is one of the four.
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/main/keyBindings.ts"),
        )
        .expect("the Electron client is beside the crates");
        let start = source.find("const NAMED").expect("the NAMED table");
        let block = &source[start..];
        let end = block.find("\n};").expect("the end of the NAMED table");

        let mut missing = Vec::new();
        let mut checked = 0usize;
        for line in block[..end].lines() {
            let Some(name) = line.strip_prefix('\t').and_then(|l| l.split(':').next()) else {
                continue;
            };
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
                continue;
            }
            checked += 1;
            if binding_for(name) == Binding::None && name != "Disabled" {
                missing.push(name.to_owned());
            }
        }
        assert!(
            missing.is_empty(),
            "names the Electron client binds and this does not: {missing:?}"
        );
        // Without this the test passes by finding nothing -- a changed table layout, a
        // renamed constant, tabs turned into spaces -- and goes on passing while the two
        // sides drift. There were 54 names when this was written.
        assert!(
            checked >= 50,
            "only {checked} names were read out of keyBindings.ts; the parse has broken"
        );
    }
}
