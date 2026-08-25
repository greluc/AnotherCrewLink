//! The twelve crew colours, and the one that is not a colour.
//!
//! A port of `src/common/playerColors.ts`, which §4.3 item 4 asks for: `acl-types` ports
//! `src/common` wholesale. The table is shared by the avatar, the overlay and the
//! recoloured sprites the helper sends over the IPC, so it belongs somewhere all three
//! can reach without any of them depending on the others.
//!
//! Each entry is a pair: the body colour, then the shadow. Both are needed to draw a
//! crewmate; using the body colour for the shadow gives a flat sticker, and it is the
//! difference anybody notices immediately and nobody can name.

/// The twelve colours the game assigns, body and shadow, in the game's own order.
///
/// **The order is the identity.** A player's colour arrives from the game as an index
/// into this table, so reordering it does not recolour one crewmate — it renames every
/// player's colour at once, and the overlay, the avatars and the sprites all disagree
/// with the game and with each other.
pub const DEFAULT_PLAYER_COLORS: [(&str, &str); 12] = [
    ("#C51111", "#7A0838"),
    ("#132ED1", "#09158E"),
    ("#117F2D", "#0A4D2E"),
    ("#ED54BA", "#AB2BAD"),
    ("#EF7D0D", "#B33E15"),
    ("#F5F557", "#C38823"),
    ("#3F474E", "#1E1F26"),
    ("#FFFFFF", "#8394BF"),
    ("#6B2FBB", "#3B177C"),
    ("#71491E", "#5E2615"),
    ("#38FEDC", "#24A8BE"),
    ("#50EF39", "#15A742"),
];

/// The colour id that means "the rainbow cosmetic", which is not an index at all.
///
/// A sentinel, and deliberately far outside the range so it cannot collide with a real
/// index or with a colour a future game version adds. `GameReader` substitutes it when
/// the read colour matches the rainbow value, and everything downstream has to recognise
/// it rather than index the table with it.
pub const RAINBOW_COLOR_ID: i32 = -99234;

/// The body and shadow for a colour id.
///
/// `None` for the rainbow sentinel and for anything out of range. Out of range is not
/// impossible: the colour is read out of another process's memory, and a game update that
/// adds a colour produces one before this table knows about it. Returning `None` gives
/// the caller a chance to draw something rather than panic on an index.
#[must_use]
pub fn colors_for(color_id: i32) -> Option<(&'static str, &'static str)> {
    if color_id == RAINBOW_COLOR_ID {
        return None;
    }
    usize::try_from(color_id)
        .ok()
        .and_then(|index| DEFAULT_PLAYER_COLORS.get(index).copied())
}

/// Whether this id is the rainbow cosmetic rather than an index.
#[must_use]
pub const fn is_rainbow(color_id: i32) -> bool {
    color_id == RAINBOW_COLOR_ID
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn the_table_matches_the_electron_client_entry_for_entry() {
        // The order is the identity: a player's colour is an index into this table, so a
        // reordering renames every player's colour at once rather than changing one.
        // Compared against the source rather than trusted, because a transcription error
        // here is invisible until somebody looks at two clients side by side.
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../src/common/playerColors.ts"),
        )
        .expect("the Electron client is beside the crates");

        let mut theirs = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim();
            let Some(inner) = trimmed
                .strip_prefix("['")
                .and_then(|l| l.strip_suffix("'],"))
            else {
                continue;
            };
            let Some((body, shadow)) = inner.split_once("', '") else {
                continue;
            };
            theirs.push((body.to_owned(), shadow.to_owned()));
        }

        assert_eq!(
            theirs.len(),
            DEFAULT_PLAYER_COLORS.len(),
            "read {} pairs out of playerColors.ts",
            theirs.len()
        );
        for (index, (body, shadow)) in theirs.iter().enumerate() {
            assert_eq!(
                (body.as_str(), shadow.as_str()),
                DEFAULT_PLAYER_COLORS[index],
                "colour {index}"
            );
        }
    }

    #[test]
    fn the_rainbow_sentinel_matches_the_electron_client() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../src/common/playerColors.ts"),
        )
        .expect("the Electron client is beside the crates");
        assert!(
            source.contains(&format!("RainbowColorId = {RAINBOW_COLOR_ID}")),
            "the rainbow sentinel has moved"
        );
    }

    #[test]
    fn the_rainbow_sentinel_is_not_an_index() {
        // Far outside the range on purpose, so it cannot collide with a real colour or
        // with one a future game version adds.
        assert!(is_rainbow(RAINBOW_COLOR_ID));
        assert_eq!(colors_for(RAINBOW_COLOR_ID), None);
        assert!(!is_rainbow(0));
        assert!(!is_rainbow(11));
    }

    #[test]
    fn every_real_colour_resolves() {
        for index in 0..12 {
            assert!(colors_for(index).is_some(), "colour {index}");
        }
    }

    #[test]
    fn a_colour_the_table_does_not_have_is_none_rather_than_a_panic() {
        // The colour is read out of another process's memory, and a game update that adds
        // one produces it before this table knows about it. Indexing on that is a crash
        // in a voice client that is meant to survive a bad frame.
        assert_eq!(colors_for(12), None);
        assert_eq!(colors_for(-1), None);
        assert_eq!(colors_for(i32::MIN), None);
        assert_eq!(colors_for(i32::MAX), None);
    }

    #[test]
    fn a_body_and_its_shadow_are_never_the_same() {
        // Using the body colour for the shadow gives a flat sticker -- the difference
        // anybody notices immediately and nobody can name.
        for (index, (body, shadow)) in DEFAULT_PLAYER_COLORS.iter().enumerate() {
            assert_ne!(body, shadow, "colour {index}");
        }
    }
}
