# Implementing this design system in egui

The Rust client draws with **egui 0.36.1** (`crates/acl-ui`, wgpu backend through eframe,
with WARP as the last rung — `renderer.rs`). egui is not CSS, so most of this design system
translates through a `Style` built once at startup rather than through per-widget code.
This file is that translation, plus the handful of places where egui cannot do what the
Electron client did and the design has to change rather than be faked.

The crate's own rule stays in force: `acl-ui` depends on `egui` **only inside
`views/`**. Everything else decides something and is testable without a window. Do not
move a decision into a paint function.

## 1. One `Style`, built from the tokens

Write it once, in a `theme.rs` beside the views, and never set a colour inline in a view.
Token values are in `tokens/colors.css`; the names below are the same ones.

```rust
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

// Shell — violet-leaning, never neutral black.
const BG_BODY:     Color32 = Color32::from_rgb(0x25, 0x23, 0x2a); // --acl-bg-body
const BG_TITLEBAR: Color32 = Color32::from_rgb(0x1d, 0x1a, 0x23); // --acl-bg-titlebar
const BG_PAPER:    Color32 = Color32::from_rgb(0x27, 0x27, 0x27); // --acl-bg-paper
const HAIRLINE:    Color32 = Color32::from_rgb(0x31, 0x31, 0x35); // --border-hairline
// Accents.
const PURPLE:      Color32 = Color32::from_rgb(0xba, 0x68, 0xc8); // --accent-primary
const RED:         Color32 = Color32::from_rgb(0xf4, 0x43, 0x36); // --accent-secondary
const GREEN:       Color32 = Color32::from_rgb(0x00, 0xff, 0x00); // --accent-action
const ICON_QUIET:  Color32 = Color32::from_rgb(0x77, 0x77, 0x77); // --text-icon

pub fn style(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG_BODY;
    visuals.window_fill = BG_PAPER;
    visuals.extreme_bg_color = BG_TITLEBAR;      // text edits, table head
    visuals.window_stroke = Stroke::new(1.0, HAIRLINE);
    visuals.window_corner_radius = CornerRadius::same(8);   // --radius-md
    visuals.selection.bg_fill = PURPLE.linear_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, PURPLE);
    visuals.hyperlink_color = PURPLE;
    visuals.error_fg_color = RED;
    visuals.warn_fg_color = Color32::from_rgb(0xe6, 0x7e, 0x22); // --state-degraded

    // Widgets: rest is quiet, hover is the green border, active is the accent.
    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = BG_BODY;
    w.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    w.inactive.bg_fill = Color32::TRANSPARENT;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(2.0, Color32::WHITE);
    w.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    w.hovered.bg_stroke = Stroke::new(2.0, GREEN);     // the client's one hover effect
    w.hovered.expansion = 0.0;                          // nothing grows on hover
    w.active.bg_stroke = Stroke::new(2.0, GREEN);
    w.active.expansion = 0.0;                           // and nothing moves on press
    for widget in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        widget.corner_radius = CornerRadius::same(10);   // --radius-lg
    }

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);   // --space-1
    style.spacing.button_padding = egui::vec2(10.0, 2.0);
    style.spacing.interact_size.y = 24.0;                // --titlebar-h, and a row's floor
    style.spacing.slider_width = 200.0;                  // --mic-bar-w
    style.text_styles = text_styles();
    ctx.set_style(style);
}
```

`expansion = 0.0` on every interactive state is not a detail: egui's default grows a
widget by a pixel on hover, and this product's hover is a colour change and nothing else.

## 2. Fonts

Both families are the client's own (`@fontsource/varela-round`, `@fontsource/source-code-pro`).
egui has no fallback chain to lean on, so register them and map the text styles; the sizes
are the client's, not egui's defaults.

```rust
pub fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("varela".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/VarelaRound-Regular.ttf"))));
    fonts.font_data.insert("code".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/SourceCodePro-Medium.ttf"))));
    fonts.families.insert(FontFamily::Proportional, vec!["varela".into()]);
    fonts.families.insert(FontFamily::Monospace, vec!["code".into()]);
    fonts
}

fn text_styles() -> std::collections::BTreeMap<TextStyle, FontId> {
    use FontFamily::{Monospace, Proportional};
    [
        (TextStyle::Heading, FontId::new(20.0, Proportional)),  // the one heading level
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Small, FontId::new(11.0, Proportional)),    // overlay names
        (TextStyle::Monospace, FontId::new(28.0, Monospace)),   // the lobby code, and only it
        (TextStyle::Name("code".into()), FontId::new(28.0, Monospace)),
    ].into()
}
```

Ship the two TTFs with the crate. A missing font in egui is not a fallback, it is a row of
tofu — and Varela Round is what makes the window look like this product.

## 3. What maps to what

| This design system | egui |
| --- | --- |
| `Button` (contained / text) | `ui.button` on the styled `Visuals`; contained = `Button::new(..).fill(RED)` |
| `OutlineButton`, `LaunchButton` | `Button::new(..).fill(TRANSPARENT).stroke(Stroke::new(4.0, WHITE))`, green stroke on hover |
| `IconButton` | `Button::image` with a loaded SVG, or `ImageButton`; tint `ICON_QUIET` |
| `Checkbox` | `ui.checkbox` — `settings_screen::Kind::Toggle` |
| `RadioOption` | `ui.radio_value` |
| `Slider` | `Slider::new(&mut v, min..=max).step_by(step)`; the label goes above, as `Kind::Slider` draws it |
| `SelectField` | `ComboBox::from_id_salt(key)` — `Kind::Device` / `Kind::Language` |
| `TextField` | `ui.text_edit_singleline`, committed on `lost_focus()` |
| `Divider` | `ui.separator()` with 16px of `add_space` either side |
| `SectionHeading` | `ui.heading(t(title))` |
| `Alert` | a `Frame` with `fill = tone.linear_multiply(0.16)` and the matching `fg_stroke` |
| `Tooltip` | `response.on_hover_text(..)` — **required**, see §5 |
| `Dialog` | a modal `egui::Window`, or `egui::Modal`; owned by the caller, not by the view |
| `MeterBar` | `ProgressBar::new(rms).desired_height(8.0)` |
| `StatusBadge` | `views::main::indicators` — egui draws a ring, not a badge; keep the pairing of colour and hover text |
| `Crewmate` | `painter.image` of a `TextureHandle` (`worn.rs` composites, `sprite.rs` rasterises); shapes fallback in `views::main::shapes_at` |
| `LobbyCode` | `RichText::new(code).monospace()` over a `Frame` filled with the crew colour |
| `LobbyTable` | a `Grid` or `TableBuilder` — `views::lobby_browser` |
| `TitleBar` | `ViewportBuilder::with_decorations(false)` plus a `TopBottomPanel` of 24px, drag with `window_drag` |

## 4. Where egui cannot do what CSS did

- **`backdrop-filter: blur(4px)`** — the settings scrim's one blur. egui has no backdrop
  filter and faking it costs a render target. Draw the settings screen as an opaque
  `CentralPanel` filled `BG_BODY`, or a `#171717` fill at full alpha. Do not settle for
  transparent-without-blur: unblurred translucency over a roster is unreadable, which is
  the thing the blur was there to prevent.
- **`box-shadow`** — nothing to port; the product has none. Leave
  `visuals.window_shadow` at `Shadow::NONE` and let borders carry depth.
- **The meeting overlay's coloured glow** (`0 0 h/100 h/100 <crew colour>`) is the one
  exception. `Shadow { blur, spread, color, offset: [0, 0] }` on the tile's frame, both
  values `height / 100`, or two `rect_stroke` passes at falling alpha.
- **Percentage-positioned cosmetics.** `hats.json` places a hat at `130% / -78% / -14%`
  of the avatar box. In egui, multiply through the avatar rect —
  `rect.width() * 1.30`, `rect.top() + rect.height() * (0.22 - 0.78)` — and let the hat
  overhang the slot rather than clipping it (Avatar.tsx renders hats outside the round
  frame; `overflow` is off by default).
- **CSS transitions.** There is no transition property. Use
  `ctx.animate_bool_with_time(id, on, 0.2)` and interpolate yourself; the four durations
  are 0.05 / 0.10 / 0.20 / 0.40 s. Anything animated must also be correct on the first
  frame, because a repaint is not guaranteed.
- **`::-webkit-scrollbar`** styling. `ScrollArea` draws egui's own bar; set
  `style.spacing.scroll.bar_width = 8.0` and let the rest go.

## 5. Rules this GUI has to keep

1. **Every colour-coded state is also words.** `views/main.rs::describe` exists for this,
   and the roster's states are `on_hover_text` without exception. egui has no `title`
   attribute to inherit — if you do not call it, the information is gone.
2. **A view never writes a setting.** Controls return `Change::{Set, Run, Capture}` and
   the caller applies them, because a warning is a dialog and a dialog outlives the frame
   that raised it (`views/settings.rs`).
3. **A disabled control says why.** `ui.add_enabled_ui(state.enabled, ..)` and then
   `response.on_hover_text(reason)` — the reason is the i18n key `availability()` returned,
   not a guess made in the view.
4. **No indicator for a healthy state.** `outline_for(Link::Connected)` returns `None` on
   purpose.
5. **Draw the crewmate square.** Sprites are square and cosmetics are placed as fractions
   of the width; fitting one into a non-square rect moves every hat.
6. **The window must work at 250px.** `height_for` and `slot_rect` are the arithmetic;
   test a fifteen-player lobby at the minimum width, not at the default one.
7. **Keep px values as intent, not contract.** §4.8: layout, spacing and control
   affordances will differ from the Electron client. What every control *does* must not.

## 6. Things worth doing because it is egui, not despite it

- `ctx.set_pixels_per_point` from the OS scale factor: the whole design is specified in
  logical px, so a 24px title bar stays 24px at 150%.
- One `TextureHandle` per (colour, cosmetic set), built once and cached — the same reason
  the Electron client generates recoloured bodies into `userData` instead of per frame.
  Fifteen players re-composited every frame would be the most expensive thing in the
  window.
- `Id`-based animation state instead of component state: `animate_bool_with_time` keyed on
  the player's client id survives the roster being re-sorted, which the CSS transition on
  `border-color` did not.
- The overlay viewport wants `with_transparent(true)` and `with_mouse_passthrough(true)`:
  it is drawn over a running game and must never take a click.
