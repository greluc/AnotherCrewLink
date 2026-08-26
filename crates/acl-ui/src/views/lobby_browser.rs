//! The public lobby list.
//!
//! §4.8 item 4, replacing `LobbyBrowser.tsx`. The ordering it draws in is
//! [`crate::lobby_list`], which was ported rather than reimplemented because the list has
//! to look the same as the one it replaces — and because the original was not a
//! consistent ordering at all.
//!
//! **Nothing here talks to a server.** Joining is `join_lobby` with a callback that
//! answers with a code or an error, and that is a socket, not a widget. The view returns
//! which row was asked for; the caller sends it and puts the answer back in
//! [`Browser::answer`].

use egui::{Grid, RichText, ScrollArea, Ui};

use crate::lobby_list::{LobbyRow, Refusal};

/// One advertised lobby, as the table needs it.
///
/// Borrowed rather than owned: these arrive over the socket several times a second while
/// the browser is open, and the list is rebuilt from the caller's map each frame.
#[derive(Clone, Copy, Debug)]
pub struct Listing<'a> {
    /// The server's id for it, which is what `join_lobby` is asked for.
    pub id: i64,
    /// What the host called it.
    pub title: &'a str,
    /// The host's name in the game.
    pub host: &'a str,
    /// The mod id it advertises, verbatim from the server.
    pub mods: &'a str,
    /// The language tag the host chose.
    pub language: &'a str,
    /// The three fields the ordering and the join rule read.
    pub row: LobbyRow,
}

/// What the player did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Ask the server for this lobby's code.
    Join(i64),
    /// Put the browser away.
    Close,
}

/// Everything the table needs that is not a lobby.
pub struct Browser<'a> {
    /// Looks up an i18n key.
    pub t: &'a dyn Fn(&str) -> String,
    /// The mod this player is running, as an id. Rows advertising anything else cannot be
    /// joined.
    pub mods: &'a str,
    /// A locale tag to its name, for the language column.
    pub language_name: &'a dyn Fn(&str) -> String,
    /// The last answer to a join, shown until the next one: a code, or an error.
    pub answer: Option<&'a str>,
}

/// Draws the browser.
///
/// The listings are copied into a local vector and sorted here rather than by the caller.
/// The order is part of what this screen is — §4.8 asks it to look like the one it
/// replaces — and a caller that forgot to sort would get a list that reshuffles itself
/// every time the server sends an update.
pub fn draw(ui: &mut Ui, listings: &[Listing<'_>], browser: &Browser<'_>) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.heading((browser.t)("lobbybrowser.header"));
        if ui.button("×").clicked() {
            action = Some(Action::Close);
        }
    });

    if let Some(answer) = browser.answer {
        // Selectable, because the whole point of it is that the player types it into the
        // game: this client stopped writing the code into the game's memory on
        // 2026-08-24, when the write path was removed.
        ui.label(RichText::new(answer).strong());
    }

    let mut sorted: Vec<Listing<'_>> = listings.to_vec();
    sorted.sort_by_key(|listing| listing.row);

    ScrollArea::vertical().show(ui, |ui| {
        Grid::new("lobbies").striped(true).show(ui, |ui| {
            for heading in [
                "lobbybrowser.list.title",
                "lobbybrowser.list.host",
                "lobbybrowser.list.players",
                "lobbybrowser.list.mods",
                "lobbybrowser.list.language",
            ] {
                ui.label(RichText::new((browser.t)(heading)).strong());
            }
            ui.end_row();

            for listing in &sorted {
                if let Some(asked) = row(ui, listing, browser) {
                    action = Some(asked);
                }
            }
        });
    });
    action
}

/// One lobby.
fn row(ui: &mut Ui, listing: &Listing<'_>, browser: &Browser<'_>) -> Option<Action> {
    let mut action = None;
    ui.label(listing.title);
    ui.label(listing.host);
    ui.label(format!("{}/{}", listing.row.players, listing.row.capacity));
    ui.label(mod_name(listing.mods));
    ui.label((browser.language_name)(listing.language));

    let refusal = listing.row.refusal(listing.mods == browser.mods);
    let response = ui
        .add_enabled(
            refusal.is_none(),
            egui::Button::new((browser.t)("lobbybrowser.code")),
        )
        .on_disabled_hover_text(explain(listing, browser, refusal));
    if response.clicked() {
        action = Some(Action::Join(listing.id));
    }
    ui.end_row();
    action
}

/// Why a row cannot be joined, in words.
///
/// The mod case names both mods. "Incompatible mods" on its own leaves the player to work
/// out which of the two is the odd one, and it is often theirs.
fn explain(listing: &Listing<'_>, browser: &Browser<'_>, refusal: Option<Refusal>) -> String {
    match refusal {
        None => String::new(),
        Some(Refusal::DifferentMod) => format!(
            "{} '{}' {} '{}'",
            (browser.t)(Refusal::DifferentMod.reason()),
            mod_name(browser.mods),
            (browser.t)("lobbybrowser.code_tooltips.and"),
            mod_name(listing.mods),
        ),
        Some(other) => (browser.t)(other.reason()),
    }
}

/// A mod's name, or its id when this build does not know it.
///
/// Not translated, and not a mistake: these are the mods' own names. The Electron browser
/// shows the raw id for an unknown one too — a lobby running something newer than this
/// build is still a lobby, and "unknown" would hide which one it is.
fn mod_name(id: &str) -> String {
    acl_types::mods::from_id(id).map_or_else(|| id.to_owned(), |known| known.label().to_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Browser, Listing, explain, mod_name};
    use crate::lobby_list::{LobbyRow, Refusal};

    fn listing(mods: &'static str) -> Listing<'static> {
        Listing {
            id: 1,
            title: "A lobby",
            host: "Red",
            mods,
            language: "en",
            row: LobbyRow {
                waiting: true,
                players: 5,
                capacity: 10,
            },
        }
    }

    fn browser(mods: &'static str) -> Browser<'static> {
        Browser {
            t: &|key| key.to_owned(),
            mods,
            language_name: &|tag| tag.to_owned(),
            answer: None,
        }
    }

    /// A known mod is shown by its name and an unknown one by its id. A lobby running
    /// something newer than this build is still a lobby, and "unknown" would hide which
    /// one it is.
    #[test]
    fn a_mod_this_build_does_not_know_is_shown_by_its_id() {
        assert_eq!(mod_name("NONE"), acl_types::mods::Mod::None.label());
        assert_eq!(mod_name("SOMETHING_NEWER"), "SOMETHING_NEWER");
        assert_eq!(mod_name(""), "");
    }

    /// The mod refusal names both mods. "Incompatible mods" on its own leaves the player
    /// to work out which of the two is the odd one, and it is often theirs.
    #[test]
    fn the_mod_refusal_names_both_sides() {
        let said = explain(
            &listing("TOWN_OF_US"),
            &browser("NONE"),
            Some(Refusal::DifferentMod),
        );
        assert!(
            said.contains(acl_types::mods::Mod::TownOfUs.label()),
            "{said}"
        );
        assert!(said.contains(acl_types::mods::Mod::None.label()), "{said}");
    }

    /// The other two are one string each, and a joinable row explains nothing.
    #[test]
    fn the_other_refusals_are_one_string_each() {
        let row = listing("NONE");
        let browser = browser("NONE");
        assert_eq!(
            explain(&row, &browser, Some(Refusal::Full)),
            Refusal::Full.reason()
        );
        assert!(explain(&row, &browser, None).is_empty());
    }

    /// Every string this view asks for is in the shipped English catalogue. It is the
    /// fallback for the other thirty-six, so it is the one that has to be whole.
    #[test]
    fn every_string_is_in_the_english_catalogue() {
        let catalogue = acl_i18n::Catalogue::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../static/locales"),
            "en",
        )
        .expect("the shipped English");
        let wanted = [
            "lobbybrowser.header",
            "lobbybrowser.code",
            "lobbybrowser.code_tooltips.and",
            "lobbybrowser.list.title",
            "lobbybrowser.list.host",
            "lobbybrowser.list.players",
            "lobbybrowser.list.mods",
            "lobbybrowser.list.language",
            Refusal::InProgress.reason(),
            Refusal::Full.reason(),
            Refusal::DifferentMod.reason(),
        ];
        let missing: Vec<&str> = wanted
            .into_iter()
            .filter(|key| !catalogue.defines(key))
            .collect();
        assert!(
            missing.is_empty(),
            "not in `en/translation.json`: {missing:?}"
        );
    }

    /// The table sorts what it is given rather than trusting the caller to have done it.
    /// A caller that forgot would get a list that reshuffles every time the server sends
    /// an update, which is the bug `sortLobbies` shipped with.
    #[test]
    fn the_table_sorts_its_own_rows() {
        let started = Listing {
            row: LobbyRow {
                waiting: false,
                players: 9,
                capacity: 10,
            },
            ..listing("NONE")
        };
        let waiting = listing("NONE");
        let mut rows = [started, waiting];
        rows.sort_by_key(|listing| listing.row);
        assert!(
            rows[0].row.waiting,
            "a game in progress is the least useful row on the screen"
        );
    }
}
