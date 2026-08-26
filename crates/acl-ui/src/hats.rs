//! The hat collection: what is in it, and how a player's cosmetic is found in it.
//!
//! [`acl_types::cosmetics`] holds the pinned URL the artwork comes from and
//! [`crate::cosmetics`] holds where each layer is drawn. This is the part in between —
//! `hats.json`, the index the client fetches once and looks every cosmetic up in.
//!
//! Ported from `getHat` and `getModHat` in `src/renderer/cosmetics.ts`, and the two rules
//! that matter are both in `getHat`.
//!
//! **The base collection is searched before the mod's.** `for (const mod of ['NONE',
//! modType])` — so a mod that names a hat the base game already has does not shadow it.
//! Reversing the two would give a modded player different artwork for a vanilla hat than
//! everybody else in the lobby sees.
//!
//! **Geometry falls back per axis.** A hat that overrides only its width keeps the
//! collection's top and left. That is what the three `??` do, and [`crate::cosmetics::resolve`]
//! is where it is implemented.
//!
//! # What the shipped file actually contains
//!
//! Measured against the vendored copy in `test/fixtures/hats`, and worth knowing because
//! two of them are load-bearing:
//!
//! - **One collection, `NONE`.** This fork carries the base game's cosmetics only.
//! - **46 of 983 entries have no `image`.** They resolve, and then draw nothing — which is
//!   what the Electron client does too, since `getModHat` returns `undefined` and the
//!   layer is skipped. A missing image is not an error and must not read as one.
//! - **No entry has its own geometry.** Every one takes the collection's defaults, so the
//!   per-hat override path is exercised by nothing that ships. It is implemented anyway,
//!   because the schema has it and a future collection may use it.

use std::collections::BTreeMap;

use crate::cosmetics::Geometry;

/// One cosmetic in the collection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hat {
    /// The file drawn in front of the player, if there is one.
    ///
    /// Optional because 46 of the shipped entries have none. See the module documentation.
    pub image: Option<String>,
    /// The file drawn behind the player, for hats that wrap around one.
    pub back_image: Option<String>,
    /// Whether the artwork is recoloured to the player's colour.
    pub multi_color: bool,
    /// What the game calls this asset, which is not the id it is stored under.
    pub asset_name: Option<String>,
    /// Which of hats, skins and visors it is.
    pub kind: Option<String>,
    /// This entry's own geometry, when it has any: top, left, width.
    own: [Option<String>; 3],
}

/// One mod's cosmetics, and the geometry they default to.
#[derive(Clone, Debug, Default)]
struct Set {
    /// Top, left, width — in [`crate::cosmetics::resolve`]'s order.
    defaults: [Option<String>; 3],
    hats: BTreeMap<String, Hat>,
}

/// The whole `hats.json`.
#[derive(Clone, Debug, Default)]
pub struct Collection {
    sets: BTreeMap<String, Set>,
}

/// A cosmetic that was found, with the geometry already resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Found<'a> {
    /// The entry.
    pub hat: &'a Hat,
    /// Which collection it came from, which is also the directory its files are in.
    pub set: &'a str,
    /// Where to draw it, the hat's own values over the collection's defaults.
    pub geometry: Geometry,
}

/// The collection every lookup starts in.
///
/// The base game's, and the string the file uses for it.
pub const BASE: &str = "NONE";

impl Collection {
    /// Reads `hats.json`.
    ///
    /// Nothing here is an error. A document that will not parse, a collection that is not
    /// an object, an entry missing a field — each costs the cosmetics it describes and
    /// nothing else, which is what the Electron client does: a failed fetch leaves
    /// `initializedHats` false and every avatar draws its base and no layers.
    ///
    /// Being strict would trade "some hats are missing" for "no hats at all, and the
    /// window says why", and nobody wants the second one.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let Ok(serde_json::Value::Object(document)) = serde_json::from_str(text) else {
            return Self::default();
        };
        let sets = document
            .into_iter()
            .filter_map(|(name, value)| {
                let object = value.as_object()?;
                let string = |key: &str| {
                    object
                        .get(key)
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                };
                let hats = object
                    .get("hats")
                    .and_then(serde_json::Value::as_object)
                    .map(|hats| {
                        hats.iter()
                            .map(|(id, entry)| (id.clone(), read_hat(entry)))
                            .collect()
                    })
                    .unwrap_or_default();
                Some((
                    name,
                    Set {
                        defaults: [
                            string("defaultTop"),
                            string("defaultLeft"),
                            string("defaultWidth"),
                        ],
                        hats,
                    },
                ))
            })
            .collect();
        Self { sets }
    }

    /// Whether anything was loaded.
    ///
    /// The client draws no cosmetic layers at all until this is true, matching
    /// `initializedHats`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sets.values().all(|set| set.hats.is_empty())
    }

    /// How many cosmetics are in it, across every collection.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sets.values().map(|set| set.hats.len()).sum()
    }

    /// Every id in the collection.
    ///
    /// For anything that has to sweep it -- a cache warm-up, or a test that checks every
    /// name the collection can produce is one the caller will accept.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.sets
            .values()
            .flat_map(|set| set.hats.keys().map(String::as_str))
    }

    /// Finds one cosmetic by the id the game reports.
    ///
    /// The base collection first, then the mod's — see the module documentation for why
    /// that order is not arbitrary.
    ///
    /// `None` for an id nothing has, which is ordinary: a player running a mod this build
    /// has no artwork for wears a hat that cannot be drawn, and the layer is skipped.
    #[must_use]
    pub fn find(&self, id: &str, mods: &str) -> Option<Found<'_>> {
        if id.is_empty() {
            return None;
        }
        for name in [BASE, mods] {
            let Some((set_name, set)) = self.sets.get_key_value(name) else {
                continue;
            };
            if let Some(hat) = set.hats.get(id) {
                return Some(Found {
                    hat,
                    set: set_name,
                    geometry: crate::cosmetics::resolve(
                        [
                            hat.own[0].as_deref(),
                            hat.own[1].as_deref(),
                            hat.own[2].as_deref(),
                        ],
                        [
                            set.defaults[0].as_deref(),
                            set.defaults[1].as_deref(),
                            set.defaults[2].as_deref(),
                        ],
                    ),
                });
            }
        }
        None
    }
}

impl Found<'_> {
    /// Where to fetch this cosmetic's artwork from.
    ///
    /// `None` when the entry has no file for that side, which 46 of the shipped ones do
    /// not for the front and most do not for the back. The caller draws nothing, which is
    /// what the Electron client's `undefined` leads to.
    ///
    /// The base is joined verbatim: [`acl_types::cosmetics::HAT_COLLECTION_URL`] already
    /// ends in a slash, and re-deriving that here is a second place for it to be wrong.
    ///
    /// **The file name is percent-encoded**, and that is not tidiness. Eighteen of the
    /// 1,095 files in the shipped collection have a space in the name, and one of those
    /// eighteen also has an apostrophe -- `pk01_Captain's Hat.png`. A raw space is not a
    /// URL at all: curl refuses it as malformed before a request goes out, and so does
    /// every other client. Measured against the CDN on 2026-08-26: the raw form does not
    /// resolve and the encoded form returns 200.
    #[must_use]
    pub fn image_url(&self, base: &str, back: bool) -> Option<String> {
        let file = if back {
            self.hat.back_image.as_ref()
        } else {
            self.hat.image.as_ref()
        }?;
        Some(format!(
            "{base}{}/{}",
            encode_segment(self.set),
            encode_segment(file)
        ))
    }
}

/// Percent-encodes one path segment.
///
/// The unreserved set of RFC 3986 — letters, digits, and `-._~` — passes through and
/// everything else becomes `%XX`. Narrow on purpose: the collection uses spaces and one
/// apostrophe today, and a rule that lists what is *allowed* needs no revisiting when it
/// starts using something else.
///
/// A slash is encoded too, so a name that contains one cannot become a path.
fn encode_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            // `char::from_digit` rather than a lookup table, which cannot be indexed
            // without the compiler being told the index is in range -- and it always is,
            // because a nibble is four bits.
            let nibble = |value: u8| {
                char::from_digit(u32::from(value), 16)
                    .unwrap_or('0')
                    .to_ascii_uppercase()
            };
            encoded.push('%');
            encoded.push(nibble(byte >> 4));
            encoded.push(nibble(byte & 0x0F));
        }
    }
    encoded
}

/// Reads one entry.
fn read_hat(entry: &serde_json::Value) -> Hat {
    let string = |key: &str| {
        entry
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
    };
    Hat {
        image: string("image"),
        back_image: string("back_image"),
        multi_color: entry
            .get("multi_color")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        asset_name: string("asset_name"),
        kind: string("hat_type"),
        own: [string("top"), string("left"), string("width")],
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{BASE, Collection};

    /// The real file, vendored. See `test/fixtures/hats/README.md` for why.
    fn shipped() -> Collection {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test/fixtures/hats/hats.json"),
        )
        .expect("the vendored collection");
        Collection::parse(&text)
    }

    /// The parser's job is to accept the file that is actually served. A sample invented
    /// alongside it would only prove it agrees with itself.
    #[test]
    fn the_whole_shipped_collection_reads() {
        let collection = shipped();
        assert!(!collection.is_empty());
        assert_eq!(collection.len(), 983, "the collection changed size");
        assert_eq!(
            collection.sets.len(),
            1,
            "this fork carries the base game's cosmetics only"
        );
        assert!(collection.sets.contains_key(BASE));
    }

    /// Forty-six entries have no image, and they resolve anyway. A missing image is a
    /// layer that is not drawn, in both clients — not an error, and not a reason to lose
    /// the entry.
    #[test]
    fn an_entry_with_no_image_still_resolves_and_draws_nothing() {
        let collection = shipped();
        let imageless: Vec<&String> = collection.sets[BASE]
            .hats
            .iter()
            .filter(|(_, hat)| hat.image.is_none())
            .map(|(id, _)| id)
            .collect();
        assert_eq!(imageless.len(), 46, "the shipped count changed");

        let id = imageless.first().expect("one of them");
        let found = collection.find(id, BASE).expect("it is still in the index");
        assert_eq!(
            found.image_url("https://example.invalid/", false),
            None,
            "an entry with no file must not produce a URL"
        );
    }

    /// A back image is a separate file, and asking for the wrong side gives the wrong
    /// answer rather than a fallback — a hat drawn behind the player where its front
    /// belongs is worse than no hat.
    #[test]
    fn the_two_sides_are_two_files() {
        let collection = shipped();
        let found = collection
            .find("hat_stardew_grandpa", BASE)
            .expect("a hat with a back image");
        let front = found
            .image_url("https://example.invalid/", false)
            .expect("a front");
        let back = found
            .image_url("https://example.invalid/", true)
            .expect("a back");
        assert_ne!(front, back);
        assert!(front.contains("grandpaHat.png"), "{front}");
        assert!(
            front.starts_with("https://example.invalid/NONE/"),
            "{front}"
        );
    }

    /// Most entries have no back image, and asking for one gives nothing rather than the
    /// front drawn twice.
    #[test]
    fn a_hat_with_no_back_has_no_back_url() {
        let collection = shipped();
        let found = collection
            .find("skin_D2Titan", BASE)
            .expect("a skin with no back");
        assert!(found.image_url("https://example.invalid/", false).is_some());
        assert_eq!(found.image_url("https://example.invalid/", true), None);
    }

    /// The recoloured ones are flagged, because they are fetched differently: the Electron
    /// client puts them behind a `generate:///` URL so the main process can recolour them.
    #[test]
    fn the_recoloured_hats_are_flagged() {
        let collection = shipped();
        let coloured = collection.sets[BASE]
            .hats
            .values()
            .filter(|hat| hat.multi_color)
            .count();
        assert_eq!(coloured, 30, "the shipped count changed");
        assert!(
            collection
                .find("hat_lny_rat", BASE)
                .expect("a recoloured hat")
                .hat
                .multi_color
        );
    }

    /// Every entry takes the collection's defaults, because none of them overrides
    /// anything. Recorded rather than assumed: if a future collection starts overriding,
    /// this is the test that says the per-axis path stopped being dead code.
    #[test]
    fn nothing_shipped_overrides_its_own_geometry() {
        let collection = shipped();
        let overriding = collection.sets[BASE]
            .hats
            .values()
            .filter(|hat| hat.own.iter().any(Option::is_some))
            .count();
        assert_eq!(overriding, 0);

        // So every hat resolves to the same geometry, which is the collection's.
        let found = collection.find("hat_lny_rat", BASE).expect("any hat");
        assert!(
            (found.geometry.top - -0.78).abs() < 1e-6,
            "{:?}",
            found.geometry
        );
        assert!((found.geometry.left - -0.14).abs() < 1e-6);
        assert!((found.geometry.width - 1.30).abs() < 1e-6);
    }

    /// A hat's own value wins over the collection's, per axis. Nothing shipped exercises
    /// this, so it is exercised here — the schema has it, and an untested path that a
    /// future collection turns on is a bug waiting for a release to happen.
    #[test]
    fn an_overriding_entry_keeps_the_defaults_it_does_not_touch() {
        let collection = Collection::parse(
            r#"{"NONE":{"defaultTop":"-78%","defaultLeft":"-14%","defaultWidth":"130%",
                "hats":{"hat_x":{"image":"x.png","width":"50%"}}}}"#,
        );
        let found = collection.find("hat_x", BASE).expect("the hat");
        assert!((found.geometry.width - 0.5).abs() < 1e-6, "its own width");
        assert!((found.geometry.top - -0.78).abs() < 1e-6, "the default top");
        assert!(
            (found.geometry.left - -0.14).abs() < 1e-6,
            "the default left"
        );
    }

    /// The base collection is searched first, so a mod cannot shadow a vanilla hat — which
    /// would give a modded player different artwork for it than everybody else sees.
    #[test]
    fn the_base_collection_wins_over_a_mods() {
        let collection = Collection::parse(
            r#"{"NONE":{"hats":{"hat_x":{"image":"base.png"}}},
                "TOWN_OF_US":{"hats":{"hat_x":{"image":"mod.png"}}}}"#,
        );
        let found = collection.find("hat_x", "TOWN_OF_US").expect("the hat");
        assert_eq!(found.set, BASE);
        assert_eq!(found.hat.image.as_deref(), Some("base.png"));
    }

    /// And a mod's own hat is still found, in the mod's own directory.
    #[test]
    fn a_mods_own_hat_is_found_in_the_mods_directory() {
        let collection = Collection::parse(
            r#"{"NONE":{"hats":{}},
                "TOWN_OF_US":{"hats":{"hat_y":{"image":"y.png"}}}}"#,
        );
        let found = collection.find("hat_y", "TOWN_OF_US").expect("the hat");
        assert_eq!(found.set, "TOWN_OF_US");
        assert_eq!(
            found
                .image_url("https://example.invalid/", false)
                .as_deref(),
            Some("https://example.invalid/TOWN_OF_US/y.png")
        );
    }

    /// A player running a mod this build has no artwork for wears a hat that cannot be
    /// drawn. That is ordinary, and the layer is skipped.
    #[test]
    fn an_id_nothing_has_is_not_found() {
        let collection = shipped();
        assert!(
            collection
                .find("hat_from_a_mod_we_do_not_ship", BASE)
                .is_none()
        );
        assert!(collection.find("hat_lny_rat", "SOMETHING_NEWER").is_some());
        assert!(collection.find("", BASE).is_none(), "an empty id is no hat");
    }

    /// A document that will not parse costs the cosmetics and nothing else. The Electron
    /// client does the same — a failed fetch leaves every avatar drawing its base and no
    /// layers — and being strict would trade "some hats are missing" for "no hats at all".
    #[test]
    fn a_broken_document_costs_the_hats_and_nothing_else() {
        for text in ["", "{", "null", "[1,2,3]", "not json", "{\"NONE\":7}"] {
            let collection = Collection::parse(text);
            assert!(collection.is_empty(), "{text:?}");
            assert!(collection.find("hat_lny_rat", BASE).is_none());
        }
    }

    /// The three kinds share one index, which is why the ids are prefixed. A skin looked
    /// up as a hat is simply a different id and misses.
    #[test]
    fn hats_skins_and_visors_share_one_index() {
        let collection = shipped();
        let mut kinds: Vec<&str> = collection.sets[BASE]
            .hats
            .values()
            .filter_map(|hat| hat.kind.as_deref())
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds, ["hats", "skins", "visors"]);
        assert!(collection.find("skin_D2Titan", BASE).is_some());
        assert!(collection.find("visor_pk01_Security1Visor", BASE).is_some());
    }

    /// Eighteen of the shipped names cannot be used raw: they contain a space, and one of
    /// them also contains an apostrophe. A raw space is not a URL — curl refuses it as
    /// malformed before a request goes out — so without encoding those eighteen cosmetics
    /// simply never load, silently, for everybody.
    #[test]
    fn the_names_with_spaces_in_them_are_encoded() {
        let collection = shipped();
        let mut encoded = 0;
        for id in collection.ids() {
            let Some(found) = collection.find(id, BASE) else {
                continue;
            };
            for back in [false, true] {
                let Some(url) = found.image_url("https://example.invalid/", back) else {
                    continue;
                };
                assert!(!url.contains(' '), "a raw space in {url}");
                assert!(!url.contains('\''), "a raw apostrophe in {url}");
                if url.contains("%20") || url.contains("%27") {
                    encoded += 1;
                }
            }
        }
        assert_eq!(encoded, 18, "the shipped count changed");
    }

    /// And the encoding is the one the CDN actually serves. Checked against it on
    /// 2026-08-26: `pk01_Captain%27s%20Hat.png` returns 200 and the raw form does not
    /// resolve at all.
    #[test]
    fn the_encoding_is_the_one_the_cdn_serves() {
        let collection = shipped();
        let found = collection
            .find("hat_pk01_Captain", BASE)
            .or_else(|| {
                collection
                    .ids()
                    .find(|id| {
                        collection
                            .find(id, BASE)
                            .and_then(|found| found.hat.image.clone())
                            .is_some_and(|image| image.contains('\''))
                    })
                    .and_then(|id| collection.find(id, BASE))
            })
            .expect("the one name with an apostrophe");
        let url = found
            .image_url("https://example.invalid/", false)
            .expect("a URL");
        assert!(url.ends_with("pk01_Captain%27s%20Hat.png"), "{url}");
    }

    /// The URL is the pinned one joined to the file, and the pin already ends in a slash.
    /// Re-deriving that here would be a second place for it to be wrong.
    #[test]
    fn the_shipped_pin_still_serves_this_file() {
        assert!(
            acl_types::cosmetics::HAT_COLLECTION_URL.ends_with('/'),
            "the join below assumes it"
        );
        let collection = shipped();
        let found = collection.find("hat_lny_rat", BASE).expect("a hat");
        let url = found
            .image_url(acl_types::cosmetics::HAT_COLLECTION_URL, false)
            .expect("a URL");
        assert!(
            url.contains(acl_types::cosmetics::HAT_COLLECTION_COMMIT),
            "the fixture was taken from a different tree than the pin names: {url}"
        );
        assert!(!url.contains("//NONE"), "a doubled slash: {url}");
    }
}
