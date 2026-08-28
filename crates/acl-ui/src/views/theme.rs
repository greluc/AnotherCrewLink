//! The one `Style`, built from the design system's tokens.
//!
//! `design/guidelines/egui-implementation.md` §1 and §2. Every colour, radius and size here
//! is a named token in `design/tokens/`; nothing in a view sets a colour inline, and the
//! comment beside each constant is the token it comes from.
//!
//! It lives under `views/` because that is where this crate is allowed to touch `egui` —
//! everything above decides something and is testable without a window.
//!
//! # What was missing
//!
//! Until 2026-08-28 the client set no style at all. It drew with egui's stock dark theme:
//! a neutral grey window, egui's own fonts, egui's radii, and a hover that grows a widget
//! by a pixel. None of that is this product. The design system describes a window that is
//! "almost entirely two dark violet-tinted greys" in Varela Round, whose one hover effect
//! is a green border and where nothing moves on press.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

// ---------------------------------------------------------------- shell ----
/// `--acl-bg-body`. Violet-leaning, and never a neutral grey.
pub const BG_BODY: Color32 = Color32::from_rgb(0x25, 0x23, 0x2a);
/// `--acl-bg-titlebar`, also the lobby table's head.
pub const BG_TITLEBAR: Color32 = Color32::from_rgb(0x1d, 0x1a, 0x23);
/// `--acl-bg-paper`. Dropdowns and popups.
pub const BG_PAPER: Color32 = Color32::from_rgb(0x27, 0x27, 0x27);
/// `--acl-border-hairline`. The line above every settings row.
pub const HAIRLINE: Color32 = Color32::from_rgb(0x31, 0x31, 0x35);

// --------------------------------------------------------------- accents ---
/// `--acl-purple-300`. The app name and every primary control.
pub const PURPLE: Color32 = Color32::from_rgb(0xba, 0x68, 0xc8);
/// `--acl-red-500`. Contained buttons and the mic meter.
pub const RED: Color32 = Color32::from_rgb(0xf4, 0x43, 0x36);
/// `--acl-focus-green`. The hover border, and nothing else in the product.
pub const GREEN: Color32 = Color32::from_rgb(0x00, 0xff, 0x00);
/// `--acl-icon-quiet`. Every icon in the chrome, and no other colour.
pub const ICON_QUIET: Color32 = Color32::from_rgb(0x77, 0x77, 0x77);

// --------------------------------------------------------------- signals ---
/// `--acl-talking`. A ring outside the body, never a fill.
pub const TALKING: Color32 = Color32::from_rgb(0x2e, 0xcc, 0x71);
/// `--acl-link-down`. No connection, as this GUI draws it.
pub const LINK_DOWN: Color32 = Color32::from_rgb(0xd2, 0x5a, 0x5a);
/// `--acl-link-silent`. Connected and carrying no voice.
pub const LINK_SILENT: Color32 = Color32::from_rgb(0xdc, 0xb4, 0x50);
/// `--state-degraded` / `--acl-novoice`. The one warning colour.
pub const DEGRADED: Color32 = Color32::from_rgb(0xe6, 0x7e, 0x22);

// ---------------------------------------------------------------- radius ---
/// `--radius-sm`. The lobby code, tooltips, the scrollbar thumb.
pub const RADIUS_SM: u8 = 5;
/// `--radius-md`. The overlay wrapper and egui's own windows.
pub const RADIUS_MD: u8 = 8;
/// `--radius-lg`. Buttons.
pub const RADIUS_LG: u8 = 10;

/// `--titlebar-h`, which is also a row's floor.
pub const TITLEBAR_H: f32 = 24.0;
/// `--resize-strip-h`.
pub const RESIZE_STRIP_H: f32 = 4.0;
/// `--mic-bar-w`.
pub const METER_W: f32 = 200.0;
/// `--mic-bar-h`.
pub const METER_H: f32 = 8.0;

/// The icons the client draws, by codepoint.
///
/// Codepoints and not ligature names, which is the whole reason this module exists:
/// Material Symbols addresses an icon as the ligature `mic_off`, and **egui applies no
/// OpenType substitutions**. Written as a name it would render as eight letters.
pub mod icon {
    /// The gear.
    pub const SETTINGS: &str = "\u{e8b8}";
    /// Reload.
    pub const REFRESH: &str = "\u{e5d5}";
    /// Close.
    pub const CLOSE: &str = "\u{e5cd}";
    /// Back, out of the settings.
    pub const ARROW_BACK: &str = "\u{e5c4}";
    /// Minimise.
    pub const MINIMIZE: &str = "\u{e931}";
    /// The public lobby browser.
    pub const PUBLIC: &str = "\u{e80b}";
    /// The microphone, open.
    pub const MIC: &str = "\u{e31d}";
    /// The microphone, muted.
    pub const MIC_OFF: &str = "\u{e02b}";
    /// Output, on.
    pub const VOLUME_UP: &str = "\u{e050}";
    /// Output, off — deafened.
    pub const VOLUME_OFF: &str = "\u{e04f}";
    /// The impostor radio. The shipped client draws a bespoke SVG here; this is the
    /// nearest glyph in the family the rest of the chrome already uses.
    pub const RADIO: &str = "\u{e03e}";
}

/// The font family the icon glyphs live in.
pub const ICON_FAMILY: &str = "icons";

/// A [`FontId`] for an icon at a given size.
#[must_use]
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICON_FAMILY.into()))
}

/// The three families the client ships.
///
/// **egui's defaults are kept as fallbacks after ours**, which the design system's own
/// snippet does not do — it replaces the family outright. Measured on 2026-08-28: of the
/// 84 characters this client can draw, Varela Round covers 83. The one it does not is
/// `✕`, and a family with no fallback renders that as tofu. Ours first, egui's behind.
#[must_use]
pub fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "varela".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/VarelaRound-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "code".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/SourceCodePro-Medium.ttf"
        ))),
    );
    fonts.font_data.insert(
        ICON_FAMILY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/MaterialSymbolsRounded-Subset.ttf"
        ))),
    );

    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "varela".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
        family.insert(0, "code".to_owned());
    }
    // Its own family, so a label asking for an icon cannot get a letter from Varela and a
    // label asking for a letter cannot get an icon.
    fonts.families.insert(
        FontFamily::Name(ICON_FAMILY.into()),
        vec![ICON_FAMILY.to_owned()],
    );
    fonts
}

/// The text scale, which is the client's and not egui's.
///
/// "The window scale is small and coarse: 28 / 24 / 20 / 19 / 14 / 12 / 11 / 10, and
/// nothing in the client goes outside it."
fn text_styles() -> std::collections::BTreeMap<TextStyle, FontId> {
    use FontFamily::{Monospace, Proportional};
    [
        // `--size-h6`. The one heading level there is.
        (TextStyle::Heading, FontId::new(20.0, Proportional)),
        // `--size-body`, and the floor for anything in the window.
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        // `--size-name-overlay`. Names over the game, and nothing smaller.
        (TextStyle::Small, FontId::new(11.0, Proportional)),
        // `--size-code`. The lobby code, and only it.
        (TextStyle::Monospace, FontId::new(28.0, Monospace)),
    ]
    .into()
}

/// Applies the design system to a context. Call once, at start-up.
///
/// Both themes get the same style and the preference is pinned to dark, because there is
/// only one palette: "a small dark tool window that sits beside a bright cartoon game".
/// Left unpinned, a machine set to light would show egui's light theme through every
/// surface this does not explicitly fill.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::ThemePreference::Dark);

    let visuals = visuals();
    let styles = text_styles();
    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        // `--space-1`.
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 2.0);
        style.spacing.interact_size.y = TITLEBAR_H;
        style.spacing.slider_width = METER_W;
        // `--scrollbar-w`, because the window is small.
        style.spacing.scroll.bar_width = 8.0;
        style.text_styles = styles.clone();
    });
}

/// The palette and the widget states.
fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG_BODY;
    visuals.window_fill = BG_PAPER;
    // Text edits and the lobby table's head.
    visuals.extreme_bg_color = BG_TITLEBAR;
    visuals.window_stroke = Stroke::new(1.0, HAIRLINE);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_MD);
    // "There is no shadow system: no `box-shadow` in the client's own CSS."
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.selection.bg_fill = PURPLE.linear_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, PURPLE);
    visuals.hyperlink_color = PURPLE;
    visuals.error_fg_color = RED;
    visuals.warn_fg_color = DEGRADED;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = BG_BODY;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.inactive.bg_fill = Color32::TRANSPARENT;
    widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::WHITE);
    widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    // The client's one hover effect.
    widgets.hovered.bg_stroke = Stroke::new(2.0, GREEN);
    widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.active.bg_stroke = Stroke::new(2.0, GREEN);
    widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.open.bg_stroke = Stroke::new(2.0, GREEN);
    for widget in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(RADIUS_LG);
        // Not a detail: egui grows a widget by a pixel on hover, and this product's hover
        // is a colour change and nothing else. "Nothing moves, scales or fills."
        widget.expansion = 0.0;
    }

    visuals
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::expect_used)]

    use super::{fonts, icon, text_styles};
    use egui::{FontFamily, TextStyle};

    /// Every size in the text scale is one the design system names.
    ///
    /// "The window scale is small and coarse: 28 / 24 / 20 / 19 / 14 / 12 / 11 / 10, and
    /// nothing in the client goes outside it." A size invented here would be a step nobody
    /// chose, and it would be invisible until somebody held the window against a mockup.
    #[test]
    fn the_text_scale_is_the_one_the_system_names() {
        const SCALE: [f32; 8] = [28.0, 24.0, 20.0, 19.0, 14.0, 12.0, 11.0, 10.0];
        for (style, font) in text_styles() {
            assert!(
                SCALE.contains(&font.size),
                "{style:?} is {}px, which is not in the scale",
                font.size
            );
        }
    }

    /// The lobby code is the one monospace thing, and it is 28px.
    #[test]
    fn the_code_is_the_only_monospace_style() {
        let styles = text_styles();
        let code = &styles[&TextStyle::Monospace];
        assert_eq!(code.family, FontFamily::Monospace);
        assert!((code.size - 28.0).abs() < f32::EPSILON);
        for (style, font) in &styles {
            if *style != TextStyle::Monospace {
                assert_eq!(
                    font.family,
                    FontFamily::Proportional,
                    "{style:?} should be the UI face"
                );
            }
        }
    }

    /// Ours are first and egui's are still there.
    ///
    /// The design system's snippet replaces the family outright, and that would lose the
    /// one glyph Varela Round does not have. Both halves are checked, because either alone
    /// is a way to be wrong: no fallback is tofu, and no Varela is not this product.
    #[test]
    fn the_shipped_faces_come_first_and_the_defaults_remain() {
        let fonts = fonts();
        let proportional = &fonts.families[&FontFamily::Proportional];
        assert_eq!(proportional.first().map(String::as_str), Some("varela"));
        assert!(
            proportional.len() > 1,
            "no fallback: a glyph Varela lacks would be tofu"
        );
        let monospace = &fonts.families[&FontFamily::Monospace];
        assert_eq!(monospace.first().map(String::as_str), Some("code"));
        assert!(monospace.len() > 1);
    }

    /// The icons are their own family, and every one is a single character.
    ///
    /// A ligature name would render as its letters: egui applies no OpenType
    /// substitutions. This is the check that nobody writes `"mic_off"` here.
    #[test]
    fn every_icon_is_one_codepoint_in_the_private_use_area() {
        for (name, glyph) in [
            ("settings", icon::SETTINGS),
            ("refresh", icon::REFRESH),
            ("close", icon::CLOSE),
            ("arrow_back", icon::ARROW_BACK),
            ("minimize", icon::MINIMIZE),
            ("public", icon::PUBLIC),
            ("mic", icon::MIC),
            ("mic_off", icon::MIC_OFF),
            ("volume_up", icon::VOLUME_UP),
            ("volume_off", icon::VOLUME_OFF),
            ("radio", icon::RADIO),
        ] {
            let mut characters = glyph.chars();
            let one = characters.next().expect("a glyph");
            assert!(characters.next().is_none(), "{name} is more than one char");
            assert!(
                (0xE000..=0xF8FF).contains(&(one as u32)),
                "{name} is U+{:04X}, outside the private use area",
                one as u32
            );
        }
    }

    /// The icon family exists and holds exactly the icon face.
    #[test]
    fn the_icons_have_a_family_of_their_own() {
        let fonts = fonts();
        let family = &fonts.families[&FontFamily::Name(super::ICON_FAMILY.into())];
        assert_eq!(family, &vec![super::ICON_FAMILY.to_owned()]);
    }
}
