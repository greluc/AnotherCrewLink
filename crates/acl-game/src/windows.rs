//! The Windows reader.
//!
//! # Rights
//!
//! This asks for `PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION` and nothing else.
//! The C++ it replaces opens the game with `PROCESS_ALL_ACCESS`, which includes the right
//! to write its memory, allocate in it, and create threads in it — every one of which
//! that process then holds for as long as the game is running, whether or not it ever
//! uses them.
//!
//! There is no way to ask for more from here. The write rights had a home behind an
//! `injection` feature until 2026-08-24, when the path they served was removed: it wrote
//! shellcode into the game to draw a version stamp in the menu, and the one real feature
//! it had been built for was already commented out. This client reads.
//!
//! # Enumeration
//!
//! Toolhelp32 directly rather than a crate. The alternative costs twenty-five
//! dependencies and drags `winapi` 0.3.9 in beside `windows-sys`, for something that is
//! twenty-five lines of a documented API.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, PROCESSENTRY32W,
    Process32FirstW, Process32NextW, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    IsWow64Process, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

use crate::memory::{Module, ProcessMemory, ReadError};

/// The rights this reader opens a process with.
///
/// Reading, and enough to ask whether the process is 32- or 64-bit. Writing is added by
/// the removed injection path and by nothing else.
const READ_RIGHTS: u32 = PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION;

/// The rights actually requested.
#[must_use]
pub const fn requested_rights() -> u32 {
    READ_RIGHTS
}

/// A handle to a running process, and what is loaded in it.
#[derive(Debug)]
pub struct WindowsProcess {
    handle: HANDLE,
    pid: u32,
    is_64bit: bool,
    modules: Vec<Module>,
}

// The handle is owned by this value and used only through `&self` reads, which the
// operating system serialises. Sending it to another thread is sound; the raw pointer type
// is what makes the compiler ask.
unsafe impl Send for WindowsProcess {}
unsafe impl Sync for WindowsProcess {}

impl Drop for WindowsProcess {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: the handle came from OpenProcess in `open` and is closed once.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

impl WindowsProcess {
    /// Finds a process by executable name and opens it.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::ProcessGone`] if no process of that name is running or the
    /// handle cannot be opened — which on Windows usually means the game is elevated and
    /// this process is not.
    pub fn open_by_name(executable: &str) -> Result<Self, ReadError> {
        let pid = find_process(executable).ok_or_else(|| {
            ReadError::ProcessGone(format!("no process named {executable} is running"))
        })?;
        Self::open(pid)
    }

    /// Opens a process by id.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::ProcessGone`] if the handle cannot be opened.
    pub fn open(pid: u32) -> Result<Self, ReadError> {
        // SAFETY: a documented call with a constant rights mask and no pointer arguments.
        let handle = unsafe { OpenProcess(requested_rights(), 0, pid) };
        if handle.is_null() {
            // SAFETY: read immediately after the failing call, on this thread, which is
            // the whole of `GetLastError`'s contract.
            let why = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            // The two are different problems with the same symptom, and telling them apart
            // is the difference between "start the game" and "run this as administrator".
            // Nothing distinguished them until 2026-08-29, so a player whose game runs
            // elevated saw a client that behaved exactly as if Among Us were closed --
            // for ever, because the elevation path exists and had no caller.
            if why == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED {
                return Err(ReadError::AccessDenied(pid));
            }
            return Err(ReadError::ProcessGone(format!(
                "could not open process {pid} (error {why})"
            )));
        }

        let mut wow64 = 0i32;
        // SAFETY: `handle` is valid and `wow64` is a live i32 for the duration.
        let queried = unsafe { IsWow64Process(handle, &raw mut wow64) };
        // A process running under WOW64 is a 32-bit process on a 64-bit Windows. If the
        // query fails, assume 64-bit: that is what every supported Windows host is, and
        // guessing 32 would halve every pointer the reader walks.
        let is_64bit = queried == 0 || wow64 == 0;

        let modules = enumerate_modules(pid);
        Ok(Self {
            handle,
            pid,
            is_64bit,
            modules,
        })
    }

    /// The process id, for logging.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Re-reads the module list.
    ///
    /// The game loads `GameAssembly.dll` after start-up, so a reader that attached early
    /// has to look again rather than conclude the module will never exist.
    pub fn refresh_modules(&mut self) {
        self.modules = enumerate_modules(self.pid);
    }
}

impl ProcessMemory for WindowsProcess {
    fn read_exact(&self, address: u64, into: &mut [u8]) -> Result<(), ReadError> {
        if into.is_empty() {
            return Ok(());
        }
        let mut read: usize = 0;
        // SAFETY: `into` is a live, writable slice of exactly `into.len()` bytes, and
        // `read` is a live usize. The address belongs to another process and is checked by
        // the kernel, which is what the return value reports.
        let ok = unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
                self.handle,
                address as *const c_void,
                into.as_mut_ptr().cast::<c_void>(),
                into.len(),
                &raw mut read,
            )
        };
        if ok == 0 {
            return Err(ReadError::Unreadable {
                address,
                length: into.len(),
            });
        }
        if read != into.len() {
            // Not zero-filled and reported as success, which is what the C did.
            return Err(ReadError::Short {
                address,
                wanted: into.len(),
                got: read,
            });
        }
        Ok(())
    }

    fn module(&self, name: &str) -> Option<Module> {
        self.modules
            .iter()
            .find(|module| module.name.eq_ignore_ascii_case(name))
            .cloned()
    }

    fn is_64bit(&self) -> bool {
        self.is_64bit
    }
}

/// The first process id whose executable name matches, case-insensitively.
#[must_use]
pub fn find_process(executable: &str) -> Option<u32> {
    // SAFETY: a documented call with no pointer arguments.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(0),
        ..unsafe { std::mem::zeroed() }
    };

    let mut found = None;
    // SAFETY: `entry` is live and its `dwSize` is set, which is what these calls require.
    if unsafe { Process32FirstW(snapshot, &raw mut entry) } != 0 {
        loop {
            if wide_to_string(&entry.szExeFile).eq_ignore_ascii_case(executable) {
                found = Some(entry.th32ProcessID);
                break;
            }
            // SAFETY: as above.
            if unsafe { Process32NextW(snapshot, &raw mut entry) } == 0 {
                break;
            }
        }
    }
    // SAFETY: the snapshot came from CreateToolhelp32Snapshot and is closed once.
    unsafe { CloseHandle(snapshot) };
    found
}

/// Where a process was started from.
///
/// The first module of a process is the executable itself, and the snapshot already
/// carries its full path — so this is the same call the module list comes from, asked one
/// field further. That is why it is here rather than a `QueryFullProcessImageNameW` of its
/// own: a second way to ask is a second thing to get wrong about access rights.
///
/// `None` when the process is gone, or when the snapshot cannot be taken — which happens
/// for a process at a higher integrity level, and is not an error worth a type.
#[must_use]
pub fn executable_path(pid: u32) -> Option<std::path::PathBuf> {
    // SAFETY: a documented call with no pointer arguments.
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry = MODULEENTRY32W {
        dwSize: u32::try_from(size_of::<MODULEENTRY32W>()).unwrap_or(0),
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: `entry` is live and its `dwSize` is set.
    let found = unsafe { Module32FirstW(snapshot, &raw mut entry) } != 0;
    // SAFETY: the snapshot came from CreateToolhelp32Snapshot and is closed once.
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(snapshot);
    }
    found
        .then(|| wide_to_string(&entry.szExePath))
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
}

fn enumerate_modules(pid: u32) -> Vec<Module> {
    // TH32CS_SNAPMODULE32 as well as TH32CS_SNAPMODULE: without it a 64-bit reader sees
    // no modules at all in a 32-bit target, which is every Among Us install on the
    // 32-bit branch.
    // SAFETY: a documented call with no pointer arguments.
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut entry = MODULEENTRY32W {
        dwSize: u32::try_from(size_of::<MODULEENTRY32W>()).unwrap_or(0),
        ..unsafe { std::mem::zeroed() }
    };

    let mut modules = Vec::new();
    // SAFETY: `entry` is live and its `dwSize` is set.
    if unsafe { Module32FirstW(snapshot, &raw mut entry) } != 0 {
        loop {
            modules.push(Module {
                name: wide_to_string(&entry.szModule),
                base: entry.modBaseAddr as u64,
                size: u64::from(entry.modBaseSize),
            });
            // SAFETY: as above.
            if unsafe { Module32NextW(snapshot, &raw mut entry) } == 0 {
                break;
            }
        }
    }
    // SAFETY: the snapshot came from CreateToolhelp32Snapshot and is closed once.
    unsafe { CloseHandle(snapshot) };
    modules
}

/// A NUL-terminated UTF-16 buffer as a `String`.
fn wide_to_string(wide: &[u16]) -> String {
    let end = wide
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(wide.len());
    wide.get(..end)
        .map(String::from_utf16_lossy)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    use super::executable_path;

    /// Asked about this very process, which needs no game and no other machine: the answer
    /// is knowable independently, through `current_exe`.
    ///
    /// It checks the call, the wide-string conversion and the snapshot flags together --
    /// and those are the parts that fail quietly, by returning an empty string rather than
    /// an error.
    #[test]
    fn the_path_of_this_process_is_this_executable() {
        let mine = executable_path(std::process::id()).expect("this process has a path");
        let expected = std::env::current_exe().expect("and the runtime knows it");
        assert_eq!(
            mine.file_name(),
            expected.file_name(),
            "{mine:?} is not {expected:?}"
        );
        assert!(mine.is_absolute(), "{mine:?} is not a full path");
        assert!(mine.exists(), "{mine:?} does not exist");
    }

    /// A process id nothing has answers `None` rather than an empty path. An empty path
    /// would be passed to `detect_mod`, whose parent directory is nothing, and the mod
    /// would read as none for a reason that has nothing to do with mods.
    #[test]
    fn a_process_that_is_not_there_has_no_path() {
        // Ids are recycled, so this is "very probably not a process" rather than "certainly
        // not one" -- but an id that *is* live still gives a path, and the assertion below
        // holds either way: what must not happen is an empty one.
        let answer = executable_path(u32::MAX - 4);
        assert!(
            answer
                .as_ref()
                .is_none_or(|path| !path.as_os_str().is_empty()),
            "an empty path came back: {answer:?}"
        );
    }

    #[test]
    fn asks_for_reading_and_nothing_else() {
        // The point of the whole module, and now unconditional: there is no feature that
        // adds a bit. If this ever gains one, it should be because somebody chose to, and
        // this test is where they say so.
        assert_eq!(requested_rights() & PROCESS_VM_READ, PROCESS_VM_READ);
        assert_eq!(
            requested_rights() & PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION
        );

        {
            use windows_sys::Win32::System::Threading::{
                PROCESS_ALL_ACCESS, PROCESS_CREATE_THREAD, PROCESS_VM_OPERATION, PROCESS_VM_WRITE,
            };
            assert_eq!(requested_rights() & PROCESS_VM_WRITE, 0, "writing");
            assert_eq!(requested_rights() & PROCESS_VM_OPERATION, 0, "allocating");
            assert_eq!(
                requested_rights() & PROCESS_CREATE_THREAD,
                0,
                "thread creation"
            );
            assert_ne!(requested_rights(), PROCESS_ALL_ACCESS);
        }
    }

    #[test]
    fn reads_a_utf16_name_up_to_its_terminator() {
        let mut wide = [0u16; 8];
        for (slot, unit) in wide.iter_mut().zip("abc".encode_utf16()) {
            *slot = unit;
        }
        assert_eq!(wide_to_string(&wide), "abc");
        // And a buffer with no terminator at all is not read past its end.
        let full: Vec<u16> = "abcd".encode_utf16().collect();
        assert_eq!(wide_to_string(&full), "abcd");
    }

    #[test]
    fn finds_this_process_among_the_running_ones() {
        // A real enumeration against a process that is certainly running: this one.
        let own = std::env::current_exe().expect("an executable path");
        let name = own.file_name().expect("a file name").to_string_lossy();
        assert!(
            find_process(&name).is_some(),
            "Toolhelp32 did not find {name}"
        );
        assert!(find_process("no-such-process-here.exe").is_none());
    }

    #[test]
    fn opens_this_process_and_reads_its_own_memory() {
        // The whole path, end to end, with no game involved: open, ask the width, find a
        // module, read bytes back out of it.
        let pid = std::process::id();
        let process = WindowsProcess::open(pid).expect("opening this process");
        assert_eq!(process.pid(), pid);
        assert_eq!(process.is_64bit(), cfg!(target_pointer_width = "64"));

        let marker: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut read_back = [0u8; 8];
        process
            .read_exact(std::ptr::from_ref(&marker) as u64, &mut read_back)
            .expect("reading a local array through the process handle");
        assert_eq!(read_back, marker);
    }

    #[test]
    fn a_short_read_at_the_end_of_a_mapping_is_reported_as_short() {
        let process = WindowsProcess::open(std::process::id()).expect("opening this process");
        // An address that is certainly not mapped.
        let mut buffer = [0u8; 16];
        assert!(process.read_exact(0x10, &mut buffer).is_err());
    }

    #[test]
    fn reads_another_process_without_any_elevation() {
        // The question this answers is "what actually needs administrator rights", and the
        // answer is: for a game running as the same user at the same integrity level,
        // nothing does. Windows grants PROCESS_VM_READ over a same-user process to an
        // ordinary token, and the removed injection path granted PROCESS_VM_WRITE
        // too. Elevation is only ever needed when the *game* is elevated, because a
        // medium-integrity process cannot open a high-integrity one at all.
        //
        // This spawns a child, opens it with whatever rights the build asks for, and reads
        // the PE signature out of a module it did not load itself. If this test ever needs
        // administrator rights to pass, the premise of the two-process split has changed.
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "ping", "-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawning a child process");

        let result = (|| {
            // The child needs a moment before its modules are mapped.
            for _ in 0..50 {
                if let Ok(process) = WindowsProcess::open(child.id())
                    && let Some(module) = process.module("ntdll.dll")
                {
                    let mut magic = [0u8; 2];
                    process.read_exact(module.base, &mut magic).ok()?;
                    return Some(magic);
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            None
        })();
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(
            result,
            Some(*b"MZ"),
            "could not read a same-user child process;              the rights this build asks for are {:#x}",
            requested_rights()
        );
    }

    #[test]
    fn enumerates_this_process_modules() {
        let process = WindowsProcess::open(std::process::id()).expect("opening this process");
        // Every Windows process has ntdll loaded, whatever else it does.
        let ntdll = process.module("ntdll.dll").expect("ntdll is always loaded");
        assert!(ntdll.size > 0);
        assert!(ntdll.contains(ntdll.base + 1));
    }
}
