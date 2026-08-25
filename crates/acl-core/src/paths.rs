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
//! `offsets.json`, `windows.json`, `logs/`, `static/` and `recordings/` — every path
//! below, in that one directory. Reading the TypeScript would have given the same answer;
//! looking is what makes it a fact rather than a derivation.

use std::path::{Path, PathBuf};

/// The directory name Electron appends, taken from `productName` in `package.json`.
///
/// `product_name_still_matches_package_json` reads the manifest and fails if the two
/// diverge, because a rename would move 1.x's files and leave this pointing at where they
/// used to be — with no error anywhere, only defaults.
pub const APP_DIRECTORY: &str = "AnotherCrewLink";

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
        })
    }

    /// The layout for an explicit directory, for tests and for a portable install.
    #[must_use]
    pub fn at(user_data: impl Into<PathBuf>) -> Self {
        Self {
            user_data: user_data.into(),
        }
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
    fn windows_uses_appdata_joined_with_the_product_name() {
        // What 1.x writes, and therefore what P8's migration has to read. A
        // `ProjectDirs`-shaped answer would be `%APPDATA%\greluc\AnotherCrewLink\config`:
        // correct, idiomatic, and empty.
        let paths = windows(r"C:\Users\p\AppData\Roaming");
        assert_eq!(
            paths.user_data(),
            Path::new(r"C:\Users\p\AppData\Roaming").join("AnotherCrewLink")
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
    }

    #[test]
    fn product_name_still_matches_package_json() {
        // Electron appends `productName`, not `name`. A rename in the manifest moves 1.x's
        // files and leaves this pointing at where they used to be — and the failure is
        // silent, because an absent configuration is indistinguishable from a first run.
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
            Some(APP_DIRECTORY),
            "productName moved; 1.x's files are no longer where APP_DIRECTORY points"
        );
    }
}
