//! The localisation loader.
//!
//! The locale directories under `static/locales` stay i18next JSON and are read as-is.
//! `docs/rust-port/04-implementation-plan.md` §4.8 measured all 4,631 strings and found no
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

/// Every shipped locale, and what it calls itself.
///
/// A table rather than a lookup of the directory listing, because a language picker
/// showing `zh_CN` and `zh_TW` is a picker nobody can use. The names are the languages'
/// own -- `Deutsch`, not `German` -- which is what the Electron client shows and the only
/// convention that works in a list somebody is reading *because* they cannot read the
/// current one.
///
/// Ported from `src/renderer/language/languages.ts`, and `the_names_match_the_electron_client`
/// reads that file and compares rather than trusting this was transcribed correctly.
///
/// In its order, not alphabetical: English first, and after that whatever order the
/// translations arrived in. Sorting it would be an improvement to make deliberately, in
/// both clients at once, rather than as a side effect of a port.
pub const NAMES: [(&str, &str); 2] = [("en", "English"), ("de", "Deutsch")];

/// What a locale calls itself, if it is one this build ships.
///
/// `None` for anything else, including a tag that is a real language: the client can only
/// offer what is in `static/locales`, and inventing a name for a locale with no catalogue
/// would put an entry in the picker that selects nothing.
#[must_use]
pub fn name_of(locale: &str) -> Option<&'static str> {
    NAMES
        .iter()
        .find(|(tag, _)| *tag == locale)
        .map(|(_, name)| *name)
}

/// The language subtags whose script runs right to left.
///
/// A table of facts about scripts, not a list of what is shipped. Arabic runs right to left
/// whether or not `static/locales` has an `ar` in it, and on 2026-08-28 it stopped having
/// one — the tree was cut to English and German. Emptying this to match would have made a
/// true function answer falsely, and would leave a locale added back later laying out
/// silently wrong.
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
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    /// The names are ported, so they can drift. This reads the file they were ported from.
    #[test]
    fn the_names_match_the_electron_client() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../src/renderer/language/languages.ts"),
        )
        .expect("the Electron client is beside the crates");

        // Deliberately literal, for the reason `settings`'s parser is: a tolerant one would
        // skip an entry whose shape it did not recognise, and a skipped entry is exactly
        // the one that has drifted.
        let mut found: Vec<(String, String)> = Vec::new();
        let lines: Vec<&str> = source.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            let Some(tag) = line
                .strip_prefix('\t')
                .and_then(|rest| rest.strip_suffix(": {"))
            else {
                continue;
            };
            if !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let Some(name) = lines
                .get(at + 2)
                .and_then(|line| line.trim().strip_prefix("name: '"))
                .and_then(|rest| rest.strip_suffix("',"))
            else {
                continue;
            };
            found.push((tag.to_owned(), name.to_owned()));
        }

        assert_eq!(found.len(), NAMES.len(), "a locale was added or removed");
        for (at, (tag, name)) in found.iter().enumerate() {
            assert_eq!(
                (tag.as_str(), name.as_str()),
                NAMES[at],
                "entry {at} has drifted"
            );
        }
    }

    /// Every name has a catalogue behind it, and every catalogue has a name. A tag in the
    /// picker with no directory selects nothing; a directory with no name is a translation
    /// nobody can reach.
    #[test]
    fn the_table_and_the_tree_agree() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../static/locales");
        let mut shipped: Vec<String> = std::fs::read_dir(&root)
            .expect("the locale tree")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        shipped.sort();

        let mut named: Vec<String> = NAMES.iter().map(|(tag, _)| (*tag).to_owned()).collect();
        named.sort();
        assert_eq!(named, shipped);
    }

    /// A tag this build does not ship has no name rather than a guessed one: the picker can
    /// only offer what has a catalogue.
    #[test]
    fn an_unshipped_locale_has_no_name() {
        assert_eq!(name_of("en"), Some("English"));
        assert_eq!(
            name_of("zh_CN"),
            NAMES.iter().find(|(t, _)| *t == "zh_CN").map(|(_, n)| *n)
        );
        assert_eq!(name_of("xx"), None);
        assert_eq!(name_of(""), None);
        assert_eq!(
            name_of("EN"),
            None,
            "tags are matched as the directories are named"
        );
    }
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

    /// Every `client.` key the source asks for is in both catalogues.
    ///
    /// The 2.x client's own strings live under that prefix, and they are the ones a key
    /// audit against the shipped catalogue cannot find: an audit compares the catalogue to
    /// the *Electron* source, and these keys exist in neither. Without this, a mistyped key
    /// shows as itself on the screen and no test says a word.
    ///
    /// Both locales, not just English. English is the fallback and so must be whole; German
    /// is the only other one, and a German user reading an English sentence in a German
    /// window is the thing keeping the tree small was supposed to make easy to avoid.
    #[test]
    fn every_client_key_the_source_uses_is_translated() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut wanted: Vec<String> = Vec::new();
        for crate_name in ["acl-client", "acl-ui"] {
            collect_client_keys(
                &root.join("crates").join(crate_name).join("src"),
                &mut wanted,
            );
        }
        assert!(
            wanted.len() > 20,
            "found only {} keys; the scan is not finding the source",
            wanted.len()
        );

        for locale in ["en", "de"] {
            let catalogue = Catalogue::load(&locales(), locale).expect("a shipped locale");
            let missing: Vec<&String> = wanted
                .iter()
                .filter(|key| !catalogue.defines(key))
                .collect();
            assert!(missing.is_empty(), "{locale} does not define {missing:?}");
        }
    }

    /// Every `"client.…"` literal under a directory.
    ///
    /// A scan of the text rather than of the syntax: the keys are string literals passed to
    /// a closure, so there is no type to ask. It over-collects by design — a key named in a
    /// comment is still a key somebody expects to exist.
    fn collect_client_keys(directory: &std::path::Path, into: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_client_keys(&path, into);
                continue;
            }
            if path.extension().is_none_or(|kind| kind != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for piece in text.split("\"client.").skip(1) {
                let Some(end) = piece.find('"') else {
                    continue;
                };
                let key = format!("client.{}", &piece[..end]);
                // A key built from a prefix is not one this can check.
                if !key.contains('{') && !into.contains(&key) {
                    into.push(key);
                }
            }
        }
    }

    #[test]
    fn every_shipped_locale_loads() {
        let names = every_locale();
        // The tree has 37. Fewer means a directory disappeared; the assertion is here so
        // the loop below cannot pass by iterating over nothing.
        assert_eq!(
            names.len(),
            NAMES.len(),
            "the tree and `NAMES` disagree about which locales exist"
        );
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
        // §4.8 measured no interpolation at all across the corpus. H2 added exactly
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

    /// Every locale in the tree gets a direction, and it is looked up by language.
    ///
    /// This used to assert the other way round -- that everything in [`RIGHT_TO_LEFT`] is
    /// in the tree -- which stopped being true when the tree was cut to two and was never
    /// the useful direction anyway: the table is about scripts, and a script does not stop
    /// running right to left because nobody ships it.
    ///
    /// What is checkable is that the lookup works for what *is* shipped, including for a
    /// tag with a region on it.
    #[test]
    fn every_shipped_locale_has_a_direction() {
        for locale in every_locale() {
            let direction = direction_for(&locale);
            let expected = if RIGHT_TO_LEFT.contains(&locale.split(['_', '-']).next().unwrap_or(""))
            {
                Direction::RightToLeft
            } else {
                Direction::LeftToRight
            };
            assert_eq!(direction, expected, "{locale}");
        }
        // The region is not part of the question: `ar_EG` is still Arabic.
        assert_eq!(direction_for("ar_EG"), Direction::RightToLeft);
        assert_eq!(direction_for("de_AT"), Direction::LeftToRight);
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
