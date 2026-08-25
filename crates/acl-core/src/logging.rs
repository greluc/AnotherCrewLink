//! What goes in the log file, when it rolls over, and what happens when it cannot.
//!
//! A port of the decisions in `src/main/logFile.ts`. Not the file handling — that is
//! `std::fs` on both sides and holds nothing worth testing — but the three rules around
//! it, each of which is a choice somebody made for a reason:
//!
//! 1. **The line shape**, because a support conversation reads 1.x and 2.x logs side by
//!    side and a changed prefix means every existing grep and every screenshot in an
//!    issue stops matching.
//! 2. **One previous file, four mebibytes**, because a long session must not fill a disk
//!    and a player who hits something once during a match must still be able to find it
//!    afterwards.
//! 3. **Logging never stops the client.** A failure switches it off for the session
//!    rather than propagating — and, just as importantly, stops it retrying, because a
//!    full disk otherwise costs a failed syscall on every line.

/// When the log rolls over.
///
/// Four mebibytes, from `logFile.ts`. Two of them at most on disk, because exactly one
/// previous file is kept.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// The name of the file, under [`crate::paths::Paths::log_directory`].
pub const LOG_FILE: &str = "anothercrewlink.log";

/// The suffix the previous file gets.
pub const PREVIOUS_SUFFIX: &str = ".1";

/// Whether the log should roll over before the next line is written.
///
/// The comparison is `>=`, matching `logFile.ts`'s `if (size < MAX_BYTES) return`. It
/// runs before the write rather than after, so the file can exceed the limit by one line
/// and never by more.
#[must_use]
pub const fn should_rotate(size: u64) -> bool {
    size >= MAX_BYTES
}

/// Formats one line exactly as the Electron client does.
///
/// `timestamp` is passed in rather than read from a clock, which is this codebase's habit
/// for anything time-dependent — see [`crate::helper`]'s neighbours in `acl-net`. It must
/// be what JavaScript's `toISOString()` produces: `2026-08-25T10:22:39.123Z`, always UTC,
/// always milliseconds. Producing it is the caller's job because doing it here would mean
/// either a date library or a hand-rolled civil-calendar conversion, and neither belongs
/// behind a log line.
#[must_use]
pub fn format_line(timestamp: &str, source: &str, level: &str, message: &str) -> String {
    format!("{timestamp} [{source}/{level}] {message}\n")
}

/// Whether the log is still being written to.
///
/// One transition, and it is deliberate: once writing has failed the sink goes quiet for
/// the rest of the session. Retrying would turn a full disk or a revoked permission into
/// a failed syscall per line, on a client that logs every game frame's worth of state
/// changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sink {
    failed: bool,
}

impl Sink {
    /// A sink that has not failed.
    #[must_use]
    pub const fn new() -> Self {
        Self { failed: false }
    }

    /// Whether a line should be attempted at all.
    #[must_use]
    pub const fn accepts(self) -> bool {
        !self.failed
    }

    /// Records that a write failed.
    ///
    /// There is no way back within a session. Logging must never be the reason the client
    /// does not start, and it must not become the reason it runs slowly either.
    pub const fn fail(&mut self) {
        self.failed = true;
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_line_is_byte_for_byte_what_the_electron_client_writes() {
        // `${new Date().toISOString()} [${source}/${level}] ${message}\n`. A support
        // conversation reads both generations' logs together; a changed prefix breaks
        // every grep in every existing issue.
        assert_eq!(
            format_line("2026-08-25T10:22:39.123Z", "main", "warn", "no relay"),
            "2026-08-25T10:22:39.123Z [main/warn] no relay\n"
        );
    }

    #[test]
    fn a_line_ends_with_exactly_one_newline() {
        // Two would double-space the file; none would run every line together, and the
        // first thing anybody does with a log is read it.
        let line = format_line("2026-08-25T10:22:39.123Z", "renderer", "error", "boom");
        assert!(line.ends_with('\n'));
        assert!(!line.ends_with("\n\n"));
    }

    #[test]
    fn a_message_containing_a_newline_is_left_alone() {
        // A stack trace is several lines and stays several lines. Escaping it would make
        // the log harder to read to make it easier to parse, and nothing parses it.
        let line = format_line("t", "main", "error", "first\nsecond");
        assert_eq!(line, "t [main/error] first\nsecond\n");
    }

    #[test]
    fn rotation_happens_at_the_limit_and_not_before() {
        assert!(!should_rotate(0));
        assert!(!should_rotate(MAX_BYTES - 1));
        assert!(should_rotate(MAX_BYTES));
        assert!(should_rotate(MAX_BYTES * 10));
    }

    #[test]
    fn at_most_two_files_can_exist() {
        // Four mebibytes each. The check is what stops a long session filling a disk, and
        // the single previous file is what stops the fix from throwing away the evidence.
        assert_eq!(MAX_BYTES * 2, 8 * 1024 * 1024);
        assert_eq!(PREVIOUS_SUFFIX, ".1");
    }

    #[test]
    fn a_sink_that_failed_stays_failed() {
        // Not stubbornness. Retrying turns a full disk into a failed syscall per line on
        // a client that logs a line per game frame's worth of state changes.
        let mut sink = Sink::new();
        assert!(sink.accepts());
        sink.fail();
        assert!(!sink.accepts());
        sink.fail();
        assert!(!sink.accepts());
    }
}
