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
use crate::views::theme;

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
pub fn draw(ui: &mut Ui, portraits: &[Portrait<'_>], say: &dyn Fn(&str) -> String) {
    if portraits.is_empty() {
        ui.label(say("client.lobby.nobody_else"));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for portrait in portraits {
            slot(ui, portrait, say);
        }
    });
}

/// One crewmate and their name.
fn slot(ui: &mut Ui, portrait: &Portrait<'_>, say: &dyn Fn(&str) -> String) {
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

    response.on_hover_text(describe(portrait, say));
}

/// What the shapes mean, for somebody who cannot tell.
///
/// The colours and the ring are quick to read once and opaque the first time, so every one
/// of them is also words on hover. It is the same reason `Avatar.tsx` has a title on its
/// connection state.
fn describe(portrait: &Portrait<'_>, say: &dyn Fn(&str) -> String) -> String {
    let mut said = vec![portrait.name.to_owned()];
    said.push(say(match portrait.state.link {
        Link::Disconnected => "client.player.no_connection",
        Link::Silent => "client.player.no_audio",
        Link::Connected => "client.player.connected",
    }));
    if portrait.state.talking {
        said.push(say("client.player.speaking"));
    }
    if !portrait.state.alive {
        said.push(say("client.player.dead"));
    }
    if portrait.state.using_radio {
        said.push(say("client.player.radio"));
    }
    // An em dash between the pieces, joined here rather than in the catalogue: a
    // translator sees the phrases, not a sentence with holes in it.
    said.join(" — ")
}

/// The crewmate itself: the artwork if there is any, the shape if there is not.
fn crewmate(ui: &Ui, centre: Pos2, portrait: &Portrait<'_>) {
    if let Some(art) = portrait.art {
        dressed(ui, centre, art);
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
fn dressed(ui: &Ui, centre: Pos2, art: egui::TextureId) {
    let rect = Rect::from_center_size(centre, Vec2::splat(AVATAR));
    // Untinted, even for a dead player.
    //
    // This used to fade the sprite to `from_white_alpha(90)`, which was right while the
    // body was two circles. It is not right now: a dead player's body is the *ghost*
    // drawing, and that drawing is already almost entirely semi-transparent — 5,663 of its
    // 5,692 drawn pixels. Fading it again leaves nothing to see. `shapes` still fades,
    // because it draws a living crewmate whichever the player is.
    ui.painter().image(
        art,
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// The ring round a crewmate, or `None` when there is nothing to say.
///
/// **A ring, not a badge**, and that is this GUI's answer rather than the shipped client's.
/// `design/guidelines/client-gui-spec.md` §4 gives both: Electron draws a `wifi_off` badge
/// where the Rust port draws a `#d25a5a` ring, and `egui-implementation.md` §3 says the
/// same of `StatusBadge` — "egui draws a ring, not a badge; keep the pairing of colour and
/// hover text". A badge sits on top of the artwork and hides the thing it is about; a ring
/// goes outside it.
///
/// Speaking wins, and the precedence is the point: it is the one state that changes several
/// times a second and the one somebody is watching for. Below it the connection, which does
/// not change while you look at it.
///
/// A healthy connection draws nothing. "An indicator that is always on is one nobody
/// reads."
const fn ring_for(state: Shown) -> Option<Color32> {
    if state.talking {
        return Some(theme::TALKING);
    }
    match state.link {
        Link::Disconnected => Some(theme::LINK_DOWN),
        Link::Silent => Some(theme::LINK_SILENT),
        Link::Connected => None,
    }
}

/// The crewmate as shapes, for when there is no artwork.
///
/// The game's silhouette in the crudest terms it can be: a body, a visor, and a shadow that
/// makes it read as round. The shadow matters more than it sounds — `player_colors` carries
/// one per colour precisely because using the body colour for it gives a flat sticker.
fn shapes(ui: &Ui, centre: Pos2, portrait: &Portrait<'_>) {
    shapes_at(ui, centre, portrait, AVATAR / 2.0);
}

/// The same, at whatever size the caller wants it.
fn shapes_at(ui: &Ui, centre: Pos2, portrait: &Portrait<'_>, half: f32) {
    let (mut body, mut shadow) = colour::crew(portrait.color_id);
    let painter = ui.painter();

    // Dead players are drawn faint rather than differently: the shape has to stay
    // recognisable, since knowing *who* is dead is the whole point of showing them.
    if !portrait.state.alive {
        body = body.gamma_multiply(0.35);
        shadow = shadow.gamma_multiply(0.35);
    }

    // Everything below is in proportion to the half-width rather than to `AVATAR`, so
    // the same crewmate can be drawn at the size your own avatar wants without the visor
    // sliding off it.
    let scale = half / (AVATAR / 2.0);
    let radius = half - 5.0 * scale;
    painter.circle_filled(centre, radius, shadow);
    painter.circle_filled(
        Pos2::new(centre.x - 1.5 * scale, centre.y - 1.5 * scale),
        radius - 2.0 * scale,
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

    if let Some(colour) = ring_for(portrait.state) {
        painter.circle_stroke(centre, AVATAR / 2.0 - 1.0, Stroke::new(2.0, colour));
    }

    // The impostor radio, drawn white over the lower part of the avatar, which is where
    // the shipped client puts its one bespoke SVG. An icon rather than the 📻 that stood
    // here: the design system's rule is "No emoji", and an emoji is drawn by whatever font
    // the machine happens to have.
    if portrait.state.using_radio {
        painter.text(
            Pos2::new(centre.x, centre.y + radius),
            Align2::CENTER_CENTER,
            theme::icon::RADIO,
            theme::icon_font(14.0),
            Color32::WHITE,
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

/// You, drawn larger and above everybody else.
///
/// `Voice.tsx` lines 1739-1752 puts the local player in their own place at the top rather
/// than in the wrapped list, and reported missing from 2.0.0-alpha.1: `main_view` filters
/// the local player out by design, and nothing put them back.
///
/// It is not vanity. This is where you check the two things only you can be: muted and
/// deafened. A client whose mute key works and shows nothing is a client you cannot trust
/// you have muted — and the border carries a third, which is whether you are connected to
/// the server at all.
pub struct Own<'a> {
    /// Your crewmate, exactly as everybody else's is described.
    pub portrait: Portrait<'a>,
    /// Your microphone is off.
    pub muted: bool,
    /// You cannot hear the lobby, which also means your microphone is off.
    pub deafened: bool,
}

/// How wide your own slot is.
pub const OWN_SLOT: f32 = 96.0;

/// How much of it the crewmate takes.
const OWN_AVATAR: f32 = 68.0;

/// Draws you.
pub fn draw_own(ui: &mut Ui, own: &Own<'_>, say: &dyn Fn(&str) -> String) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(OWN_SLOT, OWN_SLOT), egui::Sense::hover());
    let centre = Pos2::new(rect.center().x, rect.min.y + OWN_AVATAR / 2.0);

    if let Some(art) = own.portrait.art {
        let square = Rect::from_center_size(centre, Vec2::splat(OWN_AVATAR));
        // Untinted, for the reason `dressed` gives: a dead player's body is the ghost
        // drawing, which is already almost entirely semi-transparent.
        ui.painter().image(
            art,
            square,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        shapes_at(ui, centre, &own.portrait, OWN_AVATAR / 2.0);
    }

    // The same ring the others get, and it means the same things: speaking first, then
    // whether there is a connection. For you that is the connection to the server rather
    // than to a peer, which is `Voice.tsx`'s own `connected ? 'connected' : 'disconnected'`.
    if let Some(colour) = ring_for(own.portrait.state) {
        ui.painter()
            .circle_stroke(centre, OWN_AVATAR / 2.0 - 1.0, Stroke::new(2.5, colour));
    }

    // Your name, where the words "muted" and "deafened" used to be stacked. The slot
    // already reserves the room and everybody else's row has one, so leaving yours blank
    // made the list look like it started with an unlabelled crewmate.
    ui.painter().text(
        Pos2::new(rect.center().x, rect.max.y - 10.0),
        Align2::CENTER_CENTER,
        own.portrait.name,
        FontId::proportional(12.0),
        ui.visuals().text_color(),
    );

    response.on_hover_text(describe_own(own, say));
}

/// What your own badges mean, spelled out.
fn describe_own(own: &Own<'_>, say: &dyn Fn(&str) -> String) -> String {
    let mut said = vec![own.portrait.name.to_owned()];
    said.push(say(match own.portrait.state.link {
        Link::Disconnected => "client.you.not_connected",
        Link::Silent => "client.player.no_audio",
        Link::Connected => "client.player.connected",
    }));
    if own.deafened {
        said.push(say("client.you.deafened"));
    } else if own.muted {
        said.push(say("client.you.muted"));
    }
    if !own.portrait.state.alive {
        said.push("dead".to_owned());
    }
    said.join(" — ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Portrait, SLOT, describe, height_for, ring_for, slot_rect};
    use crate::roster::{Link, Shown};
    use crate::views::theme;
    use egui::Pos2;

    /// A tenth of a pixel. These are laid-out coordinates, so exact equality is a test
    /// about `f32` rather than about layout.
    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 0.1
    }

    /// Speaking is green, and it beats every connection state.
    ///
    /// The precedence is the point. A player talking on a shaky link showed the shaky link,
    /// which is the less interesting of the two things and the one that does not change
    /// while you watch it.
    #[test]
    fn speaking_wins_the_ring() {
        for link in [Link::Connected, Link::Silent, Link::Disconnected] {
            assert_eq!(
                ring_for(shown(link, true, true)),
                Some(theme::TALKING),
                "talking over {link:?}"
            );
        }
    }

    /// A quiet player still says what their connection is, and a good one says nothing.
    ///
    /// The design system gives this GUI a ring where the shipped client draws a badge —
    /// `client-gui-spec.md` §4, and `egui-implementation.md` §3 in as many words: "egui
    /// draws a ring, not a badge". A badge sits on the artwork and hides the thing it is
    /// about.
    #[test]
    fn a_quiet_player_shows_the_connection_as_a_ring() {
        assert_eq!(
            ring_for(shown(Link::Disconnected, false, true)),
            Some(theme::LINK_DOWN)
        );
        assert_eq!(
            ring_for(shown(Link::Silent, false, true)),
            Some(theme::LINK_SILENT)
        );
        // "An indicator that is always on is one nobody reads."
        assert_eq!(ring_for(shown(Link::Connected, false, true)), None);
    }

    /// The three ring colours are three, and they are the system's.
    ///
    /// Two states sharing a colour is two states nobody can tell apart, and a colour that
    /// is not a token is one nobody chose.
    #[test]
    fn the_three_rings_are_distinct_tokens() {
        let all = [theme::TALKING, theme::LINK_DOWN, theme::LINK_SILENT];
        for (at, one) in all.iter().enumerate() {
            for other in all.iter().skip(at + 1) {
                assert_ne!(one, other, "two states share a ring colour");
            }
        }
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
    ///
    /// Asserted on the *keys*, which is what this function chooses: the words behind them
    /// are the catalogue's, and `every_string_on_the_screen_is_in_the_english_catalogue`
    /// checks those. A translator that returns its key is what makes the two separable.
    #[test]
    fn every_state_is_also_said_in_words() {
        let say = |key: &str| key.to_owned();
        let quiet = Portrait {
            name: "Red",
            color_id: 0,
            state: shown(Link::Connected, false, true),
            // No artwork in these: the drawn form is what a test can assert about.
            art: None,
        };
        assert_eq!(describe(&quiet, &say), "Red — client.player.connected");

        let gone = Portrait {
            state: shown(Link::Disconnected, false, false),
            ..quiet
        };
        assert!(describe(&gone, &say).contains("client.player.no_connection"));
        assert!(describe(&gone, &say).contains("client.player.dead"));

        let mute = Portrait {
            state: shown(Link::Silent, false, true),
            ..quiet
        };
        assert!(
            describe(&mute, &say).contains("client.player.no_audio"),
            "the difference between not arrived and not audible has to be sayable"
        );

        let loud = Portrait {
            state: shown(Link::Connected, true, true),
            ..quiet
        };
        assert!(describe(&loud, &say).contains("client.player.speaking"));
    }
}
