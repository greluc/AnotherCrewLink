//! Whether another client already owns this installation.
//!
//! §4.7 of the implementation plan asks for a single-instance lock "so a 1.x and a 2.x
//! install on one machine cannot run two keyboard hooks, two overlays and two memory
//! readers against the same game", and then records that the mechanism it originally
//! named — a mutex called `Local\AnotherCrewLink` — would exclude other copies of *this*
//! client and nothing else, which is the failure the lock exists to prevent while looking
//! exactly like the fix. It left the real mechanism to be measured rather than assumed.
//!
//! # The measurement
//!
//! Taken on 2026-08-26 against a running 1.0.2 client, by enumerating its windows and its
//! named kernel objects:
//!
//! ```text
//! MSG  class=[Chrome_MessageWindow]  text=[C:\Users\lucas\AppData\Roaming\AnotherCrewLink]
//! mutex Local\AnotherCrewLink                                        does not exist
//! ```
//!
//! So Chromium's `ProcessSingleton` is a message-only window whose class is a Chromium
//! constant and whose **window text is the user-data directory**. That is what a second
//! Electron launch collides with, and it is the only thing a running 1.x holds that a
//! different implementation can find. Three properties of the lookup were measured rather
//! than assumed, because each one is a way to write this and have it silently never match:
//!
//! * the text carries **no trailing separator** — searching for one that ends in a
//!   separator finds nothing;
//! * the match is **case-insensitive**, so a differently-spelled path still collides;
//! * one process held **two** windows of that class and only one carried the path, so the
//!   class alone is not the lock. Class and text together are.
//!
//! # What this closes, and what it does not
//!
//! [`claim`] refuses to start when a 1.x is running, and excludes other 2.x clients from
//! each other. The reverse direction — a 1.x started while a 2.x is already running —
//! **cannot be closed from this side alone**. A running 1.x looks for one thing, that
//! window, and the only way to be found by it is to register the same class and answer
//! Chromium's `WM_COPYDATA` handshake convincingly enough that the newcomer believes it
//! handed its command line over and exits. Getting that wrong does not fail safe: a
//! newcomer that times out on the handshake concludes the lock is stale and takes it.
//!
//! The cheap half of that direction is a name added to 1.x, which is our own client:
//! `src/main/index.ts` takes [`SHARED_MUTEX_NAME`] alongside Electron's own lock, so a
//! patched 1.x collides with a 2.x through the same object 2.x uses for itself. Field
//! installs that never take that patch are covered by the window, which is why both are
//! checked.

use std::path::Path;

/// The window class Chromium's `ProcessSingleton` registers.
///
/// A Chromium constant, not an Electron or an application one, so it does not move when
/// this application is renamed — and does not distinguish this application from any other
/// Chromium app either. See [`Occupant::ElectronClient`] for why that is safe here.
pub const SINGLETON_WINDOW_CLASS: &str = "Chrome_MessageWindow";

/// The one name both major versions can spell, held by every client that supports it.
///
/// Fixed rather than derived from the path, because 1.x has to be able to take it in a few
/// lines of TypeScript without reproducing [`lock_name`]'s hash. It is therefore coarser:
/// it also excludes two 2.x installations that keep their files in different directories,
/// which [`lock_name`] would allow. That is the wrong answer in a case nobody has, in
/// exchange for the right answer in the case everybody has — and it is why it is claimed
/// last, after the answers that can be specific have had their turn.
pub const SHARED_MUTEX_NAME: &str = r"Local\AnotherCrewLink.shared-instance";

/// What was found already running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Occupant {
    /// An Electron client keyed on this same user-data directory.
    ///
    /// Strictly, any Chromium application whose profile is that directory — the class is
    /// Chromium's. Nothing else puts a profile in 1.x's `%APPDATA%\AnotherCrewLink`, so in
    /// practice this is 1.x, and treating a stranger there as a reason not to start is the
    /// conservative answer anyway.
    ElectronClient,
    /// Another client keeping its files in this same directory.
    ///
    /// The specific answer, and the one worth reaching before the coarse one below.
    SelfSame,
    /// Somebody holds [`SHARED_MUTEX_NAME`], and it is not this installation.
    ///
    /// Deliberately uncertain. That name is coarse — it is the one a patched 1.x can spell
    /// without reproducing [`lock_name`] — so holding it means either a 1.x, or a 2.x
    /// installed somewhere else on this machine. The refusal is right either way and the
    /// message does not claim to know which.
    OtherInstallation,
}

impl Occupant {
    /// What to tell the user, in one line.
    ///
    /// Names the other process rather than the mechanism: "another client is running" is
    /// actionable and "the mutex was already held" is not.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::ElectronClient => {
                "AnotherCrewLink 1.x is already running. Close it before starting this one: \
                 two clients would run two keyboard hooks and two memory readers against \
                 the same game."
            }
            Self::SelfSame => "AnotherCrewLink is already running.",
            Self::OtherInstallation => {
                "Another AnotherCrewLink is already running — a 1.x, or a copy installed \
                 elsewhere on this machine. Close it before starting this one."
            }
        }
    }
}

/// The kernel-object name that identifies this installation.
///
/// Derived from the user-data directory rather than from the product, for the same reason
/// Chromium derives its window text from it: two installations that keep their files in
/// different places are two installations, and a lock that ignored the difference would
/// stop a portable copy from running beside an installed one.
///
/// The path is hashed rather than embedded. A backslash separates namespaces in a kernel
/// object name and may not appear anywhere else in one, so a path cannot be used
/// literally; and the alternative — substituting the separator — produces one name for two
/// directories that differ only where the substitution landed. The hash is FNV-1a and
/// written out here rather than taken from `std`, because `DefaultHasher` is not
/// guaranteed to give the same answer in two processes and this name must.
///
/// Lower-cased first: Windows paths are case-insensitive, the window lookup this sits
/// beside was measured to be too, and two launches that spell one directory differently
/// have to take one lock.
#[must_use]
pub fn lock_name(user_data: &Path) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    for byte in user_data.to_string_lossy().to_lowercase().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    // `Local\`, not `Global\`. A second user signed in to the same machine has their own
    // game, their own user-data directory and their own reason to run this; `Global\`
    // would let one of them lock the other out.
    format!(r"Local\AnotherCrewLink.{hash:016x}")
}

#[cfg(windows)]
mod platform {
    use super::{Occupant, SHARED_MUTEX_NAME, SINGLETON_WINDOW_CLASS, lock_name};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, HWND_MESSAGE};

    /// A null-terminated UTF-16 copy, because every name here crosses into Win32.
    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Holds the lock until it is dropped.
    ///
    /// The handles are what hold it: a named mutex lives as long as one handle to it is
    /// open, so releasing is closing. Nothing waits on either and nothing acquires them —
    /// the question is only ever whether creating one found it already there.
    #[derive(Debug)]
    pub struct Guard {
        handles: Vec<HANDLE>,
    }

    // SAFETY: a mutex handle is a kernel object reference, valid in any thread of the
    // process that owns it. The guard never acquires the mutex, so there is no thread
    // affinity to preserve.
    unsafe impl Send for Guard {}

    impl Drop for Guard {
        fn drop(&mut self) {
            for handle in self.handles.drain(..) {
                if !handle.is_null() {
                    // SAFETY: every handle came from CreateMutexW in `take` and reaches
                    // this drain exactly once.
                    unsafe { CloseHandle(handle) };
                }
            }
        }
    }

    /// Whether an Electron client is running against this user-data directory.
    ///
    /// The lookup a second Electron launch performs, and the measurement in the module
    /// documentation is of exactly this call returning a handle.
    #[must_use]
    pub fn electron_client_running(user_data: &Path) -> bool {
        let class = wide(OsStr::new(SINGLETON_WINDOW_CLASS));
        let text = wide(user_data.as_os_str());
        // SAFETY: both pointers are to null-terminated buffers that outlive the call, and
        // HWND_MESSAGE is the documented parent for message-only windows.
        let found = unsafe {
            FindWindowExW(
                HWND_MESSAGE,
                std::ptr::null_mut(),
                class.as_ptr(),
                text.as_ptr(),
            )
        };
        !found.is_null()
    }

    /// Creates a named mutex, and gives back its handle only if it was not already there.
    fn take(name: &str) -> Option<HANDLE> {
        let name = wide(OsStr::new(name));
        // SAFETY: a documented call; the name outlives it, and the other two arguments are
        // the documented defaults for an unowned mutex with no security descriptor.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            // Creation failed outright rather than collided. Reported as "somebody else
            // has it", because the alternative is starting a second reader on the strength
            // of a call that did not answer.
            return None;
        }
        // SAFETY: no call has intervened since CreateMutexW, so this is its error code.
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if existed {
            // SAFETY: a handle CreateMutexW returned, closed once, on the only path that
            // does not hand it to a Guard.
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(handle)
    }

    /// Claims this installation for this process.
    ///
    /// # Errors
    ///
    /// The [`Occupant`] already holding it. Every arm is an ordinary outcome that ends in
    /// a message and an exit, not a failure to report.
    pub fn claim(user_data: &Path, legacy_user_data: Option<&Path>) -> Result<Guard, Occupant> {
        // The Electron window first, and before anything is created. It is the case a user
        // actually hits — a 1.x they forgot was running — and checking it first means the
        // refusal path leaves no object behind for the next launch to trip over.
        //
        // **Asked about 1.x's directory, not this one.** The window's title *is* the
        // profile path, so probing with 2.x's directory finds nothing however many 1.x
        // clients are running — which is what would have happened when the two versions
        // stopped sharing a directory, silently, with two readers on one game.
        if legacy_user_data.is_some_and(electron_client_running) {
            return Err(Occupant::ElectronClient);
        }
        // Then this installation's own name — the specific answer, taken before the coarse
        // one so that a second 2.x is diagnosed as a second 2.x. The other order refuses
        // just as correctly and then tells the user a 1.x is running, which is a wrong
        // sentence about a right decision.
        let own = take(&lock_name(user_data)).ok_or(Occupant::SelfSame)?;
        // Into a Guard immediately, before the second name is attempted. The `?` below can
        // return, and a bare handle held in a local at that moment is a name left claimed
        // by a process that is on its way out; owned by the Guard, the early return closes
        // it on the way past.
        let mut guard = Guard { handles: vec![own] };
        // And last the shared name, which a patched 1.x also takes, so that the two
        // versions exclude each other wherever 1.x has the patch.
        let shared = take(SHARED_MUTEX_NAME).ok_or(Occupant::OtherInstallation)?;
        guard.handles.push(shared);
        Ok(guard)
    }
}

#[cfg(windows)]
pub use platform::{Guard, claim, electron_client_running};

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Occupant, SHARED_MUTEX_NAME, lock_name};
    use std::path::Path;

    #[test]
    fn the_name_is_stable_for_one_directory() {
        let path = Path::new(r"C:\Users\lucas\AppData\Roaming\AnotherCrewLink");
        assert_eq!(lock_name(path), lock_name(path));
    }

    #[test]
    fn two_installations_get_two_names() {
        assert_ne!(
            lock_name(Path::new(r"C:\Users\lucas\AppData\Roaming\AnotherCrewLink")),
            lock_name(Path::new(r"D:\Portable\AnotherCrewLink"))
        );
    }

    /// The window lookup beside this one was measured to be case-insensitive, so a launch
    /// that spells the directory differently finds the running Electron client. This name
    /// has to agree, or the two halves of `claim` would disagree about whether two launches
    /// are the same installation.
    #[test]
    fn case_does_not_make_a_second_installation() {
        assert_eq!(
            lock_name(Path::new(r"C:\Users\lucas\AppData\Roaming\AnotherCrewLink")),
            lock_name(Path::new(r"c:\users\lucas\appdata\roaming\anothercrewlink"))
        );
    }

    /// A kernel object name may carry exactly one backslash, the namespace separator. A
    /// name built by substituting the path's separators would carry several and be
    /// rejected at creation — which fails open, since a mutex that cannot be created looks
    /// the same as one that was not there.
    #[test]
    fn the_name_carries_one_separator() {
        let name = lock_name(Path::new(r"C:\Users\lucas\AppData\Roaming\AnotherCrewLink"));
        assert_eq!(name.matches('\\').count(), 1);
        assert!(name.starts_with(r"Local\"));
        assert_eq!(SHARED_MUTEX_NAME.matches('\\').count(), 1);
    }

    /// The two names must not be the same object: the shared one is deliberately coarse and
    /// taken by 1.x too, and the derived one is what tells two 2.x installations apart.
    #[test]
    fn the_shared_name_is_not_a_derived_one() {
        assert_ne!(
            SHARED_MUTEX_NAME,
            lock_name(Path::new(r"C:\Users\lucas\AppData\Roaming\AnotherCrewLink"))
        );
    }

    /// The platform half, end to end, against real kernel objects.
    ///
    /// One test rather than three, and deliberately: `SHARED_MUTEX_NAME` is a single
    /// machine-wide name, so two test functions taking it would collide with each other
    /// under the default parallel harness and fail for a reason that has nothing to do
    /// with what either was checking.
    ///
    /// The directory is one nothing owns. Chromium is not running against it, so the
    /// window arm is not what this exercises — the mutex arms are, and they are the two
    /// that a second copy of this client actually reaches.
    #[cfg(windows)]
    #[test]
    fn a_second_claim_on_one_installation_is_refused_and_released() {
        use super::claim;

        let directory = std::env::temp_dir().join("acl-single-instance-test");

        let held = claim(&directory, None).expect("nothing owns a temporary directory");
        // The specific answer, not the coarse one. Getting `OtherInstallation` here would
        // mean the shared name is being claimed first, and a second 2.x would be told a
        // 1.x is running.
        assert_eq!(claim(&directory, None).err(), Some(Occupant::SelfSame));

        drop(held);
        // Both names, not just the derived one: a guard that released the specific name
        // and kept the shared one would let one client through and then refuse every
        // client afterwards, including itself.
        claim(&directory, None).expect("dropping the guard releases every name it took");
    }

    /// The positive half of the window lookup, against a 1.x that is actually running.
    ///
    /// Ignored, because it needs something this machine may not have. It is here rather
    /// than in a notebook because it is the only check that the class name and the text
    /// are still what they were measured to be — and a Chromium or Electron upgrade is
    /// exactly the kind of thing that would move one of them without any diff in this
    /// repository. Start the shipped client, then:
    ///
    /// ```text
    /// cargo test -p acl-core -- --ignored find_the_running_electron_client
    /// ```
    #[cfg(windows)]
    #[test]
    #[ignore = "needs the shipped 1.x client to be running"]
    fn find_the_running_electron_client() {
        // The running environment, not `option_env!`: that reads the variable of whoever
        // compiled this, which on a build machine is a different user's profile.
        let app_data = std::env::var("APPDATA").expect("APPDATA is set on Windows");
        let paths = crate::paths::Paths::resolve(crate::paths::Environment {
            app_data: Some(&app_data),
        })
        .expect("APPDATA names a directory");

        assert!(
            super::electron_client_running(paths.user_data()),
            "no Chrome_MessageWindow carries {} — either the client is not running, or              Chromium has changed what ProcessSingleton is named",
            paths.user_data().display()
        );
    }

    /// No Electron client keeps its profile in a temporary directory, so this is the
    /// negative half of the window lookup — the half that has to be right for the client
    /// to start at all.
    #[cfg(windows)]
    #[test]
    fn no_electron_client_owns_a_directory_nothing_owns() {
        assert!(!super::electron_client_running(
            &std::env::temp_dir().join("acl-no-electron-here")
        ));
    }

    #[test]
    fn every_occupant_says_what_is_running() {
        for occupant in [
            Occupant::ElectronClient,
            Occupant::SelfSame,
            Occupant::OtherInstallation,
        ] {
            assert!(occupant.message().contains("AnotherCrewLink"));
        }
    }
}
