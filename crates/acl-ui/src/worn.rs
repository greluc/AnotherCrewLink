//! What a player is wearing, in the order it has to be drawn.
//!
//! §4.8 item 2. The overlay has dressed crewmates since 2026-08-26, and it dressed them in
//! two of the four layers: the hat's front and the visor. The skin was dropped on the way —
//! `AmongUsState` carries `skin_id` and the overlay's `wearing` list only had room for two
//! strings — and `Layer::HatBack` existed, unused, since the day it was written.
//!
//! # Why this is its own module and not a method on the loader
//!
//! Because the answer does not depend on having fetched anything. Which layers a player
//! wears, where each goes and in what order is a question about a `Collection` and three
//! ids; the network turns those into pixels afterwards. Splitting it that way is what lets
//! the ordering be tested without a fetch, a GPU or a game — and the ordering is the part
//! that is silently wrong, because a hat drawn behind the body when it should be in front
//! looks like a hat that failed to load.
//!
//! # The back of a hat
//!
//! Some hats have a second image that belongs *behind* the crewmate — the brim of a
//! sombrero, the tail of a bandana. The collection has carried them all along
//! (`Hat::back_image`, and `image_url(.., true)` asks for one), and drawing them means the
//! body can no longer be the canvas everything else is pasted onto: something has to go
//! under it. So this returns every layer including [`Layer::Base`], and the caller composites
//! them in order onto a blank bitmap rather than onto the body.

use crate::cosmetics::{Geometry, Layer};
use crate::hats::Collection;

/// One layer of a dressed crewmate.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// Which layer, which is also where it goes in the order.
    pub layer: Layer,
    /// Where to fetch it, or `None` for [`Layer::Base`] — the body is the client's own and
    /// is not a file.
    pub url: Option<String>,
    /// Where to draw it, as fractions of the sprite's size.
    pub geometry: Geometry,
}

/// The layers a player wears, bottom to top.
///
/// Always contains [`Layer::Base`]; the rest depend on what they have on and on what the
/// collection knows about it. A cosmetic the collection has never heard of costs that
/// cosmetic and nothing else — the same rule `Collection::parse` follows, and the reason
/// the Electron client shows a bare crewmate rather than an error when a fetch fails.
///
/// `mods` is the modded set to look in after the base one, which is `hats::find`'s own
/// argument and is passed through rather than decided here.
#[must_use]
pub fn pieces(collection: &Collection, worn: Worn<'_>, base_url: &str, mods: &str) -> Vec<Piece> {
    let mut pieces = Vec::with_capacity(5);

    // The hat's back, if it has one, is the only thing under the body. Most hats have none,
    // so this finding nothing is the ordinary case rather than a failure.
    if let Some(back) = collection.find(worn.hat, mods).and_then(|found| {
        Some(Piece {
            layer: Layer::HatBack,
            url: Some(found.image_url(base_url, true)?),
            geometry: found.geometry,
        })
    }) {
        pieces.push(back);
    }

    pieces.push(Piece {
        layer: Layer::Base,
        url: None,
        geometry: Geometry::default(),
    });

    // Skin, then visor, then the hat's front. The order is `Layer`'s own and is not a
    // preference: a visor under a skin is a crewmate looking through their own suit.
    for (layer, id) in [
        (Layer::Skin, worn.skin),
        (Layer::Visor, worn.visor),
        (Layer::HatFront, worn.hat),
    ] {
        let Some(found) = collection.find(id, mods) else {
            continue;
        };
        let Some(url) = found.image_url(base_url, false) else {
            continue;
        };
        pieces.push(Piece {
            layer,
            url: Some(url),
            geometry: found.geometry,
        });
    }

    pieces
}

/// The three ids a player wears.
///
/// A struct rather than three `&str` arguments, because they are all the same type and
/// nothing would catch them being passed in the wrong order — which produces a crewmate
/// wearing a visor on their head, silently, on somebody else's screen.
#[derive(Clone, Copy, Debug)]
pub struct Worn<'a> {
    /// `PlayerState::hat_id`.
    pub hat: &'a str,
    /// `PlayerState::skin_id`.
    pub skin: &'a str,
    /// `PlayerState::visor_id`.
    pub visor: &'a str,
}

/// Where one piece goes on a sprite of a given size, in pixels.
///
/// The geometry is fractions of the sprite's width, and the height follows the artwork's own
/// proportions — the stylesheet this is ported from gives a width and nothing else, so a
/// height taken from anywhere but the file would stretch it.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "sprite and artwork dimensions in pixels, far below f32's exact integer range"
)]
pub fn placement(geometry: Geometry, sprite: i32, artwork: (i32, i32)) -> ((i32, i32), (i32, i32)) {
    let size = sprite as f32;
    let width = geometry.width * size;
    let height = width * artwork.1 as f32 / artwork.0.max(1) as f32;
    (
        ((geometry.left * size) as i32, (geometry.top * size) as i32),
        (width as i32, height as i32),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Worn, pieces, placement};
    use crate::cosmetics::{Geometry, Layer};
    use crate::hats::{BASE, Collection};

    const URL: &str = "https://example.invalid/";

    fn collection() -> Collection {
        let text = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../test/fixtures/hats/hats.json"),
        )
        .expect("the vendored hats index");
        Collection::parse(&text)
    }

    /// A bare crewmate is still a crewmate.
    ///
    /// Wearing nothing has to produce the body and nothing else, rather than an empty list:
    /// a caller that composites what it is given would draw no player at all.
    #[test]
    fn a_player_wearing_nothing_is_still_drawn() {
        let worn = Worn {
            hat: "",
            skin: "",
            visor: "",
        };
        let pieces = pieces(&collection(), worn, URL, BASE);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].layer, Layer::Base);
        assert!(pieces[0].url.is_none(), "the body is not a file");
    }

    /// All five layers, bottom to top.
    ///
    /// This is the assertion the module exists for, and it needs a hat that actually has a
    /// back — `hat_stardew_grandpa` is one of 158 in the vendored index. Testing the order
    /// with two layers would pass for code that cannot order five.
    ///
    /// A hat drawn behind the body when it belongs in front looks like a hat that failed to
    /// load, which is a bug nobody reports.
    #[test]
    fn all_five_layers_come_back_bottom_to_top() {
        let worn = Worn {
            hat: "hat_stardew_grandpa",
            skin: "skin_D2Titan",
            visor: "visor_sunscreenv",
        };
        let pieces = pieces(&collection(), worn, URL, BASE);
        let order: Vec<Layer> = pieces.iter().map(|piece| piece.layer).collect();

        assert_eq!(
            order,
            [
                Layer::HatBack,
                Layer::Base,
                Layer::Skin,
                Layer::Visor,
                Layer::HatFront
            ],
            "not the order Avatar.tsx's z-indices give"
        );
        // The same hat, twice, from two different files: the back and the front are separate
        // images and asking for the wrong side is how one ends up drawn on both.
        let back = pieces[0].url.as_deref().expect("a back image");
        let front = pieces[4].url.as_deref().expect("a front image");
        assert_ne!(back, front, "the hat's two sides are the same file");
    }

    /// A hat with no back image contributes no back layer.
    ///
    /// Most have none. Asking for one and getting the front instead would put the hat behind
    /// the head as well as on it.
    #[test]
    fn a_hat_without_a_back_does_not_get_one() {
        let collection = collection();
        let found = collection.find("skin_D2Titan", BASE).expect("a skin");
        assert!(
            found.image_url(URL, true).is_none(),
            "the fixture picked for this test does have a back image; pick another"
        );

        let worn = Worn {
            hat: "skin_D2Titan",
            skin: "",
            visor: "",
        };
        let pieces = pieces(&collection, worn, URL, BASE);
        assert!(
            !pieces.iter().any(|piece| piece.layer == Layer::HatBack),
            "a back layer was invented"
        );
    }

    /// A cosmetic nobody has heard of costs that cosmetic and nothing else.
    #[test]
    fn an_unknown_id_is_skipped_rather_than_fatal() {
        let worn = Worn {
            hat: "hat_that_does_not_exist",
            skin: "skin_D2Titan",
            visor: "",
        };
        let pieces = pieces(&collection(), worn, URL, BASE);
        assert!(
            pieces.iter().any(|piece| piece.layer == Layer::Skin),
            "one unknown id cost the layers around it"
        );
    }

    /// The height comes from the artwork, not from the geometry.
    ///
    /// The stylesheet gives a width and nothing else. A height from anywhere else stretches
    /// the image, which on a hat reads as the wrong hat rather than as a bug.
    #[test]
    fn the_height_follows_the_files_own_proportions() {
        let geometry = Geometry {
            top: 0.1,
            left: 0.25,
            width: 0.5,
        };
        let (at, size) = placement(geometry, 100, (40, 80));
        assert_eq!(at, (25, 10));
        assert_eq!(size.0, 50, "width is the geometry's share of the sprite");
        assert_eq!(size.1, 100, "a 1:2 image at 50 wide is 100 tall");
    }

    /// A zero-width artwork does not divide by zero.
    #[test]
    fn artwork_with_no_width_is_survivable() {
        let geometry = Geometry {
            top: 0.0,
            left: 0.0,
            width: 1.0,
        };
        let (_, size) = placement(geometry, 64, (0, 10));
        assert!(size.1.is_positive(), "height came out {}", size.1);
    }
}
