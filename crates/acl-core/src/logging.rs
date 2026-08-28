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

/// How serious a line is.
///
/// `logFile.ts` mirrors the four console levels and writes their names straight into the
/// prefix. Three here, because `log` and `info` were the same thing on that side and a
/// support conversation reading both generations should not have to know which is which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Something happened whose order is worth knowing.
    Info,
    /// Something is not as it should be, and the client carried on.
    Warn,
    /// Something failed.
    Error,
}

impl Level {
    /// What it is called in the file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Where lines go, and whether they still can.
///
/// One mutex around the path and the [`Sink`]. Two threads must not interleave halves of a
/// line, and the alternative — a channel and a writer thread — would keep the audio
/// callback off the disc at the cost of losing whatever had not been written when a crash
/// happens, which is the moment the last lines matter most. `logFile.ts` writes
/// synchronously for the same reason, and the callback logs only stream errors.
static DESTINATION: std::sync::Mutex<Option<(std::path::PathBuf, Sink)>> =
    std::sync::Mutex::new(None);

/// Starts writing to `path`, creating the directory it lives in.
///
/// Call once, as early as the profile directory is known. Returns whether logging is on;
/// ignoring the answer is fine, because nothing else depends on it.
///
/// **This is what was missing.** Every decision in this module was ported and tested and
/// no line was ever written: the client is a windows-subsystem application, so its
/// `eprintln!` goes to a handle that does not exist, and this module had no caller.
pub fn open(path: &std::path::Path) -> bool {
    if let Some(directory) = path.parent()
        && std::fs::create_dir_all(directory).is_err()
    {
        return false;
    }
    let Ok(mut destination) = DESTINATION.lock() else {
        return false;
    };
    *destination = Some((path.to_path_buf(), Sink::new()));
    true
}

/// Writes one line, if anybody is listening.
///
/// A write that fails takes the sink down for the session, which is [`Sink`]'s rule and
/// its reason: a full disc must not become a failed syscall per line.
pub fn write(source: &str, level: Level, message: &str) {
    let Ok(mut destination) = DESTINATION.lock() else {
        return;
    };
    let Some((path, sink)) = destination.as_mut() else {
        return;
    };
    if !sink.accepts() {
        return;
    }
    if std::fs::metadata(&*path).is_ok_and(|found| should_rotate(found.len())) {
        rotate(path);
    }
    let line = format_line(&now(), source, level.name(), message);
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
    if written.is_err() {
        sink.fail();
    }
}

/// Moves the current file aside, keeping exactly one.
fn rotate(path: &std::path::Path) {
    let mut previous = path.as_os_str().to_owned();
    previous.push(PREVIOUS_SUFFIX);
    let previous = std::path::PathBuf::from(previous);
    // Removed before the rename, not after: renaming onto an existing file is allowed on
    // Unix and refused on Windows, and this runs on Windows.
    let _ = std::fs::remove_file(&previous);
    let _ = std::fs::rename(path, &previous);
}

/// The time now, in the shape [`format_line`] documents.
fn now() -> String {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    stamp(since.as_secs(), since.subsec_millis())
}

/// The same, from a fixed instant, so it can be tested.
///
/// `format_line` declines to produce this and says why: it would need either a date
/// library or a hand-rolled civil-calendar conversion, and neither belongs behind a log
/// line. It belongs behind the *writer*, which is here. A library was the other option and
/// was not taken — this store audits its supply chain crate by crate, and one field of one
/// format is not worth a dependency.
fn stamp(seconds: u64, millis: u32) -> String {
    use std::fmt::Write as _;
    let (year, month, day) = civil_from_days(seconds / 86_400);
    let rest = seconds % 86_400;
    let mut out = String::with_capacity(24);
    let _ = write!(
        out,
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60,
    );
    out
}

/// Howard Hinnant's `civil_from_days`, for days since 1970-01-01.
///
/// Taken rather than derived. A calendar written from memory is wrong on a leap year that
/// somebody hits in February and nobody looks at until then; this is the algorithm the
/// standard libraries use, and the test below checks it on the days that catch mistakes.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let days = days + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Writes an informational line.
#[macro_export]
macro_rules! log_info {
    ($source:expr, $($argument:tt)*) => {
        $crate::logging::write($source, $crate::logging::Level::Info, &format!($($argument)*))
    };
}

/// Writes a warning.
#[macro_export]
macro_rules! log_warn {
    ($source:expr, $($argument:tt)*) => {
        $crate::logging::write($source, $crate::logging::Level::Warn, &format!($($argument)*))
    };
}

/// Writes an error.
#[macro_export]
macro_rules! log_error {
    ($source:expr, $($argument:tt)*) => {
        $crate::logging::write($source, $crate::logging::Level::Error, &format!($($argument)*))
    };
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    /// The epoch, and the leap days that catch a calendar written from memory.
    #[test]
    fn the_calendar_knows_about_leap_years() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // Divisible by four, so a leap year.
        assert_eq!(civil_from_days(18_321), (2020, 2, 29));
        assert_eq!(civil_from_days(18_322), (2020, 3, 1));
        // Divisible by 100 *and* 400, so a leap year after all.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // The last day of a century that is not.
        assert_eq!(civil_from_days(47_540), (2100, 2, 28));
        assert_eq!(civil_from_days(47_541), (2100, 3, 1));
    }

    #[test]
    fn a_stamp_is_the_shape_format_line_documents() {
        assert_eq!(stamp(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(stamp(1_787_855_445, 123), "2026-08-27T18:30:45.123Z");
        // Milliseconds are padded, which is what `toISOString` does.
        assert_eq!(&stamp(0, 7)[20..], "007Z");
    }

    /// One at a time for the tests that write.
    ///
    /// `open` sets a destination for the whole process, so two of these running at once
    /// would each be writing to the other's file. Poisoning is ignored: a panicking test
    /// has already failed, and refusing to run its neighbours hides which one it was.
    fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        static ORDER: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ORDER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A line reaches the file, with its source and level.
    #[test]
    fn a_line_reaches_the_file() {
        let _order = one_at_a_time();
        let directory = std::env::temp_dir().join("acl-logging-one");
        let _ = std::fs::remove_dir_all(&directory);
        let path = directory.join(LOG_FILE);
        assert!(open(&path));
        write("net", Level::Warn, "the server said no");

        let held = std::fs::read_to_string(&path).expect("the file exists");
        assert!(held.contains("[net/warn] the server said no"), "{held}");
        assert!(held.starts_with("20"), "a timestamp comes first: {held}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Past the cap the file becomes `.1` and a fresh one starts, and only one is kept.
    #[test]
    fn a_full_file_is_rotated_once() {
        let _order = one_at_a_time();
        let directory = std::env::temp_dir().join("acl-logging-two");
        let _ = std::fs::remove_dir_all(&directory);
        let path = directory.join(LOG_FILE);
        assert!(open(&path));
        std::fs::write(&path, vec![b'x'; usize::try_from(MAX_BYTES).unwrap()]).unwrap();

        write("game", Level::Info, "after the cap");
        let previous =
            std::fs::read(directory.join(format!("{LOG_FILE}{PREVIOUS_SUFFIX}"))).expect("rotated");
        assert_eq!(previous.len() as u64, MAX_BYTES);
        let fresh = std::fs::read_to_string(&path).expect("a new file");
        assert!(fresh.contains("after the cap"));
        assert!(fresh.len() < 200, "the new file starts empty");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Nothing the disc does may reach the caller.
    #[test]
    fn a_path_that_cannot_be_opened_is_not_a_panic() {
        let _order = one_at_a_time();
        // A file where the directory has to be, so `create_dir_all` refuses.
        let blocker = std::env::temp_dir().join("acl-logging-three");
        let _ = std::fs::remove_dir_all(&blocker);
        std::fs::write(&blocker, b"not a directory").unwrap();
        open(&blocker.join("logs").join(LOG_FILE));
        write(
            "client",
            Level::Error,
            "this goes nowhere, and that is fine",
        );
        let _ = std::fs::remove_file(&blocker);
    }

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
