//! The mods the client recognises, and how it tells them apart.
//!
//! A port of `src/common/Mods.ts` and of the one line in `GameReader.ts` that uses it:
//!
//! ```js
//! const mod = modList.find((o) => o.dllStartsWith && file.includes(o.dllStartsWith));
//! ```
//!
//! First match in list order wins, and that is the whole subtlety. `TownOfUsMira.dll`
//! contains both `TownOfUsMira` and `TownOfUs`, so which mod it is depends entirely on
//! which entry the scan reaches first. It reaches Mira first today because Mira is listed
//! first, and nothing in the TypeScript says that this is required rather than incidental.
//!
//! [`no_marker_is_shadowed_by_an_earlier_one`] makes it required. Reordering the two, or
//! adding a mod whose marker is contained in another's, fails there instead of quietly
//! reporting the wrong mod name in every lobby that runs it.

/// A mod the client knows by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mod {
    /// No mod, or none this client recognises.
    None,
    /// Town of Us: Mira.
    TownOfUsMira,
    /// Town of Us: Reactivated.
    TownOfUs,
    /// The Other Roles.
    TheOtherRoles,
    /// Las Monjas.
    LasMonjas,
    /// A mod loader is present but the mod is not one of the above.
    Other,
}

impl Mod {
    /// The identifier used on the wire and in the settings.
    ///
    /// These strings travel between clients in the lobby browser, so they are not
    /// display text and must not be translated or tidied.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::TownOfUsMira => "TOWN_OF_US_MIRA",
            Self::TownOfUs => "TOWN_OF_US",
            Self::TheOtherRoles => "THE_OTHER_ROLES",
            Self::LasMonjas => "LAS_MONJAS",
            Self::Other => "OTHER",
        }
    }

    /// What the lobby browser shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::TownOfUsMira => "Town of Us: Mira",
            Self::TownOfUs => "Town of Us: Reactivated",
            Self::TheOtherRoles => "The Other Roles",
            Self::LasMonjas => "Las Monjas",
            Self::Other => "Other",
        }
    }

    /// The substring that identifies this mod's plugin file, if it has one.
    ///
    /// `None` and `Other` have none: the first is the absence of a mod and the second is
    /// what a recognised loader with an unrecognised plugin resolves to.
    #[must_use]
    pub const fn marker(self) -> Option<&'static str> {
        match self {
            Self::TownOfUsMira => Some("TownOfUsMira"),
            Self::TownOfUs => Some("TownOfUs"),
            Self::TheOtherRoles => Some("TheOtherRoles"),
            Self::LasMonjas => Some("LasMonjas"),
            Self::None | Self::Other => None,
        }
    }
}

/// The scan order, which decides the answer where markers overlap.
///
/// Longest-first is not enforced by construction — it is asserted by a test, because the
/// list is also the order the settings screen shows and a sort would move `None` out of
/// first place.
pub const SCAN_ORDER: [Mod; 6] = [
    Mod::None,
    Mod::TownOfUsMira,
    Mod::TownOfUs,
    Mod::TheOtherRoles,
    Mod::LasMonjas,
    Mod::Other,
];

/// Which mod a plugin filename belongs to, if any.
///
/// A substring test rather than a prefix test, matching `file.includes(...)` in the
/// client this ports. The field is called `dllStartsWith` there and does not do that; the
/// behaviour is what has to be reproduced, because `BepInEx` plugin filenames carry
/// prefixes and version suffixes that a strict prefix match would miss.
#[must_use]
pub fn detect(plugin_file: &str) -> Option<Mod> {
    SCAN_ORDER
        .into_iter()
        .find(|candidate| candidate.marker().is_some_and(|m| plugin_file.contains(m)))
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    /// The one that matters. `TownOfUsMira.dll` contains `TownOfUs` as well, so a scan
    /// that reached the shorter marker first would report every Mira lobby as Town of Us
    /// — in the lobby browser, to everybody, silently.
    #[test]
    fn no_marker_is_shadowed_by_an_earlier_one() {
        for (index, mod_) in SCAN_ORDER.iter().enumerate() {
            let Some(marker) = mod_.marker() else {
                continue;
            };
            for earlier in &SCAN_ORDER[..index] {
                let Some(earlier_marker) = earlier.marker() else {
                    continue;
                };
                assert!(
                    !marker.contains(earlier_marker),
                    "{mod_:?}'s marker {marker:?} contains {earlier:?}'s {earlier_marker:?}, so {mod_:?} can never be detected"
                );
            }
        }
    }

    #[test]
    fn mira_is_not_reported_as_town_of_us() {
        assert_eq!(detect("TownOfUsMira.dll"), Some(Mod::TownOfUsMira));
        assert_eq!(detect("TownOfUs.dll"), Some(Mod::TownOfUs));
    }

    #[test]
    fn every_recognised_plugin_resolves() {
        assert_eq!(detect("TheOtherRoles.dll"), Some(Mod::TheOtherRoles));
        assert_eq!(detect("LasMonjas.dll"), Some(Mod::LasMonjas));
    }

    #[test]
    fn a_prefix_or_a_suffix_does_not_stop_the_match() {
        // The client uses `includes`, not `startsWith`, despite the field name. BepInEx
        // plugin filenames carry both, and a strict prefix match would miss them.
        assert_eq!(detect("BepInEx.TownOfUs.v3.dll"), Some(Mod::TownOfUs));
    }

    #[test]
    fn an_unknown_plugin_is_not_a_mod_this_list_knows() {
        // The caller turns this into `Other` when a loader is present, which is a
        // different question from "which of these is it".
        assert_eq!(detect("SomeOtherPlugin.dll"), None);
        assert_eq!(detect(""), None);
    }

    #[test]
    fn the_ids_and_labels_match_the_electron_client() {
        // The ids travel between clients in the lobby browser, so they are wire values
        // rather than display text. The labels are what the browser shows, and a player
        // comparing two clients sees any difference immediately.
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/common/Mods.ts"),
        )
        .expect("the Electron client is beside the crates");
        for mod_ in SCAN_ORDER {
            assert!(
                source.contains(&format!("id: '{}'", mod_.id())),
                "{mod_:?}'s id is not in Mods.ts"
            );
            assert!(
                source.contains(&format!("label: '{}'", mod_.label())),
                "{mod_:?}'s label is not in Mods.ts"
            );
            if let Some(marker) = mod_.marker() {
                assert!(
                    source.contains(&format!("dllStartsWith: '{marker}'")),
                    "{mod_:?}'s marker is not in Mods.ts"
                );
            }
        }
    }
}
