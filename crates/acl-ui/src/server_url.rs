//! Whether a string is a voice server this client will talk to.
//!
//! A port of `src/renderer/settings/validateServerUrl.ts`, rule for rule, and of its
//! tests case for case. The two have to agree: §4.10's rollout runs a 1.x and a 2.x
//! install side by side reading one `config.json`, so a URL one accepts and the other
//! refuses is a client that will not start on a setting the other is happily using.
//!
//! # Why this takes a dependency and `acl-net`'s `is_uri` does not
//!
//! [`crate::server_url`] needs a hostname, a scheme and a normalised path — three things
//! that are only correct if something implements the WHATWG algorithm. `acl-net`'s
//! `is_uri` needed "a valid scheme followed by a colon", which is a rule short enough to
//! write out and check against the original for every input. This one is not, and
//! hand-rolling it would produce a parser that disagrees with the TypeScript in cases
//! nobody enumerated.
//!
//! `url` is already in the tree beneath `webrtc`, so this adds a direct edge rather than
//! a crate.

use url::Url;

/// Whether the client will accept this as a server address.
///
/// Four rules, each there for a different reason.
///
/// **It has to parse.**
///
/// **The scheme has to be `http` or `https`.** Not because those are the only two that
/// parse — `javascript:` and `data:` parse — but because they are the only two this
/// client knows how to connect to.
///
/// `http` is accepted, so signalling can run in cleartext. That is recorded in
/// `docs/rust-port/08-dependency-review.md` as a finding rather than defended here: a
/// local server on a LAN is the case it exists for, and removing it is a decision with
/// users behind it.
///
/// **`discord.gg` is refused by name.** People paste invite links into text fields, and a
/// server field that accepted one would point every signal at Discord and report nothing
/// wrong.
///
/// **The path has to be empty.** The client appends its own. One already there produces
/// requests to somewhere nobody meant, and the failure looks like a server that is down.
#[must_use]
pub fn is_valid(candidate: &str) -> bool {
    let Ok(url) = Url::parse(candidate) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    // Lowercased by the parser, which is what makes a plain comparison enough.
    if url.host_str() == Some("discord.gg") {
        return false;
    }
    url.path() == "/"
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    /// Every case in `validateServerUrl.test.ts`, with the answer that file asserts.
    ///
    /// Listed together rather than split across tests so that the two files can be read
    /// against each other, which is the only way this stays true.
    const CASES: [(&str, bool); 20] = [
        ("https://aucl.greluc.me", true),
        ("https://aucl.greluc.me/", true),
        ("https://192.168.1.10:9736", true),
        // A decision, not an oversight: a local server on a LAN.
        ("http://192.168.1.10:9736", true),
        ("ws://example.com", false),
        ("ftp://example.com", false),
        ("file:///etc/passwd", false),
        ("javascript:alert(1)", false),
        ("data:text/html,<b>x</b>", false),
        ("https://discord.gg/abcdef", false),
        ("https://discord.gg", false),
        ("https://DISCORD.GG/abcdef", false),
        ("https://example.com/voice", false),
        ("https://example.com/a/b", false),
        ("https://example.com?x=1", true),
        ("https://example.com#top", true),
        ("", false),
        ("aucl.greluc.me", false),
        ("not a url", false),
        ("https://", false),
    ];

    #[test]
    fn it_agrees_with_the_electron_client_on_every_case_that_file_tests() {
        // §4.10 runs a 1.x and a 2.x install side by side on one `config.json`. A URL one
        // accepts and the other refuses is a client that will not start on a setting the
        // other is using without complaint.
        for (candidate, expected) in CASES {
            assert_eq!(is_valid(candidate), expected, "{candidate:?}");
        }
    }

    #[test]
    fn the_hostname_check_survives_the_case_it_was_pasted_in() {
        // The parser lowercases it, which is the whole reason a plain comparison is
        // enough. Asserted rather than assumed, because if it ever stopped being true the
        // rule would fail open.
        assert!(!is_valid("https://DISCORD.GG"));
        assert!(!is_valid("https://Discord.Gg/xyz"));
    }

    #[test]
    fn a_subdomain_of_the_refused_host_is_not_refused() {
        // Recording the boundary rather than widening it. The rule exists for pasted
        // invite links, which are always `discord.gg` exactly, and a suffix match would
        // refuse a server somebody legitimately runs there.
        assert!(is_valid("https://voice.discord.gg"));
    }

    #[test]
    fn a_path_the_parser_normalises_away_is_no_path() {
        // `/.` and `/..` are removed by the WHATWG algorithm before anything sees them, so
        // both sides read a path of `/` and accept the URL. Written down because the first
        // version of this test asserted the opposite and was wrong: both implementations
        // were checked against each other rather than against my expectation.
        //
        // `//` is not normalised and stays a path, which is the case that matters — it is
        // the one a client would actually append to.
        assert!(is_valid("https://example.com"));
        assert!(is_valid("https://example.com/"));
        assert!(is_valid("https://example.com/."));
        assert!(is_valid("https://example.com/.."));
        assert!(!is_valid("https://example.com//"));
    }
}
