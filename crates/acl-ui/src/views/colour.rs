//! Turning the palette's hex strings into something that can be drawn with.
//!
//! [`acl_types::player_colors`] holds the twelve crew colours as `"#C51111"`, because that
//! is what `src/common/playerColors.ts` holds and that crate must stay free of any GUI
//! dependency — the gates depend on it building without one.
//!
//! So the conversion lives here, on the drawing side, and it is a parse rather than a cast.

use egui::Color32;

/// Reads a `#rrggbb` string.
///
/// Only that shape. The palette is a fixed table in this repository, not user input, so
/// there is nothing to be permissive for — and a `#rgb` shorthand or a trailing alpha that
/// silently half-worked would be a colour that is *nearly* right, which is the hardest kind
/// of wrong to see.
///
/// # Examples
///
/// ```
/// use acl_ui::views::colour::parse_hex;
/// assert_eq!(parse_hex("#C51111").map(|c| (c.r(), c.g(), c.b())), Some((0xC5, 0x11, 0x11)));
/// assert_eq!(parse_hex("#fff"), None);
/// ```
#[must_use]
pub fn parse_hex(text: &str) -> Option<Color32> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
}

/// The body and shadow colours for a crew colour index.
///
/// Falls back to a neutral grey pair rather than to nothing. An index the palette does not
/// have comes from a mod, or from a reader that misread a byte; drawing that player grey
/// says "something is odd about this one" and drawing nothing at all says they left.
#[must_use]
pub fn crew(color_id: i32) -> (Color32, Color32) {
    const UNKNOWN: (Color32, Color32) = (
        Color32::from_rgb(0x8A, 0x8A, 0x8A),
        Color32::from_rgb(0x4A, 0x4A, 0x4A),
    );
    let Some((body, shadow)) = acl_types::player_colors::colors_for(color_id) else {
        return UNKNOWN;
    };
    match (parse_hex(body), parse_hex(shadow)) {
        (Some(body), Some(shadow)) => (body, shadow),
        _ => UNKNOWN,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{crew, parse_hex};

    #[test]
    fn a_six_digit_colour_reads() {
        let colour = parse_hex("#C51111").expect("the first crew colour");
        assert_eq!((colour.r(), colour.g(), colour.b()), (0xC5, 0x11, 0x11));
        assert_eq!(
            colour.a(),
            255,
            "opaque, since the palette carries no alpha"
        );
    }

    /// Anything else is refused. A shorthand or a trailing alpha that half-worked would be
    /// a colour that is nearly right, which is the hardest kind of wrong to see.
    #[test]
    fn anything_else_is_refused() {
        for text in ["#fff", "C51111", "#C5111", "#C511111", "", "#GGGGGG", "#"] {
            assert_eq!(parse_hex(text), None, "{text:?} should not parse");
        }
    }

    /// Every colour in the shipped table parses, which is the only thing that makes the
    /// fallback below a genuine fallback rather than the common case.
    #[test]
    fn the_whole_shipped_palette_parses() {
        for (index, (body, shadow)) in acl_types::player_colors::DEFAULT_PLAYER_COLORS
            .iter()
            .enumerate()
        {
            assert!(parse_hex(body).is_some(), "colour {index} body: {body}");
            assert!(
                parse_hex(shadow).is_some(),
                "colour {index} shadow: {shadow}"
            );
        }
    }

    /// An index the palette does not have is drawn grey rather than not drawn. It comes
    /// from a mod or from a reader that misread a byte, and "something is odd about this
    /// one" is a truer thing to show than an empty space, which reads as somebody leaving.
    #[test]
    fn an_unknown_colour_is_grey_rather_than_absent() {
        let (body, shadow) = crew(9_999);
        assert_ne!(body, shadow, "still a pair, so the crewmate has depth");
        assert_eq!(body.a(), 255);
    }

    #[test]
    fn a_known_colour_is_the_palettes() {
        let (body, _) = crew(0);
        assert_eq!(
            Some(body),
            parse_hex(acl_types::player_colors::DEFAULT_PLAYER_COLORS[0].0)
        );
    }
}
