//! The four messages the client shows instead of a lobby.
//!
//! A port of `src/common/Errors.ts`. They travel from the main process over IPC and are
//! rendered verbatim by `Voice.tsx` under a heading that says ERROR, so they are the
//! whole of what a player sees when the client cannot start.
//!
//! # They are not translated, and that is worth knowing before the GUI is written
//!
//! These four are not in the catalogue: they are English literals in a shared module, shown
//! as-is to everybody. Nobody decided that — it is what happens when a string is added on
//! the main-process side of a boundary the translation loader does not cross.
//!
//! They were ported as literals rather than quietly turned into locale keys, because doing
//! that meant writing four entries into thirty-seven files and inventing translations for
//! thirty-five of them, which is not a porting decision.
//!
//! **That reason expired on 2026-08-28**, when the tree was cut to English and German. Two
//! entries in two files is not a translation project. What keeps them literals now is that
//! nothing shows them: no caller exists, so there is no screen to decide the wording
//! against. Closing it belongs with the first thing that raises one.

/// The messages, with the identifiers the Electron client gives them.
///
/// The identifiers are internal. The text is what a player reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupError {
    /// The game is a build this client has no offsets for.
    UnsupportedVersion,
    /// The process could not be opened, which usually means elevation.
    OpenAsAdministrator,
    /// The lookup could not be fetched from the mirror or the cache.
    LookupFetchError,
    /// The offsets could not be fetched from the mirror or the cache.
    OffsetsFetchError,
}

impl StartupError {
    /// Every one, for tests and for a settings screen that wants to list them.
    pub const ALL: [Self; 4] = [
        Self::UnsupportedVersion,
        Self::OpenAsAdministrator,
        Self::LookupFetchError,
        Self::OffsetsFetchError,
    ];

    /// The name in `Errors.ts`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::UnsupportedVersion => "UNSUPPORTED_VERSION",
            Self::OpenAsAdministrator => "OPEN_AS_ADMINISTRATOR",
            Self::LookupFetchError => "LOOKUP_FETCH_ERROR",
            Self::OffsetsFetchError => "OFFSETS_FETCH_ERROR",
        }
    }

    /// What the player reads, in English, exactly as 1.x shows it.
    ///
    /// The newlines are load-bearing: `Voice.tsx` renders these with `white-space:
    /// pre-wrap`, so each one is a line break the author put there deliberately.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::UnsupportedVersion => {
                "Your version of Among Us is unsupported by AnotherCrewLink.\n"
            }
            Self::OpenAsAdministrator => {
                "Error with checking the process:\nCouldn't connect to Among Us.\nPlease re-open AnotherCrewLink as Administrator."
            }
            Self::LookupFetchError => {
                "Error with fetching lookups:\nPlease check your internet connection."
            }
            Self::OffsetsFetchError => {
                "Error with fetching offsets:\nPlease check your internet connection."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn source() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/common/Errors.ts"),
        )
        .expect("the Electron client is beside the crates")
    }

    #[test]
    fn every_identifier_is_in_the_electron_client() {
        let source = source();
        for error in StartupError::ALL {
            assert!(
                source.contains(error.id()),
                "{} is not in Errors.ts",
                error.id()
            );
        }
    }

    #[test]
    fn the_messages_survive_the_port_line_for_line() {
        // `Voice.tsx` renders these with `white-space: pre-wrap`, so every newline is a
        // line break somebody put there. A message re-flowed on the way across reads as a
        // different message on the screen it appears on.
        let source = source().replace("\\n", "\n");
        for error in StartupError::ALL {
            for line in error.message().lines() {
                assert!(
                    source.contains(line),
                    "{}: this line is not in Errors.ts: {line:?}",
                    error.id()
                );
            }
        }
    }

    #[test]
    fn the_only_message_ending_in_a_newline_is_the_one_that_does_in_the_original() {
        // A trailing newline adds a blank line under the text. Three of the four do not
        // have one and one does, and that asymmetry is in the shipped client rather than
        // in this transcription.
        assert!(StartupError::UnsupportedVersion.message().ends_with('\n'));
        for error in [
            StartupError::OpenAsAdministrator,
            StartupError::LookupFetchError,
            StartupError::OffsetsFetchError,
        ] {
            assert!(!error.message().ends_with('\n'), "{}", error.id());
        }
    }

    #[test]
    fn none_of_them_is_a_locale_key() {
        // Recording the gap rather than closing it: these four are English literals, and
        // whoever writes the screen that shows one has to decide whether that stays true.
        // A decision rather than an inheritance.
        for error in StartupError::ALL {
            let message = error.message();
            assert!(
                message.contains(' '),
                "{} looks like a key rather than a sentence",
                error.id()
            );
        }
    }
}
