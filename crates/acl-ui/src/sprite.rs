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
    fn blend(&mut self, x: i32, y: i32, colour: (u8, u8, u8), coverage: f32) {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Bitmap, Crewmate, crewmate};

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
