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

/// Every key the settings screen can capture, as virtual-key codes.
///
/// The list exists so that capturing scans a fixed set rather than all 256 codes. Most of
/// what is left is unassigned, reserved, or a key no keyboard has -- and several of the
/// ones that *are* assigned would be caught by a scan and should not be: `VK_LBUTTON` is
/// the click that started the capture.
///
/// In the order it is scanned, and the order matters in one place: the sided modifiers
/// come before the unsided ones do not appear at all, so a player pressing the right
/// control key gets `RControl` rather than `Control`. Binding one side is what the
/// shipped defaults do -- `RControl` and `RAlt` -- and a capture that widened it to both
/// would quietly change what the shortcut does.
pub const CAPTURABLE: &[u16] = &[
    // Modifiers first: a player pressing Shift+something means the modifier, and the
    // other key is what they happened to hit next.
    vk::LSHIFT,
    vk::RSHIFT,
    vk::LCONTROL,
    vk::RCONTROL,
    vk::LMENU,
    vk::RMENU,
    XBUTTON1,
    XBUTTON2,
    vk::SPACE,
    vk::BACK,
    vk::DELETE,
    vk::RETURN,
    vk::UP,
    vk::DOWN,
    vk::LEFT,
    vk::RIGHT,
    vk::HOME,
    vk::END,
    vk::PRIOR,
    vk::NEXT,
    vk::CAPITAL,
    vk::ESCAPE,
    vk::MULTIPLY,
    vk::ADD,
    vk::SUBTRACT,
    vk::DECIMAL,
    vk::DIVIDE,
    // 0-9, A-Z, F1-F12, Numpad0-9. Written out because a range would need a `const fn`
    // loop and this is read once.
    //
    // **F12 and not F24.** `binding_for` resolves F1 to F12 and nothing above, and
    // `out_of_range_lookalikes_bind_nothing` records that as deliberate -- F13 is "a name
    // nothing writes". Capturing one would write it, and it would resolve to nothing: a
    // shortcut that looks set and does nothing. So a keyboard with the extra keys cannot
    // bind them, which is what the poll can honour.
    0x30,
    0x31,
    0x32,
    0x33,
    0x34,
    0x35,
    0x36,
    0x37,
    0x38,
    0x39,
    0x41,
    0x42,
    0x43,
    0x44,
    0x45,
    0x46,
    0x47,
    0x48,
    0x49,
    0x4A,
    0x4B,
    0x4C,
    0x4D,
    0x4E,
    0x4F,
    0x50,
    0x51,
    0x52,
    0x53,
    0x54,
    0x55,
    0x56,
    0x57,
    0x58,
    0x59,
    0x5A,
    0x60,
    0x61,
    0x62,
    0x63,
    0x64,
    0x65,
    0x66,
    0x67,
    0x68,
    0x69,
    0x70,
    0x71,
    0x72,
    0x73,
    0x74,
    0x75,
    0x76,
    0x77,
    0x78,
    0x79,
    0x7A,
    0x7B,
];

/// The name to store for a key the player just pressed.
///
/// The other direction from [`binding_for`], and the settings screen's half of it: a
/// capture reads a virtual-key code and has to write down something the poll will resolve
/// back to the same key. `every_captured_name_resolves_to_the_key_it_came_from` is what
/// holds the two together.
///
/// `None` for a code this client cannot name, which is most of them. A capture that
/// stored a name the poll does not recognise would look like it worked and do nothing.
#[must_use]
pub fn name_for(virtual_key: u16) -> Option<&'static str> {
    // The sided modifiers, not the unsided ones. A player who pressed the right control
    // key means that key; widening it to both is a change to what the shortcut does, made
    // silently, at the moment they were trying to be specific.
    let named = match virtual_key {
        vk::LSHIFT => "LShift",
        vk::RSHIFT => "RShift",
        vk::LCONTROL => "LControl",
        vk::RCONTROL => "RControl",
        vk::LMENU => "LAlt",
        vk::RMENU => "RAlt",
        XBUTTON1 => "MouseButton4",
        XBUTTON2 => "MouseButton5",
        vk::SPACE => "Space",
        vk::BACK => "Backspace",
        vk::DELETE => "Delete",
        vk::RETURN => "Enter",
        vk::UP => "Up",
        vk::DOWN => "Down",
        vk::LEFT => "Left",
        vk::RIGHT => "Right",
        vk::HOME => "Home",
        vk::END => "End",
        vk::PRIOR => "PageUp",
        vk::NEXT => "PageDown",
        vk::CAPITAL => "CapsLock",
        vk::ESCAPE => "Escape",
        vk::MULTIPLY => "NumpadMultiply",
        vk::ADD => "NumpadAdd",
        vk::SUBTRACT => "NumpadSubtract",
        vk::DECIMAL => "NumpadDecimal",
        vk::DIVIDE => "NumpadDivide",
        _ => return spelled(virtual_key),
    };
    Some(named)
}

/// The names that are spelled from the code rather than looked up: letters, digits, the
/// function keys and the numeric keypad.
///
/// Static strings rather than a `String`, so a capture allocates nothing and the return
/// type stays the same as the table above. The tables are the digits themselves, which is
/// why they are short enough to write out.
fn spelled(virtual_key: u16) -> Option<&'static str> {
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    // F1 to F12, matching what `binding_for` resolves. See `CAPTURABLE`.
    const FUNCTION: [&str; 12] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    ];
    const NUMPAD: [&str; 10] = [
        "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5", "Numpad6", "Numpad7",
        "Numpad8", "Numpad9",
    ];
    match virtual_key {
        0x30..=0x39 => DIGITS.get(usize::from(virtual_key - 0x30)).copied(),
        0x41..=0x5A => LETTERS.get(usize::from(virtual_key - 0x41)).copied(),
        vk::F1..=0x7B => FUNCTION.get(usize::from(virtual_key - vk::F1)).copied(),
        vk::NUMPAD0..=0x69 => NUMPAD.get(usize::from(virtual_key - vk::NUMPAD0)).copied(),
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

    /// The two directions have to agree, or a capture writes down a name the poll will
    /// not resolve -- a shortcut that looks set and does nothing, which is the exact bug
    /// `binding_for` already carries a comment about.
    #[test]
    fn every_captured_name_resolves_to_the_key_it_came_from() {
        for &code in CAPTURABLE {
            let name = name_for(code).unwrap_or_else(|| panic!("{code:#04X} has no name"));
            let binding = binding_for(name);
            match binding {
                Binding::Keys(ref codes) => assert!(
                    codes.contains(&code),
                    "{name} came from {code:#04X} and resolves to {codes:?}"
                ),
                Binding::Mouse(button) => assert_eq!(
                    mouse_button_key(button),
                    Some(code),
                    "{name} came from {code:#04X}"
                ),
                other => panic!("{name} came from {code:#04X} and resolves to {other:?}"),
            }
        }
    }

    /// A capture never widens a sided modifier. A player pressing the right control key
    /// means that key; `Control` would bind both, silently, at the moment they were being
    /// specific -- and the shipped defaults are sided, so it would also change what an
    /// untouched installation does.
    #[test]
    fn a_captured_modifier_keeps_its_side() {
        assert_eq!(name_for(vk::RCONTROL), Some("RControl"));
        assert_eq!(name_for(vk::LCONTROL), Some("LControl"));
        assert_eq!(name_for(vk::RMENU), Some("RAlt"));
        for name in [name_for(vk::LSHIFT), name_for(vk::RSHIFT)] {
            let name = name.expect("both shift keys are nameable");
            assert_ne!(name, "Shift", "a sided key was widened to both");
        }
    }

    /// The shipped defaults are all capturable, which is what says the screen can reproduce
    /// a fresh installation rather than only depart from it.
    #[test]
    fn the_shipped_defaults_can_all_be_captured() {
        for name in ["V", "RControl", "RAlt", "F"] {
            let Binding::Keys(codes) = binding_for(name) else {
                panic!("{name} is not a key binding");
            };
            let code = codes.first().copied().expect("a bound key");
            assert!(CAPTURABLE.contains(&code), "{name} cannot be captured");
            assert_eq!(name_for(code), Some(name));
        }
    }

    /// The function keys stop where `binding_for` stops.
    ///
    /// A keyboard with F13 upward cannot bind them, and that is the honest answer rather
    /// than a limitation: `out_of_range_lookalikes_bind_nothing` records F13 as a name
    /// nothing writes, so capturing one would store a shortcut that resolves to nothing.
    #[test]
    fn the_function_keys_stop_where_the_poll_stops() {
        assert_eq!(name_for(vk::F1 + 11), Some("F12"));
        assert_eq!(
            name_for(vk::F1 + 12),
            None,
            "F13 has no binding to come back to"
        );
        assert_eq!(binding_for("F13"), Binding::None);
    }

    /// Codes this client cannot name have no name rather than a guessed one.
    #[test]
    fn an_unnameable_code_has_no_name() {
        // `VK_LBUTTON` is the click that started the capture, and is deliberately absent.
        assert_eq!(name_for(0x01), None);
        assert_eq!(name_for(0x00), None);
        assert_eq!(name_for(0xFF), None);
        assert!(!CAPTURABLE.contains(&0x01));
    }

    /// `NumpadEnter` is the one name the poll cannot reproduce, and nothing captures it:
    /// the numeric keypad's Enter is `VK_RETURN` with an extended-key flag that is not in
    /// the key state, so a capture sees plain Enter and stores `Enter`, which is true.
    #[test]
    fn nothing_captures_the_name_the_poll_cannot_reproduce() {
        for &code in CAPTURABLE {
            assert_ne!(name_for(code), Some("NumpadEnter"), "{code:#04X}");
        }
        assert_eq!(binding_for("NumpadEnter"), Binding::Unsupported);
    }

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
