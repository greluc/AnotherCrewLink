//! Whether to install what the manifest offers.
//!
//! Verification says a manifest is ours. This says whether we act on it, and the three
//! rules are §4.9 item 3's: "rollback protection that the user can bypass, because the
//! 2.0→1.x downgrade path is documented; no freeze rule, which is a fleet-wide time bomb
//! dependent on the user's clock. Never install an update while elevated."
//!
//! Each of those is a sentence about a failure somebody has had.
//!
//! **Rollback protection, bypassable.** An attacker who can serve an old *signed* manifest
//! can otherwise walk a client back to a version with a known hole, using nothing but
//! files this project really published. Refusing anything older stops that. Making the
//! refusal absolute would also stop the maintainer telling people to go back to 1.x, which
//! is a documented path — so the bypass is explicit, and it is the caller's to offer.
//!
//! **No freeze rule.** A rule that stops updating after some date depends on the user's
//! clock, which means it fires on every machine whose clock is wrong and, one day, on every
//! machine at once. There is none here, and that is the decision rather than an omission.
//!
//! **Never while elevated.** The updater runs an installer. An installer run from an
//! elevated process inherits that elevation, so a client that happened to be started as
//! administrator would silently install with more rights than the update path was designed
//! for — and the elevated helper is a *different* process for exactly that reason.

use crate::manifest::Manifest;

/// What the updater should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Install it.
    Install,
    /// Nothing to do: this is the version already running.
    AlreadyCurrent,
    /// The offer is older than what is running, and nobody asked to go back.
    Downgrade {
        /// What is running.
        running: semver::Version,
        /// What was offered.
        offered: semver::Version,
    },
    /// This process is elevated, so it must not run an installer.
    Elevated,
}

/// Everything the decision depends on that is not the manifest.
#[derive(Clone, Copy, Debug)]
pub struct Circumstances {
    /// Whether this process is running with administrator rights.
    pub elevated: bool,
    /// Whether the user explicitly asked to go to this version, whatever it is.
    ///
    /// The bypass. Off unless somebody typed something: a downgrade that happens because a
    /// server offered one is the attack, and a downgrade that happens because a person
    /// asked for one is a support instruction.
    pub asked_for_this_version: bool,
}

/// Decides.
///
/// The order matters and is not alphabetical: elevation first, because it is a property of
/// how this process was started and no manifest can change it. Telling a user their update
/// is a downgrade when the real problem is that they started the client as administrator
/// would send them to fix the wrong thing.
#[must_use]
pub fn decide(
    running: &semver::Version,
    manifest: &Manifest,
    circumstances: Circumstances,
) -> Decision {
    if circumstances.elevated {
        return Decision::Elevated;
    }
    if manifest.version == *running {
        return Decision::AlreadyCurrent;
    }
    if manifest.version < *running && !circumstances.asked_for_this_version {
        return Decision::Downgrade {
            running: running.clone(),
            offered: manifest.version.clone(),
        };
    }
    Decision::Install
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Circumstances, Decision, decide};
    use crate::manifest::Manifest;

    fn offering(version: &str) -> Manifest {
        Manifest {
            version: semver::Version::parse(version).expect("a version"),
            url: "https://example.invalid/x.exe".to_owned(),
            sha512: "a".repeat(128),
            size: 1,
        }
    }

    const ORDINARY: Circumstances = Circumstances {
        elevated: false,
        asked_for_this_version: false,
    };

    fn running(version: &str) -> semver::Version {
        semver::Version::parse(version).expect("a version")
    }

    #[test]
    fn a_newer_version_is_installed() {
        assert_eq!(
            decide(&running("2.0.0"), &offering("2.0.1"), ORDINARY),
            Decision::Install
        );
        assert_eq!(
            decide(&running("2.0.0"), &offering("3.0.0"), ORDINARY),
            Decision::Install
        );
    }

    #[test]
    fn the_version_already_running_is_not_installed_again() {
        assert_eq!(
            decide(&running("2.0.0"), &offering("2.0.0"), ORDINARY),
            Decision::AlreadyCurrent
        );
    }

    /// The rule that matters. An attacker who can serve an old *signed* manifest could
    /// otherwise walk a client back to a version with a known hole, using nothing but files
    /// this project really published.
    #[test]
    fn an_older_version_is_refused_by_default() {
        let decision = decide(&running("2.1.0"), &offering("2.0.0"), ORDINARY);
        assert_eq!(
            decision,
            Decision::Downgrade {
                running: running("2.1.0"),
                offered: running("2.0.0"),
            }
        );
    }

    /// And the bypass, because the 2.0→1.x path is documented and a support instruction has
    /// to be followable.
    #[test]
    fn an_older_version_is_installed_when_somebody_asked_for_it() {
        assert_eq!(
            decide(
                &running("2.1.0"),
                &offering("1.0.5"),
                Circumstances {
                    asked_for_this_version: true,
                    ..ORDINARY
                }
            ),
            Decision::Install
        );
    }

    /// Elevation is checked before anything else. Telling a user their update is a
    /// downgrade when the real problem is that they started the client as administrator
    /// sends them to fix the wrong thing.
    #[test]
    fn elevation_is_the_first_answer_and_not_the_third() {
        let elevated = Circumstances {
            elevated: true,
            asked_for_this_version: true,
        };
        for offered in ["1.0.0", "2.0.0", "9.9.9"] {
            assert_eq!(
                decide(&running("2.0.0"), &offering(offered), elevated),
                Decision::Elevated,
                "offered {offered}"
            );
        }
    }

    /// A pre-release is older than the release it precedes, which is semver's rule and the
    /// one a rollback check has to use: `2.0.0-rc.1` arriving at a `2.0.0` client is a
    /// downgrade, however new it looks.
    #[test]
    fn a_pre_release_is_older_than_its_release() {
        assert!(matches!(
            decide(&running("2.0.0"), &offering("2.0.0-rc.1"), ORDINARY),
            Decision::Downgrade { .. }
        ));
        assert_eq!(
            decide(&running("2.0.0-rc.1"), &offering("2.0.0"), ORDINARY),
            Decision::Install
        );
    }

    /// There is no freeze rule, and this is what says so: a client whose clock is wrong by
    /// a decade decides exactly what a client whose clock is right decides.
    ///
    /// It cannot fail while `decide` takes no time at all — which is the point. If a date
    /// ever reaches this function, this test stops compiling and somebody has to argue for
    /// it.
    #[test]
    fn nothing_here_depends_on_the_clock() {
        let decision = decide(&running("2.0.0"), &offering("2.0.1"), ORDINARY);
        assert_eq!(decision, Decision::Install);
        // The signature is the assertion: `decide(&Version, &Manifest, Circumstances)`.
        // There is nowhere for a date to enter, and adding one would be a visible change to
        // three call sites rather than a line inside a function.
        let _: fn(&semver::Version, &Manifest, Circumstances) -> Decision = decide;
    }
}
