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
    // The same style the client starts with. Without it a view that asks for the icon
    // family panics -- `FontFamily::Name("icons") is not bound to any fonts` -- which is
    // how this call got here: the theme and the views have to agree, and a bare context
    // agrees with neither.
    acl_ui::views::theme::apply(&context);
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

/// Every filled rectangle in a frame, as (side lengths, corner radius, fill).
fn rects(output: &egui::FullOutput) -> Vec<(egui::Vec2, u8, egui::Color32)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) => {
                Some((rect.rect.size(), rect.corner_radius.nw, rect.fill))
            }
            _ => None,
        })
        .collect()
}

/// The tick box is a box.
///
/// `--radius-lg` is on every interactive widget, which is right for a button and is more
/// than half the side of an eighteen-pixel box: egui clamps a radius to half the shorter
/// side, so ten made a circle out of every lobby rule. `forms/Checkbox.jsx` says two.
///
/// Checked through the shapes rather than through the style, because the style is what was
/// wrong: it said ten, it was applied exactly as written, and eleven round tick boxes came
/// out the other end.
#[test]
fn the_tick_box_is_a_rounded_square_and_not_a_circle() {
    let mut ticked = true;
    let output = run(|ui| {
        acl_ui::views::theme::checkbox(ui, &mut ticked, "Walls Block Audio");
    });
    let side = acl_ui::views::theme::BOX;
    let box_shape = rects(&output)
        .into_iter()
        .find(|(size, _, _)| (size.x - side).abs() < 1.0 && (size.y - side).abs() < 1.0);
    let Some((_, radius, fill)) = box_shape else {
        panic!("no 18px tick box among the shapes: it is not a box");
    };
    assert_eq!(radius, acl_ui::views::theme::RADIUS_BOX);
    assert_eq!(fill, acl_ui::views::theme::PURPLE, "ticked is a filled box");
}

/// The slider's thumb is the twelve pixels the design system asks for.
///
/// egui takes no handle size: it derives one, as the row height over 2.5, and the row is
/// the larger of the body line height and `interact_size.y`. `theme::slider` sets both so
/// the arithmetic lands on twelve, and this is the check — because the two halves are set
/// in one place and read in another, and Varela Round at 14px is 16.9 tall on its own,
/// which would make a fourteen-pixel thumb without anyone noticing.
#[test]
fn the_slider_thumb_is_twelve_pixels_across() {
    let mut row = 0.0_f32;
    run(|ui| {
        let mut value = 5.5_f64;
        row = acl_ui::views::theme::slider(ui, &mut value, 1.0..=10.0, 0.1)
            .rect
            .height();
    });
    // `Slider::handle_radius`, doubled.
    let thumb = row / 2.5 * 2.0;
    let wanted = acl_ui::views::theme::THUMB;
    assert!(
        (thumb - wanted).abs() < 0.5,
        "the row came out {row}px, which is a {thumb}px thumb and not {wanted}"
    );
}

/// Nothing taller than the rail is painted as a rectangle.
///
/// egui's own handle is a rectangle by default, and this slider covers the handle with a
/// disc. A rectangle under a disc of the same radius shows four pale corners around it,
/// which is exactly what the first build of `theme::slider` drew.
#[test]
fn the_thumb_has_no_corners_poking_out_from_under_it() {
    let output = run(|ui| {
        let mut value = 5.5_f64;
        acl_ui::views::theme::slider(ui, &mut value, 1.0..=10.0, 0.1);
    });
    let rail = acl_ui::views::theme::RAIL_H;
    for (size, _, _) in rects(&output) {
        assert!(
            size.y <= rail + 0.51,
            "a {size:?} rectangle, where only a {rail}px rail should be"
        );
    }
}

/// The rail is visible, and it is the height the component says.
///
/// The bug this is here for: `widgets.inactive.bg_fill` is transparent so that buttons are
/// outlines, and egui fills the slider's rail from that same field. The rail was there the
/// whole time, painted in nothing at all.
#[test]
fn the_slider_rail_is_painted_in_something() {
    let output = run(|ui| {
        let mut value = 5.5_f64;
        acl_ui::views::theme::slider(ui, &mut value, 1.0..=10.0, 0.1);
    });
    let height = acl_ui::views::theme::RAIL_H;
    let rails: Vec<_> = rects(&output)
        .into_iter()
        .filter(|(size, _, _)| (size.y - height).abs() < 0.51 && size.x > 20.0)
        .collect();
    assert!(!rails.is_empty(), "no {height}px rail was painted at all");
    for (size, _, fill) in rails {
        assert_ne!(
            fill,
            egui::Color32::TRANSPARENT,
            "a {size:?} rail painted in nothing"
        );
    }
}

/// The two switches are drawn on one vertical axis.
///
/// `mic` is 17px of ink and `volume_up` is 23px. Sized to their content the two buttons had
/// their centres five pixels apart, which is visible when they are the only two things in
/// that column -- and the hover wash, taken from a button frame rather than from the same
/// rect as the glyph, sat five pixels off the icon again. Both now come from one allocated
/// square, and this reads the glyph positions back out of the frame to say so.
#[test]
fn the_two_switches_share_one_axis() {
    let say = |key: &str| key.to_owned();
    let output = run(|ui| {
        acl_ui::views::main::draw_switches(ui, false, false, &say);
    });
    let glyphs: Vec<f32> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.pos.x + text.galley.size().x / 2.0),
            _ => None,
        })
        .collect();
    let [mic, ear] = glyphs[..] else {
        panic!("two switches, two glyphs: {glyphs:?}");
    };
    let apart = (mic - ear).abs();
    assert!(apart < 0.5, "the two icons are {apart}px apart across");
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
        input_level: None,
        testing_speaker: false,
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
        input_level: None,
        testing_speaker: false,
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
        main::draw(ui, &portraits, &|key| key.to_owned());
    });
    assert!(painted(&output), "the main view painted nothing");
    assert!(clashes(&output).is_empty(), "{:?}", clashes(&output));
}

#[test]
fn the_main_view_draws_an_empty_lobby() {
    let output = run(|ui| {
        main::draw(ui, &[], &|key| key.to_owned());
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
            main::draw(ui, &[portrait], &|key| key.to_owned());
        });
        assert!(painted(&output), "the {what} player painted nothing");
        assert!(
            clashes(&output).is_empty(),
            "{what}: {:?}",
            clashes(&output)
        );
    }
}

/// Your own avatar draws, muted and deafened and neither.
///
/// Reported missing from 2.0.0-alpha.1: `main_view` filters the local player out by design
/// — it answers "who else is here" — and nothing put them back, so you could not see
/// yourself. This is where you check the two things only you can be, and a mute key that
/// works while showing nothing is a mute key you cannot trust.
#[test]
fn your_own_avatar_draws_in_every_state() {
    let state = Shown {
        at: 0,
        talking: false,
        alive: true,
        link: Link::Connected,
        using_radio: false,
    };
    let portrait = main::Portrait {
        name: "You",
        color_id: 0,
        state,
        art: None,
    };

    for (what, muted, deafened) in [
        ("neither", false, false),
        ("muted", true, false),
        ("deafened", true, true),
    ] {
        let own = main::Own {
            portrait,
            muted,
            deafened,
        };
        let output = run(|ui| {
            main::draw_own(ui, &own, &|key| key.to_owned());
        });
        assert!(painted(&output), "{what}: painted nothing");
        assert!(
            clashes(&output).is_empty(),
            "{what}: {:?}",
            clashes(&output)
        );
    }
}

/// And with artwork, which takes the other branch.
#[test]
fn your_own_avatar_draws_dressed_too() {
    let own = main::Own {
        portrait: main::Portrait {
            name: "You",
            color_id: 3,
            state: Shown {
                at: 0,
                talking: true,
                alive: false,
                link: Link::Disconnected,
                using_radio: false,
            },
            art: Some(egui::TextureId::Managed(0)),
        },
        muted: false,
        deafened: false,
    };
    let output = run(|ui| {
        main::draw_own(ui, &own, &|key| key.to_owned());
    });
    assert!(painted(&output), "painted nothing");
    assert!(clashes(&output).is_empty(), "{:?}", clashes(&output));
}
