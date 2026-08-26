//! Getting the manifest and the artefact.
//!
//! The other half of §4.9 item 3: the half with side effects. [`crate::manifest`] decides
//! whether a release is ours and [`crate::policy`] decides whether to install it; this is
//! what puts bytes in front of them.
//!
//! Shaped like `acl_game::store` and `acl_client::hat_store` before it — a trait, so the
//! logic is testable without a network, and one implementation over `ureq` behind an
//! `http` feature. Three of these now, and the shape has earned it.
//!
//! # The order is the whole design
//!
//! Manifest, then signature, then policy, then artefact, then digest. Nothing is written
//! to disk until every one of those has passed, and the artefact is not even *fetched*
//! until the policy has said yes — a client that downloaded eighty megabytes and then
//! discovered it was a downgrade would have spent somebody's data allowance proving a
//! point.

use crate::manifest::{Manifest, ManifestError};

/// Somewhere to fetch from.
pub trait Fetch {
    /// Fetches a document.
    ///
    /// # Errors
    ///
    /// A message describing why, which reaches the user only through a log line.
    fn text(&self, url: &str) -> Result<String, String>;

    /// Fetches a file.
    ///
    /// # Errors
    ///
    /// As above.
    fn bytes(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// How long any one request may take.
///
/// Longer than the hat store's, because this one may be fetching an installer over a
/// domestic connection, and shorter than forever, because a hung release server must not
/// leave a client waiting at start-up.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// The real one.
#[derive(Debug, Default, Clone)]
pub struct Http;

#[cfg(feature = "http")]
impl Fetch for Http {
    fn text(&self, url: &str) -> Result<String, String> {
        let mut response = agent().get(url).call().map_err(|error| error.to_string())?;
        response
            .body_mut()
            .read_to_string()
            .map_err(|error| format!("{url}: {error}"))
    }

    fn bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut response = agent().get(url).call().map_err(|error| error.to_string())?;
        response
            .body_mut()
            // Capped. An artefact this project publishes is tens of megabytes; a body that
            // is not is either a mistake or somebody filling a disk, and the manifest's
            // `size` is checked against it afterwards anyway.
            .with_config()
            .limit(512 * 1024 * 1024)
            .read_to_vec()
            .map_err(|error| format!("{url}: {error}"))
    }
}

#[cfg(feature = "http")]
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .new_agent()
}

#[cfg(not(feature = "http"))]
impl Fetch for Http {
    fn text(&self, _url: &str) -> Result<String, String> {
        Err("this build has no HTTP client; enable the `http` feature".to_owned())
    }

    fn bytes(&self, _url: &str) -> Result<Vec<u8>, String> {
        Err("this build has no HTTP client; enable the `http` feature".to_owned())
    }
}

/// Why an update could not be obtained.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The manifest, its signature, or the artefact could not be fetched.
    #[error("{0}")]
    Unreachable(String),
    /// The manifest is not one this build accepts.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// The artefact is not the one the manifest names.
    ///
    /// The interesting failure. A digest that does not match means the bytes are not what
    /// was signed for — a corrupted download, a cache serving something stale, or somebody
    /// substituting a file. Nothing here tries to tell those apart, and nothing should:
    /// they are all "do not run this".
    #[error("the download is not the artefact the manifest names")]
    NotTheArtefact,
}

/// Where a release announces itself.
///
/// The manifest and its detached signature, side by side, which is minisign's own
/// convention: `x` and `x.minisig`.
#[derive(Clone, Copy, Debug)]
pub struct Feed<'a> {
    /// The manifest's URL.
    pub manifest: &'a str,
}

impl Feed<'_> {
    /// Where the signature is.
    ///
    /// Derived rather than carried, so a feed cannot name a manifest in one place and a
    /// signature somewhere else — which would let whoever controls the manifest choose
    /// which signature it is checked against.
    #[must_use]
    pub fn signature(&self) -> String {
        format!("{}.minisig", self.manifest)
    }
}

/// Fetches the manifest and verifies it.
///
/// # Errors
///
/// [`FetchError`] when it cannot be had, or is not ours.
pub fn manifest(feed: Feed<'_>, fetch: &dyn Fetch) -> Result<Manifest, FetchError> {
    let document = fetch
        .bytes(feed.manifest)
        .map_err(FetchError::Unreachable)?;
    let signature = fetch
        .text(&feed.signature())
        .map_err(FetchError::Unreachable)?;
    Ok(Manifest::verified(&document, &signature)?)
}

/// Fetches the artefact a manifest names, and checks it is the one.
///
/// # Errors
///
/// [`FetchError::Unreachable`] if it cannot be fetched, [`FetchError::NotTheArtefact`] if
/// what arrives is not what the manifest describes.
pub fn artefact(manifest: &Manifest, fetch: &dyn Fetch) -> Result<Vec<u8>, FetchError> {
    let bytes = fetch
        .bytes(&manifest.url)
        .map_err(FetchError::Unreachable)?;
    if !manifest.matches(&bytes) {
        return Err(FetchError::NotTheArtefact);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Feed, Fetch, FetchError, artefact};
    use crate::manifest::Manifest;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Fake {
        answers: BTreeMap<String, Vec<u8>>,
        asked: RefCell<Vec<String>>,
    }

    impl Fetch for Fake {
        fn text(&self, url: &str) -> Result<String, String> {
            self.bytes(url)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        }

        fn bytes(&self, url: &str) -> Result<Vec<u8>, String> {
            self.asked.borrow_mut().push(url.to_owned());
            self.answers
                .get(url)
                .cloned()
                .ok_or_else(|| format!("{url}: nothing there"))
        }
    }

    fn manifest_for(artefact: &[u8]) -> Manifest {
        use sha2::Digest as _;

        let digest = sha2::Sha512::digest(artefact);
        let sha512 = digest.iter().fold(String::new(), |mut text, byte| {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        });
        Manifest {
            version: semver::Version::new(2, 0, 1),
            url: "https://example.invalid/setup.exe".to_owned(),
            sha512,
            size: artefact.len() as u64,
        }
    }

    /// The signature's URL is derived from the manifest's, not carried in it. A feed that
    /// could name both would let whoever controls the manifest choose which signature it
    /// is checked against, which is the same as having no signature.
    #[test]
    fn the_signature_sits_beside_the_manifest() {
        let feed = Feed {
            manifest: "https://example.invalid/release.json",
        };
        assert_eq!(
            feed.signature(),
            "https://example.invalid/release.json.minisig"
        );
    }

    /// The artefact is checked against the manifest that named it.
    #[test]
    fn an_artefact_that_matches_is_returned() {
        let bytes = b"an installer, for the sake of argument".to_vec();
        let manifest = manifest_for(&bytes);
        let fetch = Fake {
            answers: [(manifest.url.clone(), bytes.clone())]
                .into_iter()
                .collect(),
            ..Fake::default()
        };
        assert_eq!(artefact(&manifest, &fetch).expect("it matches"), bytes);
    }

    /// And one that does not is refused, whatever the reason. A corrupted download, a stale
    /// cache and a substituted file are all "do not run this", and nothing here tries to
    /// tell them apart.
    #[test]
    fn an_artefact_that_does_not_match_is_refused() {
        let manifest = manifest_for(b"the real one");
        let fetch = Fake {
            answers: [(manifest.url.clone(), b"something else entirely".to_vec())]
                .into_iter()
                .collect(),
            ..Fake::default()
        };
        assert_eq!(
            artefact(&manifest, &fetch).unwrap_err(),
            FetchError::NotTheArtefact
        );
    }

    /// A download of the right length and the wrong content is the case worth having a test
    /// for: the length check is the cheap one and it passes here.
    #[test]
    fn the_right_length_is_not_enough() {
        let manifest = manifest_for(b"the real one");
        let fetch = Fake {
            answers: [(manifest.url.clone(), b"the fake one".to_vec())]
                .into_iter()
                .collect(),
            ..Fake::default()
        };
        assert_eq!(b"the real one".len(), b"the fake one".len());
        assert_eq!(
            artefact(&manifest, &fetch).unwrap_err(),
            FetchError::NotTheArtefact
        );
    }

    /// Nothing reachable is not the same as nothing valid, and the message says which.
    #[test]
    fn a_server_that_is_not_there_says_so() {
        let manifest = manifest_for(b"x");
        let fetch = Fake::default();
        assert!(matches!(
            artefact(&manifest, &fetch).unwrap_err(),
            FetchError::Unreachable(_)
        ));
    }

    /// The manifest cannot be verified by this build, because it trusts no keys yet — so
    /// `manifest` fails at the signature rather than at the network, and the artefact is
    /// never asked for.
    #[test]
    fn nothing_is_downloaded_when_the_manifest_cannot_be_trusted() {
        let feed = Feed {
            manifest: "https://example.invalid/release.json",
        };
        let fetch = Fake {
            answers: [
                (feed.manifest.to_owned(), b"{}".to_vec()),
                (feed.signature(), b"not a signature".to_vec()),
            ]
            .into_iter()
            .collect(),
            ..Fake::default()
        };
        assert!(super::manifest(feed, &fetch).is_err());
        assert!(
            !fetch
                .asked
                .borrow()
                .iter()
                .any(|url| url.ends_with("setup.exe")),
            "an artefact was fetched for a manifest that was never accepted"
        );
    }
}
