//! Rasterising a crewmate into the bytes the overlay wants.
//!
//! §4.7 puts the overlay in the elevated process and forbids it an image decoder, so what
//! crosses the pipe is "pre-rasterised sprites" — and `acl_ipc::CoreMessage::DrawSprite`
//! spells out the format: premultiplied BGRA, `width * height * 4` bytes, top row first.
//!
//! This is where a player becomes those bytes. It has no `egui` in it and does not need
//! one: the overlay is not an egui surface, it is a bitmap blitted into a layered window,
//! and going through a GPU toolkit to produce a 64-pixel circle would mean a GPU context in
//! a process §6 forbids one.
//!
//! # Premultiplied, and why that is not a detail
//!
//! `UpdateLayeredWindow` with `AC_SRC_ALPHA` requires premultiplied colour and does not
//! check. Straight alpha produces a picture that looks approximately right and has a bright
//! fringe wherever anything is partly transparent — which is every antialiased edge, so
//! every crewmate in the overlay would be outlined in white.

/// A rasterised picture: premultiplied BGRA, top row first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitmap {
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
    /// `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl Bitmap {
    /// A fully transparent bitmap.
    #[must_use]
    pub fn blank(width: i32, height: i32) -> Self {
        let count = usize::try_from(width.max(0))
            .unwrap_or(0)
            .saturating_mul(usize::try_from(height.max(0)).unwrap_or(0))
            .saturating_mul(4);
        Self {
            width,
            height,
            pixels: vec![0; count],
        }
    }

    /// The four bytes at a pixel, for a caller checking its work.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let index = usize::try_from((y * self.width + x) * 4).ok()?;
        self.pixels
            .get(index..index + 4)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
    }

    /// Blends one premultiplied pixel in, source-over.
    ///
    /// `coverage` is the antialiasing: how much of the pixel the shape covers, 0 to 1. The
    /// colour is multiplied by it here, which is what makes the result premultiplied
    /// without the caller having to think about it.
    ///
    /// Public to this crate since 2026-08-29, for [`crate::text`]: a glyph out of the font
    /// atlas is a coverage value per pixel and needs exactly this, and duplicating it there
    /// would be a second place for the premultiplication to be got wrong.
    pub(crate) fn blend(&mut self, x: i32, y: i32, colour: (u8, u8, u8), coverage: f32) {
        if coverage <= 0.0 || x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let coverage = coverage.min(1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "coverage is clamped to 0..=1, so the products are within a byte"
        )]
        let source = [
            (f32::from(colour.2) * coverage) as u8,
            (f32::from(colour.1) * coverage) as u8,
            (f32::from(colour.0) * coverage) as u8,
            (255.0 * coverage) as u8,
        ];
        let Ok(index) = usize::try_from((y * self.width + x) * 4) else {
            return;
        };
        let Some(destination) = self.pixels.get_mut(index..index + 4) else {
            return;
        };
        let inverse = u32::from(255 - source[3]);
        for (channel, byte) in destination.iter_mut().enumerate() {
            let Some(over) = source.get(channel).copied() else {
                continue;
            };
            // Rounded rather than truncated, for the reason the helper's own blend gives:
            // truncating makes repeated blends drift darker one step at a time.
            let kept = (u32::from(*byte) * inverse + 127) / 255;
            *byte = u8::try_from(u32::from(over) + kept).unwrap_or(u8::MAX);
        }
    }

    /// Draws a filled circle, antialiased.
    ///
    /// One sample per pixel against the distance to the centre, with the coverage falling
    /// off over the last pixel of the edge. Not a proper area computation — for a shape this
    /// size the difference is invisible, and the alternative is supersampling every avatar
    /// four times a second for nothing.
    pub fn circle(&mut self, centre: (f32, f32), radius: f32, colour: (u8, u8, u8)) {
        if radius <= 0.0 {
            return;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "pixel bounds, taken from coordinates already inside a bitmap"
        )]
        let bounds = (
            (centre.0 - radius - 1.0).floor() as i32,
            (centre.1 - radius - 1.0).floor() as i32,
            (centre.0 + radius + 1.0).ceil() as i32,
            (centre.1 + radius + 1.0).ceil() as i32,
        );
        for y in bounds.1..bounds.3 {
            for x in bounds.0..bounds.2 {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "pixel indices, far below f32's exact integer range"
                )]
                let distance = {
                    let dx = (x as f32) + 0.5 - centre.0;
                    let dy = (y as f32) + 0.5 - centre.1;
                    dx.hypot(dy)
                };
                self.blend(x, y, colour, radius - distance + 0.5);
            }
        }
    }

    /// Draws a circular outline, antialiased.
    pub fn ring(&mut self, centre: (f32, f32), radius: f32, thickness: f32, colour: (u8, u8, u8)) {
        if radius <= 0.0 || thickness <= 0.0 {
            return;
        }
        let outer = radius + thickness / 2.0;
        let inner = radius - thickness / 2.0;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "pixel bounds, taken from coordinates already inside a bitmap"
        )]
        let bounds = (
            (centre.0 - outer - 1.0).floor() as i32,
            (centre.1 - outer - 1.0).floor() as i32,
            (centre.0 + outer + 1.0).ceil() as i32,
            (centre.1 + outer + 1.0).ceil() as i32,
        );
        for y in bounds.1..bounds.3 {
            for x in bounds.0..bounds.2 {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "pixel indices, far below f32's exact integer range"
                )]
                let distance = {
                    let dx = (x as f32) + 0.5 - centre.0;
                    let dy = (y as f32) + 0.5 - centre.1;
                    dx.hypot(dy)
                };
                let coverage = (outer - distance + 0.5).min(distance - inner + 0.5);
                self.blend(x, y, colour, coverage);
            }
        }
    }
}

/// What one crewmate in the overlay looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crewmate {
    /// The body colour, as red, green and blue.
    pub body: (u8, u8, u8),
    /// The shadow colour.
    pub shadow: (u8, u8, u8),
    /// Whether to draw the speaking ring.
    pub talking: bool,
    /// Whether to draw them at full strength.
    pub alive: bool,
}

/// The colour of the ring that says somebody is speaking.
///
/// The same green the main view uses, so the two windows agree about what green means.
pub const TALKING: (u8, u8, u8) = (80, 220, 120);

/// Rasterises one crewmate into a square bitmap.
///
/// The shape matches the main view's: a shadow, a body offset up and left of it, and a
/// visor. Two windows drawing the same player differently is a bug report about the wrong
/// thing.
#[must_use]
pub fn crewmate(size: i32, crew: Crewmate) -> Bitmap {
    let mut bitmap = Bitmap::blank(size, size);
    if size <= 0 {
        return bitmap;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a sprite size in pixels, far below f32's exact integer range"
    )]
    let extent = size as f32;
    let centre = (extent / 2.0, extent / 2.0);
    // Room for the ring, whether or not it is drawn: a sprite that changed size when
    // somebody started speaking would jump in the overlay every time they did.
    let radius = extent / 2.0 - extent * 0.12;

    let (body, shadow) = if crew.alive {
        (crew.body, crew.shadow)
    } else {
        // Faint rather than different, for the reason the main view gives: knowing *who* is
        // dead is the point of showing them.
        (dim(crew.body), dim(crew.shadow))
    };

    if crew.talking {
        bitmap.ring(centre, extent / 2.0 - extent * 0.04, extent * 0.06, TALKING);
    }
    bitmap.circle(centre, radius, shadow);
    bitmap.circle(
        (centre.0 - extent * 0.03, centre.1 - extent * 0.03),
        radius - extent * 0.04,
        body,
    );
    bitmap.circle(
        (centre.0 + radius * 0.35, centre.1 - radius * 0.25),
        radius * 0.42,
        (0xBE, 0xE3, 0xF5),
    );
    bitmap
}

/// Takes a colour down to a third, the way the main view fades a dead player.
fn dim(colour: (u8, u8, u8)) -> (u8, u8, u8) {
    (colour.0 / 3, colour.1 / 3, colour.2 / 3)
}

/// Reads a PNG into a bitmap, premultiplied.
///
/// The hat artwork is 8-bit RGBA — `ratHat.png` is 270×428, colour type 6 — but this asks
/// the decoder to normalise anyway, because the collection is 983 files and one of them
/// being a palette or 16-bit would otherwise be a panic on somebody's machine rather than
/// a hat that looks slightly wrong here.
///
/// **Premultiplied on the way in**, like everything else in this module: the layered window
/// requires it, and converting once at the edge is the only way to keep the invariant the
/// blending relies on. See the module documentation.
///
/// `None` for anything that is not a PNG this decoder can read, which costs one cosmetic
/// layer. The alternative — refusing to draw the avatar — is worse for a file served over
/// a CDN.
#[must_use]
pub fn decode_png(bytes: &[u8]) -> Option<Bitmap> {
    // `Cursor`, because `png` 0.18 wants `BufRead + Seek` and a slice is neither.
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // Both are no-ops for the 8-bit RGBA the collection ships, and both are what keeps a
    // stray palette or 16-bit file from arriving in a layout this does not expect.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut raw = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut raw).ok()?;
    let width = i32::try_from(info.width).ok()?;
    let height = i32::try_from(info.height).ok()?;

    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        // Indexed is gone after `normalize_to_color8`; if it somehow is not, the pixel
        // layout is not what the loop below assumes.
        png::ColorType::Indexed => return None,
    };

    let mut bitmap = Bitmap::blank(width, height);
    for (at, pixel) in raw.chunks_exact(channels).enumerate() {
        // `chunks_exact` guarantees the length, but the lint does not read that far and
        // an index that "cannot" be out of range is exactly the kind that turns out to be.
        let channel_at = |index: usize| pixel.get(index).copied().unwrap_or(0);
        let (red, green, blue, alpha) = match channels {
            4 => (channel_at(0), channel_at(1), channel_at(2), channel_at(3)),
            3 => (channel_at(0), channel_at(1), channel_at(2), 255),
            2 => (channel_at(0), channel_at(0), channel_at(0), channel_at(1)),
            _ => (channel_at(0), channel_at(0), channel_at(0), 255),
        };
        let Some(slot) = bitmap.pixels.get_mut(at * 4..at * 4 + 4) else {
            break;
        };
        // Straight alpha in the file, premultiplied here.
        // Blue, green, red, alpha -- the order `UpdateLayeredWindow` wants, which is what
        // every other producer in this module writes.
        slot.copy_from_slice(&[
            premultiply(blue, alpha),
            premultiply(green, alpha),
            premultiply(red, alpha),
            alpha,
        ]);
    }
    Some(bitmap)
}

/// Clips a bitmap to the circle inscribed in it.
///
/// `Avatar.tsx` puts the crewmate in a `border-radius: 50%` box with `overflow: hidden`, so
/// the parts of the body that reach past the circle are not drawn. Without this the sprite
/// is a square one, and the ring the view draws around it no longer follows its edge.
///
/// Antialiased over the last pixel of the radius, because a hard cut on a 52-point avatar
/// is a visible staircase. Premultiplied throughout: scaling all four channels by the same
/// coverage is what keeps it so.
pub fn clip_to_circle(bitmap: &mut Bitmap) {
    let (width, height) = (bitmap.width, bitmap.height);
    if width <= 0 || height <= 0 {
        return;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "sprite dimensions in pixels, far below f32's exact integer range"
    )]
    let (centre_x, centre_y) = ((width as f32 - 1.0) / 2.0, (height as f32 - 1.0) / 2.0);
    let radius = centre_x.min(centre_y);

    for y in 0..height {
        for x in 0..width {
            #[expect(
                clippy::cast_precision_loss,
                reason = "pixel coordinates, far below f32's exact integer range"
            )]
            let distance = ((x as f32 - centre_x).powi(2) + (y as f32 - centre_y).powi(2)).sqrt();
            // One pixel of feather at the rim: fully in at `radius - 1`, fully out at
            // `radius`.
            let coverage = (radius - distance).clamp(0.0, 1.0);
            if coverage >= 1.0 {
                continue;
            }
            let Ok(index) = usize::try_from((y * width + x) * 4) else {
                continue;
            };
            let Some(slot) = bitmap.pixels.get_mut(index..index + 4) else {
                continue;
            };
            for channel in slot {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a byte scaled by a value in 0..=1 is a byte"
                )]
                {
                    *channel = (f32::from(*channel) * coverage).round() as u8;
                }
            }
        }
    }
}

/// One channel, scaled by its own alpha.
///
/// Rounded rather than truncated. Truncating biases every channel down by up to one level,
/// which over five composited layers is a visible darkening — and it can push a channel
/// below its alpha's rounding, which is the one thing premultiplied colour must never do.
pub(crate) fn premultiply(channel: u8, alpha: u8) -> u8 {
    let scaled = u32::from(channel) * u32::from(alpha) + 127;
    // The exact rounding of `x * a / 255`, without the division twice.
    u8::try_from((scaled + scaled / 255) / 256).unwrap_or(alpha)
}

impl Bitmap {
    /// Draws another bitmap over this one, scaled into a rectangle.
    ///
    /// Source-over in premultiplied space, which is one multiply per channel:
    /// `dst = src + dst * (1 - src_alpha)`. That is the whole reason the rest of this
    /// module keeps colour premultiplied — the straight-alpha form needs a divide and
    /// gets the halo wrong at every partly transparent edge.
    ///
    /// **The filter is an area average**, not nearest. A hat is 270×428 and lands in a
    /// box of about forty pixels on the overlay; taking one sample out of a fifty-pixel
    /// square gives a different hat depending on which pixel it lands on, which shows up
    /// as sparkle when the sprite moves. Averaging the square costs the same order of
    /// work and is stable. Where the destination is *larger* than the source the square
    /// covers less than one pixel and this degenerates to nearest, which is what an
    /// upscaled sprite has to look like anyway.
    ///
    /// Anything outside this bitmap is clipped rather than wrapped or refused.
    pub fn composite(&mut self, source: &Bitmap, at: (i32, i32), size: (i32, i32)) {
        let (width, height) = size;
        if width <= 0 || height <= 0 || source.width <= 0 || source.height <= 0 {
            return;
        }
        for row in 0..height {
            for column in 0..width {
                let (x, y) = (at.0 + column, at.1 + row);
                if x < 0 || y < 0 || x >= self.width || y >= self.height {
                    continue;
                }
                let sample = source.area((column, row), (width, height));
                self.over(x, y, sample);
            }
        }
    }

    /// The average of the source pixels one destination pixel covers.
    ///
    /// Premultiplied colour is what makes this a plain average: straight alpha would need
    /// the colours weighted by their alphas before averaging, and forgetting that is the
    /// classic dark fringe around a resized sprite.
    fn area(&self, destination: (i32, i32), size: (i32, i32)) -> [u32; 4] {
        let span = |at: i32, out_of: i32, source: i32| -> (i32, i32) {
            let start = at * source / out_of;
            let end = ((at + 1) * source).div_euclid(out_of).max(start + 1);
            (start, end.min(source))
        };
        let (left, right) = span(destination.0, size.0, self.width);
        let (top, bottom) = span(destination.1, size.1, self.height);

        let mut total = [0_u32; 4];
        let mut count = 0_u32;
        for y in top..bottom {
            for x in left..right {
                let Some(pixel) = self.at(x, y) else {
                    continue;
                };
                for (sum, channel) in total.iter_mut().zip(pixel) {
                    *sum += u32::from(channel);
                }
                count += 1;
            }
        }
        if count == 0 {
            return [0; 4];
        }
        for sum in &mut total {
            *sum = (*sum + count / 2) / count;
        }
        total
    }

    /// Source-over, in premultiplied space.
    fn over(&mut self, x: i32, y: i32, source: [u32; 4]) {
        let Ok(at) = usize::try_from((y * self.width + x) * 4) else {
            return;
        };
        let Some(slot) = self.pixels.get_mut(at..at + 4) else {
            return;
        };
        let inverse = 255 - source[3].min(255);
        for (channel, value) in slot.iter_mut().zip(source) {
            let kept = (u32::from(*channel) * inverse + 127) / 255;
            *channel = u8::try_from((value + kept).min(255)).unwrap_or(255);
        }
    }
}

/// A bitmap as egui wants it.
///
/// `Bitmap` holds **premultiplied** RGBA — `decode_png` premultiplies on the way in, because
/// `UpdateLayeredWindow` wants it that way and the overlay was the first consumer. `Color32`
/// is premultiplied too, so this is a copy and not a conversion, and there is no alpha maths
/// here to get wrong.
///
/// It lives beside the bitmap rather than in the client because both views want it: the
/// overlay hands its pixels to Win32 and the main window hands the same pixels to egui.
#[must_use]
pub fn to_image(bitmap: &Bitmap) -> egui::ColorImage {
    let size = [
        usize::try_from(bitmap.width).unwrap_or(0),
        usize::try_from(bitmap.height).unwrap_or(0),
    ];
    let mut pixels = Vec::with_capacity(size[0].saturating_mul(size[1]));
    let (whole, _) = bitmap.pixels.as_chunks::<4>();
    // Blue first. A `Bitmap` is in the order `UpdateLayeredWindow` wants and egui's
    // `Color32` is in the order everything else does, so the two ends are swapped and this
    // is where they meet.
    for [blue, green, red, alpha] in whole {
        pixels.push(egui::Color32::from_rgba_premultiplied(
            *red, *green, *blue, *alpha,
        ));
    }
    // Trusting `width * height * 4` would panic inside egui on a bitmap that disagreed with
    // itself; this cannot, and a short image is visibly wrong rather than fatal.
    pixels.resize(size[0].saturating_mul(size[1]), egui::Color32::TRANSPARENT);
    egui::ColorImage {
        size,
        #[expect(
            clippy::cast_precision_loss,
            reason = "a bitmap's pixel dimensions, which are in the hundreds"
        )]
        source_size: egui::Vec2::new(size[0] as f32, size[1] as f32),
        pixels,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Bitmap, Crewmate, clip_to_circle, crewmate, decode_png};

    /// A red pixel is red on the other side.
    ///
    /// Alpha is left alone — both sides are premultiplied, so a conversion there would show
    /// as a halo round every hat — but the *order* is not the same on both sides, and this
    /// is the only place the two meet.
    ///
    /// **This test used to assert the bug.** It wrote `[10, 20, 30, 40]` and expected
    /// `from_rgba_premultiplied(10, 20, 30, 40)`, which is only true if a `Bitmap`'s first
    /// byte is red — and it is blue, as `blend` and `decode_png` both plainly show. Named
    /// after the property it was checking, "the same pixels", it read as obviously correct.
    /// Live, on 2026-08-27: a red crewmate drawn blue, a blue one drawn orange, and a yellow
    /// hard hat drawn cyan. So the values here are chosen to make a swap fail — three
    /// different channels, none of them a grey.
    #[test]
    fn an_image_keeps_its_colours() {
        let mut bitmap = super::Bitmap::blank(2, 1);
        // Blue, green, red, alpha: an opaque red with a little green in it.
        bitmap.pixels[0..4].copy_from_slice(&[10, 90, 200, 255]);
        bitmap.pixels[4..8].copy_from_slice(&[0, 0, 0, 0]);

        let image = super::to_image(&bitmap);
        assert_eq!(image.size, [2, 1]);
        assert_eq!(
            image.pixels[0],
            egui::Color32::from_rgba_premultiplied(200, 90, 10, 255),
            "red and blue are the two that get swapped"
        );
        assert_eq!(image.pixels[1], egui::Color32::TRANSPARENT);
    }

    /// End to end: the palette's red comes out of the texture path red.
    ///
    /// The unit above tests four bytes. This one runs the real thing — decode the vendored
    /// master, recolour it, hand it to egui — because that is the path every crewmate in the
    /// window takes and it is the one that was wrong.
    #[test]
    fn a_red_crewmate_reaches_the_texture_red() {
        // Colour zero, `#C51111` and its shadow. Red enough that a swap is unmistakable.
        let body = crate::body::recoloured(true, (0xc5, 0x11, 0x11), (0x7a, 0x08, 0x38))
            .expect("the vendored artwork decodes");
        let image = super::to_image(&body);
        // The middle of the chest, which is body colour rather than shadow or visor.
        let pixel = image.pixels[50 * 100 + 50];
        assert!(
            pixel.r() > pixel.b() * 2,
            "red {} should dominate blue {}",
            pixel.r(),
            pixel.b()
        );
    }

    /// The corners go, the middle stays.
    ///
    /// Premultiplied, so a cleared pixel is four zeroes rather than a black one with zero
    /// alpha — a black corner with the alpha alone cleared would still darken whatever it
    /// was composited onto.
    #[test]
    fn clipping_leaves_a_circle() {
        let mut bitmap = Bitmap::blank(32, 32);
        for pixel in bitmap.pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[255, 255, 255, 255]);
        }
        clip_to_circle(&mut bitmap);

        assert_eq!(bitmap.at(16, 16), Some([255, 255, 255, 255]), "the middle");
        for corner in [(0, 0), (31, 0), (0, 31), (31, 31)] {
            assert_eq!(
                bitmap.at(corner.0, corner.1),
                Some([0, 0, 0, 0]),
                "corner {corner:?}"
            );
        }
        // The top middle is inside the circle and survives, though it is within the pixel
        // of feather at the rim, so it is not quite full.
        let [_, _, _, alpha] = bitmap.at(16, 1).expect("in range");
        assert!(alpha > 200, "the top middle came out at alpha {alpha}");
        // And the rim really is feathered rather than cut: somewhere on it a pixel is
        // partly covered. A hard edge on a 52-point avatar is a visible staircase.
        assert!(
            bitmap
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| (1..255).contains(&pixel[3])),
            "no partly covered pixel: the edge is hard"
        );
    }

    /// A bitmap with no pixels is not a panic and not a divide by zero.
    #[test]
    fn clipping_nothing_is_nothing() {
        let mut bitmap = Bitmap::blank(0, 0);
        clip_to_circle(&mut bitmap);
        assert!(bitmap.pixels.is_empty());
    }

    /// A bitmap that disagrees with itself is short, not fatal.
    ///
    /// egui panics on a pixel count that does not match the size it was given, and this is
    /// the one place a mismatch could reach it. Padding makes such an image visibly wrong
    /// instead of taking the window down.
    #[test]
    fn a_short_bitmap_does_not_take_egui_down() {
        let bitmap = super::Bitmap {
            width: 4,
            height: 4,
            pixels: vec![0; 8],
        };
        let image = super::to_image(&bitmap);
        assert_eq!(
            image.pixels.len(),
            16,
            "egui would panic on any other count"
        );
    }

    /// The real artwork, decoded. A hat invented for a test only proves the decoder agrees
    /// with itself; these are two of the 983 files players actually download.
    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test/fixtures/hats")
                .join(name),
        )
        .expect("the vendored artwork")
    }

    #[test]
    fn a_real_hat_decodes_at_its_own_size() {
        let hat = decode_png(&fixture("ratHat.png")).expect("a PNG");
        assert_eq!((hat.width, hat.height), (270, 428));
        assert_eq!(hat.pixels.len(), 270 * 428 * 4);
    }

    /// And it comes out premultiplied, like everything else here. The layered window
    /// requires it and does not check; straight alpha is a bright fringe on every
    /// antialiased edge, which on a hat is its whole outline.
    #[test]
    fn a_decoded_hat_is_premultiplied() {
        for name in ["ratHat.png", "grandpaHat.png"] {
            let hat = decode_png(&fixture(name)).expect("a PNG");
            for (at, pixel) in hat.pixels.as_chunks::<4>().0.iter().enumerate() {
                assert!(
                    pixel[0] <= pixel[3] && pixel[1] <= pixel[3] && pixel[2] <= pixel[3],
                    "{name} pixel {at} is {pixel:?}, which exceeds its alpha"
                );
            }
        }
    }

    /// A hat has transparent corners and opaque middle -- which is what says the alpha
    /// channel survived rather than being filled in with 255.
    #[test]
    fn a_decoded_hat_has_a_transparent_edge_and_an_opaque_middle() {
        let hat = decode_png(&fixture("ratHat.png")).expect("a PNG");
        assert_eq!(hat.at(0, 0).map(|pixel| pixel[3]), Some(0), "the corner");
        let opaque = hat
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] == 255)
            .count();
        assert!(opaque > 1000, "only {opaque} opaque pixels");
    }

    /// Anything that is not a PNG costs one cosmetic layer rather than the avatar.
    #[test]
    fn something_that_is_not_a_png_is_not_a_panic() {
        assert!(decode_png(&[]).is_none());
        assert!(decode_png(b"not a png at all").is_none());
        let mut truncated = fixture("ratHat.png");
        truncated.truncate(64);
        assert!(decode_png(&truncated).is_none());
    }

    /// Compositing an opaque square replaces what is under it, and a fully transparent one
    /// changes nothing. The two ends of the blend.
    #[test]
    fn the_two_ends_of_the_blend_do_what_they_say() {
        let mut canvas = Bitmap::blank(8, 8);
        canvas.circle((4.0, 4.0), 4.0, (255, 0, 0));

        let mut opaque = Bitmap::blank(2, 2);
        for pixel in opaque.pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[0, 255, 0, 255]);
        }
        let before = canvas.at(4, 4);
        canvas.composite(&opaque, (3, 3), (2, 2));
        assert_ne!(canvas.at(4, 4), before, "an opaque square drew nothing");
        assert_eq!(canvas.at(4, 4).map(|pixel| pixel[3]), Some(255));

        let clear = Bitmap::blank(2, 2);
        let unchanged = canvas.clone();
        canvas.composite(&clear, (3, 3), (2, 2));
        assert_eq!(
            canvas.pixels, unchanged.pixels,
            "a clear square drew something"
        );
    }

    /// The result is still premultiplied, which is the invariant every later blit depends
    /// on -- and the one a blend written in straight alpha quietly breaks.
    #[test]
    fn compositing_keeps_the_result_premultiplied() {
        let mut canvas = crewmate(
            64,
            Crewmate {
                body: (0xC5, 0x11, 0x11),
                shadow: (0x7A, 0x08, 0x38),
                talking: true,
                alive: true,
            },
        );
        let hat = decode_png(&fixture("ratHat.png")).expect("a PNG");
        canvas.composite(&hat, (8, -10), (48, 40));
        for (at, pixel) in canvas.pixels.as_chunks::<4>().0.iter().enumerate() {
            assert!(
                pixel[0] <= pixel[3] && pixel[1] <= pixel[3] && pixel[2] <= pixel[3],
                "pixel {at} is {pixel:?}, which exceeds its alpha"
            );
        }
    }

    /// A hat drawn partly off the edge is clipped, not wrapped and not refused. Every hat
    /// is drawn partly off: the collection's default top is -78%.
    ///
    /// An opaque square rather than the real artwork, because the artwork's own margins
    /// are transparent -- see `the_artwork_has_transparent_margins` -- and a test that
    /// depended on which part of a hat happens to have ink in it would be testing the
    /// hat.
    #[test]
    fn a_sprite_hanging_off_the_edge_is_clipped() {
        let mut opaque = Bitmap::blank(8, 8);
        for pixel in opaque.pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[255, 255, 255, 255]);
        }
        let mut canvas = Bitmap::blank(32, 32);
        canvas.composite(&opaque, (-20, -20), (40, 40));
        canvas.composite(&opaque, (28, 28), (40, 40));
        assert_eq!(canvas.pixels.len(), 32 * 32 * 4, "the canvas was resized");
        assert_eq!(canvas.at(0, 0).map(|pixel| pixel[3]), Some(255), "top left");
        assert_eq!(
            canvas.at(31, 31).map(|pixel| pixel[3]),
            Some(255),
            "bottom right"
        );

        // And the real artwork over the same edges, which must also not panic.
        let hat = decode_png(&fixture("ratHat.png")).expect("a PNG");
        canvas.composite(&hat, (-260, -400), (270, 428));
        canvas.composite(&hat, (30, 30), (270, 428));
        assert_eq!(canvas.pixels.len(), 32 * 32 * 4);
    }

    /// The artwork is a fixed canvas with the hat somewhere inside it, and the margins are
    /// empty: `ratHat.png` is 270x428 with nothing at all in its top quarter or its bottom
    /// quarter.
    ///
    /// That is why the collection's defaults are what they are -- `top: -78%`, `width:
    /// 130%` -- and it is why nothing here may crop artwork to its content as an
    /// optimisation. The empty margin is load-bearing: it is what the percentages are
    /// measured against, so trimming it moves every hat.
    #[test]
    fn the_artwork_has_transparent_margins() {
        let hat = decode_png(&fixture("ratHat.png")).expect("a PNG");
        let ink_in = |from: i32, to: i32| {
            (from..to)
                .flat_map(|y| (0..hat.width).map(move |x| (x, y)))
                .filter(|(x, y)| hat.at(*x, *y).is_some_and(|pixel| pixel[3] > 0))
                .count()
        };
        assert_eq!(ink_in(0, hat.height / 4), 0, "the top quarter is empty");
        assert_eq!(
            ink_in(hat.height * 3 / 4, hat.height),
            0,
            "the bottom quarter is empty"
        );
        assert!(
            ink_in(hat.height / 4, hat.height * 3 / 4) > 1000,
            "and the middle is not"
        );
    }

    /// Scaling averages rather than sampling. A hat is 270 pixels wide and lands in about
    /// forty; one sample out of a fifty-pixel square gives a different hat depending on
    /// which pixel it hits, which shows up as sparkle when the sprite moves.
    #[test]
    fn downscaling_averages_instead_of_sampling() {
        // A source that is half opaque white and half transparent, in alternating columns.
        // Any single sample gives 0 or 255; the average is in between.
        let mut striped = Bitmap::blank(16, 1);
        for (at, pixel) in striped.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if at % 2 == 0 {
                pixel.copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let mut canvas = Bitmap::blank(2, 1);
        canvas.composite(&striped, (0, 0), (2, 1));
        let alpha = canvas.at(0, 0).map(|pixel| pixel[3]).expect("a pixel");
        assert!(
            (100..=155).contains(&alpha),
            "expected roughly half, got {alpha}"
        );
    }

    /// Every destination pixel takes at least one source pixel, so an upscale draws
    /// something everywhere rather than leaving gaps between samples.
    #[test]
    fn upscaling_leaves_no_gaps() {
        let mut source = Bitmap::blank(2, 2);
        for pixel in source.pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[255, 255, 255, 255]);
        }
        let mut canvas = Bitmap::blank(9, 9);
        canvas.composite(&source, (0, 0), (9, 9));
        for y in 0..9 {
            for x in 0..9 {
                assert_eq!(
                    canvas.at(x, y).map(|pixel| pixel[3]),
                    Some(255),
                    "gap at {x},{y}"
                );
            }
        }
    }

    /// A zero-sized or empty source draws nothing instead of dividing by zero.
    #[test]
    fn nothing_sized_draws_nothing() {
        let hat = decode_png(&fixture("ratHat.png")).expect("a PNG");
        let mut canvas = Bitmap::blank(8, 8);
        let before = canvas.clone();
        canvas.composite(&hat, (0, 0), (0, 8));
        canvas.composite(&hat, (0, 0), (8, -1));
        canvas.composite(&Bitmap::blank(0, 0), (0, 0), (8, 8));
        assert_eq!(canvas.pixels, before.pixels);
    }

    const RED: (u8, u8, u8) = (0xC5, 0x11, 0x11);
    const DARK: (u8, u8, u8) = (0x7A, 0x08, 0x38);

    fn alive() -> Crewmate {
        Crewmate {
            body: RED,
            shadow: DARK,
            talking: false,
            alive: true,
        }
    }

    /// The format the pipe and `UpdateLayeredWindow` both require. A short buffer is not a
    /// wrong picture but a read past the end of an allocation on the far side.
    #[test]
    fn the_buffer_is_exactly_the_size_the_dimensions_claim() {
        let sprite = crewmate(48, alive());
        assert_eq!(sprite.width, 48);
        assert_eq!(sprite.height, 48);
        assert_eq!(sprite.pixels.len(), 48 * 48 * 4);
    }

    /// The corners are outside the crewmate, and the overlay is a window over a game — a
    /// sprite with an opaque background would be a square of colour sitting on the map.
    #[test]
    fn the_corners_are_fully_transparent() {
        let sprite = crewmate(48, alive());
        for (x, y) in [(0, 0), (47, 0), (0, 47), (47, 47)] {
            assert_eq!(
                sprite.at(x, y),
                Some([0, 0, 0, 0]),
                "the corner at {x},{y} is not transparent"
            );
        }
    }

    /// Premultiplied means no channel may exceed the alpha. `UpdateLayeredWindow` does not
    /// check, and the symptom of getting it wrong is a bright fringe on every edge.
    #[test]
    fn no_channel_ever_exceeds_its_alpha() {
        let sprite = crewmate(
            64,
            Crewmate {
                talking: true,
                ..alive()
            },
        );
        for y in 0..64 {
            for x in 0..64 {
                let [b, g, r, a] = sprite.at(x, y).expect("inside the bitmap");
                assert!(
                    b <= a && g <= a && r <= a,
                    "pixel {x},{y} is not premultiplied: {b},{g},{r} over alpha {a}"
                );
            }
        }
    }

    /// The middle of a crewmate is the body colour at full opacity, which is what says the
    /// shape was drawn at all rather than merely allocated.
    #[test]
    fn the_middle_is_the_body_colour() {
        let sprite = crewmate(64, alive());
        let [b, g, r, a] = sprite.at(30, 34).expect("inside the bitmap");
        assert_eq!(a, 255, "the middle should be opaque");
        assert_eq!((r, g, b), RED, "expected the body colour");
    }

    /// A dead player is drawn faint and still drawn. An empty sprite would read as somebody
    /// leaving, which is a different thing.
    #[test]
    fn a_dead_crewmate_is_faint_and_not_absent() {
        let dead = crewmate(
            64,
            Crewmate {
                alive: false,
                ..alive()
            },
        );
        let [_, _, r, a] = dead.at(30, 34).expect("inside the bitmap");
        assert_eq!(a, 255, "still opaque, so the shape is still there");
        assert!(r < RED.0, "expected a dimmed body, got {r}");
    }

    /// The sprite is the same size whether or not somebody is speaking, so the overlay does
    /// not jump every time they start.
    #[test]
    fn speaking_does_not_change_the_size() {
        let quiet = crewmate(64, alive());
        let loud = crewmate(
            64,
            Crewmate {
                talking: true,
                ..alive()
            },
        );
        assert_eq!((quiet.width, quiet.height), (loud.width, loud.height));
        assert_ne!(quiet.pixels, loud.pixels, "and the ring is actually drawn");
    }

    /// Drawing outside the bitmap is clipped rather than wrapped or panicking. Every
    /// coordinate here comes from arithmetic on a size, and one of them will be off the
    /// edge eventually.
    #[test]
    fn drawing_off_the_edge_is_clipped() {
        let mut bitmap = Bitmap::blank(4, 4);
        bitmap.circle((-10.0, -10.0), 3.0, RED);
        assert!(bitmap.pixels.iter().all(|byte| *byte == 0));
        bitmap.circle((100.0, 100.0), 3.0, RED);
        assert!(bitmap.pixels.iter().all(|byte| *byte == 0));
    }

    /// A degenerate size is a bitmap with nothing in it rather than a panic. It is what a
    /// window of no size produces, which is what a minimised game looks like.
    #[test]
    fn a_sprite_of_no_size_is_empty() {
        for size in [0, -1] {
            let sprite = crewmate(size, alive());
            assert!(sprite.pixels.is_empty());
        }
    }
}
