//! The main view: who is in the lobby, and what is happening to their voice.
//!
//! §4.8 item 2, and it replaces `App.tsx`, `Voice.tsx`'s UI half and `Avatar.tsx`.
//!
//! # Two ways of drawing a player, and the second one is not a fallback for the first
//!
//! A crewmate here is the real thing when the artwork has arrived: a recoloured body with a
//! hat, a skin and a visor over it, composited by [`crate::worn`] from files fetched at run
//! time. Until it arrives — and on the first frame of every session it has not — the same
//! player is drawn as shapes: a body, a visor, a shadow.
//!
//! The drawn form is not a placeholder to be removed. Artwork can fail to arrive at all, on
//! a machine with no network or against a collection that has moved, and a window that
//! showed nothing in that case would be a window that looks broken for a reason the user
//! cannot see. It is also what a player wearing nothing gets, which is most of them.
//!
//! What both forms share is the part a player is actually reading: who is here, who is
//! speaking, who cannot be heard, and who is dead. Those are drawn by this view over
//! whichever body it has, so they never depend on a download.
//!
//! §4.8 grants the licence for the difference: "the Rust UI will not be pixel-identical to
//! the React one. Layout, spacing and control affordances will differ. What must not differ
//! is what every control *does*."

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Ui, Vec2};

use crate::roster::{Link, Shown};
use crate::views::colour;

/// One player, as this view needs them.
///
/// A name, a colour and what [`crate::roster`] decided. Everything else about a player
/// belongs to the game state, and copying it here would be a second place for it to go
/// stale.
#[derive(Clone, Copy, Debug)]
pub struct Portrait<'a> {
    /// Their name, with rich-text tags already stripped by the reader.
    pub name: &'a str,
    /// Their crew colour, as an index into the palette.
    ///
    /// Used to draw them when there is no artwork, and it is also part of what the artwork
    /// is keyed on — two players in different colours are two different sprites.
    pub color_id: i32,
    /// What the roster said about them.
    pub state: Shown,
    /// Their dressed crewmate, if it has been composited and uploaded.
    ///
    /// A texture id rather than a bitmap: building one costs a fetch and a composite, and
    /// doing that per frame for fifteen players would be the most expensive thing in the
    /// window. The caller owns the cache; this only draws what it is handed.
    pub art: Option<egui::TextureId>,
}

/// How wide a slot is, including its name.
pub const SLOT: f32 = 76.0;

/// How much of a slot the crewmate takes.
const AVATAR: f32 = 52.0;

/// Draws the players.
///
/// Wraps, because the window is 250 pixels wide at its minimum and a lobby holds fifteen.
pub fn draw(ui: &mut Ui, portraits: &[Portrait<'_>]) {
    if portraits.is_empty() {
        ui.label("Nobody else is here yet.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for portrait in portraits {
            slot(ui, portrait);
        }
    });
}

/// One crewmate and their name.
fn slot(ui: &mut Ui, portrait: &Portrait<'_>) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(SLOT, SLOT), egui::Sense::hover());
    let centre = Pos2::new(rect.center().x, rect.min.y + AVATAR / 2.0);
    crewmate(ui, centre, portrait);

    // Under the crewmate rather than over it, and clipped to the slot: names run to ten
    // characters in this game and the slot is not that wide.
    ui.painter().text(
        Pos2::new(rect.center().x, rect.max.y - 10.0),
        Align2::CENTER_CENTER,
        portrait.name,
        FontId::proportional(11.0),
        ui.visuals().text_color(),
    );

    response.on_hover_text(describe(portrait));
}

/// What the shapes mean, for somebody who cannot tell.
///
/// The colours and the ring are quick to read once and opaque the first time, so every one
/// of them is also words on hover. It is the same reason `Avatar.tsx` has a title on its
/// connection state.
fn describe(portrait: &Portrait<'_>) -> String {
    let mut said = vec![portrait.name.to_owned()];
    said.push(
        match portrait.state.link {
            Link::Disconnected => "no connection",
            Link::Silent => "connected, no audio",
            Link::Connected => "connected",
        }
        .to_owned(),
    );
    if portrait.state.talking {
        said.push("speaking".to_owned());
    }
    if !portrait.state.alive {
        said.push("dead".to_owned());
    }
    if portrait.state.using_radio {
        said.push("on the impostor radio".to_owned());
    }
    said.join(" — ")
}

/// The crewmate itself: the artwork if there is any, the shape if there is not.
fn crewmate(ui: &Ui, centre: Pos2, portrait: &Portrait<'_>) {
    if let Some(art) = portrait.art {
        dressed(ui, centre, portrait, art);
    } else {
        shapes(ui, centre, portrait);
    }
    indicators(ui, centre, portrait);
}

/// The composited crewmate.
///
/// Square, because the sprites are: `sprite::crewmate` draws into a square bitmap and the
/// cosmetics are placed as fractions of its width. Fitting it to a non-square box would move
/// every hat.
fn dressed(ui: &Ui, centre: Pos2, portrait: &Portrait<'_>, art: egui::TextureId) {
    let rect = Rect::from_center_size(centre, Vec2::splat(AVATAR));
    // The same fade the drawn form uses for a dead player, applied to the whole sprite
    // rather than to two colours. `WHITE` is "unchanged" for a tinted image.
    let tint = if portrait.state.alive {
        Color32::WHITE
    } else {
        Color32::from_white_alpha(90)
    };
    ui.painter().image(
        art,
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        tint,
    );
}

/// The crewmate as shapes, for when there is no artwork.
///
/// The game's silhouette in the crudest terms it can be: a body, a visor, and a shadow that
/// makes it read as round. The shadow matters more than it sounds — `player_colors` carries
/// one per colour precisely because using the body colour for it gives a flat sticker.
fn shapes(ui: &Ui, centre: Pos2, portrait: &Portrait<'_>) {
    let (mut body, mut shadow) = colour::crew(portrait.color_id);
    let painter = ui.painter();

    // Dead players are drawn faint rather than differently: the shape has to stay
    // recognisable, since knowing *who* is dead is the whole point of showing them.
    if !portrait.state.alive {
        body = body.gamma_multiply(0.35);
        shadow = shadow.gamma_multiply(0.35);
    }

    // The talking ring, first so everything else is over it. It grows outward rather than
    // recolouring the body, which would fight with the crew colour it has to stay legible
    // against.
    if portrait.state.talking {
        painter.circle_stroke(
            centre,
            AVATAR / 2.0,
            Stroke::new(3.0, Color32::from_rgb(80, 220, 120)),
        );
    }

    let radius = AVATAR / 2.0 - 5.0;
    painter.circle_filled(centre, radius, shadow);
    painter.circle_filled(
        Pos2::new(centre.x - 1.5, centre.y - 1.5),
        radius - 2.0,
        body,
    );
    // The visor, offset the way the game draws it: up and to the side.
    painter.circle_filled(
        Pos2::new(centre.x + radius * 0.35, centre.y - radius * 0.25),
        radius * 0.42,
        Color32::from_rgb(0xBE, 0xE3, 0xF5),
    );
}

/// What this view says about a player, over whichever body it drew.
///
/// Separate from both so that neither can lose them. These are the things somebody is
/// actually looking at the window for, and they must not depend on whether a download
/// finished.
fn indicators(ui: &Ui, centre: Pos2, portrait: &Portrait<'_>) {
    let painter = ui.painter();
    let radius = AVATAR / 2.0;

    // The connection, as an outline, because it is a property of the whole player rather
    // than of any part of them. Nothing is drawn when the connection is good: an indicator
    // that is always on is one nobody reads.
    let outline = match portrait.state.link {
        Link::Disconnected => Some(Color32::from_rgb(210, 90, 90)),
        Link::Silent => Some(Color32::from_rgb(220, 180, 80)),
        Link::Connected => None,
    };
    if let Some(colour) = outline {
        painter.circle_stroke(centre, AVATAR / 2.0 - 1.0, Stroke::new(2.0, colour));
    }

    if portrait.state.using_radio {
        painter.text(
            Pos2::new(centre.x, centre.y + radius),
            Align2::CENTER_CENTER,
            "📻",
            FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
    }
}

/// The bounding box one row of `count` players needs, at a given width.
///
/// Here rather than in the caller because the window has a minimum of 250 pixels and a
/// lobby holds fifteen: how many rows that is decides whether the window opens usable.
#[must_use]
pub fn height_for(count: usize, available_width: f32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a player count, bounded by the game at fifteen"
    )]
    let players = count as f32;
    let per_row = (available_width / SLOT).floor().max(1.0);
    (players / per_row).ceil() * SLOT
}

/// The rectangle a slot occupies, for a caller placing something beside it.
#[must_use]
pub fn slot_rect(origin: Pos2, index: usize, available_width: f32) -> Rect {
    let per_row = (available_width / SLOT).floor().max(1.0);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a player index, bounded by the game at fifteen"
    )]
    let at = index as f32;
    let column = at % per_row;
    let row = (at / per_row).floor();
    Rect::from_min_size(
        Pos2::new(origin.x + column * SLOT, origin.y + row * SLOT),
        Vec2::splat(SLOT),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Portrait, SLOT, describe, height_for, slot_rect};
    use crate::roster::{Link, Shown};
    use egui::Pos2;

    /// A tenth of a pixel. These are laid-out coordinates, so exact equality is a test
    /// about `f32` rather than about layout.
    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 0.1
    }

    fn shown(link: Link, talking: bool, alive: bool) -> Shown {
        Shown {
            at: 0,
            talking,
            alive,
            link,
            using_radio: false,
        }
    }

    /// Fifteen players in a 250-pixel window is three rows of three, not one row that runs
    /// off the side. The window's minimum is what makes this arithmetic worth having.
    #[test]
    fn a_full_lobby_wraps_in_the_narrowest_window() {
        let per_row = (250.0_f32 / SLOT).floor();
        assert!(per_row >= 3.0, "three across at the minimum width");
        let height = height_for(15, 250.0);
        assert!(
            height >= 5.0 * SLOT,
            "fifteen players should need at least five rows at this width, got {height}"
        );
    }

    /// One player is one row, and none is none — a lobby that has not filled yet must not
    /// reserve space for one that has.
    #[test]
    fn the_height_follows_the_count() {
        assert!(close(height_for(0, 250.0), 0.0));
        assert!(close(height_for(1, 250.0), SLOT));
    }

    /// A window too narrow for even one slot still lays out one per row rather than
    /// dividing by zero.
    #[test]
    fn an_impossibly_narrow_window_still_lays_out() {
        assert!(close(height_for(3, 10.0), 3.0 * SLOT));
        let second = slot_rect(Pos2::ZERO, 1, 10.0);
        assert!(close(second.min.x, 0.0));
        assert!(close(second.min.y, SLOT));
    }

    /// Slots fill along a row and then wrap, which is what makes the height arithmetic
    /// above describe the same layout the drawing produces.
    #[test]
    fn slots_fill_across_before_they_wrap() {
        let width = SLOT * 3.0;
        assert_eq!(slot_rect(Pos2::ZERO, 0, width).min, Pos2::new(0.0, 0.0));
        assert_eq!(
            slot_rect(Pos2::ZERO, 2, width).min,
            Pos2::new(SLOT * 2.0, 0.0)
        );
        assert_eq!(slot_rect(Pos2::ZERO, 3, width).min, Pos2::new(0.0, SLOT));
    }

    /// Every state a player can be shown in is also words, because the colours and the ring
    /// are quick to read once and opaque the first time.
    #[test]
    fn every_state_is_also_said_in_words() {
        let quiet = Portrait {
            name: "Red",
            color_id: 0,
            state: shown(Link::Connected, false, true),
            // No artwork in these: the drawn form is what a test can assert about.
            art: None,
        };
        assert_eq!(describe(&quiet), "Red — connected");

        let gone = Portrait {
            state: shown(Link::Disconnected, false, false),
            ..quiet
        };
        assert!(describe(&gone).contains("no connection"));
        assert!(describe(&gone).contains("dead"));

        let mute = Portrait {
            state: shown(Link::Silent, false, true),
            ..quiet
        };
        assert!(
            describe(&mute).contains("no audio"),
            "the difference between not arrived and not audible has to be sayable"
        );

        let loud = Portrait {
            state: shown(Link::Connected, true, true),
            ..quiet
        };
        assert!(describe(&loud).contains("speaking"));
    }
}
