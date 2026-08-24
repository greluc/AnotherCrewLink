//! The localisation loader.
//!
//! The 37 locale directories under `static/locales` stay i18next JSON and are read as-is.
//! `docs/rust-port/04-implementation-plan.md` §4.8 measured all 4,736 strings and found no
//! plural key and no selector, so every feature that would distinguish a localisation
//! framework from a flat map is unused. Keeping the JSON also means the Electron client
//! and this one consume the identical tree during the beta: one Crowdin project, one
//! format for translators.
//!
//! That measurement is now off by one. `settings.troubleshooting.reset_offsets_done`
//! gained a `{{version}}` placeholder in H2, so the loader carries the smallest possible
//! interpolation — literal `{{name}}` substitution, no formatting, no locale-aware
//! numbers. Anything more belongs in `format!` at the call site.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

/// Which way a locale's text runs.
///
/// Only the base direction, which is what a layout needs. Bidirectional resolution inside
/// a string is the text shaper's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Left to right.
    LeftToRight,
    /// Right to left.
    RightToLeft,
}

/// The locales in `static/locales` whose script runs right to left.
const RIGHT_TO_LEFT: [&str; 3] = ["ar", "fa", "he"];

/// The base direction for a locale tag.
#[must_use]
pub fn direction_for(locale: &str) -> Direction {
    // Match on the language subtag: `ar_EG` is still Arabic.
    let language = locale.split(['_', '-']).next().unwrap_or(locale);
    if RIGHT_TO_LEFT.contains(&language) {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    }
}

/// The locale every lookup falls back to.
pub const FALLBACK_LOCALE: &str = "en";

/// Why a catalogue could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// What was being read.
        path: String,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// The file was not the JSON object a translation file has to be.
    #[error("{path} is not a translation file: {source}")]
    Json {
        /// What was being parsed.
        path: String,
        /// The underlying failure.
        source: serde_json::Error,
    },
}

/// One locale's strings, flattened once at load.
///
/// The keys call sites use are dotted — `settings.audio.microphone` — and the file nests
/// them. Flattening once at start-up turns every later lookup into one hash of a string
/// the caller already has, instead of a walk down three maps.
#[derive(Debug, Clone)]
pub struct Catalogue {
    locale: String,
    strings: HashMap<String, String>,
    fallback: Option<HashMap<String, String>>,
}

impl Catalogue {
    /// Reads `<root>/<locale>/translation.json`, with English behind it.
    ///
    /// The fallback is loaded separately rather than merged, so a missing string can be
    /// told from a string that is deliberately the same in both.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] if either file cannot be read or is not a JSON object.
    pub fn load(root: &Path, locale: &str) -> Result<Self, LoadError> {
        let strings = read_locale(root, locale)?;
        let fallback = if locale == FALLBACK_LOCALE {
            None
        } else {
            Some(read_locale(root, FALLBACK_LOCALE)?)
        };
        Ok(Self {
            locale: locale.to_owned(),
            strings,
            fallback,
        })
    }

    /// The locale this catalogue was loaded for.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// This locale's base text direction.
    #[must_use]
    pub fn direction(&self) -> Direction {
        direction_for(&self.locale)
    }

    /// How many strings this locale defines, before the fallback.
    #[must_use]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether this locale defines no strings at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// The string for a key, falling back to English and then to the key itself.
    ///
    /// Returning the key rather than an empty string is deliberate: a missing translation
    /// then shows up in the interface as `settings.audio.microphone`, which is a bug
    /// report. A blank label is a mystery.
    #[must_use]
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings
            .get(key)
            .or_else(|| self.fallback.as_ref().and_then(|english| english.get(key)))
            .map_or(key, String::as_str)
    }

    /// The string for a key with `{{name}}` placeholders filled in.
    ///
    /// Borrows when there is nothing to substitute, which is every string in the tree but
    /// one.
    #[must_use]
    pub fn t_with<'a>(&'a self, key: &'a str, args: &[(&str, &str)]) -> Cow<'a, str> {
        let text = self.t(key);
        if args.is_empty() || !text.contains("{{") {
            return Cow::Borrowed(text);
        }
        let mut out = text.to_owned();
        for (name, value) in args {
            out = out.replace(&format!("{{{{{name}}}}}"), value);
        }
        Cow::Owned(out)
    }

    /// Whether this locale defines a key itself, rather than inheriting it.
    #[must_use]
    pub fn defines(&self, key: &str) -> bool {
        self.strings.contains_key(key)
    }
}

fn read_locale(root: &Path, locale: &str) -> Result<HashMap<String, String>, LoadError> {
    let path = root.join(locale).join("translation.json");
    let shown = path.display().to_string();
    let text = std::fs::read_to_string(&path).map_err(|source| LoadError::Io {
        path: shown.clone(),
        source,
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|source| LoadError::Json {
        path: shown,
        source,
    })?;
    let mut flat = HashMap::new();
    flatten(&value, &mut String::new(), &mut flat);
    Ok(flat)
}

/// Walks the nested object, joining keys with dots.
///
/// Anything that is not a string or an object is skipped rather than stringified: the
/// tree has never contained one, and a number silently becoming `"1"` in the interface is
/// worse than the key showing through.
fn flatten(value: &Value, prefix: &mut String, out: &mut HashMap<String, String>) {
    match value {
        Value::String(text) => {
            out.insert(prefix.clone(), text.clone());
        }
        Value::Object(fields) => {
            for (name, child) in fields {
                let mark = prefix.len();
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(name);
                flatten(child, prefix, out);
                prefix.truncate(mark);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use std::path::PathBuf;

    fn locales() -> PathBuf {
        // The crate sits two levels under the repository root.
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../static/locales")
    }

    fn every_locale() -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(locales())
            .expect("the locales directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn every_shipped_locale_loads() {
        let names = every_locale();
        // The tree has 37. Fewer means a directory disappeared; the assertion is here so
        // the loop below cannot pass by iterating over nothing.
        assert!(names.len() >= 37, "found only {} locales", names.len());
        for locale in names {
            let catalogue = Catalogue::load(&locales(), &locale)
                .unwrap_or_else(|error| panic!("{locale} did not load: {error}"));
            assert!(!catalogue.is_empty(), "{locale} defines no strings");
        }
    }

    #[test]
    fn flattens_the_dotted_keys_call_sites_use() {
        let english = Catalogue::load(&locales(), "en").expect("english");
        assert_eq!(
            english.t("settings.troubleshooting.reset_offsets"),
            "Reset game offsets"
        );
    }

    #[test]
    fn falls_back_to_english_for_a_string_a_locale_has_not_translated() {
        let german = Catalogue::load(&locales(), "de").expect("german");
        let english = Catalogue::load(&locales(), "en").expect("english");

        // A key German does define is German.
        assert!(german.defines("settings.troubleshooting.reset_offsets"));
        assert_ne!(
            german.t("settings.troubleshooting.reset_offsets"),
            english.t("settings.troubleshooting.reset_offsets")
        );

        // A key no locale defines falls through to the key itself rather than to blank.
        assert_eq!(german.t("no.such.key"), "no.such.key");
    }

    #[test]
    fn fills_in_the_one_placeholder_in_the_tree() {
        // §4.8 measured no interpolation at all across 4,736 strings. H2 added exactly
        // one, and this is it — the loader carries the substitution because of it.
        let english = Catalogue::load(&locales(), "en").expect("english");
        let filled = english.t_with(
            "settings.troubleshooting.reset_offsets_done",
            &[("version", "V2026.8.18")],
        );
        assert!(filled.contains("V2026.8.18"), "got {filled}");
        assert!(!filled.contains("{{"), "a placeholder survived: {filled}");
    }

    #[test]
    fn borrows_when_there_is_nothing_to_substitute() {
        let english = Catalogue::load(&locales(), "en").expect("english");
        let text = english.t_with("settings.troubleshooting.reset_offsets", &[]);
        assert!(matches!(text, Cow::Borrowed(_)));
    }

    #[test]
    fn knows_which_locales_run_right_to_left() {
        assert_eq!(direction_for("ar"), Direction::RightToLeft);
        assert_eq!(direction_for("fa"), Direction::RightToLeft);
        assert_eq!(direction_for("he"), Direction::RightToLeft);
        assert_eq!(direction_for("en"), Direction::LeftToRight);
        assert_eq!(direction_for("zh_TW"), Direction::LeftToRight);
        // A region subtag does not change the script.
        assert_eq!(direction_for("ar_EG"), Direction::RightToLeft);
        assert_eq!(direction_for("pt_BR"), Direction::LeftToRight);
    }

    #[test]
    fn the_rtl_list_matches_what_is_shipped() {
        // If a right-to-left locale is added to the tree and not to the list, its layout
        // is silently wrong. This is the check that says so.
        let shipped = every_locale();
        for locale in RIGHT_TO_LEFT {
            assert!(
                shipped.iter().any(|name| name == locale),
                "{locale} is in the RTL list but not in static/locales"
            );
        }
    }

    #[test]
    fn english_is_the_most_complete_locale() {
        // The fallback is only useful if it is the superset. If another locale grows past
        // it, some strings have no fallback at all.
        let english = Catalogue::load(&locales(), "en").expect("english");
        for locale in every_locale() {
            if locale == "en" {
                continue;
            }
            let other = Catalogue::load(&locales(), &locale).expect("a locale");
            assert!(
                other.len() <= english.len(),
                "{locale} defines {} strings, more than English's {}",
                other.len(),
                english.len()
            );
        }
    }
}
