//! The crewmate body, recoloured the way the shipped client recolours it.
//!
//! There is one drawing of a crewmate and one of a ghost, and every colour in the game is
//! that drawing with three channels reinterpreted: red says how much of the player's colour
//! a pixel gets, blue how much of their shadow, and green how much of the visor's fixed
//! blue-grey. A pixel that is none of those keeps whatever it already was, which is how the
//! outline and the shading survive being tinted.
//!
//! Ported from `src/main/avatarGenerator.ts`, which does this once per colour at first run
//! and writes the results into the user's profile. This does it in memory instead: the files
//! were never anything but a cache, and a client that has just been installed should not
//! have to do disc work before it can draw a player.
//!
//! # Why not draw one
//!
//! Because the drawing is the artwork. [`crate::sprite::crewmate`] paints two circles and a
//! visor, which reads as a crewmate at sixteen points and as a smiley at fifty-two — and
//! next to the shipped client, which is what people compare it against, it reads as broken.

use crate::sprite::Bitmap;

/// The crewmate.
const PLAYER: &[u8] = include_bytes!("../assets/player.png");
/// The ghost, which is the same drawing with a tail instead of legs.
const GHOST: &[u8] = include_bytes!("../assets/ghost.png");

/// The visor's colour, which is not the player's and does not vary.
///
/// A literal in `avatarGenerator.ts` and a literal here. It is the one part of a crewmate
/// that is the same on all of them.
const VISOR: (u8, u8, u8) = (0x9a, 0xca, 0xd5);

/// How saturated a pixel must be before it is treated as tintable.
///
/// Below this it is outline or shading, and tinting it turns the black outline into a dark
/// version of the player's colour.
const SATURATION: f64 = 0.4;

/// A decoded master, in straight alpha.
///
/// Straight, not premultiplied, and it never leaves this module: the hue and saturation the
/// rule tests are of the colour a pixel *is*, and a premultiplied pixel at half alpha has
/// half of it. [`recoloured`] premultiplies on the way out, so what callers get is an
/// ordinary [`Bitmap`] like every other one.
struct Master {
    width: i32,
    height: i32,
    /// `width * height * 4`, red, green, blue, alpha.
    pixels: Vec<u8>,
}

/// Decodes one of the two masters, once.
fn master(alive: bool) -> Option<&'static Master> {
    static PLAYER_ONCE: std::sync::OnceLock<Option<Master>> = std::sync::OnceLock::new();
    static GHOST_ONCE: std::sync::OnceLock<Option<Master>> = std::sync::OnceLock::new();
    let cell = if alive { &PLAYER_ONCE } else { &GHOST_ONCE };
    cell.get_or_init(|| decode(if alive { PLAYER } else { GHOST }))
        .as_ref()
}

/// Straight RGBA out of a PNG.
fn decode(bytes: &[u8]) -> Option<Master> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut raw = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut raw).ok()?;
    let width = i32::try_from(info.width).ok()?;
    let height = i32::try_from(info.height).ok()?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        _ => return None,
    };

    let count = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    let mut pixels = vec![0u8; count.checked_mul(4)?];
    for (at, pixel) in raw.chunks_exact(channels).enumerate() {
        let channel_at = |index: usize| pixel.get(index).copied().unwrap_or(0);
        let alpha = if channels == 4 { channel_at(3) } else { 255 };
        let Some(slot) = pixels.get_mut(at * 4..at * 4 + 4) else {
            break;
        };
        slot.copy_from_slice(&[channel_at(0), channel_at(1), channel_at(2), alpha]);
    }
    Some(Master {
        width,
        height,
        pixels,
    })
}

/// Hue in degrees and saturation, from bytes.
///
/// Value stays on the byte scale rather than being normalised, because the rule only ever
/// compares saturation — and saturation is chroma over value, which the scale cancels out
/// of. Transliterated from `avatarGenerator.ts` including its zero cases: no chroma is hue
/// zero, and no value is saturation zero.
#[expect(
    clippy::float_cmp,
    reason = "`value` is one of the three by construction -- it is their maximum -- so this               asks which one it is, not whether two computed floats happen to agree"
)]
fn hsv(red: u8, green: u8, blue: u8) -> (f64, f64) {
    let (red, green, blue) = (f64::from(red), f64::from(green), f64::from(blue));
    let value = red.max(green).max(blue);
    let chroma = value - red.min(green).min(blue);
    let hue = if chroma == 0.0 {
        0.0
    } else if value == red {
        (green - blue) / chroma
    } else if value == green {
        2.0 + (blue - red) / chroma
    } else {
        4.0 + (red - green) / chroma
    };
    let hue = 60.0 * if hue < 0.0 { hue + 6.0 } else { hue };
    let saturation = if value == 0.0 { 0.0 } else { chroma / value };
    (hue, saturation)
}

/// Whether two hues are within `spread` degrees of each other, the short way round.
///
/// The short way matters: red is at zero and at three hundred and sixty, and a plain
/// subtraction puts half of it three hundred and sixty degrees from itself.
fn near(hue: f64, centre: f64, spread: f64) -> bool {
    180.0 - ((hue - centre).abs() - 180.0).abs() < spread
}

/// One channel of a linear blend.
fn mix(from: f64, to: f64, weight: f64) -> f64 {
    weight.mul_add(to - from, from)
}

/// The body in a player's colours, premultiplied and ready to composite.
///
/// `None` only if the vendored artwork does not decode, which would be a broken build
/// rather than anything a running client can do.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "every mix is between two channel values, so the result is within 0..=255"
)]
pub fn recoloured(alive: bool, body: (u8, u8, u8), shadow: (u8, u8, u8)) -> Option<Bitmap> {
    let master = master(alive)?;
    let mut out = Bitmap::blank(master.width, master.height);
    let body = (f64::from(body.0), f64::from(body.1), f64::from(body.2));
    let shadow = (
        f64::from(shadow.0),
        f64::from(shadow.1),
        f64::from(shadow.2),
    );

    for (at, pixel) in master.pixels.as_chunks::<4>().0.iter().enumerate() {
        let channel_at = |index: usize| pixel.get(index).copied().unwrap_or(0);
        let (red, green, blue, alpha) =
            (channel_at(0), channel_at(1), channel_at(2), channel_at(3));
        let (hue, saturation) = hsv(red, green, blue);
        // Blue, red and green, with those spreads. The red window is a hundred degrees wide
        // because the artwork's reds run well into orange.
        let tintable = saturation > SATURATION
            && (near(hue, 240.0, 30.0) || near(hue, 0.0, 100.0) || near(hue, 120.0, 40.0));

        let (red, green, blue) = if tintable {
            // Black, then the shadow by blue, then the body by red, then the visor by
            // green. That order is `avatarGenerator.ts`'s and it does not commute: each mix
            // is against what the one before it produced.
            let weights = (
                f64::from(blue) / 255.0,
                f64::from(red) / 255.0,
                f64::from(green) / 255.0,
            );
            let mixed = |shadow_channel: f64, body_channel: f64, visor_channel: f64| {
                let value = mix(0.0, shadow_channel, weights.0);
                let value = mix(value, body_channel, weights.1);
                mix(value, visor_channel, weights.2).round() as u8
            };
            (
                mixed(shadow.0, body.0, f64::from(VISOR.0)),
                mixed(shadow.1, body.1, f64::from(VISOR.1)),
                mixed(shadow.2, body.2, f64::from(VISOR.2)),
            )
        } else {
            (red, green, blue)
        };

        let Some(slot) = out.pixels.get_mut(at * 4..at * 4 + 4) else {
            break;
        };
        // Blue, green, red, alpha, premultiplied — the layout every other producer in
        // `sprite` writes and the one `UpdateLayeredWindow` wants.
        slot.copy_from_slice(&[
            crate::sprite::premultiply(blue, alpha),
            crate::sprite::premultiply(green, alpha),
            crate::sprite::premultiply(red, alpha),
            alpha,
        ]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{master, near, recoloured};

    /// Colour eight, the purple in the screenshots this was fixed from.
    const BODY: (u8, u8, u8) = (0x6b, 0x2f, 0xbb);
    /// Its shadow.
    const SHADOW: (u8, u8, u8) = (0x3b, 0x17, 0x7c);

    #[test]
    fn both_masters_decode_at_the_size_the_stylesheet_assumes() {
        for alive in [true, false] {
            let master = master(alive).expect("the vendored artwork decodes");
            assert_eq!((master.width, master.height), (100, 100));
            assert_eq!(master.pixels.len(), 100 * 100 * 4);
        }
    }

    /// Against an independent implementation of the same rule.
    ///
    /// The expected values come from a transliteration of `avatarGenerator.ts` run over the
    /// same file — not from this code — because a channel mix is exactly the kind of thing
    /// that survives a transposition unnoticed: swap red and blue and every crewmate is
    /// still plausibly coloured, just wrong.
    ///
    /// The probes are fully opaque pixels, so premultiplying does not move them; the one
    /// transparent probe checks that nothing is invented outside the drawing.
    #[test]
    fn the_recolour_agrees_with_the_shipped_one() {
        let out = recoloured(true, BODY, SHADOW).expect("recolours");
        for (x, y, expected) in [
            // Mostly red in the master, so mostly the player's colour out.
            (50, 50, [110_u8, 57, 189, 255]),
            (40, 45, [110, 57, 189, 255]),
            // Green in the master, so the visor's blue-grey mixed over black.
            (50, 20, [23, 30, 32, 255]),
            (50, 35, [76, 100, 105, 255]),
            // Blue in the master, so the shadow.
            (30, 60, [10, 4, 21, 255]),
            (70, 75, [31, 12, 66, 255]),
            // Outside the drawing.
            (5, 5, [0, 0, 0, 0]),
        ] {
            let [blue, green, red, alpha] = out.at(x, y).expect("in range");
            assert_eq!(
                [red, green, blue, alpha],
                expected,
                "pixel ({x}, {y}) as red, green, blue, alpha"
            );
        }
    }

    /// Two colours must actually differ, or the cache key is the only thing that changed.
    #[test]
    fn a_different_colour_is_a_different_body() {
        let purple = recoloured(true, BODY, SHADOW).expect("recolours");
        let red = recoloured(true, (0xc5, 0x11, 0x11), (0x7a, 0x08, 0x38)).expect("recolours");
        assert_ne!(purple.at(50, 50), red.at(50, 50));
        // And the transparent corner is transparent in both.
        assert_eq!(purple.at(5, 5), red.at(5, 5));
    }

    /// The ghost is a different drawing, not a recolour of the same one.
    #[test]
    fn the_ghost_is_not_the_crewmate() {
        let alive = recoloured(true, BODY, SHADOW).expect("recolours");
        let dead = recoloured(false, BODY, SHADOW).expect("recolours");
        assert_ne!(alive.pixels, dead.pixels);
    }

    /// Hue wraps, and red is the case that proves it.
    #[test]
    fn the_hue_window_goes_the_short_way_round() {
        assert!(near(350.0, 0.0, 100.0), "350 degrees is ten from red");
        assert!(near(10.0, 0.0, 100.0));
        assert!(!near(180.0, 0.0, 100.0), "cyan is not red");
    }
}
