//! The program's own face, for the window and the taskbar.
//!
//! Windows finds a taskbar button's icon in three places, in order: the icon the window was
//! given, the one its window class carries, and failing both, the icon compiled into the
//! executable. This client had none of the three, so what people saw was the blank sheet
//! Windows falls back to — a client that looks like something that failed to install, next
//! to a 1.x client with a proper icon.
//!
//! This is the first of the three. The third is a resource compiled into the binary, which
//! is what Explorer, a pinned shortcut and Add/Remove Programs read instead; the installer
//! already points `DisplayIcon` at the executable, so that one is worth having too and is a
//! build-time job rather than a run-time one.

/// The design system's mark, rendered by `scripts/rasterise-icon`.
///
/// **Not** `resources/icon.png`, which this used until 2026-08-28 and which is
/// the artwork of `BetterCrewLink` — inherited through the fork, still named `BCL-*` where
/// it lives. Putting it on this window meant the rewrite introduced itself under the logo
/// of the project it forked from.
///
/// `assets/icon.svg` is the source and is `design/assets/logos/acl-client-outlined.svg`
/// byte for byte: the ring mark on its own plate, with the letters already paths so no font
/// has to be present to draw them. Reached out of the crate rather than copied in, because
/// the executables' icon comes from the same render and the two must not be able to differ.
const ARTWORK: &[u8] = include_bytes!("../../../assets/icon.png");

/// The icon to give the window, or `None` if it could not be read.
///
/// `None` rather than a panic: a client that starts without its icon is a client, and a
/// client that refuses to start because of one is not. The caller leaves the icon unset and
/// Windows falls back the way it did before.
#[must_use]
pub fn window() -> Option<egui::IconData> {
    decode(ARTWORK)
}

/// Straight RGBA8, which is the one shape [`egui::IconData`] takes.
fn decode(bytes: &[u8]) -> Option<egui::IconData> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // The artwork is sixteen bits a channel and this asks for eight, because the sixteen
    // are not carried anywhere: the field below is bytes. Without it the reader hands back
    // twice the data and every second byte is a low half nobody wants.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut raw = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut raw).ok()?;
    raw.truncate(info.buffer_size());

    let rgba = match info.color_type {
        png::ColorType::Rgba => raw,
        // Opaque, which is what a source without alpha means.
        png::ColorType::Rgb => raw
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[red, green, blue]| [red, green, blue, 0xff])
            .collect(),
        _ => return None,
    };

    // Four bytes a pixel, and the icon is square in every size Windows asks for. A mismatch
    // here would be a decoder that disagreed with its own header, which is worth refusing
    // rather than handing on: the viewport takes the slice on trust.
    if rgba.len() != (info.width as usize) * (info.height as usize) * 4 {
        return None;
    }

    Some(egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{ARTWORK, window};

    #[test]
    fn the_shipped_artwork_decodes_to_an_icon() {
        // The whole failure this fixes is silent -- an icon that does not load is an icon
        // Windows draws its fallback for, and nobody reports a blank square as an error.
        // So the assertion is that it loaded, not that nothing threw.
        let icon = window().expect("the shipped icon must decode");
        assert_eq!(icon.width, 256, "the artwork is 256 square");
        assert_eq!(icon.height, 256);
        assert_eq!(
            icon.rgba.len(),
            256 * 256 * 4,
            "straight RGBA8, four a pixel"
        );
    }

    #[test]
    fn it_is_not_a_transparent_square() {
        // A 256-square block of zeroes satisfies every assertion above and draws nothing at
        // all, which is the same blank taskbar button this set out to fix. The mark sits on
        // a filled plate, so most of the square is opaque.
        let icon = window().expect("the shipped icon must decode");
        let opaque = icon
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] > 0x80)
            .count();
        assert!(
            opaque > 256 * 256 / 2,
            "only {opaque} pixels of the icon are opaque; it would draw as nothing"
        );
    }

    #[test]
    fn artwork_that_is_not_a_png_gives_no_icon_rather_than_a_panic() {
        assert!(super::decode(b"this is not a png").is_none());
        assert!(super::decode(&[]).is_none());
        // Truncated part-way through, which is the shape a half-written file has.
        assert!(super::decode(&ARTWORK[..64]).is_none());
    }
}
