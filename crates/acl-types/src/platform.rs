//! Where the game is launched from, and how.
//!
//! A port of `src/common/GamePlatform.ts`. Three built-in platforms, two ways of starting
//! a game, and a set of strings that are not this project's to choose: the Steam app id
//! and the Epic catalogue GUID belong to the stores, and a wrong character in either
//! produces a launch button that does nothing at all.
//!
//! The user can add their own, which is what `customPlatforms` in the settings holds.
//! Those are not here — they have no fixed values to protect.

/// One of the stores the client knows how to start Among Us from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Steam.
    Steam,
    /// The Epic Games launcher.
    Epic,
    /// The Microsoft Store.
    Microsoft,
}

/// How a platform is started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunType {
    /// Hand a URI to the operating system and let the launcher take it.
    Uri,
    /// Run an executable found on disk.
    Exe,
}

impl Platform {
    /// Every platform, in the order the settings screen lists them.
    pub const ALL: [Self; 3] = [Self::Steam, Self::Epic, Self::Microsoft];

    /// The key stored in `config.json` under `launchPlatform`.
    ///
    /// Written by 1.x and read by 2.x during the rollout, so these are not display text
    /// and must not be tidied.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Steam => "STEAM",
            Self::Epic => "EPIC",
            Self::Microsoft => "MICROSOFT",
        }
    }

    /// Whether the platform is started by URI or by executable.
    #[must_use]
    pub const fn run_type(self) -> RunType {
        match self {
            Self::Steam | Self::Epic => RunType::Uri,
            Self::Microsoft => RunType::Exe,
        }
    }

    /// The URI handed to the operating system, for the two that use one.
    ///
    /// The identifiers inside belong to the stores. `945360` is Among Us on Steam and the
    /// GUID is its Epic catalogue entry; neither is guessable and a wrong character
    /// produces a launch button that silently does nothing.
    #[must_use]
    pub const fn run_path(self) -> &'static str {
        match self {
            Self::Steam => "steam://rungameid/945360",
            Self::Epic => {
                "com.epicgames.launcher://apps/963137e4c29d4c79a81323b8fab03a40?action=launch&silent=true"
            }
            // Not a URI, and not empty either: the shipped client writes the string
            // `none` here, and the settings screen shows it.
            Self::Microsoft => "none",
        }
    }

    /// The executable to look for, for the one that is started that way.
    #[must_use]
    pub const fn executable(self) -> Option<&'static str> {
        match self {
            Self::Microsoft => Some("Among Us.exe"),
            Self::Steam | Self::Epic => None,
        }
    }

    /// The locale key for the platform's name.
    #[must_use]
    pub const fn translate_key(self) -> &'static str {
        match self {
            Self::Steam => "platform.steam",
            Self::Epic => "platform.epicgames",
            Self::Microsoft => "platform.microsoft",
        }
    }

    /// Reads the key stored in `config.json`.
    ///
    /// `None` for anything else, which includes every user-added platform: those are
    /// stored under their own names in `customPlatforms` and are not one of these.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|platform| platform.key() == key)
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn source() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../src/common/GamePlatform.ts"),
        )
        .expect("the Electron client is beside the crates")
    }

    #[test]
    fn the_store_identifiers_match_the_electron_client() {
        // Neither is guessable and neither is ours. A wrong character produces a launch
        // button that does nothing at all, with no error to see.
        let source = source();
        for platform in Platform::ALL {
            assert!(
                source.contains(&format!("runPath: '{}'", platform.run_path())),
                "{platform:?}'s run path is not in GamePlatform.ts"
            );
        }
    }

    #[test]
    fn the_stored_keys_match_the_electron_client() {
        // These are in 1.x's `config.json` under `launchPlatform`, and the 2.0 build reads
        // that file during the rollout. A key that does not match reads as unset, and the
        // player's chosen launcher silently reverts to Steam.
        let source = source();
        for platform in Platform::ALL {
            assert!(
                source.contains(&format!("{} = '{}'", platform.key(), platform.key())),
                "{platform:?}'s key is not in GamePlatform.ts"
            );
            assert_eq!(Platform::from_key(platform.key()), Some(platform));
        }
    }

    #[test]
    fn every_translate_key_exists_in_the_locale_file() {
        // A missing one shows the raw key in the settings dropdown rather than a name.
        let english = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../static/locales/en/translation.json"),
        )
        .expect("the locales are beside the crates");
        for platform in Platform::ALL {
            let leaf = platform
                .translate_key()
                .rsplit('.')
                .next()
                .expect("a dotted key");
            assert!(
                english.contains(&format!("\"{leaf}\":")),
                "{platform:?}'s translate key {:?} has no entry",
                platform.translate_key()
            );
        }
    }

    #[test]
    fn only_the_microsoft_store_is_started_by_executable() {
        // The other two hand a URI to the launcher, which finds the game itself. The
        // Microsoft Store gives no URI, which is why it is the one that needs a path.
        assert_eq!(Platform::Microsoft.run_type(), RunType::Exe);
        assert_eq!(Platform::Microsoft.executable(), Some("Among Us.exe"));
        for platform in [Platform::Steam, Platform::Epic] {
            assert_eq!(platform.run_type(), RunType::Uri, "{platform:?}");
            assert_eq!(platform.executable(), None, "{platform:?}");
        }
    }

    #[test]
    fn an_unknown_key_is_not_one_of_these() {
        // A user-added platform is stored under its own name in `customPlatforms`, so an
        // unrecognised key is ordinary rather than an error.
        assert_eq!(Platform::from_key("ITCH"), None);
        assert_eq!(Platform::from_key(""), None);
        assert_eq!(Platform::from_key("steam"), None, "the key is upper case");
    }
}
