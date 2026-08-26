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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

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
