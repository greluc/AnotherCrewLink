//! Turning a player's name into the bytes the overlay wants.
//!
//! [`crate::sprite`] rasterises shapes and has no font in it, deliberately: a circle is
//! arithmetic and a glyph is not. This is the other half — the one place in the port that
//! turns a string into pixels — and it does it with the font machinery the client already
//! carries rather than with one of its own.
//!
//! # No new dependency, and that is the whole reason it is done this way
//!
//! `egui` is already a direct dependency of this crate, and it re-exports `epaint`, which
//! embeds a font and rasterises it. So a name costs no crate, no `.ttf` in the repository
//! and no licence to account for. The alternatives were a bitmap font written here, which
//! would draw a Cyrillic or CJK name as a row of boxes, or a rasteriser crate plus a face,
//! which is two entries in the dependency review for a label.
//!
//! What comes back is the same premultiplied BGRA the rest of the overlay is: the elevated
//! half composites bitmaps and knows nothing about text.
//!
//! # The atlas is copied, so ask for everything at once
//!
//! `FontsView::image()` clones the whole font atlas — one to four megabytes, depending on
//! what the window has drawn. [`lines`] therefore takes every string it is to rasterise
//! and copies once. Calling it per name would copy per name.

use crate::sprite::Bitmap;

/// RFC-free house rule: a name is drawn at this fraction of the crewmate's height.
///
/// The shipped overlay sizes its text in `vh` — a fraction of the *game window* — and the
/// port sizes its crewmates in pixels, so neither number carries over. This keeps the name
/// proportional to the thing it labels, which is what a reader actually compares it
/// against, and it is the same choice the port already made for the sprite itself.
pub const NAME_HEIGHT: f32 = 0.34;

/// How much room the plate leaves around the text, as a fraction of the text height.
///
/// `overlay.css:102` is a 10px transparent border on a ~13px face, and the translucent
/// black paints under it, so the plate is a little over three quarters of the text height
/// of padding on each side. This is that ratio rather than that pixel count, for the same
/// reason [`NAME_HEIGHT`] is a ratio.
pub const PLATE_PADDING: f32 = 0.75;

/// How dark the plate is. `rgba(0, 0, 0, 0.322)` — `overlay.css:100`.
const PLATE_ALPHA: f32 = 0.322;

/// Longest name the overlay will draw before it stops.
///
/// Among Us caps a name at ten characters, so this is not a truncation anybody will meet;
/// it is a bound on what a hostile or corrupted frame can make this allocate, because the
/// name comes out of another process's memory.
const MOST_CHARACTERS: usize = 32;

/// One line of text per string, rasterised at `pixels` tall.
///
/// `None` in a slot means that name could not be drawn — empty after trimming, or laid out
/// to nothing. The caller draws no name rather than an empty plate.
///
/// # Panics
///
/// Does not. `Context::fonts_mut` panics before the first frame, and every caller here is
/// inside one.
#[must_use]
pub fn lines(
    ctx: &egui::Context,
    texts: &[String],
    pixels: f32,
    colour: (u8, u8, u8),
) -> Vec<Option<Bitmap>> {
    if texts.is_empty() || pixels < 1.0 {
        return vec![None; texts.len()];
    }
    // A galley is laid out in *points* and the overlay works in the game's *pixels*, so the
    // size asked for is divided by the scale the atlas was rasterised at. Getting this
    // wrong does not fail, it draws the name at the wrong size on any display that is not
    // at 100%.
    let scale = ctx.pixels_per_point().max(0.1);
    let font = egui::FontId::proportional(pixels / scale);

    let galleys: Vec<Option<std::sync::Arc<egui::Galley>>> = ctx.fonts_mut(|fonts| {
        texts
            .iter()
            .map(|text| {
                let trimmed: String = text.trim().chars().take(MOST_CHARACTERS).collect();
                if trimmed.is_empty() {
                    return None;
                }
                Some(fonts.layout_no_wrap(trimmed, font.clone(), egui::Color32::WHITE))
            })
            .collect()
    });

    // After the layouts, never before: laying a string out is what puts its glyphs in the
    // atlas, so an image taken first would be missing exactly the glyphs about to be read.
    let atlas = ctx.fonts_mut(|fonts| fonts.image());

    galleys
        .into_iter()
        .map(|galley| draw(galley?.as_ref(), &atlas, scale, colour))
        .collect()
}

/// One galley's glyphs, blitted out of the atlas.
fn draw(
    galley: &egui::Galley,
    atlas: &egui::ColorImage,
    scale: f32,
    colour: (u8, u8, u8),
) -> Option<Bitmap> {
    let width = ceil(galley.rect.width() * scale);
    let height = ceil(galley.rect.height() * scale);
    if width <= 0 || height <= 0 {
        return None;
    }
    let mut bitmap = Bitmap::blank(width, height);
    let mut drew = false;

    for placed in &galley.rows {
        for glyph in &placed.row.glyphs {
            let uv = glyph.uv_rect;
            // A space, or a glyph the font has nothing for. `text_layout.rs:1158` skips it
            // for the same reason: there is nothing in the atlas to copy.
            if uv.is_nothing() {
                continue;
            }
            let left = round((placed.pos.x + glyph.pos.x + uv.offset.x) * scale);
            let top = round((placed.pos.y + glyph.pos.y + uv.offset.y) * scale);
            let from_x = usize::from(uv.min[0]);
            let from_y = usize::from(uv.min[1]);
            let across = usize::from(uv.max[0]).saturating_sub(from_x);
            let down = usize::from(uv.max[1]).saturating_sub(from_y);

            for row in 0..down {
                for column in 0..across {
                    let Some(texel) = atlas.pixels.get(
                        (from_y + row)
                            .checked_mul(atlas.size[0])
                            .and_then(|start| start.checked_add(from_x + column))?,
                    ) else {
                        continue;
                    };
                    // Every channel of a glyph texel is its coverage: the rasteriser paints
                    // opaque white and the atlas stores `Color32::from_white_alpha`. So the
                    // alpha is the antialiasing, and the colour is this caller's.
                    let coverage = f32::from(texel.a()) / 255.0;
                    if coverage > 0.0 {
                        drew = true;
                    }
                    bitmap.blend(
                        left + i32::try_from(column).unwrap_or(0),
                        top + i32::try_from(row).unwrap_or(0),
                        colour,
                        coverage,
                    );
                }
            }
        }
    }

    drew.then_some(bitmap)
}

/// A name on the plate the shipped overlay draws behind it.
///
/// The plate is what makes a name legible over a bright cartoon, and it is the only thing
/// doing that job: `overlay.css` has no text shadow and no stroke.
#[must_use]
pub fn plate(text: &Bitmap) -> Bitmap {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "pixel counts, and the padding is a ratio of one"
    )]
    let padding = (text.height as f32 * PLATE_PADDING).round() as i32;
    let width = text.width + 2 * padding;
    let height = text.height + 2 * padding;
    let mut plate = Bitmap::blank(width, height);

    // A stadium: `border-radius: 40px` on a box this size clamps to half the height.
    #[expect(clippy::cast_precision_loss, reason = "pixel counts")]
    let radius = (height as f32) / 2.0;
    fill_stadium(&mut plate, radius, PLATE_ALPHA);
    plate.composite(text, (padding, padding), (text.width, text.height));
    plate
}

/// A rounded-rectangle fill, black at `alpha`.
fn fill_stadium(into: &mut Bitmap, radius: f32, alpha: f32) {
    #[expect(clippy::cast_precision_loss, reason = "pixel counts")]
    let (width, height) = (into.width as f32, into.height as f32);
    for y in 0..into.height {
        for x in 0..into.width {
            #[expect(clippy::cast_precision_loss, reason = "pixel counts")]
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // Distance outside the inner rectangle the corners are rounded around.
            let dx = (radius - px).max(px - (width - radius)).max(0.0);
            let dy = (radius - py).max(py - (height - radius)).max(0.0);
            let outside = dx.hypot(dy) - radius;
            // One pixel of feathering, so the corners are not stepped.
            let coverage = (0.5 - outside).clamp(0.0, 1.0);
            if coverage > 0.0 {
                into.blend(x, y, (0, 0, 0), coverage * alpha);
            }
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a text box is tens of pixels, not two billion"
)]
fn ceil(value: f32) -> i32 {
    value.ceil().clamp(0.0, 4096.0) as i32
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a glyph sits inside a text box"
)]
fn round(value: f32) -> i32 {
    value.round().clamp(-4096.0, 4096.0) as i32
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_plate_is_bigger_than_its_text_and_darkest_in_the_middle() {
        let text = Bitmap::blank(40, 12);
        let plate = plate(&text);
        assert!(plate.width > text.width && plate.height > text.height);

        // The middle is the plate at full strength; the corner is outside the stadium.
        let middle = plate.at(plate.width / 2, plate.height / 2).unwrap();
        assert!(middle[3] > 0, "the plate is drawn at all");
        assert_eq!(plate.at(0, 0).unwrap()[3], 0, "the corner is rounded away");
    }

    #[test]
    fn a_plate_stays_premultiplied() {
        // `UpdateLayeredWindow` does not check, and straight alpha would fringe every
        // rounded corner. Black at any coverage is zero in all three colour channels.
        let plate = plate(&Bitmap::blank(20, 10));
        let (pixels, _) = plate.pixels.as_chunks::<4>();
        for pixel in pixels {
            assert!(pixel[0] <= pixel[3] && pixel[1] <= pixel[3] && pixel[2] <= pixel[3]);
        }
    }

    #[test]
    fn a_plate_for_an_empty_text_is_still_a_plate() {
        // `lines` returns `None` for a name that laid out to nothing, so `plate` is never
        // called with one -- but a zero-width bitmap must not panic if it ever is.
        let plate = plate(&Bitmap::blank(0, 0));
        assert_eq!(plate.width, 0);
        assert!(plate.pixels.is_empty());
    }
}
