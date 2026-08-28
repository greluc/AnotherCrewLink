//! Reading and writing the settings file 1.x already owns.
//!
//! [`crate::settings`] holds the schema and the defaults, cross-checked against
//! `SettingsStore.tsx`. This is the file they live in: `config.json`, in the shape
//! `electron-store` writes.
//!
//! 1.x's copy is at `%APPDATA%\AnotherCrewLink\config.json` and 2.x's at
//! `%APPDATA%\ACL\config.json`. They are separate on purpose — see
//! `acl_core::paths::import`, which brings the first forward into the second once and never
//! writes back. This module is given a path; it does not choose one.
//!
//! §4.8 is explicit about what may change and what may not: "the settings schema is ported
//! unchanged, including defaults, so that an existing `config.json` keeps working". §4.10
//! then has both clients installed at once during the rollout, so *keeps working* means in
//! both directions — a 2.x that wrote the file must leave a 1.x able to read it, and the
//! other way about.
//!
//! Three things follow, and each is a test.
//!
//! **Unknown keys survive.** A 1.x knows settings this build does not, and a document
//! rewritten as a typed struct loses every one of them. So the file is kept as it was read
//! and only the touched keys are replaced.
//!
//! **A key that is not there reads as its default**, rather than as an error or a zero.
//! That is what `electron-store`'s `defaults` does, and it is why a fresh installation has
//! a working microphone gain before anybody opens the settings.
//!
//! **A file that will not parse reads as a fresh one.** A settings file damaged by a
//! half-finished write is a client that refuses to start, and there is nothing in it that
//! is worth that — every value in it has a default.
//!
//! # The tabs are deliberate, and the ordering is not preserved
//!
//! `electron-store` indents with a tab, and so does this. Two spaces would work and would
//! make every line of the file differ from what the other client writes.
//!
//! Key *order* is a different matter and is not preserved: `serde_json::Map` is a
//! `BTreeMap`, so a document written here comes back alphabetised while `electron-store`
//! writes in insertion order. Nothing breaks — order is not meaning in JSON — but the first
//! write from this client does reorder the file once. Fixing it wants `indexmap`, which is
//! a dependency for the sake of a one-time diff; `the_round_trip_alphabetises_and_that_is_known`
//! records the decision rather than leaving it to be rediscovered.

use serde_json::{Map, Value};

use crate::settings::{Default_, defaults};

/// The settings document.
///
/// Held as the JSON that was read rather than as a struct, so that a key this build does
/// not know about is still there after a write.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    document: Map<String, Value>,
}

impl Config {
    /// An empty document. Every setting reads as its default.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads a settings file.
    ///
    /// Anything that is not a JSON object reads as empty — see the module documentation for
    /// why that is better than refusing to start.
    #[must_use]
    pub fn read(text: &str) -> Self {
        Self {
            document: serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|value| match value {
                    Value::Object(map) => Some(map),
                    _ => None,
                })
                .unwrap_or_default(),
        }
    }

    /// Writes it back, in the shape `electron-store` writes.
    #[must_use]
    pub fn write(&self) -> String {
        let mut out = Vec::new();
        let indent = b"\t";
        let mut serializer = serde_json::Serializer::with_formatter(
            &mut out,
            serde_json::ser::PrettyFormatter::with_indent(indent),
        );
        if serde::Serialize::serialize(&Value::Object(self.document.clone()), &mut serializer)
            .is_err()
        {
            return String::new();
        }
        String::from_utf8(out).unwrap_or_default()
    }

    /// The raw value at a dotted path, if it is there.
    ///
    /// Dotted because `electron-store` is: the shipped client writes
    /// `playerConfigMap.<nameHash>` as one key and means a nested object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        let mut steps = key.split('.');
        let mut current = self.document.get(steps.next()?)?;
        for step in steps {
            current = current.get(step)?;
        }
        Some(current)
    }

    /// A boolean setting, or its documented default.
    #[must_use]
    pub fn bool_at(&self, key: &str) -> bool {
        self.get(key)
            .and_then(Value::as_bool)
            .unwrap_or_else(|| match default_for(key) {
                Some(Default_::Bool(value)) => value,
                _ => false,
            })
    }

    /// A numeric setting, or its documented default.
    #[must_use]
    pub fn number_at(&self, key: &str) -> f64 {
        self.get(key)
            .and_then(Value::as_f64)
            .unwrap_or_else(|| match default_for(key) {
                Some(Default_::Number(value)) => value,
                _ => 0.0,
            })
    }

    /// A text setting, or its documented default.
    #[must_use]
    pub fn text_at(&self, key: &str) -> String {
        self.get(key).and_then(Value::as_str).map_or_else(
            || match default_for(key) {
                Some(Default_::Text(value)) => value.to_owned(),
                _ => String::new(),
            },
            ToOwned::to_owned,
        )
    }

    /// A list of strings, or an empty one.
    ///
    /// No default: the schema holds scalars, and the one list that reaches this is a custom
    /// platform's `execute` -- a program and its arguments, which nobody but the player who
    /// added that platform can have a value for. Anything in the array that is not a string
    /// is dropped rather than stringified, because `execute[0]` becomes a path and a number
    /// turned into one is a path that does not exist.
    #[must_use]
    pub fn strings_at(&self, key: &str) -> Vec<String> {
        self.get(key)
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Sets a value at a dotted path, creating the objects on the way.
    ///
    /// A step that exists and is not an object is replaced. `electron-store` does the same,
    /// and the alternative — refusing — leaves a client unable to save a setting because of
    /// something else in the file.
    pub fn set(&mut self, key: &str, value: Value) {
        let mut steps: Vec<&str> = key.split('.').collect();
        let Some(last) = steps.pop() else {
            return;
        };
        let mut current = &mut self.document;
        for step in steps {
            let entry = current
                .entry(step.to_owned())
                .or_insert_with(|| Value::Object(Map::new()));
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            let Some(object) = entry.as_object_mut() else {
                return;
            };
            current = object;
        }
        current.insert(last.to_owned(), value);
    }
}

/// What the schema says a key defaults to.
fn default_for(key: &str) -> Option<Default_> {
    defaults()
        .into_iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

/// Everything that happens to a gain after the proximity rules have decided it.
///
/// `Voice.tsx` lines 1584-1595, in that order and for that reason: three multipliers that
/// `calculateVoiceAudio` deliberately does not know about, because they are the listener's
/// preferences rather than the game's rules.
///
/// `None` means silence — the peer is not placed at all, which is cheaper than mixing
/// nothing and is what the original's `gain = 0` amounts to.
///
/// # The order is not decoration
///
/// Mute wins over the per-player volume, because a muted player with a volume above one
/// would otherwise come back. `crewVolumeAsGhost` is applied before `masterVolume` because
/// the master is the last word on everything; swapping them changes nothing while both are
/// at their defaults and changes the result the moment either is not.
///
/// `crewVolumeAsGhost` is the listener's, and applies only when **they** are dead and the
/// speaker is not: it is how loud the living crew is to a ghost. `ghostVolumeAsImpostor` is
/// a different setting entirely and belongs to the rules, where `voice_params` already has
/// it.
#[must_use]
pub fn after_the_rules(config: &Config, listener: Listener, gain: f32) -> Option<f32> {
    let gain = per_player_gain(config, listener.speaker_name_hash, gain)?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "percentages from the settings file, which the schema bounds at 100"
    )]
    let scaled = {
        let ghost = if listener.is_dead && !listener.speaker_is_dead {
            config.number_at("crewVolumeAsGhost") / 100.0
        } else {
            1.0
        };
        gain * (ghost * (config.number_at("masterVolume") / 100.0)) as f32
    };
    (scaled > 0.0).then_some(scaled)
}

/// Who is listening to whom, for the three multipliers above.
///
/// Named fields because two of the three are booleans about *different people*, and
/// `after_the_rules(config, true, false, gain)` is a call nobody can read.
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    /// The speaker's name hash, which their volume and mute are stored under.
    pub speaker_name_hash: i32,
    /// Whether the person listening is dead.
    pub is_dead: bool,
    /// Whether the person being listened to is dead.
    pub speaker_is_dead: bool,
}

/// One player's own volume and mute, applied to a gain.
///
/// `None` means they are muted and nothing should be placed for them at all — a peer left
/// out of the map is a peer the mixer does not mix, which is cheaper than mixing silence.
///
/// # Keyed on the name hash, which is 1.x's choice and the right one
///
/// `Voice.tsx` lines 1584-1590. Client and socket ids change every session; turning
/// somebody down is a decision about a *person*, and it has to survive them reconnecting.
/// The hash is signed, because it is JavaScript's `hashCode` ending in `| 0`, so the key
/// can carry a minus sign and the formatting must not lose it.
///
/// The order is theirs too: mute to zero first, multiply second. It reads as the same
/// answer either way and is not — a muted player with a volume of 2 would come back.
#[must_use]
pub fn per_player_gain(config: &Config, name_hash: i32, gain: f32) -> Option<f32> {
    let key = format!("playerConfigMap.{name_hash}");
    if config.bool_at(&format!("{key}.isMuted")) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a volume multiplier from the settings file, which the UI bounds"
    )]
    let volume = config
        .get(&format!("{key}.volume"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0) as f32;
    Some(gain * volume)
}

#[cfg(test)]
mod tests {

    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    /// A custom platform's `execute` comes back as it is, and nothing else does.
    ///
    /// `execute[0]` becomes a path. A number turned into a string is a path that does not
    /// exist, and a launch button that fails on one is worse than one that has no program
    /// to run.
    #[test]
    fn a_list_of_strings_drops_what_is_not_one() {
        let config = Config::read(
            r#"{"customPlatforms":{"Mine":{"execute":["Among Us.exe","--windowed",7,null]}}}"#,
        );
        assert_eq!(
            config.strings_at("customPlatforms.Mine.execute"),
            vec!["Among Us.exe".to_owned(), "--windowed".to_owned()]
        );
        // Absent, and not an array, are both empty rather than an invented default.
        assert!(config.strings_at("customPlatforms.None.execute").is_empty());
        assert!(config.strings_at("customPlatforms").is_empty());
    }

    /// The master volume reaches the gain at all, which it did not until 2026-08-27.
    ///
    /// It and `crewVolumeAsGhost` were named in a comment explaining why `voice_params`
    /// does not take them, and applied nowhere. Both are settings with sliders in the
    /// window, and both did nothing.
    #[test]
    fn the_master_volume_is_applied() {
        let mut config = Config::default();
        config.set("masterVolume", json!(50.0));
        let listener = super::Listener {
            speaker_name_hash: 1,
            is_dead: false,
            speaker_is_dead: false,
        };
        assert_eq!(super::after_the_rules(&config, listener, 1.0), Some(0.5));
    }

    /// A ghost hears the living crew at their own setting, and only then.
    #[test]
    fn a_ghost_hears_the_living_at_their_own_volume() {
        let mut config = Config::default();
        config.set("crewVolumeAsGhost", json!(20.0));

        let ghost_hearing_living = super::Listener {
            speaker_name_hash: 1,
            is_dead: true,
            speaker_is_dead: false,
        };
        assert_eq!(
            super::after_the_rules(&config, ghost_hearing_living, 1.0),
            Some(0.2)
        );

        // Two ghosts hear each other normally: this is about the living being quieter to
        // the dead, not about the dead being quiet.
        let ghost_hearing_ghost = super::Listener {
            speaker_is_dead: true,
            ..ghost_hearing_living
        };
        assert_eq!(
            super::after_the_rules(&config, ghost_hearing_ghost, 1.0),
            Some(1.0)
        );

        // And the living hear each other normally.
        let living = super::Listener {
            is_dead: false,
            ..ghost_hearing_living
        };
        assert_eq!(super::after_the_rules(&config, living, 1.0), Some(1.0));
    }

    /// The master volume at zero is silence, not a gain of zero.
    ///
    /// A peer left out of the map is a peer the mixer does not mix, which is what the
    /// original's `gain = 0` amounts to and is cheaper than mixing nothing.
    #[test]
    fn a_master_volume_of_zero_places_nobody() {
        let mut config = Config::default();
        config.set("masterVolume", json!(0.0));
        let listener = super::Listener {
            speaker_name_hash: 1,
            is_dead: false,
            speaker_is_dead: false,
        };
        assert_eq!(super::after_the_rules(&config, listener, 1.0), None);
    }

    /// All three multiply, in `Voice.tsx`'s order.
    #[test]
    fn the_three_multipliers_compose() {
        let mut config = Config::default();
        config.set("playerConfigMap.7", json!({"volume": 0.5}));
        config.set("crewVolumeAsGhost", json!(50.0));
        config.set("masterVolume", json!(50.0));
        let listener = super::Listener {
            speaker_name_hash: 7,
            is_dead: true,
            speaker_is_dead: false,
        };
        // 1.0 * 0.5 * 0.5 * 0.5
        assert_eq!(super::after_the_rules(&config, listener, 1.0), Some(0.125));
    }

    /// A player nobody touched sounds exactly as the rules left them.
    #[test]
    fn an_untouched_player_keeps_their_gain() {
        let config = Config::default();
        assert_eq!(
            super::per_player_gain(&config, 1_741_422_841, 0.5),
            Some(0.5)
        );
    }

    /// Turning somebody down multiplies what the rules decided.
    #[test]
    fn a_volume_multiplies_rather_than_replaces() {
        let mut config = Config::default();
        config.set("playerConfigMap.1741422841", json!({"volume": 0.5}));
        assert_eq!(
            super::per_player_gain(&config, 1_741_422_841, 0.8),
            Some(0.4)
        );
    }

    /// Muting is not a volume of zero: nothing is placed for them at all.
    ///
    /// And it wins over the volume, which is `Voice.tsx`'s order. Reading it the other way
    /// round is the same answer until somebody has a volume above one, at which point a
    /// muted player comes back.
    #[test]
    fn muting_beats_whatever_the_volume_says() {
        let mut config = Config::default();
        config.set(
            "playerConfigMap.1741422841",
            json!({"volume": 2.0, "isMuted": true}),
        );
        assert_eq!(super::per_player_gain(&config, 1_741_422_841, 0.8), None);
    }

    /// A negative hash is a key like any other.
    ///
    /// `hashCode` ends in `| 0`, so half of them are negative. A formatter that lost the
    /// sign would quietly apply one player's settings to a different player.
    #[test]
    fn a_negative_name_hash_addresses_its_own_player() {
        let mut config = Config::default();
        config.set("playerConfigMap.-42", json!({"volume": 0.25}));
        assert_eq!(super::per_player_gain(&config, -42, 1.0), Some(0.25));
        assert_eq!(super::per_player_gain(&config, 42, 1.0), Some(1.0));
    }

    use super::Config;
    use serde_json::{Value, json};

    /// The load-bearing one. A 1.x knows settings this build does not, and rewriting the
    /// document as a typed struct would drop every one of them — silently, and only for
    /// people who ran both clients, which is everybody during the rollout.
    #[test]
    fn a_setting_this_build_does_not_know_survives_a_write() {
        let mut config = Config::read(r#"{"alwaysOnTop":true,"somethingNewer":{"a":1}}"#);
        config.set("alwaysOnTop", json!(false));
        let written = config.write();
        assert!(
            written.contains("somethingNewer"),
            "an unknown setting was dropped: {written}"
        );
        assert!(!Config::read(&written).bool_at("alwaysOnTop"));
    }

    /// A key that is not in the file reads as what the schema says, which is what makes a
    /// fresh installation work before anybody opens the settings.
    #[test]
    fn a_missing_key_reads_as_its_default() {
        let config = Config::new();
        // From `settings::defaults`, which is itself checked against `SettingsStore.tsx`.
        assert!(!config.bool_at("alwaysOnTop"));
        // Read out of the schema rather than written down twice: this test would
        // otherwise pass while disagreeing with the file it is checking.
        let expected = crate::settings::defaults()
            .into_iter()
            .find_map(|(name, value)| match (name, value) {
                ("serverURL", crate::settings::Default_::Text(text)) => Some(text.to_owned()),
                _ => None,
            })
            .expect("the schema has a default server");
        assert_eq!(config.text_at("serverURL"), expected);
    }

    /// A file damaged by a half-finished write is not a reason to refuse to start: every
    /// value in it has a default, so a fresh document loses preferences and nothing else.
    #[test]
    fn a_file_that_will_not_parse_reads_as_a_fresh_one() {
        for text in ["", "{", "null", "[1,2,3]", "not json"] {
            let config = Config::read(text);
            assert_eq!(config, Config::new(), "{text:?} should have read as empty");
            assert!(!config.bool_at("alwaysOnTop"));
        }
    }

    /// `electron-store` treats a dot as a path, and the shipped client relies on it:
    /// `setSetting("playerConfigMap.<hash>", ...)` means a nested object, not a key with a
    /// dot in its name.
    #[test]
    fn a_dotted_key_is_a_path_and_not_a_name() {
        let mut config = Config::new();
        config.set("playerConfigMap.1741422841", json!({"volume": 0.5}));
        assert_eq!(
            config.get("playerConfigMap.1741422841.volume"),
            Some(&json!(0.5))
        );
        let written = config.write();
        assert!(
            written.contains("\"playerConfigMap\""),
            "the path should have nested: {written}"
        );
        assert!(!written.contains("playerConfigMap.1741422841"));
    }

    /// Setting a second key under the same path keeps the first, which is what a per-player
    /// volume map is for.
    #[test]
    fn two_keys_under_one_path_both_stay() {
        let mut config = Config::new();
        config.set("playerConfigMap.a", json!({"volume": 1}));
        config.set("playerConfigMap.b", json!({"volume": 2}));
        assert_eq!(config.get("playerConfigMap.a.volume"), Some(&json!(1)));
        assert_eq!(config.get("playerConfigMap.b.volume"), Some(&json!(2)));
    }

    /// A step that is in the way is replaced rather than refused. Refusing would leave a
    /// client unable to save one setting because of the state of another.
    #[test]
    fn a_scalar_in_the_way_of_a_path_is_replaced() {
        let mut config = Config::read(r#"{"playerConfigMap":7}"#);
        config.set("playerConfigMap.a", json!(1));
        assert_eq!(config.get("playerConfigMap.a"), Some(&json!(1)));
    }

    /// Tabs, because that is what `electron-store` writes. Two spaces would work and would
    /// show the whole file as changed every time the other client touched it.
    #[test]
    fn the_file_is_indented_the_way_the_other_client_indents_it() {
        let mut config = Config::new();
        config.set("alwaysOnTop", json!(true));
        let written = config.write();
        assert!(
            written.contains("\n\t\"alwaysOnTop\""),
            "expected a tab-indented document, got {written:?}"
        );
    }

    /// What the round trip does *not* preserve, said out loud.
    ///
    /// `serde_json::Map` is a `BTreeMap` unless the `preserve_order` feature is on, so a
    /// document written here comes back alphabetised while `electron-store` writes in
    /// insertion order. Nothing breaks — key order is not meaning in JSON — but it does mean
    /// the first write from this client reorders the file, and somebody comparing two
    /// versions of it sees every line move once.
    ///
    /// Left as it is rather than fixed with `indexmap`: a dependency for the sake of a
    /// one-time diff is a poor trade, and this test is here so the next person to notice
    /// finds the answer instead of the question.
    #[test]
    fn the_round_trip_alphabetises_and_that_is_known() {
        let config = Config::read("{\"zebra\":1,\"alpha\":2}");
        let written = config.write();
        assert!(
            written.find("alpha") < written.find("zebra"),
            "expected alphabetical order, got {written}"
        );
    }

    /// A value of the wrong type reads as the default rather than as zero or as a panic.
    /// The file is written by another program and edited by hand more often than anybody
    /// would like.
    #[test]
    fn a_value_of_the_wrong_type_falls_back_to_the_default() {
        let config = Config::read(r#"{"alwaysOnTop":"yes please","micVolume":"loud"}"#);
        assert!(!config.bool_at("alwaysOnTop"));
        assert!(config.get("micVolume").is_some_and(Value::is_string));
    }
}
