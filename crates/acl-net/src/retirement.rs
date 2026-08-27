//! What a client is told when the server has stopped speaking its protocol.
//!
//! A port of `src/common/protocolRetirement.ts`. §4.12 item 6: when the switch-off happens
//! the server answers an old handshake with a message the client displays, rather than
//! closing the socket on it. A client that has not updated must be told *why* it stopped
//! working — in the app, in its own language. That is the difference between a sunset and
//! an outage.
//!
//! # Why the sentinel is also a sentence
//!
//! Because a client can only translate a string it already has. The server sends
//! [`PROTOCOL_RETIRED`]; a client old enough to predate this file receives it and shows it
//! verbatim, because showing the server's error text is what such a client has always done.
//!
//! So the marker is readable English as well as a marker. If the ordering is ever got wrong
//! — the switch-off shipped before the release that can translate it — what a user sees is
//! still a sentence telling them what to do, rather than a serial number.
//!
//! The ordering itself is not this file's to enforce and is the whole of the difficulty:
//! the fleet must have this *before* the server starts sending it.

/// What the server sends instead of accepting a retired handshake.
///
/// Byte for byte what `protocolRetirement.ts` exports. The two generations must agree on it
/// exactly: a 2.x server telling a 1.x client something 1.x does not recognise leaves that
/// client showing a string it cannot translate, which is the case this constant exists to
/// avoid.
pub const PROTOCOL_RETIRED: &str =
    "PROTOCOL_RETIRED: this version is no longer supported, please update";

/// The key a translated version lives under.
pub const RETIRED_KEY: &str = "game.error_retired";

/// Turns a server error into what the player should read.
///
/// Anything that is not the sentinel is passed through untouched. The server sends real
/// errors too, and replacing one of those with a translated guess would hide the thing the
/// player needs to see.
///
/// `translate` is expected to return its argument when it has no entry for it, which is
/// what both catalogues do. A locale that has not been translated yet falls back to English
/// on its own; this covers the case where the key is missing from every locale, and there
/// the server's own sentence is better than the key.
#[must_use]
pub fn message(from_server: &str, translate: impl Fn(&str) -> String) -> String {
    if !from_server.starts_with("PROTOCOL_RETIRED") {
        return from_server.to_owned();
    }
    let translated = translate(RETIRED_KEY);
    if translated == RETIRED_KEY || translated.is_empty() {
        from_server.to_owned()
    } else {
        translated
    }
}

#[cfg(test)]
mod tests {
    use super::{PROTOCOL_RETIRED, RETIRED_KEY, message};

    /// The catalogue that has it.
    fn german(key: &str) -> String {
        if key == RETIRED_KEY {
            "Diese Version wird nicht mehr unterstützt. Bitte aktualisiere.".to_owned()
        } else {
            key.to_owned()
        }
    }

    /// The one that does not, which returns the key.
    fn empty(key: &str) -> String {
        key.to_owned()
    }

    #[test]
    fn the_sentinel_becomes_the_players_own_language() {
        assert!(message(PROTOCOL_RETIRED, german).starts_with("Diese Version"));
    }

    /// The prefix, not the whole string.
    ///
    /// `protocolRetirement.ts` matches on `startsWith`, so a server that appends a detail —
    /// a date, a version — is still recognised. Matching the whole sentence would turn any
    /// such addition back into a raw sentinel on every screen.
    #[test]
    fn a_sentinel_with_something_after_it_still_counts() {
        let longer = format!("{PROTOCOL_RETIRED} (from 2027-01-01)");
        assert!(message(&longer, german).starts_with("Diese Version"));
    }

    /// A real error is not replaced by a guess.
    #[test]
    fn anything_else_is_passed_through() {
        for real in [
            "the lobby is full",
            "rate limited",
            "",
            "protocol_retired in lower case is not the sentinel",
        ] {
            assert_eq!(message(real, german), real);
        }
    }

    /// With no translation anywhere, the server's own sentence is what shows.
    ///
    /// It is English and it is a sentence, which is the reason the sentinel is written the
    /// way it is. The alternative -- showing `game.error_retired` -- is a serial number.
    #[test]
    fn a_missing_translation_leaves_the_readable_english() {
        assert_eq!(message(PROTOCOL_RETIRED, empty), PROTOCOL_RETIRED);
        assert!(
            !message(PROTOCOL_RETIRED, empty).contains(RETIRED_KEY),
            "the key must never reach a player"
        );
    }
}
