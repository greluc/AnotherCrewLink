//! Running what was downloaded, and finding out whether we may.
//!
//! §4.9 item 3's last sentence: "Never install an update while elevated." This is where
//! that is answered rather than asserted, and where the installer is actually spawned.
//!
//! # Why the arguments here are the arguments in `installer/anothercrewlink.nsi`
//!
//! They are the same three the installed 1.x fleet's `electron-updater` uses —
//! `--updated /S /D=<dir>` — and using them here rather than inventing a private set means
//! the path 2.x updates itself along is the path P8's bridge already has to work on. One
//! contract, exercised twice, instead of two contracts of which one is exercised once a
//! year.
//!
//! `/D=` is last and unquoted. That is NSIS's rule, not a style: it takes the rest of the
//! command line verbatim, so a quoted path arrives with its quotes and a following argument
//! becomes part of the directory name.

use std::path::{Path, PathBuf};

/// Whether this process is running with administrator rights.
///
/// §4.9 item 3 refuses to install while elevated, and this is the question that refusal
/// asks. An installer inherits the rights of whatever spawned it, so an update run from an
/// elevated client installs with more of them than the update path was designed for — and
/// the helper being a *separate* elevated process is the whole reason the client is not one.
///
/// A failure to ask counts as elevated. The refusal is the safe direction: a client that
/// cannot tell should not be the one running installers.
#[cfg(windows)]
#[must_use]
pub fn elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::TOKEN_QUERY;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: a documented call taking a process handle and a place to put a token.
    // `GetCurrentProcess` returns a pseudo-handle that needs no closing.
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) };
    if opened == 0 {
        return true;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0_u32;
    // SAFETY: `token` is a live token from the call above, and `elevation` is a live
    // `TOKEN_ELEVATION` whose size is what is passed.
    let asked = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or(0),
            &raw mut returned,
        )
    };
    // SAFETY: `token` came from `OpenProcessToken` and is closed once.
    unsafe {
        CloseHandle(token);
    }
    if asked == 0 {
        return true;
    }
    elevation.TokenIsElevated != 0
}

/// Off Windows there is no token to ask, and no installer to run either.
#[cfg(not(windows))]
#[must_use]
pub fn elevated() -> bool {
    true
}

/// What to run, and with what.
///
/// Held as data rather than spawned inline so that the arguments can be tested: they are
/// the part that has to match `installer/anothercrewlink.nsi`, and a test that read them
/// out of a `Command` after the fact would be testing the standard library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The installer that was downloaded and verified.
    pub installer: PathBuf,
    /// Its arguments, in order.
    pub arguments: Vec<String>,
}

impl Plan {
    /// The plan for updating an installation in place.
    ///
    /// `--updated` first, then `/S`, then `/D=` last — and `/D=` unquoted, because NSIS
    /// takes the rest of the command line verbatim.
    #[must_use]
    pub fn updating(installer: PathBuf, install_directory: &Path) -> Self {
        Self {
            installer,
            arguments: vec![
                "--updated".to_owned(),
                "/S".to_owned(),
                format!("/D={}", install_directory.display()),
            ],
        }
    }
}

/// Why an update was not installed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    /// This process is elevated. See [`elevated`].
    #[error("an update must not be installed from an elevated process")]
    Elevated,
    /// The verified bytes could not be written somewhere runnable.
    #[error("the installer could not be written: {0}")]
    NotWritten(String),
    /// The installer could not be started.
    #[error("the installer could not be started: {0}")]
    NotStarted(String),
}

/// Writes the verified artefact somewhere and runs it.
///
/// The bytes are the ones [`crate::fetch::artefact`] returned, which means they have
/// already been checked against a manifest a trusted key signed. Nothing here re-checks
/// them, and nothing here should: a second check in a second place is a second thing to get
/// wrong, and it would not be checking the same bytes anyway once they are on a disk
/// somebody else can write.
///
/// # Errors
///
/// [`InstallError`], and [`InstallError::Elevated`] before anything is written.
pub fn run(artefact: &[u8], into: &Path, install_directory: &Path) -> Result<Plan, InstallError> {
    if elevated() {
        return Err(InstallError::Elevated);
    }
    std::fs::write(into, artefact).map_err(|error| InstallError::NotWritten(error.to_string()))?;
    let plan = Plan::updating(into.to_path_buf(), install_directory);

    #[cfg(windows)]
    {
        std::process::Command::new(&plan.installer)
            .args(&plan.arguments)
            .spawn()
            .map_err(|error| InstallError::NotStarted(error.to_string()))?;
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{InstallError, Plan, elevated, run};
    use std::path::{Path, PathBuf};

    /// The three arguments the installed fleet's updater uses, in the order NSIS needs.
    ///
    /// `/D=` last, because NSIS takes the rest of the command line verbatim from there.
    /// Anything after it becomes part of the directory name, which is an update installed
    /// into a path nobody has.
    #[test]
    fn the_arguments_are_the_ones_the_installer_expects() {
        let plan = Plan::updating(
            PathBuf::from(r"C:\Temp\Setup.exe"),
            Path::new(r"C:\Users\p\AppData\Local\Programs\ACL"),
        );
        assert_eq!(
            plan.arguments,
            [
                "--updated",
                "/S",
                r"/D=C:\Users\p\AppData\Local\Programs\ACL"
            ]
        );
        assert_eq!(
            plan.arguments.last().map(String::as_str),
            Some(r"/D=C:\Users\p\AppData\Local\Programs\ACL"),
            "/D= is not last"
        );
    }

    /// Unquoted, even with a space in it. Quoting it is the obvious thing to do and it is
    /// wrong: NSIS would take the quotes as part of the path.
    #[test]
    fn the_directory_is_not_quoted_even_when_it_has_a_space() {
        let plan = Plan::updating(
            PathBuf::from("Setup.exe"),
            Path::new(r"C:\Program Files\Some Where"),
        );
        let directory = plan.arguments.last().expect("an argument");
        assert_eq!(directory, r"/D=C:\Program Files\Some Where");
        assert!(!directory.contains('"'), "{directory}");
    }

    /// The arguments here are the arguments in the installer script. Two files that have to
    /// agree, and the script is the one the 1.x fleet's updater will also be calling.
    ///
    /// `common.nsh` since 2026-08-27: the argument handling is shared by all three scripts,
    /// so this reads the file that holds it. Reading `anothercrewlink.nsi` alone was what
    /// this used to do, and the refactor broke it — correctly, because the sentence it was
    /// checking had moved and a test that still passed would have been checking nothing.
    #[test]
    fn the_installer_script_handles_what_is_sent_to_it() {
        let script = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../installer/common.nsh"),
        )
        .expect("the shared installer contract is in the repository");
        let plan = Plan::updating(PathBuf::from("Setup.exe"), Path::new(r"C:\x"));
        assert!(
            script.contains(&plan.arguments[0]),
            "the script does not read {}",
            plan.arguments[0]
        );
        // `/S` and `/D=` are NSIS's own and appear in the script as prose rather than as
        // code, so what is checked is that the script still says it honours them --
        // `installer_contract.rs` is where the behavioural half of that lives.
        assert!(script.contains("/D="), "the script no longer mentions /D=");
    }

    /// Elevation is refused **before** anything is written, so a client that should not be
    /// installing has not left an installer on the disk either.
    #[test]
    fn an_elevated_process_writes_nothing() {
        if !elevated() {
            // The ordinary case for a test runner, and the one the assertion below cannot
            // be made in. `elevation_is_the_first_answer_and_not_the_third` in `policy`
            // covers the decision itself without needing a token.
            return;
        }
        let scratch = std::env::temp_dir().join("acl-updater-elevated-test.exe");
        let _ = std::fs::remove_file(&scratch);
        assert_eq!(
            run(b"x", &scratch, Path::new(r"C:\x")).unwrap_err(),
            InstallError::Elevated
        );
        assert!(!scratch.exists(), "an elevated process wrote an installer");
    }

    /// Asking twice gives the same answer.
    ///
    /// Elevation is a property of the process's token and cannot change while it runs, so
    /// a second answer that differed would mean the call is reading something else. It also
    /// exercises the call itself -- one Win32 function, two failure paths, both of which
    /// return `true` because a client that cannot tell should not be running installers.
    #[test]
    fn elevation_does_not_change_under_us() {
        assert_eq!(elevated(), elevated());
    }
}
