//! Where the window opens, and whether that is still somewhere a screen is.
//!
//! §4.8 item 1 asks for window state persistence with the shell. The persistence itself is
//! `serde_json` and a path `acl-core::paths::window_state_file` already names; what is here
//! is the two decisions around it, which are the parts that can be wrong.
//!
//! # The one that matters
//!
//! A saved rectangle can name a monitor that is no longer there — unplugged, or moved in the
//! display arrangement. Restore it anyway and the application opens at coordinates nothing
//! draws, which from the user's side is an application that did not start.
//!
//! The shipped client already lifted this out of its own window code, for the reason given
//! there: the caller reaches for the display list and this is a comparison of rectangles.
//! `src/main/windowOverlap.ts` is what this ports, quirks included and named.

use serde::{Deserialize, Serialize};

/// A window position and size, or a display's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: i32,
    /// Height.
    pub height: i32,
}

impl Rect {
    /// Whether two rectangles share any area at all.
    ///
    /// Strict inequalities, so rectangles that only touch along an edge do not count: a
    /// window whose right edge is exactly a monitor's left edge has nothing on that
    /// monitor.
    ///
    /// **A rectangle of zero width or height still counts as overlapping**, because a
    /// degenerate interval satisfies both strict comparisons. That is the shipped
    /// behaviour and it is kept rather than quietly fixed; `windowOverlap.ts` names it
    /// too. It means a saved window of zero size reads as visible and is restored as one
    /// nobody can grab — a narrow hole, since the bounds come from a real window, and
    /// closing it is a change to what ships rather than a port decision.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    /// Whether the window would be visible on at least one connected display.
    ///
    /// **Any overlap counts, including a single pixel.** Shipped behaviour, kept rather
    /// than tightened: a stricter rule would move windows that people had deliberately
    /// parked half off-screen, and this exists to catch the monitor that is gone, not to
    /// police placement. The consequence is that a window left one pixel on-screen is
    /// restored there with no title bar to grab — rare, and less bad than relocating a
    /// window somebody put where they wanted it.
    #[must_use]
    pub fn is_visible_on(self, displays: &[Self]) -> bool {
        displays.iter().any(|display| self.overlaps(*display))
    }
}

/// What is remembered about one window.
///
/// The position is optional and the size is not, which is the shape the shipped client
/// stores: a window that has never been moved has a size worth keeping and no position
/// worth restoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    /// Its width.
    pub width: i32,
    /// Its height.
    pub height: i32,
    /// Its left edge, if it has been placed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub x: Option<i32>,
    /// Its top edge, if it has been placed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub y: Option<i32>,
}

impl WindowState {
    /// A window of a given size, unplaced.
    #[must_use]
    pub const fn sized(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            x: None,
            y: None,
        }
    }

    /// Its rectangle, if it has one.
    #[must_use]
    pub const fn rect(self) -> Option<Rect> {
        match (self.x, self.y) {
            (Some(x), Some(y)) => Some(Rect {
                x,
                y,
                width: self.width,
                height: self.height,
            }),
            _ => None,
        }
    }
}

/// The whole of `windows.json`.
///
/// A map and not a single window, because that is what the file is: `electron-store` is
/// given the name `windows` and each window is a key in it. §4.10 has the 2.0 build read a
/// file 1.x is still writing, so the shape is the shipped one rather than a tidier one.
///
/// Unknown keys are kept on the way through. A 1.x that tracks a window this build does
/// not would otherwise lose its position the first time 2.x wrote the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Stored(pub std::collections::BTreeMap<String, WindowState>);

impl Stored {
    /// What was saved for one window.
    #[must_use]
    pub fn get(&self, window: &str) -> Option<WindowState> {
        self.0.get(window).copied()
    }

    /// Records one window's state, leaving every other key alone.
    pub fn set(&mut self, window: &str, state: WindowState) {
        self.0.insert(window.to_owned(), state);
    }
}

/// The key the shipped client stores the main window under.
///
/// `windowStateKeeper({ name: ... })` in `src/main/index.ts`. Getting it wrong is not an
/// error anywhere — it is a window that silently forgets its size across the upgrade.
pub const MAIN_WINDOW: &str = "main";

/// Where to open a window, given what was saved and what screens exist.
///
/// Falls back to the default size, unplaced, whenever the saved rectangle would not be
/// visible — which includes the case of a saved size with no position, because a window
/// that has never been placed has nothing to check and the window manager should choose.
///
/// The fallback drops the *saved size* as well as the position, and that is the shipped
/// behaviour rather than an oversight: `windowStateKeeper` replaces the whole state with
/// the defaults. A monitor that is gone was often a different size, so keeping its
/// dimensions on the remaining one is as likely to be wrong as right.
#[must_use]
pub fn restore(
    saved: Option<WindowState>,
    displays: &[Rect],
    default_width: i32,
    default_height: i32,
) -> WindowState {
    let fallback = WindowState::sized(default_width, default_height);
    let Some(saved) = saved else {
        return fallback;
    };
    match saved.rect() {
        Some(rect) if rect.is_visible_on(displays) => saved,
        _ => fallback,
    }
}

/// The file the shipped client reads once, for people upgrading from further back.
///
/// `electron-window-state` — replaced in 1.x because it was last published in 2022 — wrote a
/// flat `{width,height,x,y}` into `window-state.json` beside the newer `windows.json`.
/// `windowState.ts` still reads it when there is nothing under the window's key, so that
/// somebody upgrading keeps the size they had.
///
/// Worth carrying because the population it serves is exactly the one 2.0 inherits: anybody
/// who last ran a build older than that replacement and then jumps to the Rust client has
/// only this file.
pub const LEGACY_FILE: &str = "window-state.json";

/// Reads that file's contents, if they are usable.
///
/// **Both dimensions must be numbers**, which is the check `windowState.ts` makes and the
/// only one it makes: a file half-written by a crash, or one belonging to something else
/// that happened to use the name, is ignored rather than turned into a window of no size.
/// The coordinates are taken as they come, because a wrong position is caught by
/// [`restore`] and a wrong size is not.
#[must_use]
pub fn from_legacy(contents: &str) -> Option<WindowState> {
    let parsed: serde_json::Value = serde_json::from_str(contents).ok()?;
    let number = |key: &str| -> Option<i32> {
        parsed
            .get(key)
            .and_then(serde_json::Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
    };
    Some(WindowState {
        width: number("width")?,
        height: number("height")?,
        x: number("x"),
        y: number("y"),
    })
}

/// Whether the current bounds are worth writing down.
///
/// A minimised, maximised or full-screen window reports the bounds of that state rather
/// than the ones to restore to, so saving then means reopening maximised-sized-but-not-
/// maximised, or at a minimised window's coordinates. `windowStateKeeper` skips all three
/// and this does the same.
#[must_use]
pub const fn worth_saving(minimised: bool, maximised: bool, fullscreen: bool) -> bool {
    !(minimised || maximised || fullscreen)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Rect, WindowState, restore, worth_saving};

    const SCREEN: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    const SECOND_SCREEN: Rect = Rect {
        x: 1920,
        y: 0,
        width: 1920,
        height: 1080,
    };

    fn placed_at(width: i32, height: i32, x: i32, y: i32) -> WindowState {
        WindowState {
            width,
            height,
            x: Some(x),
            y: Some(y),
        }
    }

    fn placed(x: i32, y: i32) -> WindowState {
        WindowState {
            width: 800,
            height: 600,
            x: Some(x),
            y: Some(y),
        }
    }

    /// Touching along an edge is not overlapping: a window whose right edge is exactly a
    /// monitor's left edge has nothing on that monitor.
    #[test]
    fn a_shared_edge_is_not_a_shared_area() {
        let against = Rect {
            x: -800,
            y: 0,
            width: 800,
            height: 600,
        };
        assert!(!against.overlaps(SCREEN));
        assert!(
            Rect { x: -799, ..against }.overlaps(SCREEN),
            "one pixel over the edge does overlap"
        );
    }

    /// The quirk the shipped client names and keeps: a degenerate rectangle satisfies both
    /// strict comparisons, so a zero-sized window reads as visible.
    #[test]
    fn a_window_of_no_size_still_counts_as_visible() {
        let nothing = Rect {
            x: 10,
            y: 10,
            width: 0,
            height: 0,
        };
        assert!(nothing.overlaps(SCREEN));
    }

    /// A single pixel is enough, deliberately. The rule exists to catch a monitor that is
    /// gone, not to move a window somebody parked where they wanted it.
    #[test]
    fn one_pixel_on_a_screen_is_enough_to_be_restored() {
        let barely = placed(1919, 1079);
        assert_eq!(restore(Some(barely), &[SCREEN], 640, 480), barely);
    }

    /// The case it exists for: the monitor the window was on is no longer there.
    #[test]
    fn a_window_on_a_monitor_that_is_gone_falls_back_to_the_defaults() {
        let elsewhere = placed(2400, 300);
        assert_eq!(
            restore(Some(elsewhere), &[SECOND_SCREEN], 640, 480),
            elsewhere,
            "with that monitor connected it is restored"
        );
        assert_eq!(
            restore(Some(elsewhere), &[SCREEN], 640, 480),
            WindowState::sized(640, 480),
            "without it, the defaults"
        );
    }

    /// The fallback drops the saved size too, which is what the shipped keeper does: a
    /// monitor that is gone was often a different size, so its dimensions are as likely to
    /// be wrong as right on the one that is left.
    #[test]
    fn the_fallback_forgets_the_size_as_well_as_the_position() {
        let big = WindowState {
            width: 3000,
            height: 2000,
            x: Some(4000),
            y: Some(0),
        };
        assert_eq!(
            restore(Some(big), &[SCREEN], 640, 480),
            WindowState::sized(640, 480)
        );
    }

    /// A saved size with no position has nothing to check, so the window manager chooses.
    #[test]
    fn a_size_with_no_position_is_not_restored_as_a_position() {
        let unplaced = WindowState::sized(1000, 700);
        assert_eq!(
            restore(Some(unplaced), &[SCREEN], 640, 480),
            WindowState::sized(640, 480)
        );
    }

    #[test]
    fn nothing_saved_is_the_defaults() {
        assert_eq!(
            restore(None, &[SCREEN], 640, 480),
            WindowState::sized(640, 480)
        );
    }

    /// A window with no displays at all — which is what an unplugged laptop dock looks like
    /// for a moment — is not restored to coordinates nothing draws.
    #[test]
    fn no_displays_means_the_defaults() {
        assert_eq!(
            restore(Some(placed(10, 10)), &[], 640, 480),
            WindowState::sized(640, 480)
        );
    }

    /// A minimised, maximised or full-screen window reports the bounds of *that* state, so
    /// saving them means reopening at a minimised window's coordinates or
    /// maximised-sized-but-not-maximised.
    #[test]
    fn only_an_ordinary_window_is_worth_saving() {
        assert!(worth_saving(false, false, false));
        assert!(!worth_saving(true, false, false));
        assert!(!worth_saving(false, true, false));
        assert!(!worth_saving(false, false, true));
    }

    /// The stored shape is the shipped one: an unplaced window writes no coordinates at
    /// all rather than nulls, because that is what a 1.x `windows.json` contains and §4.10
    /// has the 2.0 build read the file 1.x is still writing.
    #[test]
    fn an_unplaced_window_stores_no_coordinates() {
        let text = serde_json::to_string(&WindowState::sized(800, 600)).expect("it serialises");
        assert_eq!(text, r#"{"width":800,"height":600}"#);
        assert_eq!(
            serde_json::from_str::<WindowState>(&text).expect("it parses"),
            WindowState::sized(800, 600)
        );
    }

    /// The file is a map keyed by window name, and a key this build does not know must
    /// survive being read and written — otherwise a 1.x tracking a window 2.x does not
    /// loses its position the first time the newer client saves.
    #[test]
    fn an_unknown_window_in_the_file_is_not_lost() {
        let text =
            r#"{"main":{"width":800,"height":600},"someOtherWindow":{"width":1,"height":2}}"#;
        let mut stored: super::Stored = serde_json::from_str(text).expect("it parses");
        assert_eq!(stored.get("main"), Some(WindowState::sized(800, 600)));
        stored.set("main", WindowState::sized(900, 700));
        let written = serde_json::to_string(&stored).expect("it serialises");
        assert!(
            written.contains("someOtherWindow"),
            "a window this build does not know was dropped: {written}"
        );
    }

    /// The older file is flat, and only its two dimensions are required.
    #[test]
    fn the_legacy_file_is_read_when_it_has_a_size() {
        assert_eq!(
            super::from_legacy(r#"{"width":1024,"height":768,"x":10,"y":20}"#),
            Some(placed_at(1024, 768, 10, 20))
        );
        assert_eq!(
            super::from_legacy(r#"{"width":1024,"height":768}"#),
            Some(WindowState::sized(1024, 768))
        );
    }

    /// And ignored otherwise. A file half-written by a crash, or one belonging to something
    /// else that used the name, must not become a window of no size.
    #[test]
    fn a_legacy_file_without_a_size_is_ignored() {
        for contents in [
            "{}",
            r#"{"width":1024}"#,
            r#"{"width":"1024","height":"768"}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(
                super::from_legacy(contents),
                None,
                "{contents:?} should not have parsed"
            );
        }
    }

    /// And a placed one round-trips with them.
    #[test]
    fn a_placed_window_round_trips() {
        let state = placed(100, 200);
        let text = serde_json::to_string(&state).expect("it serialises");
        assert_eq!(
            serde_json::from_str::<WindowState>(&text).expect("it parses"),
            state
        );
    }
}
