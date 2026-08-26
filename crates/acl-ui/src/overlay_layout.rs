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

/// The meeting hud's own layout, which is not the strip's.
///
/// `meetingOverlay` switches on a second overlay: while a meeting is up, the players are
/// drawn over the seats of the meeting table rather than in a corner. It is the one part of
/// the overlay that has to line up with something the game is drawing, and it does that
/// **by arithmetic over the window size** -- there is nothing read out of memory for it
/// except one boolean, `oldMeetingHud`, which says which of two tables the build has.
///
/// Every number below is `Overlay.tsx`'s or `css`'s. They are not derivable from anything;
/// they were fitted against the game, and the port has no way to check them beyond
/// reproducing them exactly. That is why they are written as named constants rather than
/// folded into expressions -- a reader comparing this against the stylesheet should be able
/// to find each one.
pub mod meeting {
    // Every number in here is a pixel dimension or a screen coordinate, and every cast is
    // between those and the fractions the stylesheet works in. They are all far below
    // `f32`'s exact integer range and all rounded where it matters, so the two cast lints
    // are answered once here rather than five times at the arithmetic.
    #![expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "pixel dimensions and screen coordinates, rounded where it matters"
    )]

    use super::Rect;

    /// The aspect ratio of the tablet the old meeting hud is drawn on.
    ///
    /// `854 / 579`, from `Overlay.tsx`. A real device's dimensions, which is why it is not
    /// a round number.
    const TABLET: f32 = 854.0 / 579.0;

    /// How much of the window's height the old hud takes.
    const OLD_HEIGHT: f32 = 0.96;

    /// The window ratio the new hud's three cases are measured against, and the two
    /// distances that separate them.
    const NEW_RATIO: f32 = 1.7;
    /// See [`NEW_RATIO`].
    const NEW_NEAR: f32 = 0.25;
    /// See [`NEW_RATIO`].
    const NEW_FAR: f32 = 0.5;

    /// What the window's width is divided by, in each of the three cases.
    const NEW_DIVISORS: [f32; 3] = [1.192, 1.146, 1.591];

    /// The new hud's width-to-height ratio.
    const NEW_ASPECT: f32 = 1.72;

    /// The table, as a fraction of the hud: width, height, left, top.
    const OLD_TABLE: [f32; 4] = [0.8845, 0.105, 0.047, 0.184_703];
    /// See [`OLD_TABLE`].
    const NEW_TABLE: [f32; 4] = [1.0, 0.105, 0.004, 0.15];

    /// One seat, as a fraction of the table: width, height, bottom margin, right margin,
    /// left margin.
    const OLD_SEAT: [f32; 5] = [0.4641, 1.0, 0.02, 0.0234, 0.0];
    /// See [`OLD_SEAT`].
    const NEW_SEAT: [f32; 5] = [0.30, 1.09, 0.019, 0.0023, 0.024];

    /// Where the meeting hud sits on the game window.
    ///
    /// Centred, and sized by the window's shape. The two branches are the two tables: the
    /// old one is a tablet with a fixed aspect ratio fitted into the window, and the new one
    /// is a fraction of the width that changes at two ratio thresholds.
    #[must_use]
    pub fn hud(game: Rect, old_hud: bool) -> Rect {
        let (width, height) = (game.width as f32, game.height as f32);
        if width <= 0.0 || height <= 0.0 {
            return Rect {
                x: game.x,
                y: game.y,
                width: 0,
                height: 0,
            };
        }

        let (hud_width, hud_height) = if old_hud {
            // The tablet is fitted into the window: whichever side runs out first is the
            // one that decides.
            if width / (height * OLD_HEIGHT) > TABLET {
                let fitted = height * OLD_HEIGHT;
                (fitted * TABLET, fitted)
            } else {
                (width, width / TABLET)
            }
        } else {
            let apart = (width / height - NEW_RATIO).abs();
            let divisor = if apart < NEW_NEAR {
                NEW_DIVISORS[0]
            } else if apart < NEW_FAR {
                NEW_DIVISORS[1]
            } else {
                NEW_DIVISORS[2]
            };
            let fitted = width / divisor;
            (fitted, fitted / NEW_ASPECT)
        };

        // Rounded, not truncated. The stylesheet works in fractional pixels and this does
        // not, and truncating both the size and the offset biases each down by up to a
        // pixel -- which shows as a hud one pixel higher than it is low.
        Rect {
            x: game.x + ((width - hud_width) / 2.0).round() as i32,
            y: game.y + ((height - hud_height) / 2.0).round() as i32,
            width: hud_width.round() as i32,
            height: hud_height.round() as i32,
        }
    }

    /// Where each player's seat is, in screen coordinates.
    ///
    /// One rectangle per player, in the order given. The seats wrap: `flex-wrap` in the
    /// stylesheet, which puts as many across as fit and starts a row below. The rows run
    /// *past* the bottom of the table -- the table is 10.5% of the hud and a seat is its
    /// full height -- and that is not a mistake being reproduced: it is how the rows line
    /// up with the table the game draws.
    #[must_use]
    pub fn seats(game: Rect, old_hud: bool, count: usize) -> Vec<Rect> {
        let hud = hud(game, old_hud);
        if count == 0 || hud.width <= 0 || hud.height <= 0 {
            return Vec::new();
        }
        let (table, seat) = if old_hud {
            (OLD_TABLE, OLD_SEAT)
        } else {
            (NEW_TABLE, NEW_SEAT)
        };

        let (hud_width, hud_height, hud_x, hud_y) = (
            hud.width as f32,
            hud.height as f32,
            hud.x as f32,
            hud.y as f32,
        );
        let table_width = hud_width * table[0];
        let table_height = hud_height * table[1];
        let table_x = hud_width.mul_add(table[2], hud_x);
        let table_y = hud_height.mul_add(table[3], hud_y);

        let seat_width = table_width * seat[0];
        let seat_height = table_height * seat[1];
        let (margin_bottom, margin_right, margin_left) = (
            table_height * seat[2],
            table_width * seat[3],
            table_width * seat[4],
        );
        // How many fit across, which is what `flex-wrap` works out for itself. At least one,
        // or a seat wider than its table would divide by zero.
        let across =
            ((table_width / (seat_width + margin_right + margin_left)).floor() as i32).max(1);

        let mut seats = Vec::with_capacity(count);
        for at in 0..count {
            let at = i32::try_from(at).unwrap_or(i32::MAX);
            let (column, row) = ((at % across) as f32, (at / across) as f32);
            seats.push(Rect {
                x: (table_x + margin_left + column * (seat_width + margin_right + margin_left))
                    as i32,
                y: (table_y + row * (seat_height + margin_bottom)) as i32,
                width: seat_width as i32,
                height: seat_height as i32,
            });
        }
        seats
    }
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

    mod meeting {
        #![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

        use super::super::meeting::{hud, seats};
        use super::{GAME, Rect};

        /// The hud is centred on the game window, whichever table it is.
        #[test]
        fn the_hud_is_centred() {
            for old_hud in [true, false] {
                let hud = hud(GAME, old_hud);
                let left = hud.x - GAME.x;
                let right = (GAME.x + GAME.width) - (hud.x + hud.width);
                assert!(
                    (left - right).abs() <= 1,
                    "old_hud={old_hud}: {left} on the left and {right} on the right"
                );
                let top = hud.y - GAME.y;
                let bottom = (GAME.y + GAME.height) - (hud.y + hud.height);
                assert!((top - bottom).abs() <= 1, "old_hud={old_hud}");
            }
        }

        /// And it fits inside it. A hud larger than the window is one whose seats are drawn
        /// off the side of the game.
        #[test]
        fn the_hud_fits_the_window() {
            for game in [
                GAME,
                Rect {
                    x: 0,
                    y: 0,
                    width: 1280,
                    height: 720,
                },
                Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1080,
                },
                Rect {
                    x: 0,
                    y: 0,
                    width: 1024,
                    height: 1024,
                },
                Rect {
                    x: 300,
                    y: 200,
                    width: 800,
                    height: 600,
                },
            ] {
                for old_hud in [true, false] {
                    let hud = hud(game, old_hud);
                    assert!(
                        hud.width <= game.width && hud.height <= game.height,
                        "{game:?} old_hud={old_hud} gave {hud:?}"
                    );
                    assert!(
                        hud.width > 0 && hud.height > 0,
                        "{game:?} old_hud={old_hud}"
                    );
                }
            }
        }

        /// The old table keeps the tablet's proportions, which is what makes the seats line
        /// up with the picture of a tablet the game draws.
        #[test]
        fn the_old_table_keeps_the_tablets_shape() {
            let hud = hud(GAME, true);
            let ratio = hud.width as f32 / hud.height as f32;
            assert!(
                (ratio - 854.0 / 579.0).abs() < 0.01,
                "expected the tablet ratio, got {ratio}"
            );
        }

        /// The new table has three cases, and they are chosen by how far the window is from
        /// 16:9. A window at 16:9 gets the narrowest divisor and so the widest hud.
        #[test]
        fn the_new_table_has_three_cases() {
            let sixteen_by_nine = hud(
                Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                false,
            );
            let four_by_three = hud(
                Rect {
                    x: 0,
                    y: 0,
                    width: 1024,
                    height: 768,
                },
                false,
            );
            let ultrawide = hud(
                Rect {
                    x: 0,
                    y: 0,
                    width: 3440,
                    height: 1440,
                },
                false,
            );
            // 1920/1080 is 1.777, which is 0.077 from 1.7: the near case, divisor 1.192.
            assert_eq!(sixteen_by_nine.width, (1920.0_f32 / 1.192).round() as i32);
            // 1024/768 is 1.333, which is 0.367 away: the middle case, divisor 1.146.
            assert_eq!(four_by_three.width, (1024.0_f32 / 1.146).round() as i32);
            // 3440/1440 is 2.389, which is 0.689 away: the far case, divisor 1.591.
            assert_eq!(ultrawide.width, (3440.0_f32 / 1.591).round() as i32);
            // And they really are three different answers, or the thresholds do nothing.
            assert!(sixteen_by_nine.width > 1920 / 2);
            assert_ne!(
                four_by_three.width * 1440 / 768,
                ultrawide.width,
                "two of the three cases agree"
            );
        }

        /// One seat per player, and none for none.
        #[test]
        fn there_is_one_seat_for_each_player() {
            assert!(seats(GAME, true, 0).is_empty());
            assert_eq!(seats(GAME, true, 1).len(), 1);
            assert_eq!(seats(GAME, false, 15).len(), 15);
        }

        /// The seats wrap: two across on the old table, three on the new. That is what the
        /// widths in the stylesheet come to, and it is what makes the rows line up with the
        /// rows of the table.
        #[test]
        fn the_seats_wrap_where_the_table_does() {
            let old = seats(GAME, true, 6);
            assert_eq!(old[0].y, old[1].y, "the first two share a row");
            assert!(old[2].y > old[0].y, "the third starts a new one");
            assert!(old[1].x > old[0].x);

            let new = seats(GAME, false, 6);
            assert_eq!(new[0].y, new[1].y);
            assert_eq!(new[1].y, new[2].y, "three across on the new table");
            assert!(new[3].y > new[0].y);
        }

        /// A row of seats fits across the table it belongs to. A seat that ran off the side
        /// would sit over the table's edge rather than over a player.
        #[test]
        fn a_row_of_seats_fits_across_the_table() {
            for old_hud in [true, false] {
                let hud = hud(GAME, old_hud);
                let seats = seats(GAME, old_hud, 15);
                let first_row: Vec<&Rect> =
                    seats.iter().filter(|seat| seat.y == seats[0].y).collect();
                let last = first_row.last().expect("a row");
                assert!(
                    last.x + last.width <= hud.x + hud.width,
                    "old_hud={old_hud}: the row runs to {} and the hud ends at {}",
                    last.x + last.width,
                    hud.x + hud.width
                );
                assert!(seats[0].x >= hud.x, "old_hud={old_hud}");
            }
        }

        /// The seats move with the game window, so a windowed game gets them on the window.
        #[test]
        fn the_seats_follow_the_window() {
            let moved = Rect {
                x: 500,
                y: 300,
                ..GAME
            };
            let here = seats(GAME, false, 3);
            let there = seats(moved, false, 3);
            assert_eq!(there[0].x - here[0].x, 500);
            assert_eq!(there[0].y - here[0].y, 300);
            assert_eq!(there[0].width, here[0].width);
        }

        /// A window with no area has no hud and no seats, rather than a division by zero.
        #[test]
        fn a_window_with_no_area_has_no_seats() {
            let nothing = Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            };
            assert_eq!(hud(nothing, true).width, 0);
            assert!(seats(nothing, true, 5).is_empty());
            assert!(seats(Rect { width: 0, ..GAME }, false, 5).is_empty());
        }
    }
}
