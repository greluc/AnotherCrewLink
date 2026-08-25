//! Where the hat artwork comes from.
//!
//! A port of `src/common/hatCollection.ts`, and of the constraint its comment describes
//! but nothing enforced: *"Moving the pin means moving both lines. The commit alone points
//! at a tree the new repository does not have."*
//!
//! # Why this is pinned to a commit
//!
//! jsDelivr serves whatever a branch holds at the moment of the request, with no
//! integrity check. A branch pin therefore lets the artwork every user downloads change
//! without a release on this side — and until 2026-08-24 the branch in question was in an
//! account this project does not control. A commit is immutable, so what shipped is what
//! arrives.
//!
//! [`the_url_carries_the_pinned_commit`] is the check the comment asked for: a URL and a
//! commit that have drifted apart point at a tree that does not exist, and every hat
//! quietly fails to load.

/// The commit in the hat repository that this release's artwork comes from.
pub const HAT_COLLECTION_COMMIT: &str = "14bb0cb592a23d2cee25a0c368506446abadaad8";

/// The base URL every hat image is built from.
///
/// It has to contain [`HAT_COLLECTION_COMMIT`]; see the module documentation.
pub const HAT_COLLECTION_URL: &str = "https://cdn.jsdelivr.net/gh/greluc/AnotherCrewLink-Hats@14bb0cb592a23d2cee25a0c368506446abadaad8/";

/// The repository the artwork is served from.
///
/// This fork's own, not upstream's. The fork carries the base game's cosmetics only: the
/// four mod collections upstream ships went with the third-party artwork, and a player
/// running one of those mods sees no mod hat rather than an error.
pub const HAT_COLLECTION_REPOSITORY: &str = "greluc/AnotherCrewLink-Hats";

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn the_url_carries_the_pinned_commit() {
        // The constraint the TypeScript comment states and nothing checked. Move one line
        // and not the other and the URL points at a tree that does not exist -- every hat
        // fails to load, and the failure is a missing image rather than an error.
        assert!(
            HAT_COLLECTION_URL.contains(HAT_COLLECTION_COMMIT),
            "the URL and the commit have drifted apart"
        );
    }

    #[test]
    fn the_pin_is_a_commit_and_not_a_branch() {
        // The whole reason for the pin. jsDelivr serves whatever a branch holds at request
        // time with no integrity check, so a branch would let the artwork every user
        // downloads change without a release on this side.
        assert_eq!(HAT_COLLECTION_COMMIT.len(), 40, "not a full commit hash");
        assert!(
            HAT_COLLECTION_COMMIT
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "not a lowercase hex hash"
        );
    }

    #[test]
    fn the_artwork_comes_from_this_project_s_own_fork() {
        // Until 2026-08-24 it came from an account this project does not control.
        assert!(HAT_COLLECTION_URL.contains(HAT_COLLECTION_REPOSITORY));
        assert!(HAT_COLLECTION_URL.starts_with("https://cdn.jsdelivr.net/gh/"));
    }

    #[test]
    fn the_url_ends_in_a_separator_so_a_filename_can_be_appended() {
        // Callers join a path onto it directly. Without the trailing slash the first
        // segment of every filename would be glued to the commit.
        assert!(HAT_COLLECTION_URL.ends_with('/'));
    }

    #[test]
    fn it_is_served_over_tls() {
        // The images are decoded by this client, and until 2026-08-24 that decoding
        // happened in a process that also held a memory reader. Cleartext artwork from a
        // CDN is a decoder fed by anyone on the path.
        assert!(HAT_COLLECTION_URL.starts_with("https://"));
    }

    #[test]
    fn both_constants_match_the_electron_client() {
        // The renderer builds the image URLs and the main process recolours images from
        // this exact origin. A divergence here means the two halves of one client
        // disagree about where the artwork is.
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../src/common/hatCollection.ts"),
        )
        .expect("the Electron client is beside the crates");
        assert!(
            source.contains(&format!(
                "HAT_COLLECTION_COMMIT = '{HAT_COLLECTION_COMMIT}'"
            )),
            "the pinned commit has moved"
        );
        assert!(
            source.contains(HAT_COLLECTION_REPOSITORY),
            "the repository has moved"
        );
    }
}
