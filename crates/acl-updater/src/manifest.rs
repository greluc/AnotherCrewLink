//! The update manifest, and the signature over it.
//!
//! §4.9 item 3: the updater is built "around `minisign` verification and `self-replace` —
//! that is not writing crypto, the verification stays in a purpose-built crate". This is
//! the half that decides whether a manifest is one this project published.
//!
//! # What the signature covers, and what it does not
//!
//! It covers the manifest, and through the manifest's digest the artefact named in it. It
//! says nothing about whether Windows trusts the artefact — §4.9 item 2 ships unsigned, so
//! `SmartScreen` will warn on every install forever, and that is written down rather than
//! implied.
//!
//! # Why signed here when the offsets bundle is not
//!
//! §4.9, and the reason is availability rather than importance. A release is a planned
//! event: it happens when the maintainer decides, so a key that has to be fetched from
//! somewhere safe costs nothing. An offsets bundle is an unplanned burst that starts when
//! Among Us updates and ends when players can hear each other again, and a human holding a
//! key in that window *is* the outage.
//!
//! # It fails closed, and it is closed today
//!
//! [`PUBLIC_KEYS`] is empty. No release key exists yet — generating one is a ceremony the
//! maintainer performs offline, and inventing a placeholder here would be a key whose
//! private half is in a scratch directory somewhere. Until it is filled in, every manifest
//! is refused: `no_keys_means_no_updates` is the test, and it is a feature. An updater that
//! accepted unsigned manifests while the keys were "not done yet" is the exact shape of the
//! accident this design exists to prevent.

use minisign_verify::{PublicKey, Signature};

/// The keys a manifest may be signed by.
///
/// **Two, when they exist**, and §4.9 says which: "the operational key held offline and
/// never in a release-workflow secret". One signs releases from CI; the other never touches
/// a workflow and is what the project recovers with if the first is lost or stolen. A
/// client that trusts both can be handed a manifest signed by the second without an update
/// having to reach it first — which is the whole point, since a compromised release key is
/// exactly when updates cannot be trusted to arrive.
///
/// Empty until the ceremony happens. See the module documentation.
pub const PUBLIC_KEYS: &[&str] = &[];

/// What an update is.
///
/// Deliberately small. Every field is something the updater has to act on, and a manifest
/// that carried release notes or a channel would be a manifest with fields nothing checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// The version being offered.
    pub version: semver::Version,
    /// Where the artefact is.
    pub url: String,
    /// Its SHA-512, lower-case hex.
    ///
    /// The same digest `latest.yml` carries for the 1.x fleet, so the two paths agree about
    /// what "the same file" means.
    pub sha512: String,
    /// How large it is, in bytes.
    ///
    /// Checked before the digest is, because a download that is the wrong length is one
    /// that can be abandoned without hashing gigabytes of it.
    pub size: u64,
}

/// Why a manifest was not accepted.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// This build trusts no keys, so it can accept nothing.
    #[error("this build has no release keys, so no update can be verified")]
    NoKeys,
    /// The signature is not by any key this build trusts.
    #[error("the manifest is not signed by a key this build trusts")]
    NotOurs,
    /// The signature is not a signature.
    #[error("the signature could not be read: {0}")]
    Unreadable(String),
    /// The manifest is not a manifest.
    #[error("the manifest could not be read: {0}")]
    Malformed(String),
}

impl Manifest {
    /// Reads a manifest, and only if it is signed by a key this build trusts.
    ///
    /// There is no unverified read. A function that parsed first and checked later would be
    /// one somebody could call halfway.
    ///
    /// # Errors
    ///
    /// [`ManifestError`] when the signature is missing, wrong, or by a key this build does
    /// not know — or when what it covers is not a manifest.
    pub fn verified(document: &[u8], signature: &str) -> Result<Self, ManifestError> {
        Self::verified_with(document, signature, PUBLIC_KEYS)
    }

    /// The same, against a given set of keys.
    ///
    /// Separate so the tests can use a keypair they made a moment ago, rather than the
    /// shipped keys — which do not exist yet, and which no test should be able to sign for
    /// even when they do.
    ///
    /// # Errors
    ///
    /// As [`Manifest::verified`].
    pub fn verified_with(
        document: &[u8],
        signature: &str,
        keys: &[&str],
    ) -> Result<Self, ManifestError> {
        if keys.is_empty() {
            return Err(ManifestError::NoKeys);
        }
        let signature = Signature::decode(signature)
            .map_err(|error| ManifestError::Unreadable(error.to_string()))?;
        let trusted = keys.iter().any(|key| {
            PublicKey::from_base64(key)
                .is_ok_and(|key| key.verify(document, &signature, false).is_ok())
        });
        if !trusted {
            return Err(ManifestError::NotOurs);
        }
        Self::read(document)
    }

    /// Parses the document, once it is known to be ours.
    fn read(document: &[u8]) -> Result<Self, ManifestError> {
        let text = std::str::from_utf8(document)
            .map_err(|error| ManifestError::Malformed(error.to_string()))?;
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| ManifestError::Malformed(error.to_string()))?;
        let field = |name: &str| -> Result<&serde_json::Value, ManifestError> {
            value
                .get(name)
                .ok_or_else(|| ManifestError::Malformed(format!("no {name}")))
        };
        let text_field = |name: &str| -> Result<String, ManifestError> {
            field(name)?
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| ManifestError::Malformed(format!("{name} is not a string")))
        };

        let version = semver::Version::parse(&text_field("version")?)
            .map_err(|error| ManifestError::Malformed(format!("version: {error}")))?;
        let sha512 = text_field("sha512")?.to_lowercase();
        if sha512.len() != 128 || !sha512.chars().all(|digit| digit.is_ascii_hexdigit()) {
            return Err(ManifestError::Malformed(
                "sha512 is not 128 hex digits".to_owned(),
            ));
        }
        let size = field("size")?
            .as_u64()
            .ok_or_else(|| ManifestError::Malformed("size is not a count".to_owned()))?;
        if size == 0 {
            return Err(ManifestError::Malformed("size is zero".to_owned()));
        }
        Ok(Self {
            version,
            url: text_field("url")?,
            sha512,
            size,
        })
    }

    /// Whether some bytes are the artefact this manifest names.
    ///
    /// Length first, then the digest: a download of the wrong length can be thrown away
    /// without hashing it, and the length is the cheaper of the two lies to tell.
    #[must_use]
    pub fn matches(&self, artefact: &[u8]) -> bool {
        use sha2::Digest as _;

        if artefact.len() as u64 != self.size {
            return false;
        }
        // Compared as text rather than by decoding the manifest's hex, so a manifest with an
        // upper-case digest and one with a lower-case digest are the same manifest.
        hex(&sha2::Sha512::digest(artefact)) == self.sha512
    }
}

/// Lower-case hex, which is the shape the manifest carries a digest in.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes
        .iter()
        .fold(String::with_capacity(128), |mut text, byte| {
            // Writing into a `String` cannot fail, and there is nothing to do about it if it
            // somehow did: the comparison would simply not match.
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Manifest, ManifestError, PUBLIC_KEYS};

    /// A keypair made for this test, and a manifest signed with it.
    ///
    /// Made rather than committed, so no private key is in the repository and the test
    /// exercises a real signature rather than a recorded one.
    fn signed(document: &str) -> (String, String) {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("a keypair");
        let signature = minisign::sign(
            None,
            &pair.sk,
            std::io::Cursor::new(document.as_bytes()),
            None,
            None,
        )
        .expect("a signature")
        .to_string();
        (pair.pk.to_base64(), signature)
    }

    fn document(version: &str) -> String {
        let digest = "a".repeat(128);
        format!(
            r#"{{"version":"{version}","url":"https://example.invalid/x.exe","sha512":"{digest}","size":42}}"#
        )
    }

    /// **The shipped build trusts nothing, and that is deliberate.** No release key exists
    /// yet; a placeholder here would be a key whose private half is in a scratch directory.
    /// An updater that accepted unsigned manifests while the keys were "not done yet" is
    /// the exact accident this design exists to prevent.
    #[test]
    fn no_keys_means_no_updates() {
        assert!(
            PUBLIC_KEYS.is_empty(),
            "the ceremony happened -- update this test and the module header with it"
        );
        let (_, signature) = signed(&document("2.0.0"));
        assert_eq!(
            Manifest::verified(document("2.0.0").as_bytes(), &signature),
            Err(ManifestError::NoKeys)
        );
    }

    /// A manifest signed by a key we trust is read.
    #[test]
    fn a_manifest_we_signed_is_read() {
        let text = document("2.1.0");
        let (key, signature) = signed(&text);
        let manifest = Manifest::verified_with(text.as_bytes(), &signature, &[&key])
            .expect("our own signature");
        assert_eq!(manifest.version, semver::Version::new(2, 1, 0));
        assert_eq!(manifest.url, "https://example.invalid/x.exe");
        assert_eq!(manifest.size, 42);
    }

    /// One signed by somebody else is not, however well-formed it is.
    #[test]
    fn a_manifest_somebody_else_signed_is_refused() {
        let text = document("2.1.0");
        let (_theirs, signature) = signed(&text);
        let (ours, _) = signed("something else");
        assert_eq!(
            Manifest::verified_with(text.as_bytes(), &signature, &[&ours]),
            Err(ManifestError::NotOurs)
        );
    }

    /// And a manifest changed after it was signed is not. This is the whole point: the
    /// digest and the URL are inside what the signature covers.
    #[test]
    fn a_manifest_edited_after_signing_is_refused() {
        let text = document("2.1.0");
        let (key, signature) = signed(&text);
        let tampered = text.replace("https://example.invalid", "https://example.test");
        assert_eq!(
            Manifest::verified_with(tampered.as_bytes(), &signature, &[&key]),
            Err(ManifestError::NotOurs)
        );
    }

    /// Either key may sign. §4.9 wants two so that a lost release key does not need an
    /// update to reach the fleet before it can be replaced -- which is impossible, because
    /// a lost release key is exactly when updates cannot be trusted.
    #[test]
    fn either_of_the_two_keys_may_sign() {
        let text = document("2.1.0");
        let (operational, _) = signed("nothing in particular");
        let (release, signature) = signed(&text);
        for keys in [
            vec![release.clone(), operational.clone()],
            vec![operational, release],
        ] {
            let borrowed: Vec<&str> = keys.iter().map(String::as_str).collect();
            assert!(
                Manifest::verified_with(text.as_bytes(), &signature, &borrowed).is_ok(),
                "the order of the keys decided the answer"
            );
        }
    }

    /// A signature that is not one is refused as unreadable rather than as somebody else's,
    /// because the two are different problems and only one of them is an attack.
    #[test]
    fn something_that_is_not_a_signature_says_so() {
        let (key, _) = signed("x");
        assert!(matches!(
            Manifest::verified_with(b"{}", "not a signature", &[&key]),
            Err(ManifestError::Unreadable(_))
        ));
    }

    /// Every field is checked, because a manifest that verified and then failed to parse is
    /// a manifest this project signed and got wrong.
    #[test]
    fn a_manifest_missing_anything_is_refused() {
        let digest = "a".repeat(128);
        for broken in [
            r#"{"url":"u","sha512":"","size":1}"#.to_owned(),
            format!(r#"{{"version":"2.0.0","sha512":"{digest}","size":1}}"#),
            r#"{"version":"2.0.0","url":"u","size":1}"#.to_owned(),
            format!(r#"{{"version":"2.0.0","url":"u","sha512":"{digest}"}}"#),
            format!(r#"{{"version":"two","url":"u","sha512":"{digest}","size":1}}"#),
            r#"{"version":"2.0.0","url":"u","sha512":"tooshort","size":1}"#.to_owned(),
            format!(r#"{{"version":"2.0.0","url":"u","sha512":"{digest}","size":0}}"#),
            "not json at all".to_owned(),
        ] {
            let (key, signature) = signed(&broken);
            assert!(
                matches!(
                    Manifest::verified_with(broken.as_bytes(), &signature, &[&key]),
                    Err(ManifestError::Malformed(_))
                ),
                "{broken} was accepted"
            );
        }
    }

    /// The digest is compared case-insensitively, so a manifest written with upper-case hex
    /// is the same manifest.
    #[test]
    fn the_digest_is_the_same_digest_in_either_case() {
        use sha2::Digest as _;

        let artefact = b"an installer, for the sake of argument";
        let digest = super::hex(&sha2::Sha512::digest(artefact)).to_uppercase();
        let text = format!(
            r#"{{"version":"2.0.0","url":"u","sha512":"{digest}","size":{}}}"#,
            artefact.len()
        );
        let (key, signature) = signed(&text);
        let manifest = Manifest::verified_with(text.as_bytes(), &signature, &[&key]).expect("ours");
        assert!(manifest.matches(artefact));
    }

    /// A download of the wrong length or the wrong content is not the artefact.
    #[test]
    fn the_wrong_bytes_are_not_the_artefact() {
        use sha2::Digest as _;

        let artefact = b"the real one";
        let digest = super::hex(&sha2::Sha512::digest(artefact));
        let text = format!(
            r#"{{"version":"2.0.0","url":"u","sha512":"{digest}","size":{}}}"#,
            artefact.len()
        );
        let (key, signature) = signed(&text);
        let manifest = Manifest::verified_with(text.as_bytes(), &signature, &[&key]).expect("ours");

        assert!(manifest.matches(artefact));
        assert!(
            !manifest.matches(b"the wrong one"),
            "same length, other bytes"
        );
        assert!(!manifest.matches(b"short"), "wrong length");
        assert!(!manifest.matches(b""), "nothing at all");
    }
}
