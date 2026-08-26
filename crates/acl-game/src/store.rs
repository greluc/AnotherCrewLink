//! Where the offsets come from, and what stands between them and the reader.
//!
//! Three sources, tried in order: the mirror, the cache in `userData`, and the floor
//! compiled into this binary. **Every one of them is validated.** The bundle carries no
//! signature, so the validator and the floor are the whole of the check — and a cache hit
//! is exactly the path that would otherwise never be examined again after the first
//! download.
//!
//! # On the network
//!
//! `ureq` with `platform-verifier`, blocking, and deliberately so. It is the caller's job
//! to drive this from `spawn_blocking`, which is a feature rather than a compromise: an
//! update check then cannot stall the runtime the voice path shares.

use std::path::{Path, PathBuf};
// Only the retry policy uses it, and that is behind the HTTP client.
#[cfg(feature = "http")]
use std::time::Duration;

use crate::offsets::{BundleContext, Lookup, Offsets, Rejected};

/// The mirror. Ours, pinned to a branch we control and review.
pub const PRIMARY_MIRROR: &str =
    "https://raw.githubusercontent.com/greluc/AnotherCrewlink-Offsets/main";
/// The fallback, reached when the primary cannot be.
///
/// jsDelivr caches a branch reference for up to twelve hours, so this can be stale. The
/// mirror repository purges it on every push for exactly that reason.
pub const FALLBACK_MIRROR: &str = "https://cdn.jsdelivr.net/gh/greluc/AnotherCrewlink-Offsets@main";

/// How long one request may take before it is abandoned.
#[cfg(feature = "http")]
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// How many times both hosts are tried.
#[cfg(feature = "http")]
const ROUNDS: u32 = 3;
/// How long to wait between rounds, multiplied by the round number.
#[cfg(feature = "http")]
const RETRY_DELAY: Duration = Duration::from_millis(1500);

/// The floor, compiled in. See `assets/README.md`.
const EMBEDDED_LOOKUP: &str = include_str!("../assets/lookup.json");
const EMBEDDED_X64: &str = include_str!("../assets/offsets-x64.json");
const EMBEDDED_X86: &str = include_str!("../assets/offsets-x86.json");

/// The game build the embedded floor describes.
pub const EMBEDDED_GAME_VERSION: &str = "V2026.8.18";
/// The offsets file the embedded lookup's default entry names.
pub const EMBEDDED_OFFSETS_FILE: &str = "V2026.8.18/offsets.json";

/// Where a bundle came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Downloaded just now.
    Mirror,
    /// From the cache in `userData`.
    Cache,
    /// From the copy compiled into this binary.
    Embedded,
}

/// Why a bundle could not be had at all.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Nothing could be reached and nothing was cached.
    #[error("no offsets are available: {0}")]
    Unavailable(String),
    /// Even the embedded floor did not validate, which is a broken build.
    #[error("the embedded offsets are not valid: {0}")]
    BrokenFloor(#[from] Rejected),
    /// The bundle is not JSON.
    #[error("the offsets are not readable: {0}")]
    Malformed(#[from] serde_json::Error),
}

/// What was loaded, and where it came from.
#[derive(Debug, Clone)]
pub struct Loaded<T> {
    /// The bundle.
    pub value: T,
    /// Which of the three sources answered.
    pub source: Source,
    /// Why the mirror was not used, when it was not.
    ///
    /// A client quietly running a two-year-old embedded bundle looks identical to one
    /// that is up to date, and the difference only surfaces as "why can nobody hear me on
    /// the new map".
    pub reason: Option<String>,
}

/// Something that can fetch a path from a mirror.
///
/// A trait so the cache and floor logic is testable without a network, which is most of
/// what there is to get wrong here.
pub trait Fetcher {
    /// Fetches one path, relative to a mirror root.
    ///
    /// # Errors
    ///
    /// Returns a message describing why, which reaches the user only through a log line.
    fn fetch(&self, path: &str) -> Result<String, String>;
}

/// The real one: two mirrors, three rounds, a timeout on every request.
///
/// Rate limiting is why the retry exists rather than a single attempt.
/// `raw.githubusercontent.com` limits per address, so a household where several people
/// start the app at once, or anyone behind a shared address, is turned away — and before
/// the retry existed that was the end of it, with the user told to check their internet
/// connection.
#[derive(Debug, Default, Clone)]
pub struct HttpFetcher;

#[cfg(feature = "http")]
impl Fetcher for HttpFetcher {
    fn fetch(&self, path: &str) -> Result<String, String> {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .new_agent();

        let mut last = String::from("no attempt was made");
        for round in 0..ROUNDS {
            for base in [PRIMARY_MIRROR, FALLBACK_MIRROR] {
                let url = format!("{base}/{path}");
                match agent.get(&url).call() {
                    Ok(mut response) => match response.body_mut().read_to_string() {
                        Ok(body) => return Ok(body),
                        Err(error) => last = format!("{url}: {error}"),
                    },
                    // A rate-limit page or a 404 body must not be parsed as if it were the
                    // data, which is what happened before the status was checked.
                    Err(error) => last = format!("{url}: {error}"),
                }
            }
            if round + 1 < ROUNDS {
                std::thread::sleep(RETRY_DELAY * (round + 1));
            }
        }
        Err(last)
    }
}

#[cfg(not(feature = "http"))]
impl Fetcher for HttpFetcher {
    fn fetch(&self, _path: &str) -> Result<String, String> {
        Err("this build has no HTTP client; enable the `http` feature".to_owned())
    }
}

/// The three sources, in order, with the cache on disk.
#[derive(Debug, Clone)]
pub struct OffsetStore {
    cache: PathBuf,
    client_version: String,
}

impl OffsetStore {
    /// A store that caches under `directory`.
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>, client_version: impl Into<String>) -> Self {
        Self {
            cache: directory.into(),
            client_version: client_version.into(),
        }
    }

    /// The lookup, from the mirror if it can be reached and believed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::BrokenFloor`] if even the compiled-in copy does not validate,
    /// which means this build shipped broken and there is nothing left to fall back to.
    pub fn load_lookup(&self, fetcher: &dyn Fetcher) -> Result<Loaded<Lookup>, StoreError> {
        let held = self
            .cached_lookup()
            .ok()
            .and_then(|lookup| lookup.bundle_version);
        let context = BundleContext {
            client_version: self.client_version.clone(),
            held_bundle_version: held,
        };

        let mut reason;
        match fetcher.fetch("lookup.json") {
            Ok(body) => match parse_and_validate_lookup(&body, &context) {
                Ok(lookup) => {
                    self.write_cache("lookup.json", &body);
                    return Ok(Loaded {
                        value: lookup,
                        source: Source::Mirror,
                        reason: None,
                    });
                }
                Err(error) => reason = Some(error.to_string()),
            },
            Err(error) => reason = Some(error),
        }

        // The cache is validated too. It lives where anything running as this user can
        // edit it, so checking only at download time would examine the one copy an
        // attacker has least reason to touch.
        match self.cached_lookup() {
            Ok(lookup) => {
                return Ok(Loaded {
                    value: lookup,
                    source: Source::Cache,
                    reason,
                });
            }
            Err(Some(error)) => {
                // A cache that no longer validates is discarded rather than repaired.
                let _ = std::fs::remove_file(self.cache.join("lookup.json"));
                reason = Some(error.to_string());
            }
            Err(None) => {}
        }

        Ok(Loaded {
            value: parse_and_validate_lookup(EMBEDDED_LOOKUP, &context)?,
            source: Source::Embedded,
            reason: reason.or_else(|| Some("nothing was cached".to_owned())),
        })
    }

    /// One offsets file, by the path the lookup names.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Unavailable`] when the file cannot be had and the floor does
    /// not describe the build being asked for — a player on an older Among Us with an
    /// unreachable mirror gets an honest error rather than offsets for a different game.
    pub fn load_offsets(
        &self,
        fetcher: &dyn Fetcher,
        is_64bit: bool,
        file: &str,
    ) -> Result<Loaded<Offsets>, StoreError> {
        let arch = if is_64bit { "x64" } else { "x86" };
        let path = format!("offsets/{arch}/{file}");
        let cache_name = format!("offsets-{arch}-{}", file.replace(['/', '\\'], "_"));

        let mut reason;
        match fetcher.fetch(&path) {
            Ok(body) => match parse_and_validate_offsets(&body) {
                Ok(offsets) => {
                    self.write_cache(&cache_name, &body);
                    return Ok(Loaded {
                        value: offsets,
                        source: Source::Mirror,
                        reason: None,
                    });
                }
                Err(error) => reason = Some(error.to_string()),
            },
            Err(error) => reason = Some(error),
        }

        if let Ok(body) = std::fs::read_to_string(self.cache.join(&cache_name)) {
            match parse_and_validate_offsets(&body) {
                Ok(offsets) => {
                    return Ok(Loaded {
                        value: offsets,
                        source: Source::Cache,
                        reason,
                    });
                }
                Err(error) => {
                    let _ = std::fs::remove_file(self.cache.join(&cache_name));
                    reason = Some(error.to_string());
                }
            }
        }

        if file == EMBEDDED_OFFSETS_FILE {
            let body = if is_64bit { EMBEDDED_X64 } else { EMBEDDED_X86 };
            return Ok(Loaded {
                value: parse_and_validate_offsets(body)?,
                source: Source::Embedded,
                reason: reason.or_else(|| Some("nothing was cached".to_owned())),
            });
        }

        Err(StoreError::Unavailable(reason.unwrap_or_else(|| {
            format!("{file} is not the build the floor describes")
        })))
    }

    /// Drops both caches, so the next load reads the floor and then the mirror again.
    ///
    /// The manual recovery path. Without a signature there is no signed floor to supersede
    /// a bad bundle from, so this is what a user has instead of revocation.
    pub fn reset(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.cache) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "lookup.json" || name.starts_with("offsets-") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    /// `Err(Some(_))` means there was a cache and it was rejected; `Err(None)` means there
    /// was none.
    fn cached_lookup(&self) -> Result<Lookup, Option<Rejected>> {
        let Ok(body) = std::fs::read_to_string(self.cache.join("lookup.json")) else {
            return Err(None);
        };
        // No replay check against itself: the cache *is* the held version.
        let context = BundleContext {
            client_version: self.client_version.clone(),
            held_bundle_version: None,
        };
        match parse_and_validate_lookup(&body, &context) {
            Ok(lookup) => Ok(lookup),
            Err(StoreError::BrokenFloor(rejected)) => Err(Some(rejected)),
            Err(_) => Err(Some(Rejected {
                code: crate::offsets::Rejection::WrongType,
                path: "cached lookup".to_owned(),
                detail: "the cached file is not JSON".to_owned(),
            })),
        }
    }

    fn write_cache(&self, name: &str, body: &str) {
        if std::fs::create_dir_all(&self.cache).is_err() {
            return;
        }
        let _ = std::fs::write(self.cache.join(name), body);
    }
}

fn parse_and_validate_lookup(body: &str, context: &BundleContext) -> Result<Lookup, StoreError> {
    let lookup: Lookup = serde_json::from_str(body)?;
    lookup.validate(context)?;
    Ok(lookup)
}

fn parse_and_validate_offsets(body: &str) -> Result<Offsets, StoreError> {
    let offsets: Offsets = serde_json::from_str(body)?;
    offsets.validate()?;
    Ok(offsets)
}

/// Whether a path is inside the cache directory. For callers that show it to a user.
#[must_use]
pub fn is_cache_file(directory: &Path, candidate: &Path) -> bool {
    candidate.starts_with(directory)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use std::cell::RefCell;

    /// A fetcher that answers from a script, and records what was asked for.
    struct Scripted {
        answers: RefCell<Vec<Result<String, String>>>,
        asked: RefCell<Vec<String>>,
    }

    impl Scripted {
        fn new(answers: Vec<Result<String, String>>) -> Self {
            Self {
                answers: RefCell::new(answers),
                asked: RefCell::new(Vec::new()),
            }
        }
    }

    impl Fetcher for Scripted {
        fn fetch(&self, path: &str) -> Result<String, String> {
            self.asked.borrow_mut().push(path.to_owned());
            let mut answers = self.answers.borrow_mut();
            if answers.is_empty() {
                Err("no more scripted answers".to_owned())
            } else {
                answers.remove(0)
            }
        }
    }

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("acl-store-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory");
        path
    }

    fn store(name: &str) -> OffsetStore {
        OffsetStore::new(temporary(name), "1.0.3")
    }

    #[test]
    fn the_embedded_floor_validates() {
        // A build that ships a malformed floor should fail its own tests rather than its
        // users. This is that test.
        let store = store("floor");
        let offline = Scripted::new(vec![Err("offline".to_owned())]);

        let lookup = store.load_lookup(&offline).expect("the floor loads");
        assert_eq!(lookup.source, Source::Embedded);
        assert!(lookup.reason.is_some(), "it has to say why");
        assert!(lookup.value.versions.contains_key("default"));

        for is_64 in [true, false] {
            let offline = Scripted::new(vec![Err("offline".to_owned())]);
            let offsets = store
                .load_offsets(&offline, is_64, EMBEDDED_OFFSETS_FILE)
                .expect("the floor loads");
            assert_eq!(offsets.source, Source::Embedded);
        }
    }

    #[test]
    fn a_mirror_answer_is_validated_before_it_is_cached() {
        // A hostile or broken mirror answering with a 200 is what the validator exists
        // for, and it must not reach the cache.
        let store = store("bad-mirror");
        let bad = Scripted::new(vec![Ok(
            r#"{"patterns":{},"versions":{"default":{"version":"x","file":"../../etc/x.json","offsetsVersion":1}}}"#
                .to_owned(),
        )]);

        let loaded = store.load_lookup(&bad).expect("falls back");
        assert_eq!(loaded.source, Source::Embedded);
        assert!(
            !store.cache.join("lookup.json").exists(),
            "it was cached anyway"
        );
    }

    #[test]
    fn a_good_answer_is_cached_and_served_from_there_when_the_mirror_goes_away() {
        let store = store("cache");
        let body = EMBEDDED_LOOKUP.to_owned();

        let online = Scripted::new(vec![Ok(body)]);
        assert_eq!(
            store.load_lookup(&online).expect("loads").source,
            Source::Mirror
        );
        assert!(store.cache.join("lookup.json").exists());

        let offline = Scripted::new(vec![Err("offline".to_owned())]);
        let loaded = store.load_lookup(&offline).expect("loads");
        assert_eq!(loaded.source, Source::Cache);
        assert!(loaded.reason.is_some());
    }

    #[test]
    fn a_cache_edited_on_disk_is_discarded_rather_than_used() {
        // The check the plan insists on: validation at load, not only at download. The
        // cache lives where anything running as this user can edit it.
        let store = store("tampered");
        let online = Scripted::new(vec![Ok(EMBEDDED_LOOKUP.to_owned())]);
        store.load_lookup(&online).expect("loads");

        let mut tampered: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(store.cache.join("lookup.json")).unwrap(),
        )
        .unwrap();
        tampered["versions"]["default"]["file"] =
            serde_json::Value::from("https://evil.example/x.json");
        std::fs::write(
            store.cache.join("lookup.json"),
            serde_json::to_string(&tampered).unwrap(),
        )
        .unwrap();

        let offline = Scripted::new(vec![Err("offline".to_owned())]);
        let loaded = store.load_lookup(&offline).expect("falls to the floor");
        assert_eq!(loaded.source, Source::Embedded);
        // Discarded rather than carried forward on every subsequent start.
        assert!(!store.cache.join("lookup.json").exists());
    }

    #[test]
    fn refuses_rather_than_serving_the_floor_for_a_build_it_does_not_describe() {
        // Handing a player on a two-year-old Among Us the current offsets would read the
        // wrong fields and report nothing.
        let store = store("wrong-build");
        let offline = Scripted::new(vec![Err("offline".to_owned())]);
        let result = store.load_offsets(&offline, false, "V2021.3.31/offsets.json");
        assert!(matches!(result, Err(StoreError::Unavailable(_))));
    }

    #[test]
    fn asks_for_the_path_the_lookup_names_under_the_right_architecture() {
        let store = store("paths");
        let fetcher = Scripted::new(vec![Err("offline".to_owned())]);
        let _ = store.load_offsets(&fetcher, true, "V2026.8.18/offsets.json");
        assert_eq!(
            fetcher.asked.borrow().as_slice(),
            ["offsets/x64/V2026.8.18/offsets.json"]
        );
    }

    #[test]
    fn resetting_drops_the_cache_and_leaves_everything_else_alone() {
        let store = store("reset");
        let online = Scripted::new(vec![Ok(EMBEDDED_LOOKUP.to_owned())]);
        store.load_lookup(&online).expect("loads");
        std::fs::write(store.cache.join("settings.json"), "{}").unwrap();

        store.reset();
        assert!(!store.cache.join("lookup.json").exists());
        // Not a directory wipe: the cache shares a home with the user's settings.
        assert!(store.cache.join("settings.json").exists());
    }

    #[test]
    fn a_replayed_bundle_is_refused_against_what_is_cached() {
        let store = store("replay");
        let mut newer: serde_json::Value = serde_json::from_str(EMBEDDED_LOOKUP).unwrap();
        newer["bundle_version"] = serde_json::Value::from(9);
        let online = Scripted::new(vec![Ok(serde_json::to_string(&newer).unwrap())]);
        assert_eq!(
            store.load_lookup(&online).expect("loads").source,
            Source::Mirror
        );

        // The mirror was reverted, or someone replayed an old file at it.
        let mut older: serde_json::Value = serde_json::from_str(EMBEDDED_LOOKUP).unwrap();
        older["bundle_version"] = serde_json::Value::from(4);
        let replayed = Scripted::new(vec![Ok(serde_json::to_string(&older).unwrap())]);
        let loaded = store.load_lookup(&replayed).expect("loads");

        // The held bundle stays in force.
        assert_eq!(loaded.source, Source::Cache);
        assert_eq!(loaded.value.bundle_version, Some(9));
    }
}
