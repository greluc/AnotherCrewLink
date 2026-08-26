//! Starting the elevated half, and what it means when the user says no.
//!
//! [`crate::helper`] holds the decisions: what still works without a helper, and when the
//! client is allowed to ask for one. This is the call those decisions were written for.
//!
//! # Two ways to start it, and the order matters
//!
//! §6 of `docs/rust-port/06-security.md`: the core "starts the helper on demand,
//! unelevated, and re-launches it through UAC only when the game's integrity level denies
//! the read". So [`Elevation::AsIs`] is tried first and [`Elevation::Elevated`] is the
//! second attempt, not the first. A prompt nobody needed is a prompt answered No next
//! time — and most players do not run the game elevated.
//!
//! There is no service. Elevation is per launch and per session, and it lapses when the
//! process exits: a permanently installed `LocalSystem` component with debug-level access
//! to arbitrary processes is a larger standing privilege than a dialog.
//!
//! # Why the process id comes back
//!
//! It is half of the mutual check across the pipe. The helper is told this process's id on
//! its command line and refuses a client that is not it; this side is told the helper's id
//! by the launch and refuses a pipe server that is not it. Neither end trusts the name.

use std::path::Path;

/// How to start it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// At this process's own integrity level. No prompt.
    AsIs,
    /// Through UAC, with a prompt the user can decline.
    Elevated,
}

/// Why the helper did not start.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    /// The user answered No to the elevation prompt.
    ///
    /// An ordinary state and not a failure — §4.7 is explicit about it. The caller moves
    /// [`crate::helper::HelperState`] to `Refused`, says accurately what does not work,
    /// and carries on: voice, including push-to-talk, does not depend on the helper.
    #[error("the elevation prompt was declined")]
    Refused,
    /// Anything else.
    #[error("could not start the helper: {0}")]
    Io(#[from] std::io::Error),
}

/// The name of the binary, beside this one.
///
/// Beside, and not searched for on `PATH`: `PATH` is writable by things this client is not
/// in a position to vouch for, and what would be started from it runs elevated.
pub const HELPER_EXECUTABLE: &str = "acl-helper.exe";

/// Where the helper is, given where this executable is.
///
/// # Errors
///
/// Whatever `current_exe` says. A client that cannot locate itself cannot locate anything
/// beside it either, and guessing a directory here is guessing which binary to elevate.
pub fn helper_beside_this_one() -> std::io::Result<std::path::PathBuf> {
    let mut path = std::env::current_exe()?;
    path.set_file_name(HELPER_EXECUTABLE);
    Ok(path)
}

/// The command line the helper is started with.
///
/// One argument, and it is the whole handshake: both ends derive the pipe name from it and
/// both ends check the other's process id against it. Built here rather than at the call
/// site so that the string the helper parses and the string this side sends cannot drift.
#[must_use]
pub fn arguments(core_process_id: u32) -> [String; 2] {
    ["--core-pid".to_owned(), core_process_id.to_string()]
}

#[cfg(windows)]
mod platform {
    use super::{Elevation, LaunchError, arguments};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
    use std::path::Path;
    use std::ptr;
    use windows_sys::Win32::Foundation::{ERROR_CANCELLED, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{GetProcessId, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    /// A null-terminated UTF-16 copy.
    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// A running helper.
    ///
    /// Holding the handle is what makes [`Self::is_running`] answerable: without one, the
    /// process id can be reused by the time anybody asks, and the answer would be about
    /// somebody else's process.
    #[derive(Debug)]
    pub struct Helper {
        process_id: u32,
        handle: OwnedHandle,
    }

    // SAFETY: a process handle is a kernel object reference with no thread affinity.
    unsafe impl Send for Helper {}

    impl Helper {
        /// Its process id, for the check at the other end of the pipe.
        #[must_use]
        pub const fn process_id(&self) -> u32 {
            self.process_id
        }

        /// Whether it is still there.
        ///
        /// Answered from the handle rather than by looking the id up, so a helper that
        /// exited and had its id reused reads as gone rather than as somebody else.
        #[must_use]
        pub fn is_running(&self) -> bool {
            // SAFETY: a valid process handle; zero means "do not wait, just report".
            let waited = unsafe { WaitForSingleObject(self.handle.as_raw_handle().cast(), 0) };
            waited == WAIT_TIMEOUT
        }
    }

    /// Starts the helper.
    ///
    /// # Errors
    ///
    /// [`LaunchError::Refused`] when the elevation prompt is declined, which is an ordinary
    /// state rather than a failure; [`LaunchError::Io`] for anything else.
    pub fn start(
        executable: &Path,
        core_process_id: u32,
        elevation: Elevation,
    ) -> Result<Helper, LaunchError> {
        match elevation {
            Elevation::AsIs => start_as_is(executable, core_process_id),
            Elevation::Elevated => start_elevated(executable, core_process_id),
        }
    }

    /// The ordinary case, which is also the common one.
    fn start_as_is(executable: &Path, core_process_id: u32) -> Result<Helper, LaunchError> {
        let child = std::process::Command::new(executable)
            .args(arguments(core_process_id))
            .spawn()?;
        let process_id = child.id();
        Ok(Helper {
            process_id,
            // The handle the child already carries, taken over rather than opened again:
            // reopening by id has the same reuse race `is_running` exists to avoid.
            //
            // SAFETY: `into_raw_handle` transfers ownership of a live process handle.
            handle: unsafe { OwnedHandle::from_raw_handle(child.into_raw_handle()) },
        })
    }

    /// The elevated case, which is `ShellExecuteEx` because nothing in `std` can do it.
    ///
    /// `CreateProcess` cannot elevate — elevation goes through the `AppInfo` service, and the
    /// shell is the documented way to ask it. That is also why the pipe is named rather
    /// than inherited: this path cannot pass a handle to the child.
    fn start_elevated(executable: &Path, core_process_id: u32) -> Result<Helper, LaunchError> {
        let verb = wide(OsStr::new("runas"));
        let file = wide(executable.as_os_str());
        let parameters = wide(OsStr::new(&arguments(core_process_id).join(" ")));

        let mut info = SHELLEXECUTEINFOW {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the struct's own field is u32 and the struct is far smaller"
            )]
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            // `NOCLOSEPROCESS` is what returns a handle at all; without it there is no way
            // to learn the child's process id, and the pipe check has nothing to compare
            // against. `NOASYNC` because this function returns a started process, and the
            // asynchronous form can return before the shell has finished with the request.
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            hwnd: ptr::null_mut(),
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            lpDirectory: ptr::null(),
            // The helper has no window. A console flashing up in front of a game is not a
            // thing to ship, and the prompt itself is the shell's, not ours.
            nShow: SW_HIDE,
            hInstApp: ptr::null_mut(),
            lpIDList: ptr::null_mut(),
            lpClass: ptr::null(),
            hkeyClass: ptr::null_mut(),
            dwHotKey: 0,
            Anonymous: unsafe { std::mem::zeroed() },
            hProcess: ptr::null_mut(),
        };

        // SAFETY: every pointer in `info` is either null or to a buffer that outlives the
        // call, and `cbSize` is the size of the struct being passed.
        let started = unsafe { ShellExecuteExW(&raw mut info) };
        if started == 0 {
            let error = std::io::Error::last_os_error();
            // The one error code that is not an error. `ERROR_CANCELLED` from `runas` is
            // the user having answered No, which §4.7 requires be an ordinary state.
            if error.raw_os_error() == Some(ERROR_CANCELLED.cast_signed()) {
                return Err(LaunchError::Refused);
            }
            return Err(LaunchError::Io(error));
        }
        if info.hProcess.is_null() {
            return Err(LaunchError::Io(std::io::Error::other(
                "the shell started the helper but returned no handle to it",
            )));
        }
        // SAFETY: a live process handle the shell handed over because of NOCLOSEPROCESS.
        let handle = unsafe { OwnedHandle::from_raw_handle(info.hProcess.cast()) };
        // SAFETY: the handle is valid and owned above.
        let process_id = unsafe { GetProcessId(handle.as_raw_handle().cast()) };
        Ok(Helper { process_id, handle })
    }
}

#[cfg(windows)]
pub use platform::{Helper, start};

/// Whether a path is one this client should be willing to elevate.
///
/// Not a security boundary — anything that can write beside the client can also replace the
/// client — but it does refuse the two shapes that would turn a bug elsewhere into an
/// elevated launch of somebody else's binary: a relative path, which resolves against a
/// working directory this process does not control, and a name that is not the helper's.
#[must_use]
pub fn is_plausible_helper(path: &Path) -> bool {
    path.is_absolute()
        && path
            .file_name()
            .is_some_and(|name| name == HELPER_EXECUTABLE)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{HELPER_EXECUTABLE, arguments, helper_beside_this_one, is_plausible_helper};
    use std::path::Path;

    /// The helper parses exactly this, and a change on either side that is not made on both
    /// produces a helper that exits with "no --core-pid" and a core that waits for a pipe
    /// nobody creates.
    #[test]
    fn the_command_line_is_the_one_the_helper_parses() {
        assert_eq!(
            arguments(4321),
            ["--core-pid".to_owned(), "4321".to_owned()]
        );
    }

    #[test]
    fn the_helper_is_looked_for_beside_this_executable() {
        let path = helper_beside_this_one().expect("this executable has a path");
        assert!(path.is_absolute());
        assert_eq!(path.file_name().unwrap(), HELPER_EXECUTABLE);
        assert_eq!(
            path.parent(),
            std::env::current_exe().unwrap().parent(),
            "the helper must come from this executable's own directory"
        );
    }

    #[test]
    fn a_relative_path_is_not_something_to_elevate() {
        assert!(!is_plausible_helper(Path::new("acl-helper.exe")));
        assert!(!is_plausible_helper(Path::new("../acl-helper.exe")));
    }

    #[test]
    fn nor_is_a_different_binary() {
        assert!(!is_plausible_helper(Path::new(
            r"C:\Windows\System32\cmd.exe"
        )));
        assert!(is_plausible_helper(Path::new(
            r"C:\Program Files\AnotherCrewLink\acl-helper.exe"
        )));
    }
}
