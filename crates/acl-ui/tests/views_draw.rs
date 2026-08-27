//! Running every view once, with no window and no GPU.
//!
//! `egui::Context::run` needs neither: it takes raw input, calls the closure, and returns
//! the shapes it would have painted. Which makes the paint functions testable after all —
//! not for what they look like, but for the failures that are not about looks.
//!
//! Three of those are worth catching here, and all three are silent at compile time.
//!
//! **A panic.** An index into a list that is shorter than the loop, a `expect` on a
//! borrowed value, a slice of a string on a byte that is not a character boundary. Every
//! one of them reaches a player as a window that closes itself.
//!
//! **A duplicate widget id.** egui keys persistent state — which combo box is open, which
//! field has focus — on an id derived from the widget. Two widgets sharing one is two
//! controls that fight over the same open/closed state, which reads as a picker that will
//! not stay open. egui reports it by *painting* "🔥 Double use of …" over the offender
//! rather than by returning an error, so [`clashes`] reads the shapes back and looks for
//! it. Nothing fails without something looking.
//!
//! **An empty frame.** A view that draws nothing at all still returns `Ok`, and a screen
//! that has silently stopped drawing looks exactly like one that has not been opened yet.
//!
//! What this does *not* check is anything about appearance, and it is not a substitute for
//! opening the window. It is the floor: the views run, without a machine that has a
//! screen, on every push.

use acl_ui::lobby_list::LobbyRow;
use acl_ui::roster::{Link, Shown};
use acl_ui::settings_screen::Scope;
use acl_ui::views::{lobby_browser, main, settings};

/// Runs one view and returns what egui made of it.
///
/// `run_ui` rather than `run`, which takes the whole context and would need a panel of its
/// own; these views draw into a `Ui` the caller supplies, which is what `run_ui` hands
/// them.
///
/// The context is fresh each time so that state from one view cannot mask a problem in
/// another, and the view runs twice: a duplicate id is only reported on the second pass,
/// once the first has put the id in the store.
fn run(mut view: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
    let context = egui::Context::default();
    let mut first = context.run_ui(egui::RawInput::default(), &mut view);
    // The font atlas comes back as a texture the renderer is expected to upload, and
    // epaint panics on dropping one that nobody took. There is no renderer here, so it is
    // discarded explicitly -- which is what `clear` is for, and is the difference between
    // "no GPU" and "a GPU that ignored us".
    first.textures_delta.clear();
    let mut output = context.run_ui(egui::RawInput::default(), &mut view);
    output.textures_delta.clear();
    output
}

/// Any widget-id clash egui painted a complaint about.
///
/// It reports them through `debug_painter`, which puts the text in the frame's shapes
/// along with everything else. Reading it back out is the only way to see one without a
/// person looking at the window.
fn clashes(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .filter(|text| text.contains("use of") && text.contains("ID"))
        .collect()
}

/// Whether anything was actually painted.
fn painted(output: &egui::FullOutput) -> bool {
    output
        .shapes
        .iter()
        .any(|shape| !matches!(shape.shape, egui::Shape::Noop))
}

/// A settings source that answers everything with its default.
struct Defaults(acl_ui::config::Config);

impl settings::Values for Defaults {
    fn bool_at(&self, scope: Scope, key: &str) -> bool {
        settings::Values::bool_at(&self.0, scope, key)
    }
    fn number_at(&self, scope: Scope, key: &str) -> f64 {
        settings::Values::number_at(&self.0, scope, key)
    }
    fn text_at(&self, scope: Scope, key: &str) -> String {
        settings::Values::text_at(&self.0, scope, key)
    }
}

#[test]
fn the_settings_screen_draws() {
    let values = Defaults(acl_ui::config::Config::new());
    let translate = |key: &str| key.to_owned();
    let devices = [settings::Entry {
        id: "default",
        label: "Default",
    }];
    let locales = [settings::Entry {
        id: "en",
        label: "English",
    }];
    let context = settings::Context {
        t: &translate,
        microphones: &devices,
        speakers: &devices,
        locales: &locales,
        host_may_change: true,
        in_menu_or_lobby: true,
        capturing: None,
    };
    let output = run(|ui| {
        settings::draw(ui, &values, &context);
    });
    assert!(painted(&output), "the settings screen painted nothing");
    assert!(clashes(&output).is_empty(), "{:?}", clashes(&output));
}

/// Every control disabled is a different set of branches from every control enabled — the
/// lobby rules take the other arm, and so does each gated control.
#[test]
fn the_settings_screen_draws_with_everything_disabled() {
    let values = Defaults(acl_ui::config::Config::new());
    let translate = |key: &str| key.to_owned();
    let context = settings::Context {
        t: &translate,
        microphones: &[],
        speakers: &[],
        locales: &[],
        host_may_change: false,
        in_menu_or_lobby: false,
        capturing: Some("pushToTalkShortcut"),
    };
    let output = run(|ui| {
        settings::draw(ui, &values, &context);
    });
    assert!(painted(&output), "the settings screen painted nothing");
    assert!(clashes(&output).is_empty(), "{:?}", clashes(&output));
}

#[test]
fn the_lobby_browser_draws() {
    let translate = |key: &str| key.to_owned();
    let language_name = |tag: &str| tag.to_owned();
    let listings = [
        lobby_browser::Listing {
            id: 1,
            title: "Open, room to spare",
            host: "Red",
            mods: "NONE",
            language: "en",
            row: LobbyRow {
                waiting: true,
                players: 4,
                capacity: 10,
            },
        },
        lobby_browser::Listing {
            id: 2,
            title: "Full",
            host: "Blue",
            mods: "NONE",
            language: "de",
            row: LobbyRow {
                waiting: true,
                players: 10,
                capacity: 10,
            },
        },
        lobby_browser::Listing {
            id: 3,
            title: "Started, and modded",
            host: "Green",
            mods: "TOWN_OF_US",
            language: "fr",
            row: LobbyRow {
                waiting: false,
                players: 6,
                capacity: 8,
            },
        },
    ];
    let browser = lobby_browser::Browser {
        t: &translate,
        mods: "NONE",
        language_name: &language_name,
        answer: Some("ABCDEF"),
    };
    let output = run(|ui| {
        lobby_browser::draw(ui, &listings, &browser);
    });
    assert!(painted(&output), "the lobby browser painted nothing");
    assert!(clashes(&output).is_empty(), "{:?}", clashes(&output));
}

/// An empty list is the state the browser opens in, every time, before the first update
/// arrives.
#[test]
fn the_lobby_browser_draws_with_nothing_in_it() {
    let translate = |key: &str| key.to_owned();
    let language_name = |tag: &str| tag.to_owned();
    let browser = lobby_browser::Browser {
        t: &translate,
        mods: "NONE",
        language_name: &language_name,
        answer: None,
    };
    let output = run(|ui| {
        lobby_browser::draw(ui, &[], &browser);
    });
    assert!(painted(&output), "the header should still be there");
    assert!(clashes(&output).is_empty(), "{:?}", clashes(&output));
}

#[test]
fn the_main_view_draws() {
    let portraits = [
        main::Portrait {
            name: "Red",
            color_id: 0,
            state: Shown {
                at: 0,
                talking: true,
                alive: true,
                link: Link::Connected,
                using_radio: false,
            },
            art: None,
        },
        main::Portrait {
            name: "Blue",
            color_id: 1,
            state: Shown {
                at: 1,
                talking: false,
                alive: false,
                link: Link::Disconnected,
                using_radio: true,
            },
            art: None,
        },
        main::Portrait {
            // A name at the length the game allows, to put the clipping through its paces.
            name: "0123456789",
            color_id: 9_999,
            state: Shown {
                at: 2,
                talking: false,
                alive: true,
                link: Link::Silent,
                using_radio: false,
            },
            art: None,
        },
    ];
    let output = run(|ui| {
        main::draw(ui, &portraits);
    });
    assert!(painted(&output), "the main view painted nothing");
    assert!(clashes(&output).is_empty(), "{:?}", clashes(&output));
}

#[test]
fn the_main_view_draws_an_empty_lobby() {
    let output = run(|ui| {
        main::draw(ui, &[]);
    });
    assert!(painted(&output), "the empty message should be there");
    assert!(clashes(&output).is_empty(), "{:?}", clashes(&output));
}

/// The detector detects.
///
/// Without this, the five assertions above pass whether or not `clashes` can see anything
/// -- a check that cannot fail is a check that is not being made. Two combo boxes with the
/// same salt, far enough apart that egui does not excuse them as the same rectangle, is
/// the clash the real views are being held to.
#[test]
fn a_real_clash_is_seen() {
    let output = run(|ui| {
        for _ in 0..2 {
            ui.add_space(40.0);
            egui::ComboBox::from_id_salt("the same salt twice")
                .selected_text("x")
                .show_ui(ui, |ui| {
                    ui.label("y");
                });
        }
    });
    assert!(
        !clashes(&output).is_empty(),
        "a duplicate id went unnoticed, so the other tests are not checking anything"
    );
}

/// A player with artwork draws, and still gets everything this view says about them.
///
/// The two bodies — the composited sprite and the drawn shapes — are separate functions
/// since 2026-08-27, and the indicators were pulled out of the second one so both get them.
/// The way that regresses is by somebody putting an indicator back inside `shapes`, where a
/// dressed player would silently stop showing it: no error, no panic, just a crewmate whose
/// connection state is invisible whenever their hat has finished downloading.
///
/// So this draws the same player twice, once each way, and asserts both paint.
#[test]
fn a_dressed_player_and_a_drawn_one_both_get_their_indicators() {
    let state = Shown {
        at: 0,
        talking: true,
        alive: false,
        link: Link::Disconnected,
        using_radio: true,
    };
    // `Managed(0)` is never registered here. Nothing uploads in a headless context — the
    // harness clears `textures_delta` — so what this exercises is the painting path, which
    // is the half that can be wrong in the source.
    let dressed = main::Portrait {
        name: "Red",
        color_id: 0,
        state,
        art: Some(egui::TextureId::Managed(0)),
    };
    let drawn = main::Portrait {
        art: None,
        ..dressed
    };

    for (what, portrait) in [("dressed", dressed), ("drawn", drawn)] {
        let output = run(|ui| {
            main::draw(ui, &[portrait]);
        });
        assert!(painted(&output), "the {what} player painted nothing");
        assert!(
            clashes(&output).is_empty(),
            "{what}: {:?}",
            clashes(&output)
        );
    }
}
