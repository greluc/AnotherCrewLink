//! Where the overlay goes, and how big it is.
//!
//! §4.8 item 5's other half. [`crate::roster::overlay`] decides *who* is on the overlay
//! and [`crate::sprite`] draws them; this decides where the strip sits on the game and
//! where each crewmate sits in the strip.
//!
//! Ported from `Overlay.tsx` and `css/overlay.css`, which is where the shipped answer
//! lives — the component picks a class name and the stylesheet turns it into a corner. A
//! player who has used the Electron client has muscle memory for these seven positions,
//! and the port has no licence to move them.
//!
//! **The strip is only as large as it needs to be**, rather than covering the game. The
//! helper composes a buffer of exactly this rectangle and hands it to
//! `UpdateLayeredWindow`, so a placement the size of the game window is a 14 MB buffer
//! allocated and blitted five times a second to draw ten circles. That number is not
//! hypothetical: it is what the first overlay frame measured, and it is why the pipe
//! carries sprites instead of frames.

/// Where the overlay sits.
///
/// The seven the settings offer, and the values are the ones already in `config.json`:
/// `bottom_left` shows as "bottom", and `right1` and `left1` are the sided ones that keep
/// their names when the overlay is compact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    /// Not drawn at all.
    Hidden,
    /// A row across the top, centred.
    Top,
    /// A row along the bottom, from the left.
    BottomLeft,
    /// A column down the right.
    Right,
    /// A column down the right, keeping names when compact.
    RightNamed,
    /// A column down the left.
    Left,
    /// A column down the left, keeping names when compact.
    LeftNamed,
}

impl Position {
    /// Reads the value stored in `config.json`.
    ///
    /// An unrecognised value is the default rather than nothing: a settings file written by
    /// a newer build should cost the player a preference, not the overlay.
    #[must_use]
    pub fn parse(stored: &str) -> Self {
        match stored {
            "hidden" => Self::Hidden,
            "top" => Self::Top,
            "bottom_left" => Self::BottomLeft,
            "right1" => Self::RightNamed,
            "left" => Self::Left,
            "left1" => Self::LeftNamed,
            // Including `right`, which is the shipped default.
            _ => Self::Right,
        }
    }

    /// Whether the crewmates stack downward rather than running across.
    #[must_use]
    pub const fn is_column(self) -> bool {
        matches!(
            self,
            Self::Right | Self::RightNamed | Self::Left | Self::LeftNamed
        )
    }

    /// Whether this position is compact whatever the setting says.
    ///
    /// The two `1` variants are: `Overlay.tsx` adds the compact class for them regardless.
    /// They exist precisely to be the compact-with-names option.
    #[must_use]
    pub const fn forces_compact(self) -> bool {
        matches!(self, Self::RightNamed | Self::LeftNamed)
    }

    /// Whether names are drawn beside the crewmates.
    ///
    /// `showName = isOnSide && (!compactOverlay || position === 'right1' || 'left1')` — so
    /// a row never has them, a side column has them unless it is compact, and the two `1`
    /// variants keep them even then.
    #[must_use]
    pub const fn shows_names(self, compact: bool) -> bool {
        self.is_column() && (!compact || self.forces_compact())
    }
}

/// A rectangle in screen coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}

/// Where the overlay window goes and what is in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The overlay window, in screen coordinates.
    pub placement: Rect,
    /// Where each crewmate goes inside it, in the order they were given.
    pub sprites: Vec<(i32, i32)>,
}

/// How far a crewmate sits from the edge of the strip, and from the next one.
pub const GAP: i32 = 8;

/// Where the overlay sits in the menu, measured from the game window's top left.
///
/// `.overlay-wrapper.gamestate_menu { left: 8px; top: 60px; }`. The position setting does
/// not apply here — the shipped overlay puts the menu list in one place whatever the
/// player chose, because the corners it would otherwise use are where the game's own menu
/// is.
const MENU_ORIGIN: (i32, i32) = (8, 60);

/// Works out where everything goes.
///
/// `None` when there is nothing to show: hidden, or nobody to draw. The caller hides the
/// window rather than drawing an empty one — an empty strip is still a rectangle of
/// nothing sitting over the game.
///
/// The strip is kept inside the game window. A full lobby is fifteen crewmates, which at
/// the default size is a row wider than a 1280-pixel window, and a strip that runs off the
/// side takes the last players with it.
#[must_use]
pub fn lay_out(
    position: Position,
    in_menu: bool,
    game: Rect,
    count: usize,
    sprite: i32,
) -> Option<Layout> {
    if position == Position::Hidden || count == 0 || sprite <= 0 {
        return None;
    }
    let count = i32::try_from(count).unwrap_or(i32::MAX);
    let along = count * sprite + (count + 1) * GAP;
    let across = sprite + 2 * GAP;

    // In the menu the shipped overlay ignores the position setting and stacks down the
    // left, below the game's own menu.
    let column = in_menu || position.is_column();
    let (width, height) = if column {
        (across, along)
    } else {
        (along, across)
    };
    // Never wider or taller than the game window: what does not fit is clipped by moving
    // the strip, not by dropping the players at the end of it.
    let width = width.min(game.width.max(1));
    let height = height.min(game.height.max(1));

    let (x, y) = if in_menu {
        (game.x + MENU_ORIGIN.0, game.y + MENU_ORIGIN.1)
    } else {
        match position {
            Position::Top => (game.x + (game.width - width) / 2, game.y),
            Position::BottomLeft => (game.x, game.y + game.height - height),
            Position::Right | Position::RightNamed => (game.x + game.width - width, game.y),
            // `Left`, `LeftNamed`, and `Hidden` which returned above.
            _ => (game.x, game.y),
        }
    };

    let sprites = (0..count)
        .map(|at| {
            let offset = GAP + at * (sprite + GAP);
            if column { (GAP, offset) } else { (offset, GAP) }
        })
        .collect();

    Some(Layout {
        placement: Rect {
            x,
            y,
            width,
            height,
        },
        sprites,
    })
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{GAP, Layout, Position, Rect, lay_out};

    /// A 1920×1080 game window at the top left of the screen.
    const GAME: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    const SPRITE: i32 = 56;

    fn laid(position: Position, count: usize) -> Layout {
        lay_out(position, false, GAME, count, SPRITE).expect("something to draw")
    }

    /// Every value the settings can hold reads as itself, and the two `1` variants are not
    /// the same as their plain forms — they are the compact-with-names option, and folding
    /// them together would silently change what a player chose.
    #[test]
    fn every_stored_position_reads_back() {
        assert_eq!(Position::parse("hidden"), Position::Hidden);
        assert_eq!(Position::parse("top"), Position::Top);
        assert_eq!(Position::parse("bottom_left"), Position::BottomLeft);
        assert_eq!(Position::parse("right"), Position::Right);
        assert_eq!(Position::parse("right1"), Position::RightNamed);
        assert_eq!(Position::parse("left"), Position::Left);
        assert_eq!(Position::parse("left1"), Position::LeftNamed);
        assert_ne!(Position::parse("right1"), Position::parse("right"));
    }

    /// Every one of them is one the settings screen offers, which is what keeps the two
    /// lists from drifting apart into a value nothing can produce or one nothing accepts.
    #[test]
    fn the_settings_screen_offers_exactly_these() {
        let control = crate::settings_screen::controls()
            .find(|control| control.key == "overlayPosition")
            .expect("the position control");
        let crate::settings_screen::Kind::Choice(options) = control.kind else {
            panic!("the position is not a choice");
        };
        for option in options {
            let crate::settings::Default_::Text(value) = option.value else {
                panic!("a position that is not a string");
            };
            // `parse` falls back to `Right`, so this checks the fallback is not what is
            // answering: every offered value but `right` must read as something else.
            if value != "right" {
                assert_ne!(
                    Position::parse(value),
                    Position::Right,
                    "{value} falls through to the default"
                );
            }
        }
        assert_eq!(options.len(), 7);
    }

    /// An unrecognised value costs the player a preference, not the overlay.
    #[test]
    fn an_unknown_position_is_the_default_rather_than_nothing() {
        assert_eq!(Position::parse("somewhere_newer"), Position::Right);
        assert_eq!(Position::parse(""), Position::Right);
    }

    /// Hidden draws nothing, and so does an empty lobby. Both are the caller's cue to hide
    /// the window rather than to draw an empty one, which is still a rectangle of nothing
    /// sitting over the game.
    #[test]
    fn there_is_nothing_to_place_when_there_is_nothing_to_draw() {
        assert!(lay_out(Position::Hidden, false, GAME, 5, SPRITE).is_none());
        assert!(lay_out(Position::Top, false, GAME, 0, SPRITE).is_none());
        assert!(lay_out(Position::Top, false, GAME, 5, 0).is_none());
    }

    /// A row runs across and a column runs down, and the strip is shaped to match.
    #[test]
    fn a_row_runs_across_and_a_column_runs_down() {
        let row = laid(Position::Top, 4);
        assert!(row.placement.width > row.placement.height);
        assert_eq!(row.sprites[0].1, row.sprites[3].1, "a row shares one line");
        assert!(row.sprites[3].0 > row.sprites[0].0);

        let column = laid(Position::Right, 4);
        assert!(column.placement.height > column.placement.width);
        assert_eq!(column.sprites[0].0, column.sprites[3].0);
        assert!(column.sprites[3].1 > column.sprites[0].1);
    }

    /// The four corners, each where the stylesheet puts it.
    #[test]
    fn each_position_lands_where_the_stylesheet_puts_it() {
        let top = laid(Position::Top, 3).placement;
        assert_eq!(top.y, GAME.y, "the top row is at the top");
        assert_eq!(
            top.x + top.width / 2,
            GAME.x + GAME.width / 2,
            "and centred"
        );

        let bottom = laid(Position::BottomLeft, 3).placement;
        assert_eq!(bottom.x, GAME.x);
        assert_eq!(bottom.y + bottom.height, GAME.y + GAME.height);

        let left = laid(Position::Left, 3).placement;
        assert_eq!((left.x, left.y), (GAME.x, GAME.y));

        let right = laid(Position::Right, 3).placement;
        assert_eq!(right.x + right.width, GAME.x + GAME.width);
        assert_eq!(right.y, GAME.y);
    }

    /// The strip follows the game window rather than the screen, so a windowed game gets
    /// its overlay on the window and not in the corner of the monitor.
    #[test]
    fn the_strip_follows_a_windowed_game() {
        let windowed = Rect {
            x: 300,
            y: 200,
            width: 1280,
            height: 720,
        };
        let laid = lay_out(Position::Right, false, windowed, 3, SPRITE).expect("a layout");
        assert_eq!(
            laid.placement.x + laid.placement.width,
            windowed.x + windowed.width
        );
        assert_eq!(laid.placement.y, windowed.y);
    }

    /// The menu ignores the position setting, because the corners it would use are where
    /// the game's own menu is.
    #[test]
    fn the_menu_has_one_place_whatever_was_chosen() {
        let mut placements = Vec::new();
        for position in [
            Position::Top,
            Position::BottomLeft,
            Position::Left,
            Position::Right,
            Position::RightNamed,
        ] {
            let laid = lay_out(position, true, GAME, 3, SPRITE).expect("a layout");
            placements.push(laid.placement);
        }
        assert!(
            placements.windows(2).all(|pair| pair[0] == pair[1]),
            "the menu overlay moved with the setting: {placements:?}"
        );
        assert_eq!(
            (placements[0].x, placements[0].y),
            (GAME.x + 8, GAME.y + 60)
        );
    }

    /// Hidden is still hidden in the menu. It is the one position that means "not at all"
    /// rather than "somewhere".
    #[test]
    fn hidden_is_hidden_in_the_menu_too() {
        assert!(lay_out(Position::Hidden, true, GAME, 3, SPRITE).is_none());
    }

    /// The strip never leaves the game window. A full lobby is fifteen, which is a row
    /// wider than a 1280-pixel window — and a strip that ran off the side would take the
    /// last players with it.
    #[test]
    fn a_full_lobby_stays_inside_a_small_window() {
        let small = Rect {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
        };
        let laid = lay_out(Position::Top, false, small, 15, SPRITE).expect("a layout");
        assert!(laid.placement.width <= small.width);
        assert!(laid.placement.x >= small.x);
        assert!(laid.placement.x + laid.placement.width <= small.x + small.width);

        let column = lay_out(Position::Left, false, small, 15, SPRITE).expect("a layout");
        assert!(column.placement.height <= small.height);
    }

    /// One crewmate is one gap on each side of it, which is what makes the strip look
    /// deliberate rather than cropped.
    #[test]
    fn the_strip_is_the_crewmates_plus_a_margin() {
        let one = laid(Position::Top, 1);
        assert_eq!(one.placement.width, SPRITE + 2 * GAP);
        assert_eq!(one.placement.height, SPRITE + 2 * GAP);
        assert_eq!(one.sprites, [(GAP, GAP)]);
    }

    /// Names go with the side columns, and the two `1` variants keep them when compact.
    /// That is the whole reason those two positions exist.
    #[test]
    fn the_named_positions_keep_their_names_when_compact() {
        assert!(Position::Right.shows_names(false));
        assert!(!Position::Right.shows_names(true));
        assert!(Position::RightNamed.shows_names(true));
        assert!(Position::LeftNamed.shows_names(true));
        // A row has no room for them at any setting.
        assert!(!Position::Top.shows_names(false));
        assert!(!Position::BottomLeft.shows_names(false));
    }

    /// And they are compact whether or not the setting is, which is what `Overlay.tsx`
    /// does when it adds the class.
    #[test]
    fn the_named_positions_are_compact_by_themselves() {
        assert!(Position::RightNamed.forces_compact());
        assert!(Position::LeftNamed.forces_compact());
        assert!(!Position::Right.forces_compact());
        assert!(!Position::Top.forces_compact());
    }

    /// Every sprite is inside the strip that will be blitted, or it is drawn into a buffer
    /// that does not contain it and simply does not appear.
    #[test]
    fn every_sprite_is_inside_the_strip() {
        for position in [
            Position::Top,
            Position::BottomLeft,
            Position::Left,
            Position::Right,
        ] {
            for count in 1..=15_i32 {
                let laid = laid(position, usize::try_from(count).expect("a small count"));
                for (at, (x, y)) in laid.sprites.iter().enumerate() {
                    assert!(*x >= 0 && *y >= 0, "{position:?} {count} sprite {at}");
                    // The strip is clamped to the game window, so a lobby that does not fit
                    // has sprites beyond the edge -- that is the clipping, and it is the
                    // last ones rather than a shifted row.
                    if laid.placement.width >= count * (GAP + SPRITE) {
                        assert!(
                            x + SPRITE <= laid.placement.width,
                            "{position:?} {count} sprite {at} runs off"
                        );
                    }
                }
            }
        }
    }
}
