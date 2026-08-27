//! `latest.yml`: the file that moves the installed fleet.
//!
//! §4.12 item 1. The bridge is published into the 1.x feed as **1.1.0**, and what the
//! installed `electron-updater` actually reads is this one small YAML document. Everything
//! about the migration goes through it, which makes it the highest-blast-radius file this
//! project produces: every machine running 1.x polls it, and whatever it names is what they
//! run.
//!
//! It is generated here rather than by `electron-builder` because the bridge is built by
//! the Rust pipeline, and a hand-written one is a hand-written one either way. Generated
//! means tested.
//!
//! # The mechanism, read out of the installed `electron-updater`
//!
//! §4.12 states it and this reproduces it: "`latest.yml` supplies version, path and
//! SHA-512; `findFile` picks by extension and then prefers a filename containing `x64` or
//! `ia32`; `NsisUpdater` spawns the installer with `--updated /S /D=<installDirectory>`."
//!
//! # Two things here are easy to get wrong and are why this has tests
//!
//! **The digest is base64, not hex.** `acl_updater::manifest` carries the same SHA-512 as
//! 128 hex characters, because that is what a person reading a manifest can compare.
//! `electron-updater` decodes base64 and compares bytes, and a hex digest in this field is
//! a digest that never matches — every client refusing the update, quietly, with a
//! checksum error nobody sees.
//!
//! **There is no `.blockmap`.** §4.12 item 2: "No `.blockmap` asset for the bridge, or the
//! updater attempts a differential download against a file that is not there." Naming one
//! that does not exist is worse than naming none, and `electron-builder` names one by
//! default — so this is an omission that has to be deliberate and stay deliberate.

use base64::Engine as _;

/// What a release announces to the 1.x fleet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyRelease {
    /// The version, as 1.x will compare it.
    ///
    /// The bridge is `1.1.0`: a 1.x version, because a 1.x updater will only take a version
    /// it considers newer than what it is running.
    pub version: String,
    /// The installer's file name, which is also its URL relative to the release.
    ///
    /// The name 1.x has always published under. `findFile` picks by extension and then
    /// prefers a name containing `x64` or `ia32`; 1.x published no token and one `.exe`, so
    /// any single `.exe` keeps being picked.
    pub file_name: String,
    /// The installer's SHA-512, raw bytes.
    ///
    /// Bytes rather than a string, so the encoding is decided in one place — here — and
    /// cannot arrive already wrong.
    pub sha512: Vec<u8>,
    /// How large the installer is.
    pub size: u64,
    /// When it was released, in the shape 1.x writes.
    ///
    /// Passed in rather than read from the clock: this is generated in a release job whose
    /// output should be a function of its inputs, and a timestamp taken here is one that
    /// differs between two builds of the same commit.
    pub released: String,
}

impl LegacyRelease {
    /// Writes the document.
    ///
    /// By hand rather than through a YAML serialiser. The shape is fixed, four of its five
    /// values are ours, and the fifth is base64 — there is nothing here a serialiser would
    /// get right that this gets wrong, and a dependency whose job is to emit six lines is a
    /// dependency in the highest-blast-radius file the project has.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        let digest = base64::engine::general_purpose::STANDARD.encode(&self.sha512);
        // `files` and the top-level `path`/`sha512` both, because the installed fleet spans
        // `electron-updater` versions and the older ones read the top-level pair while the
        // newer ones read `files`. Writing one of the two is choosing which half of the
        // fleet updates.
        format!(
            "version: {version}\n\
             files:\n\
             \x20 - url: {file}\n\
             \x20   sha512: {digest}\n\
             \x20   size: {size}\n\
             path: {file}\n\
             sha512: {digest}\n\
             releaseDate: '{released}'\n",
            version = self.version,
            file = self.file_name,
            digest = digest,
            size = self.size,
            released = self.released,
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::LegacyRelease;

    /// The document electron-builder actually published for 1.0.5, reproduced.
    ///
    /// This is the strongest check available without a fleet. `test/fixtures/latest-1.0.5.yml`
    /// is the real file from the v1.0.5 release — written by the tool this generator replaces,
    /// read by the updaters this generator has to satisfy. If the two agree byte for byte on the
    /// same inputs, the format is right for reasons that have nothing to do with what anybody
    /// here believed about it.
    ///
    /// It also settles `there_is_no_blockmap` from the other direction: electron-builder's own
    /// 1.0.5 feed has no blockmap entry either, so omitting one is matching it rather than
    /// departing from it.
    #[test]
    fn it_reproduces_the_feed_electron_builder_published() {
        use base64::Engine as _;

        let real = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../test/fixtures/latest-1.0.5.yml"),
        )
        .expect("the vendored 1.0.5 feed");

        // Read back out of the fixture rather than retyped, so this cannot drift from it.
        let digest = real
            .lines()
            .find_map(|line| line.strip_prefix("sha512: "))
            .expect("a top-level digest");
        let size = real
            .lines()
            .find_map(|line| line.trim().strip_prefix("size: "))
            .expect("a size")
            .parse()
            .expect("a number");
        let released = real
            .lines()
            .find_map(|line| line.strip_prefix("releaseDate: '")?.strip_suffix('\''))
            .expect("a release date");

        let ours = LegacyRelease {
            version: "1.0.5".to_owned(),
            file_name: "AnotherCrewLink-Setup-1.0.5.exe".to_owned(),
            sha512: base64::engine::general_purpose::STANDARD
                .decode(digest)
                .expect("the digest is base64"),
            size,
            released: released.to_owned(),
        }
        .to_yaml();

        assert_eq!(
            ours, real,
            "the generated feed differs from the one the fleet was actually served"
        );
    }

    fn release() -> LegacyRelease {
        use sha2::Digest as _;

        LegacyRelease {
            version: "1.1.0".to_owned(),
            file_name: "AnotherCrewLink-Setup-1.1.0.exe".to_owned(),
            sha512: sha2::Sha512::digest(b"an installer").to_vec(),
            size: 12,
            released: "2026-08-26T12:00:00.000Z".to_owned(),
        }
    }

    /// **The digest is base64.** `electron-updater` decodes it and compares bytes; a hex
    /// digest here is one that never matches, and every client refuses the update quietly
    /// with a checksum error nobody sees.
    ///
    /// This is the field most likely to be filled in by somebody copying it from
    /// `acl_updater::manifest`, which is deliberately hex.
    #[test]
    fn the_digest_is_base64_and_not_hex() {
        use base64::Engine as _;

        let release = release();
        let yaml = release.to_yaml();
        let expected = base64::engine::general_purpose::STANDARD.encode(&release.sha512);
        assert!(yaml.contains(&expected), "{yaml}");
        assert!(
            expected.ends_with('=') || expected.len().is_multiple_of(4),
            "base64 is padded: {expected}"
        );

        let hex = release.sha512.iter().fold(String::new(), |mut text, byte| {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        });
        assert!(
            !yaml.contains(&hex),
            "the hex digest reached latest.yml, and no client would accept it"
        );
    }

    /// No `.blockmap`, ever. §4.12 item 2: naming one that does not exist makes the updater
    /// attempt a differential download against a file that is not there.
    ///
    /// `electron-builder` names one by default, so this is an omission that has to be
    /// deliberate — and stay deliberate.
    #[test]
    fn there_is_no_blockmap() {
        assert!(
            !release().to_yaml().contains("blockmap"),
            "a blockmap was named, and the file does not exist"
        );
    }

    /// Both the `files` list and the top-level pair. The installed fleet spans
    /// `electron-updater` versions: older ones read `path` and `sha512` at the top level,
    /// newer ones read `files`. Writing one of the two is choosing which half of the fleet
    /// updates.
    #[test]
    fn both_shapes_are_written_because_the_fleet_spans_versions() {
        let yaml = release().to_yaml();
        assert!(yaml.contains("files:"), "{yaml}");
        assert!(yaml.contains("  - url: "), "{yaml}");
        assert!(
            yaml.lines().any(|line| line.starts_with("path: ")),
            "no top-level path: {yaml}"
        );
        assert!(
            yaml.lines().any(|line| line.starts_with("sha512: ")),
            "no top-level sha512: {yaml}"
        );
    }

    /// The document parses as YAML, and as the YAML `electron-updater` expects. Written by
    /// hand, so this is the check that hand-writing did not produce something only a human
    /// can read.
    #[test]
    fn it_parses_as_the_document_the_updater_reads() {
        let release = release();
        let parsed: serde_yaml::Value =
            serde_yaml::from_str(&release.to_yaml()).expect("valid YAML");

        assert_eq!(parsed["version"].as_str(), Some(release.version.as_str()));
        assert_eq!(parsed["path"].as_str(), Some(release.file_name.as_str()));
        assert_eq!(parsed["size"].as_u64(), None, "size is not top-level");

        let files = parsed["files"].as_sequence().expect("a files list");
        assert_eq!(files.len(), 1, "one artefact, as 1.x has always published");
        assert_eq!(files[0]["url"].as_str(), Some(release.file_name.as_str()));
        assert_eq!(files[0]["size"].as_u64(), Some(release.size));
        assert_eq!(
            files[0]["sha512"].as_str(),
            parsed["sha512"].as_str(),
            "the two digests disagree, so half the fleet would refuse"
        );
    }

    /// The release date is quoted. Unquoted, YAML parses it as a timestamp and
    /// `electron-updater` receives something that is not the string it compares.
    #[test]
    fn the_release_date_is_a_string() {
        let yaml = release().to_yaml();
        assert!(
            yaml.contains("releaseDate: '2026-08-26T12:00:00.000Z'"),
            "{yaml}"
        );
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("valid YAML");
        assert_eq!(
            parsed["releaseDate"].as_str(),
            Some("2026-08-26T12:00:00.000Z")
        );
    }

    /// The name is the one 1.x has always published under, because `findFile` picks by
    /// extension and then by name. A `.msi` or a renamed `.exe` is the same act as
    /// abandoning the installed base.
    #[test]
    fn the_artefact_is_one_exe_under_the_old_name() {
        let yaml = release().to_yaml();
        assert!(yaml.contains("AnotherCrewLink-Setup-1.1.0.exe"), "{yaml}");
        assert_eq!(
            yaml.matches(".exe").count(),
            2,
            "one name, written twice -- in `files` and at the top level"
        );
    }

    /// The version is a 1.x one. A 1.x updater only takes what it considers newer than what
    /// it is running, and `2.0.0` compared against `1.0.2` is newer — but a bridge announced
    /// as 2.0.0 is a bridge that claims to be the release it is bridging to.
    #[test]
    fn the_bridge_announces_itself_as_a_one_x_version() {
        let release = release();
        assert!(
            release.version.starts_with("1."),
            "the bridge is {}, which is not a 1.x version",
            release.version
        );
        let bridge = semver::Version::parse(&release.version).expect("a version");
        let installed = semver::Version::parse("1.0.2").expect("a version");
        assert!(bridge > installed, "the fleet would not take it");
    }
}
