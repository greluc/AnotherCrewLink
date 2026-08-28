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
/// `--state-muted` / `--acl-muted`. A switch that is closed, and the badge behind it.
pub const MUTED: Color32 = Color32::from_rgb(0xea, 0x3c, 0x2a);
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

// -------------------------------------------------------------- controls ---
/// `forms/Slider.jsx`: the rail behind the thumb, `rgba(255,255,255,.26)`.
///
/// Not in `tokens/colors.css`, and that is not an omission: it is MUI's own value, and
/// the component file is the only place the client ever wrote it down.
pub const RAIL: Color32 = Color32::from_rgba_premultiplied(66, 66, 66, 66);
/// `forms/Checkbox.jsx`: the edge of an unticked box, `rgba(255,255,255,.6)`.
pub const BOX_EDGE: Color32 = Color32::from_rgba_premultiplied(153, 153, 153, 153);

/// The rail's height, from `forms/Slider.jsx`.
pub const RAIL_H: f32 = 2.0;
/// The thumb's diameter, from the same file.
pub const THUMB: f32 = 12.0;
/// The tick box, from `forms/Checkbox.jsx`.
pub const BOX: f32 = 18.0;
/// The tick inside it.
pub const CHECK: f32 = 12.0;

/// The two radii that are not on the client's scale.
///
/// `tokens/radius.css` opens with "Four radii and one circle. Nothing in the client is
/// subtly rounded", and it is right about the client's own CSS. These two belong to the
/// MUI controls underneath it, which the client draws and did not draw itself: two pixels
/// on the tick box, one on the rail. Rounded up to `--radius-sm` the box would carry a
/// five-pixel radius on an eighteen-pixel side.
pub const RADIUS_BOX: u8 = 2;
/// See [`RADIUS_BOX`].
pub const RADIUS_RAIL: u8 = 1;

/// egui's divisor for a slider's handle: the row height over this is the handle radius.
///
/// Read out of `Slider::handle_radius` rather than assumed, and used twice — once to size
/// the row so that the handle lands on [`THUMB`], and once to cover egui's handle with the
/// one the design system asks for.
const HANDLE_DIVISOR: f32 = 2.5;

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

/// An icon button, as `core/IconButton.jsx` draws it.
///
/// A 30px round hit area with a **hover wash** rather than the green border every other
/// control gets. That is the component's own rule and not an oversight here: a 2px ring
/// around a 20px glyph reads as a boxed icon, and the chrome's icons already opt out of it
/// the same way.
///
/// `active` is the state the icon is *about* -- muted, deafened -- and it turns the glyph
/// `--state-muted`. The icon changes too, so the colour is the second signal rather than
/// the only one.
pub fn icon_button(ui: &mut egui::Ui, glyph: &str, active: bool, hint: &str) -> egui::Response {
    /// `IconButton`'s `size="small"`.
    const BOX: f32 = 30.0;
    /// The glyph inside it.
    const GLYPH: f32 = 20.0;

    // Allocated and painted here rather than handed to `egui::Button`, and that is the
    // second time this has been rewritten. A button's frame is its content plus
    // `button_padding`, minus its stroke width, offset by its expansion -- four numbers
    // that come from the shared style, and any of them being non-zero moves the frame
    // relative to the glyph inside it. The hover wash sat five pixels right of the icon it
    // was supposed to be behind. Here the wash and the glyph are both centred on one rect,
    // so they cannot drift apart.
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(BOX), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        // `rgba(255,255,255,0.08)`, and a little more while it is held.
        let wash = if response.is_pointer_button_down_on() {
            31
        } else if response.hovered() {
            20
        } else {
            0
        };
        if wash > 0 {
            ui.painter().circle_filled(
                rect.center(),
                BOX / 2.0,
                Color32::from_rgba_premultiplied(wash, wash, wash, wash),
            );
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            icon_font(GLYPH),
            if active { MUTED } else { Color32::WHITE },
        );
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
}

/// A settings toggle, as `forms/Checkbox.jsx` draws it.
///
/// Two things the shared style cannot say.
///
/// **The radius.** Every interactive widget carries `--radius-lg`, which is right for a
/// button and is more than half the side of an eighteen-pixel box: at ten, egui rounds it
/// to a circle. It did, until 2026-08-28, and eleven lobby rules were drawn with round
/// tick boxes.
///
/// **The two pictures.** Ticked is a filled purple box with a dark tick and no edge;
/// unticked is an empty box with a white one. egui reads one set of widget visuals
/// whatever the box holds, so the value picks them, in a scope that ends with the widget.
pub fn checkbox<'a>(
    ui: &mut egui::Ui,
    checked: &'a mut bool,
    label: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    let ticked = *checked;
    ui.scope(|ui| {
        let style = ui.style_mut();
        style.spacing.icon_width = BOX;
        style.spacing.icon_width_inner = CHECK;
        // The tick is stroked with `fg_stroke`, and the label takes its colour from
        // `fg_stroke` as well. The label is white in both states, so it is pinned here
        // rather than left to follow the tick into the dark.
        style.visuals.override_text_color = Some(Color32::WHITE);
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
        ] {
            widget.corner_radius = CornerRadius::same(RADIUS_BOX);
            widget.bg_fill = if ticked { PURPLE } else { Color32::TRANSPARENT };
            widget.fg_stroke = Stroke::new(2.0, BG_TITLEBAR);
        }
        // Green on hover is the client's one hover effect and stays. At rest a ticked box
        // has no edge at all, because the fill is the whole of it.
        style.visuals.widgets.inactive.bg_stroke = if ticked {
            Stroke::NONE
        } else {
            Stroke::new(2.0, BOX_EDGE)
        };
        ui.checkbox(checked, label)
    })
    .inner
}

/// A settings slider, as `forms/Slider.jsx` draws it.
///
/// Four things the shared style cannot say, each because the field is shared with
/// something that wants the opposite.
///
/// **The rail.** egui fills it with `widgets.inactive.bg_fill`, which the style leaves
/// transparent so that buttons are outlines. A transparent rail is no rail: until
/// 2026-08-28 the voice-distance slider was a ring floating over nothing, which is how
/// this function came to exist.
///
/// **Its radius**, which is `widgets.inactive.corner_radius` — ten pixels asked of a
/// two-pixel rail.
///
/// **The travelled part**, which egui takes from `selection.bg_fill`. That is also the
/// text-selection colour, and the system asks for 35% purple there and solid purple here.
///
/// **The thumb.** egui paints it from the same `bg_fill` as the rail, so at rest the two
/// cannot differ — and the design has a purple disc on a pale rail. So egui's handle is
/// covered by the disc the system asks for, at exactly egui's own radius, which is why the
/// row is sized to make that radius half of [`THUMB`].
pub fn slider(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
) -> egui::Response {
    let (low, high) = (*range.start(), *range.end());
    let response = ui
        .scope(|ui| {
            let style = ui.style_mut();
            style.spacing.slider_rail_height = RAIL_H;
            // egui takes the row as the larger of the body line height and this, and the
            // handle as the row over `HANDLE_DIVISOR`. Both halves are set, because the
            // body face at 14px is already taller than the row this wants.
            style.spacing.interact_size.y = THUMB * HANDLE_DIVISOR / 2.0;
            style
                .text_styles
                .insert(TextStyle::Body, FontId::new(12.0, FontFamily::Proportional));
            style.visuals.selection.bg_fill = PURPLE;
            // egui's default handle is a rectangle -- `HandleShape::Rect { aspect_ratio:
            // 0.75 }` -- and this one is covered by a disc. A rectangle under a disc of
            // the same radius leaves four pale corners sticking out of it, which is what
            // the first build of this function put on screen.
            style.visuals.handle_shape = egui::style::HandleShape::Circle;
            let widgets = &mut style.visuals.widgets;
            widgets.inactive.bg_fill = RAIL;
            widgets.inactive.corner_radius = CornerRadius::same(RADIUS_RAIL);
            // egui outlines the handle with `fg_stroke`, which the shared style leaves a
            // white pixel wide. Half of that stroke falls outside the disc drawn over it,
            // so at rest the thumb wore a pale ring nothing asked for. Hover keeps its
            // green one -- the outer half is what shows, which is the border the client's
            // one hover effect means.
            widgets.inactive.fg_stroke = Stroke::NONE;
            widgets.hovered.fg_stroke = Stroke::new(2.0, GREEN);
            widgets.active.fg_stroke = Stroke::new(2.0, GREEN);
            ui.add(
                egui::Slider::new(value, range)
                    .step_by(step)
                    // The number goes in the label — "Voice Distance: 5.3" — which is
                    // where the component puts it. egui's box beside the rail is a second
                    // place to read one number, and a second thing to line up.
                    .show_value(false)
                    .trailing_fill(true),
            )
        })
        .inner;

    let rect = response.rect;
    let radius = rect.height() / HANDLE_DIVISOR;
    // The same span egui drags over: the rail inset by a handle at each end, so the thumb
    // cannot hang past either. `Slider::position_range`, and the reason the disc lands
    // where the pointer left it.
    let travel = rect.x_range().shrink(radius);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a fraction of the way along a slider, and f32 is what the painter takes"
    )]
    let along = (((*value - low) / (high - low)) as f32).clamp(0.0, 1.0);
    let centre = egui::pos2(travel.min + travel.span() * along, rect.center().y);
    ui.painter().circle_filled(centre, radius, PURPLE);
    response
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
