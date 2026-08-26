//! The settings, with the defaults 1.x already wrote.
//!
//! §4.8, on what the port is allowed to change: "the Rust UI will not be pixel-identical
//! to the React one. Layout, spacing and control affordances will differ. What must not
//! differ is what every control *does* — the settings schema is ported unchanged,
//! including defaults, so that an existing `config.json` keeps working."
//!
//! The defaults are the half that fails silently. A control that moved is noticed the
//! first time somebody looks for it; a default that changed is noticed by a player whose
//! microphone gain is suddenly wrong, three weeks later, with nothing to connect it to.
//! And `electron-store` writes only what differs from the default, so a changed default
//! rewrites the setting for everyone who never touched it.
//!
//! `every_default_matches_the_electron_client` reads `SettingsStore.tsx` and compares all
//! of them, rather than trusting that this file was transcribed correctly.

/// One default, in whichever of the three shapes the schema uses.
///
/// `Copy` because every variant already is one: [`crate::settings_screen`] holds these
/// inside `const` tables and passes them by value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Default_ {
    /// A `boolean` in the schema.
    Bool(bool),
    /// A `number`.
    Number(f64),
    /// A `string`.
    Text(&'static str),
}

/// Every scalar setting and the value 1.x defaults it to.
///
/// The two `object` entries — `playerConfigMap` and `customPlatforms` — are not here.
/// Both default to `{}`, both are maps the user fills, and neither has a value to get
/// wrong.
#[must_use]
pub fn defaults() -> Vec<(&'static str, Default_)> {
    use Default_::{Bool, Number, Text};
    vec![
        ("alwaysOnTop", Bool(false)),
        // Not a typo here: the shipped default really is `unkown`, and it is a sentinel
        // meaning "ask the operating system" rather than a language tag. Correcting the
        // spelling would make every existing installation look like a fresh one.
        ("language", Text("unkown")),
        ("microphone", Text("Default")),
        ("speaker", Text("Default")),
        ("microphoneLabel", Text("")),
        ("speakerLabel", Text("")),
        // `pushToTalkOptions.VOICE`.
        ("pushToTalkMode", Number(0.0)),
        ("serverURL", Text("https://aucl.greluc.me")),
        ("pushToTalkShortcut", Text("V")),
        ("deafenShortcut", Text("RControl")),
        ("impostorRadioShortcut", Text("F")),
        ("muteShortcut", Text("RAlt")),
        ("hideCode", Bool(false)),
        ("compactOverlay", Bool(false)),
        ("overlayPosition", Text("right")),
        ("meetingOverlay", Bool(true)),
        ("enableOverlay", Bool(true)),
        ("crewVolumeAsGhost", Number(100.0)),
        ("ghostVolumeAsImpostor", Number(10.0)),
        ("masterVolume", Number(100.0)),
        ("microphoneGain", Number(100.0)),
        ("microphoneGainEnabled", Bool(false)),
        ("micSensitivity", Number(0.15)),
        ("micSensitivityEnabled", Bool(false)),
        ("natFix", Bool(false)),
        ("vadEnabled", Bool(true)),
        ("hardware_acceleration", Bool(true)),
        ("enableSpatialAudio", Bool(true)),
        ("echoCancellation", Bool(true)),
        ("noiseSuppression", Bool(true)),
        ("oldSampleDebug", Bool(false)),
        // `GamePlatform.STEAM`.
        ("launchPlatform", Text("STEAM")),
    ]
}

/// The lobby settings a host shares with the room, and their defaults.
///
/// A separate list because they travel separately: these are sent to every peer, so a
/// changed default here does not just alter one player's client, it alters what everyone
/// in their lobby hears.
#[must_use]
pub fn lobby_defaults() -> Vec<(&'static str, Default_)> {
    use Default_::{Bool, Number, Text};
    vec![
        ("maxDistance", Number(5.32)),
        ("haunting", Bool(false)),
        ("commsSabotage", Bool(false)),
        ("hearImpostorsInVents", Bool(false)),
        ("impostersHearImpostersInvent", Bool(false)),
        ("impostorRadioEnabled", Bool(false)),
        ("deadOnly", Bool(false)),
        ("meetingGhostOnly", Bool(false)),
        ("visionHearing", Bool(false)),
        ("hearThroughCameras", Bool(false)),
        ("wallsBlockAudio", Bool(false)),
        ("publicLobby_on", Bool(false)),
        ("publicLobby_title", Text("")),
        ("publicLobby_language", Text("en")),
        ("publicLobby_mods", Text("NONE")),
    ]
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use std::collections::BTreeMap;

    fn settings_store() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../src/renderer/settings/SettingsStore.tsx"),
        )
        .expect("the Electron client is beside the crates")
    }

    /// Reads `key: { type: '...', default: X }` out of the schema.
    ///
    /// Deliberately literal. A tolerant parser would quietly skip an entry whose shape it
    /// did not recognise, and a skipped entry is exactly the one that has drifted.
    fn parse_defaults(source: &str) -> BTreeMap<String, Default_> {
        let mut found = BTreeMap::new();
        let lines: Vec<&str> = source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            // Depth two is a setting; depth four is a property of `localLobbySettings`.
            // Deeper is inside `playerConfigMap` or `customPlatforms`, whose entries are
            // fields of a map the user fills rather than settings of their own — and
            // reading them was this parser's first mistake, caught by the test below.
            let depth = line.len() - line.trim_start_matches('\t').len();
            if depth != 2 && depth != 4 {
                continue;
            }
            let Some(key) = line.trim().strip_suffix(": {") else {
                continue;
            };
            if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let Some(kind) = lines.get(index + 1).map(|l| l.trim()) else {
                continue;
            };
            let Some(default) = lines.get(index + 2).map(|l| l.trim()) else {
                continue;
            };
            let Some(default) = default
                .strip_prefix("default: ")
                .map(|value| value.trim_end_matches(','))
            else {
                continue;
            };
            let parsed = match kind {
                "type: 'boolean'," => Default_::Bool(default == "true"),
                "type: 'string'," => {
                    // The two symbolic defaults, resolved by name rather than by guess.
                    let text = match default {
                        "GamePlatform.STEAM" => "STEAM".to_owned(),
                        other => other.trim_matches('\'').to_owned(),
                    };
                    // Leaked so the comparison is against the same type; the test process
                    // is about to end anyway.
                    Default_::Text(Box::leak(text.into_boxed_str()))
                }
                "type: 'number'," => match default {
                    "pushToTalkOptions.VOICE" => Default_::Number(0.0),
                    other => match other.parse::<f64>() {
                        Ok(number) => Default_::Number(number),
                        Err(_) => continue,
                    },
                },
                _ => continue,
            };
            found.insert(key.to_owned(), parsed);
        }
        found
    }

    #[test]
    fn every_default_matches_the_electron_client() {
        // `electron-store` writes only what differs from the default, so a changed default
        // silently rewrites the setting for everyone who never touched it -- and the
        // player has nothing to connect the change to.
        let source = settings_store();
        let theirs = parse_defaults(&source);
        assert!(
            theirs.len() >= 45,
            "only {} defaults were read out of SettingsStore.tsx; the parse has broken",
            theirs.len()
        );

        let mut wrong = Vec::new();
        let mut absent = Vec::new();
        for (key, ours) in defaults().into_iter().chain(lobby_defaults()) {
            match theirs.get(key) {
                None => absent.push(key),
                Some(found) if *found != ours => {
                    wrong.push(format!("{key}: ours {ours:?}, theirs {found:?}"));
                }
                Some(_) => {}
            }
        }
        assert!(absent.is_empty(), "not in SettingsStore.tsx: {absent:?}");
        assert!(wrong.is_empty(), "defaults that have drifted: {wrong:?}");
    }

    #[test]
    fn no_setting_the_electron_client_has_is_missing_here() {
        // The other direction. A key added to 1.x and not here is a setting the 2.0 client
        // silently ignores -- the player changes it, nothing happens, and their
        // `config.json` still says what they asked for.
        let source = settings_store();
        let theirs = parse_defaults(&source);
        let ours: Vec<&str> = defaults()
            .into_iter()
            .chain(lobby_defaults())
            .map(|(key, _)| key)
            .collect();

        let missing: Vec<&String> = theirs
            .keys()
            .filter(|key| !ours.contains(&key.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "settings 1.x has and this does not: {missing:?}"
        );
    }

    #[test]
    fn the_misspelt_language_sentinel_is_kept_on_purpose() {
        // `unkown`, not `unknown`. It means "ask the operating system", and correcting the
        // spelling would make every existing installation look like a fresh one -- the
        // stored value would stop matching the sentinel and the client would treat a
        // deliberate choice as unset.
        assert!(defaults().contains(&("language", Default_::Text("unkown"))));
    }

    #[test]
    fn the_lobby_defaults_are_listed_separately_because_they_travel() {
        // These are sent to every peer, so a changed default does not alter one player's
        // client -- it alters what everyone in their lobby hears.
        assert!(
            lobby_defaults()
                .iter()
                .any(|(key, _)| *key == "maxDistance")
        );
        assert!(!defaults().iter().any(|(key, _)| *key == "maxDistance"));
    }
}
