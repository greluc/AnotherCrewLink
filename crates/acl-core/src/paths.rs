//! Where the client's files are, which is not where a Rust program would put them.
//!
//! §4.7 lists "paths" in one word. The word hides a constraint: P8 migrates the 1.x
//! configuration forward, and §4.10's rollout has the 2.0 build read it while 1.x is
//! still installed and still using it. So these are not this program's paths to choose —
//! they are Electron's, and the job is to reproduce them exactly.
//!
//! **This is why there is no `directories` crate here.** `ProjectDirs::from("me",
//! "greluc", "AnotherCrewLink")` is what a Rust program would reach for, and on Windows
//! it resolves to `%APPDATA%\greluc\AnotherCrewLink\config` — a correct, idiomatic,
//! empty directory. The migration would read nothing, find nothing, and report success:
//! every player would start 2.0 with default settings, a default server, and no
//! shortcuts, and nothing anywhere would look like an error.
//!
//! Electron's rule is `app.getPath('userData')`, which is the platform's application-data
//! directory joined with the application's name — `productName` from `package.json`, not
//! `name`. On Windows that is `%APPDATA%`.
//!
//! There was a `Platform` enum here until 2026-08-25, with a `Unix` arm resolving
//! `$XDG_CONFIG_HOME` and falling back to `$HOME/.config`, and the enum existed so both
//! rules were testable from either host. One rule needs no enum to choose between.
//!
//! **Confirmed against a real installation on 2026-08-25**, not only against the source
//! that produces it: `%APPDATA%\AnotherCrewLink` holds `config.json`, `lookup.json`,
//! `offsets/`, `logs/` and `static/generated`, and 2.x reads exactly one of those.
//!
//! **2.x keeps its own files in `%APPDATA%\ACL`**, and that is §4.9 item 4 rather than a
//! preference. The two clients are installed side by side for a whole release cycle, and
//! "neither may silently rewrite the other's settings" cannot hold while they share a
//! `config.json`. See [`import`] for the one thing that crosses between them.

use std::path::{Path, PathBuf};

/// Where 2.x keeps its files.
///
/// **Not 1.x's directory, and that is §4.9 item 4.** The two clients are installed side by
/// side for a whole release cycle, and "neither may silently rewrite the other's settings"
/// is not achievable while they share a `config.json` — this one would alphabetise the
/// file on its first write, which is a diff in every line of something 1.x owns.
///
/// A sibling rather than a child of [`LEGACY_APP_DIRECTORY`]: 1.x's uninstaller removes
/// its own tree, and a 2.x that lived inside it would lose every setting the moment
/// somebody tidied up. No space in the name, because it is about to appear in an NSIS
/// script and in shortcut targets, and one missed pair of quotes there is a broken
/// installer.
pub const APP_DIRECTORY: &str = "ACL";

/// Where 1.x keeps its files, which this client reads and never writes.
///
/// Electron appends `productName` from `package.json`, so this is that name.
/// `product_name_still_matches_package_json` reads the manifest and fails if the two
/// diverge — a rename there would leave this pointing at where 1.x's files used to be,
/// with no error anywhere, only defaults.
pub const LEGACY_APP_DIRECTORY: &str = "AnotherCrewLink";

/// Why the directory could not be worked out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PathsError {
    /// `%APPDATA%` was not set.
    ///
    /// Not a case to paper over with a relative path: writing the configuration into the
    /// working directory would scatter it wherever the client happened to be launched
    /// from, and the next launch would find none of it.
    #[error("no application-data directory: APPDATA is not set")]
    NoHome,
}

/// What the environment supplies, so the rule can be tested without one.
#[derive(Clone, Copy, Debug, Default)]
pub struct Environment<'a> {
    /// `%APPDATA%`.
    ///
    /// An empty variable counts as unset, for the reason the XDG arm used to give: a
    /// shell that exports it empty otherwise puts the configuration in a filesystem root.
    pub app_data: Option<&'a str>,
}

/// Every location the client reads or writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    user_data: PathBuf,
    /// 1.x's directory, when the environment named somewhere to look for it.
    ///
    /// Read-only, always: see [`APP_DIRECTORY`]. Held rather than recomputed so that
    /// [`Paths::at`] can be given both in a test.
    legacy: Option<PathBuf>,
}

impl Paths {
    /// Works out the layout from an environment.
    ///
    /// # Errors
    ///
    /// [`PathsError::NoHome`] when nothing in the environment names a place to put files.
    pub fn resolve(environment: Environment<'_>) -> Result<Self, PathsError> {
        let base = environment
            .app_data
            .filter(|app_data| !app_data.is_empty())
            .ok_or(PathsError::NoHome)?;
        Ok(Self {
            user_data: PathBuf::from(base).join(APP_DIRECTORY),
            legacy: Some(PathBuf::from(base).join(LEGACY_APP_DIRECTORY)),
        })
    }

    /// The layout for an explicit directory, for tests and for a portable install.
    #[must_use]
    pub fn at(user_data: impl Into<PathBuf>) -> Self {
        Self {
            user_data: user_data.into(),
            legacy: None,
        }
    }

    /// The same, with 1.x's directory named too.
    #[must_use]
    pub fn at_with_legacy(user_data: impl Into<PathBuf>, legacy: impl Into<PathBuf>) -> Self {
        Self {
            user_data: user_data.into(),
            legacy: Some(legacy.into()),
        }
    }

    /// 1.x's directory, if this layout knows where it is.
    ///
    /// Everything reached through it is read and never written. There is exactly one thing
    /// worth reading there — the settings, once, on first run — and one thing worth
    /// looking for: a running 1.x, which advertises itself by putting this path in the
    /// title of a message-only window.
    #[must_use]
    pub fn legacy_user_data(&self) -> Option<&Path> {
        self.legacy.as_deref()
    }

    /// 1.x's settings file, which is imported once and never written back.
    #[must_use]
    pub fn legacy_config_file(&self) -> Option<PathBuf> {
        self.legacy
            .as_ref()
            .map(|legacy| legacy.join("config.json"))
    }

    /// Electron's `userData`: everything below is relative to it.
    #[must_use]
    pub fn user_data(&self) -> &Path {
        &self.user_data
    }

    /// The settings, written by `electron-store`'s default store.
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.user_data.join("config.json")
    }

    /// The cached offsets bundle.
    #[must_use]
    pub fn offsets_file(&self) -> PathBuf {
        self.user_data.join("offsets.json")
    }

    /// The cached build-to-offsets lookup.
    #[must_use]
    pub fn lookup_file(&self) -> PathBuf {
        self.user_data.join("lookup.json")
    }

    /// Remembered window positions.
    #[must_use]
    pub fn window_state_file(&self) -> PathBuf {
        self.user_data.join("windows.json")
    }

    /// Where the log goes.
    #[must_use]
    pub fn log_directory(&self) -> PathBuf {
        self.user_data.join("logs")
    }

    /// The log itself.
    #[must_use]
    pub fn log_file(&self) -> PathBuf {
        self.log_directory().join("anothercrewlink.log")
    }

    /// The recoloured avatars the client generates on first run.
    #[must_use]
    pub fn generated_static(&self) -> PathBuf {
        self.user_data.join("static").join("generated")
    }

    /// The downloaded hat artwork, and the index that names it.
    ///
    /// Beside `generated`, under the same `static` directory, because both are pictures the
    /// client puts there rather than pictures it ships. The difference is only where they
    /// come from: one is drawn here, the other is fetched from the pinned collection.
    #[must_use]
    pub fn hat_cache(&self) -> PathBuf {
        self.user_data.join("static").join("hats")
    }
}

/// Bringing 1.x's settings forward, once.
///
/// §4.9 item 4: "read the existing `electron-store` `config.json` on first run and write it
/// forward [...] The importer reads once and **never writes back** — during the beta a user
/// runs both clients, and neither may silently rewrite the other's settings."
///
/// Never writing back is not a matter of restraint. This client alphabetises a document it
/// rewrites — `serde_json::Map` is a `BTreeMap` — so a single write into 1.x's file would
/// show as a diff in every line of something 1.x owns, and 1.x would then rewrite it back
/// on its next save. The two would take turns reformatting one file forever.
///
/// It is also why the import copies the *text* rather than a parsed document: a key this
/// build does not know survives, because nothing here has an opinion about it.
pub mod import {
    use std::path::Path;

    /// What happened.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Outcome {
        /// 1.x's settings were copied into this client's directory.
        Imported,
        /// This client already had settings, so nothing was touched.
        ///
        /// Deliberately *not* "import again": a player who changed a setting here and then
        /// opened 1.x would otherwise lose their change the next time this client started.
        AlreadyHere,
        /// There was no 1.x installation to import from.
        NothingToImport,
        /// Something could not be read or written, and it is not worth stopping for.
        ///
        /// A first run with default settings is a working client. Refusing to start over a
        /// file that may not even exist is not.
        Failed,
    }

    /// Copies 1.x's settings forward if this client has none of its own.
    ///
    /// Reads exactly one file and writes exactly one, and never the other way round.
    ///
    /// The outcome is worth looking at -- `Failed` is the one a caller should say
    /// something about -- but ignoring it is not wrong: every outcome leaves a client that
    /// starts.
    #[must_use]
    pub fn settings_forward(ours: &Path, theirs: Option<&Path>) -> Outcome {
        if ours.exists() {
            return Outcome::AlreadyHere;
        }
        let Some(theirs) = theirs.filter(|path| path.exists()) else {
            return Outcome::NothingToImport;
        };
        let Ok(text) = std::fs::read_to_string(theirs) else {
            return Outcome::Failed;
        };
        // Parsed only to check it is a settings document. A half-written file, or something
        // that is not JSON at all, is not worth carrying forward -- and copying it would
        // hand this client a `config.json` it then treats as "already here" forever.
        if !super::settings_document(&text) {
            return Outcome::Failed;
        }
        if let Some(directory) = ours.parent()
            && std::fs::create_dir_all(directory).is_err()
        {
            return Outcome::Failed;
        }
        // The text, byte for byte. A parse and a re-serialise would alphabetise the keys and
        // drop nothing -- but it would also mean this build's idea of the schema decided
        // what survived, and it does not know every key 1.x has.
        if std::fs::write(ours, text).is_err() {
            return Outcome::Failed;
        }
        Outcome::Imported
    }
}

/// Whether a document is a JSON object, which is all a settings file has to be.
fn settings_document(text: &str) -> bool {
    matches!(
        serde_json::from_str::<serde_json::Value>(text),
        Ok(serde_json::Value::Object(_))
    )
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn windows(app_data: &str) -> Paths {
        Paths::resolve(Environment {
            app_data: Some(app_data),
        })
        .unwrap()
    }

    #[test]
    fn windows_uses_appdata_joined_with_the_directory_name() {
        // A `ProjectDirs`-shaped answer would be `%APPDATA%\greluc\ACL\config`: correct,
        // idiomatic, and not where anybody's files are.
        let paths = windows(r"C:\Users\p\AppData\Roaming");
        assert_eq!(
            paths.user_data(),
            Path::new(r"C:\Users\p\AppData\Roaming").join("ACL")
        );
        // And 1.x's, which is what the settings are imported from and where a running 1.x
        // announces itself.
        assert_eq!(
            paths.legacy_user_data(),
            Some(Path::new(r"C:\Users\p\AppData\Roaming").join("AnotherCrewLink")).as_deref()
        );
    }

    #[test]
    fn nothing_in_the_environment_is_an_error_rather_than_a_relative_path() {
        // Writing into the working directory would scatter the configuration wherever the
        // client was launched from, and the next launch would find none of it.
        assert_eq!(
            Paths::resolve(Environment::default()),
            Err(PathsError::NoHome)
        );
        // An exported-but-empty APPDATA is unset, not a root directory.
        assert_eq!(
            Paths::resolve(Environment { app_data: Some("") }),
            Err(PathsError::NoHome)
        );
    }

    #[test]
    fn every_file_the_electron_client_writes_has_the_same_name_here() {
        // Names taken from the shipped client: `electron-store`'s default store is
        // `config.json`, `offsetStore.ts` names `offsets` and `lookup`, `windowState.ts`
        // names `windows`, and `logFile.ts` joins `logs/anothercrewlink.log`.
        let paths = Paths::at("/u");
        assert_eq!(paths.config_file(), Path::new("/u/config.json"));
        assert_eq!(paths.offsets_file(), Path::new("/u/offsets.json"));
        assert_eq!(paths.lookup_file(), Path::new("/u/lookup.json"));
        assert_eq!(paths.window_state_file(), Path::new("/u/windows.json"));
        assert_eq!(paths.log_directory(), Path::new("/u/logs"));
        assert_eq!(paths.log_file(), Path::new("/u/logs/anothercrewlink.log"));
        assert_eq!(paths.generated_static(), Path::new("/u/static/generated"));
        assert_eq!(
            paths.legacy_user_data(),
            None,
            "`at` names one directory and knows nothing about the other"
        );
        assert_eq!(paths.hat_cache(), Path::new("/u/static/hats"));
        assert_ne!(
            paths.hat_cache(),
            paths.generated_static(),
            "downloaded artwork and generated artwork must not share a directory"
        );
    }

    /// 2.x and 1.x are siblings, not the same place and not nested. Sharing would mean
    /// sharing `config.json`, which §4.9 item 4 forbids; nesting would put 2.x's settings
    /// inside a tree 1.x's uninstaller removes.
    #[test]
    fn the_two_versions_keep_their_files_apart() {
        let paths = Paths::resolve(Environment {
            app_data: Some(r"C:\Users\x\AppData\Roaming"),
        })
        .expect("a layout");
        let ours = paths.user_data().to_owned();
        let theirs = paths
            .legacy_user_data()
            .expect("resolve knows where 1.x is")
            .to_owned();

        assert_ne!(ours, theirs);
        assert!(!ours.starts_with(&theirs), "{ours:?} is inside {theirs:?}");
        assert!(!theirs.starts_with(&ours), "{theirs:?} is inside {ours:?}");
        assert_eq!(ours.parent(), theirs.parent(), "they are siblings");
        assert_eq!(paths.legacy_config_file(), Some(theirs.join("config.json")));
    }

    /// The directory 2.x owns has no space in it. It goes into an NSIS script and into
    /// shortcut targets, and one missed pair of quotes there is a broken installer.
    #[test]
    fn the_directory_is_safe_to_put_in_a_script() {
        assert!(!APP_DIRECTORY.contains(' '), "{APP_DIRECTORY}");
        assert!(
            APP_DIRECTORY
                .chars()
                .all(|character| character.is_ascii_alphanumeric()),
            "{APP_DIRECTORY}"
        );
    }

    #[test]
    fn product_name_still_matches_package_json() {
        // Electron appends `productName`, not `name`. A rename in the manifest moves 1.x's
        // files and leaves the *legacy* path pointing at where they used to be — and the
        // failure is silent, because an absent configuration is indistinguishable from a
        // first run. 2.x's own directory is this project's to choose and is not affected.
        let manifest = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("package.json"),
        )
        .expect("the workspace manifest is beside the crates");
        let parsed: serde_json::Value =
            serde_json::from_str(&manifest).expect("package.json parses");
        assert_eq!(
            parsed["productName"].as_str(),
            Some(LEGACY_APP_DIRECTORY),
            "productName moved; 1.x's files are no longer where LEGACY_APP_DIRECTORY points"
        );
    }

    mod import {
        use super::super::import::{Outcome, settings_forward};
        use std::path::PathBuf;

        fn scratch(name: &str) -> PathBuf {
            let directory = std::env::temp_dir().join("acl-import-test").join(name);
            let _ = std::fs::remove_dir_all(&directory);
            std::fs::create_dir_all(&directory).expect("a scratch directory");
            directory
        }

        /// The ordinary first run: 1.x has settings, this client has none.
        #[test]
        fn a_first_run_brings_the_old_settings_forward() {
            let root = scratch("first-run");
            let theirs = root.join("AnotherCrewLink").join("config.json");
            std::fs::create_dir_all(theirs.parent().expect("a parent")).expect("a directory");
            std::fs::write(&theirs, "{\n\t\"alwaysOnTop\": true\n}").expect("their settings");
            let ours = root.join("ACL").join("config.json");

            assert_eq!(settings_forward(&ours, Some(&theirs)), Outcome::Imported);
            assert_eq!(
                std::fs::read_to_string(&ours).expect("ours now"),
                "{\n\t\"alwaysOnTop\": true\n}",
                "the text was not copied byte for byte"
            );
        }

        /// A key this build has never heard of comes across, because the file is copied as
        /// text. A parse and a re-serialise would let this build's schema decide what
        /// survives, and it does not know every key 1.x has.
        #[test]
        fn a_setting_this_build_does_not_know_comes_across() {
            let root = scratch("unknown-key");
            let theirs = root.join("old").join("config.json");
            std::fs::create_dir_all(theirs.parent().expect("a parent")).expect("a directory");
            std::fs::write(&theirs, r#"{"somethingNewer":{"a":1},"zebra":2,"alpha":3}"#)
                .expect("their settings");
            let ours = root.join("new").join("config.json");

            assert_eq!(settings_forward(&ours, Some(&theirs)), Outcome::Imported);
            let carried = std::fs::read_to_string(&ours).expect("ours now");
            assert!(carried.contains("somethingNewer"), "{carried}");
            assert!(
                carried.find("zebra") < carried.find("alpha"),
                "the keys were reordered on the way: {carried}"
            );
        }

        /// **1.x's file is never written.** The whole of item 4 is here: the two clients
        /// share a machine for a release cycle, and this one alphabetises what it rewrites.
        #[test]
        fn the_old_file_is_never_touched() {
            let root = scratch("read-only");
            let theirs = root.join("old").join("config.json");
            std::fs::create_dir_all(theirs.parent().expect("a parent")).expect("a directory");
            let original = r#"{"zebra":1,"alpha":2}"#;
            std::fs::write(&theirs, original).expect("their settings");
            let before = std::fs::metadata(&theirs)
                .expect("metadata")
                .modified()
                .ok();

            let ours = root.join("new").join("config.json");
            // Twice: the second call takes the `AlreadyHere` path, which must not write
            // either.
            let _ = settings_forward(&ours, Some(&theirs));
            let _ = settings_forward(&ours, Some(&theirs));

            assert_eq!(
                std::fs::read_to_string(&theirs).expect("theirs still"),
                original,
                "1.x's settings were rewritten"
            );
            assert_eq!(
                std::fs::metadata(&theirs)
                    .expect("metadata")
                    .modified()
                    .ok(),
                before,
                "1.x's settings were opened for writing"
            );
        }

        /// A second run imports nothing. Otherwise a player who changed a setting here and
        /// then opened 1.x would lose their change the next time this client started.
        #[test]
        fn a_second_run_keeps_what_this_client_has() {
            let root = scratch("second-run");
            let theirs = root.join("old").join("config.json");
            std::fs::create_dir_all(theirs.parent().expect("a parent")).expect("a directory");
            std::fs::write(&theirs, r#"{"micVolume":1}"#).expect("their settings");
            let ours = root.join("new").join("config.json");
            std::fs::create_dir_all(ours.parent().expect("a parent")).expect("a directory");
            std::fs::write(&ours, r#"{"micVolume":2}"#).expect("our settings");

            assert_eq!(settings_forward(&ours, Some(&theirs)), Outcome::AlreadyHere);
            assert_eq!(
                std::fs::read_to_string(&ours).expect("ours still"),
                r#"{"micVolume":2}"#
            );
        }

        /// No 1.x is the common case for a new user, and it is not a failure.
        #[test]
        fn no_old_installation_is_not_a_failure() {
            let root = scratch("fresh");
            let ours = root.join("new").join("config.json");
            assert_eq!(settings_forward(&ours, None), Outcome::NothingToImport);
            assert_eq!(
                settings_forward(&ours, Some(&root.join("nowhere").join("config.json"))),
                Outcome::NothingToImport
            );
            assert!(!ours.exists(), "an empty file was left behind");
        }

        /// A file that is not a settings document is not carried forward. Copying it would
        /// hand this client a `config.json` it then treats as "already here" forever, so a
        /// half-written file at the wrong moment would cost the import permanently.
        #[test]
        fn a_file_that_is_not_settings_is_not_carried_forward() {
            let root = scratch("garbage");
            let theirs = root.join("old").join("config.json");
            std::fs::create_dir_all(theirs.parent().expect("a parent")).expect("a directory");
            let ours = root.join("new").join("config.json");

            for text in ["", "{", "null", "[1,2,3]", "not json"] {
                let _ = std::fs::remove_file(&ours);
                std::fs::write(&theirs, text).expect("their settings");
                assert_eq!(
                    settings_forward(&ours, Some(&theirs)),
                    Outcome::Failed,
                    "{text:?} was imported"
                );
                assert!(!ours.exists(), "{text:?} left a file behind");
            }
        }
    }
}
