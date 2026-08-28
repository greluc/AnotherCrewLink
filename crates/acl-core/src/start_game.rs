//! Starting Among Us, the way the shipped client's launch button starts it.
//!
//! A port of `src/main/ipc-handlers.ts`'s two branches. [`acl_types::platform`] has said
//! which store is which and what its identifier is since the port began, with a test that
//! compares every string against `GamePlatform.ts` — and nothing used it. There was no way
//! to start the game from this client at all.
//!
//! # Two ways, and they are not interchangeable
//!
//! Steam and Epic are started through a **URI** their launcher registered:
//! `steam://rungameid/945360`. Handing that to `CreateProcess` does nothing — it is not a
//! program — so it goes to the shell, which is what resolves a scheme to whatever claimed
//! it.
//!
//! The Microsoft Store copy, and every custom entry, is started as a **program**: a
//! directory, an executable in it, and arguments. Passing that to the shell would work by
//! accident and would also open a document if somebody pointed the setting at one.
//!
//! [`plan`] decides which, from what the settings hold, and is where the tests are.
//! [`start`] does it, and is the part that cannot be tested without launching a game.

use std::path::{Path, PathBuf};

/// What starting a platform comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// Hand this to the shell, which resolves the scheme.
    Uri(String),
    /// Run this program, in this directory, with these arguments.
    Program {
        /// Where it lives.
        directory: PathBuf,
        /// The executable's own name.
        executable: String,
        /// What to pass it.
        arguments: Vec<String>,
    },
}

/// Why a platform could not be started.
#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    /// The settings name a platform, and say nothing about how to start it.
    Incomplete,
    /// The operating system refused.
    Refused(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incomplete => formatter.write_str("this platform has no path to start"),
            Self::Refused(why) => write!(formatter, "the system refused to start it: {why}"),
        }
    }
}

impl std::error::Error for StartError {}

/// How a platform says it should be started, as the settings hold it.
///
/// The built-in three fill this from [`acl_types::platform::Platform`]; a custom entry fills
/// it from `customPlatforms`, which is the same four fields under names a player chose.
#[derive(Clone, Copy, Debug)]
pub struct Described<'a> {
    /// `steam://…` for a URI platform, or the directory for a program one.
    pub run_path: &'a str,
    /// Whether `run_path` is a URI. `GamePlatform.ts`'s `launchType`.
    pub is_uri: bool,
    /// For a program platform: the executable and then its arguments, which is exactly
    /// `execute` in `GamePlatform.ts` — the first entry is the program.
    pub execute: &'a [String],
}

/// Works out what starting this platform means, or refuses.
///
/// Refusing matters more than it looks. `run_path` is `none` for the Microsoft Store in
/// `GamePlatform.ts` — a literal string, not an empty one — because that entry is started
/// from a path the player sets, and until they have set one there is nothing to run. A
/// launch button that shells out to a directory called `none` is one that fails in a way
/// nobody can read.
///
/// # Errors
///
/// [`StartError::Incomplete`] when there is nothing to start: an empty URI, a missing
/// directory, or a program list with no program in it.
pub fn plan(described: Described<'_>) -> Result<Start, StartError> {
    let run_path = described.run_path.trim();
    if run_path.is_empty() || run_path == "none" {
        return Err(StartError::Incomplete);
    }
    if described.is_uri {
        return Ok(Start::Uri(run_path.to_owned()));
    }
    let Some((executable, arguments)) = described.execute.split_first() else {
        return Err(StartError::Incomplete);
    };
    if executable.trim().is_empty() {
        return Err(StartError::Incomplete);
    }
    Ok(Start::Program {
        directory: PathBuf::from(run_path),
        executable: executable.clone(),
        arguments: arguments.to_vec(),
    })
}

/// Starts what [`plan`] decided.
///
/// # Errors
///
/// [`StartError::Refused`] with whatever the operating system said.
#[cfg(windows)]
pub fn start(what: &Start) -> Result<(), StartError> {
    match what {
        Start::Uri(uri) => open_with_the_shell(uri),
        Start::Program {
            directory,
            executable,
            arguments,
        } => run(directory, executable, arguments),
    }
}

/// Elsewhere there is no game to start, and this exists so the crate builds.
#[cfg(not(windows))]
pub fn start(_what: &Start) -> Result<(), StartError> {
    Err(StartError::Refused("not Windows".to_owned()))
}

/// Runs a program in its own directory.
#[cfg(windows)]
fn run(directory: &Path, executable: &str, arguments: &[String]) -> Result<(), StartError> {
    std::process::Command::new(directory.join(executable))
        // The game's own directory, because a game started from somewhere else looks for
        // its data relative to the working directory and does not find it.
        .current_dir(directory)
        .args(arguments)
        .spawn()
        .map(|_| ())
        .map_err(|error| StartError::Refused(error.to_string()))
}

/// Hands a URI to whatever registered its scheme.
#[cfg(windows)]
fn open_with_the_shell(uri: &str) -> Result<(), StartError> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both strings are null-terminated and live across the call, and the three null
    // pointers are the documented "no parent, no arguments, no directory".
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Documented: anything above 32 is success, and the value is otherwise an error code.
    // It is an `HINSTANCE` for compatibility with sixteen-bit Windows and means nothing.
    if result as isize > 32 {
        Ok(())
    } else {
        Err(StartError::Refused(format!(
            "ShellExecute returned {}",
            result as isize
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{Described, Start, StartError, plan};

    fn strings(of: &[&str]) -> Vec<String> {
        of.iter().map(|each| (*each).to_owned()).collect()
    }

    /// Steam's identifier goes to the shell untouched.
    #[test]
    fn a_store_uri_is_handed_to_the_shell() {
        let planned = plan(Described {
            run_path: acl_types::platform::Platform::Steam.run_path(),
            is_uri: true,
            execute: &[],
        });
        assert_eq!(
            planned,
            Ok(Start::Uri("steam://rungameid/945360".to_owned()))
        );
    }

    /// A program platform runs its first `execute` entry and passes the rest.
    #[test]
    fn a_program_platform_runs_its_executable_with_the_rest_as_arguments() {
        let execute = strings(&["Among Us.exe", "--windowed"]);
        let planned = plan(Described {
            run_path: r"C:\Games\Among Us",
            is_uri: false,
            execute: &execute,
        })
        .expect("a plan");
        assert_eq!(
            planned,
            Start::Program {
                directory: std::path::PathBuf::from(r"C:\Games\Among Us"),
                executable: "Among Us.exe".to_owned(),
                arguments: strings(&["--windowed"]),
            }
        );
    }

    /// `none` is the Microsoft Store's unset path, and it is a word rather than an absence.
    ///
    /// `GamePlatform.ts` writes that literal, and the settings screen shows it. Treating it
    /// as a directory gives a launch button that fails somewhere nobody can read.
    #[test]
    fn the_word_none_is_not_a_path() {
        let execute = strings(&["Among Us.exe"]);
        assert_eq!(
            plan(Described {
                run_path: "none",
                is_uri: false,
                execute: &execute,
            }),
            Err(StartError::Incomplete)
        );
        // And the Microsoft entry is exactly that until a player sets a path.
        assert_eq!(
            acl_types::platform::Platform::Microsoft.run_path(),
            "none",
            "the shipped client's own value"
        );
    }

    /// Nothing to run is refused rather than started.
    #[test]
    fn an_empty_description_is_refused() {
        assert_eq!(
            plan(Described {
                run_path: "",
                is_uri: true,
                execute: &[]
            }),
            Err(StartError::Incomplete)
        );
        assert_eq!(
            plan(Described {
                run_path: r"C:\Games",
                is_uri: false,
                execute: &[]
            }),
            Err(StartError::Incomplete),
            "a program platform with no program"
        );
        // `GamePlatform.ts` gives its URI entries `execute: ['']`, so an empty first entry
        // is the normal state rather than a broken one -- and it is still not a program.
        assert_eq!(
            plan(Described {
                run_path: r"C:\Games",
                is_uri: false,
                execute: &strings(&[""]),
            }),
            Err(StartError::Incomplete)
        );
    }
}
