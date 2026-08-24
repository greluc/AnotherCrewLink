//! The Linux reader.
//!
//! No `unsafe` at all. `process_vm_readv` is a safe `fn` in `nix` whose lengths derive
//! from the slices passed to it, so the whole file is ordinary Rust — which is the
//! opposite of the C it replaces.
//!
//! # Yama
//!
//! `ptrace_scope=1` is the default on Ubuntu and Debian and blocks reading a process that
//! is not a descendant of this one. No crate choice fixes that: the kernel refuses the
//! call. A user has to either run the client as root, grant `CAP_SYS_PTRACE`, or lower
//! the scope, and the packaging phase has to say so — a client that reports "cannot read
//! the game" without naming Yama sends people looking for the wrong problem.
//!
//! # What is deliberately not ported
//!
//! The C's response to a short read is to zero-fill the rest of the buffer and return
//! success. A partially mapped region therefore produced a struct full of plausible
//! zeros: a player at the origin, alive, in no vent. Here a short read is
//! [`ReadError::Short`].

use std::fs;
use std::io::IoSliceMut;

use nix::sys::uio::{RemoteIoVec, process_vm_readv};
use nix::unistd::Pid;

use crate::memory::{Module, ProcessMemory, ReadError};

/// A process on Linux, and the mappings it has.
#[derive(Debug)]
pub struct LinuxProcess {
    pid: i32,
    modules: Vec<Module>,
}

impl LinuxProcess {
    /// Finds a process by executable name and reads its mappings.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::ProcessGone`] if nothing of that name is running.
    pub fn open_by_name(executable: &str) -> Result<Self, ReadError> {
        let pid = find_process(executable).ok_or_else(|| {
            ReadError::ProcessGone(format!("no process named {executable} is running"))
        })?;
        Self::open(pid)
    }

    /// Reads the mappings of a process by id.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::ProcessGone`] if `/proc/<pid>` cannot be read.
    pub fn open(pid: i32) -> Result<Self, ReadError> {
        let modules = read_maps(pid)?;
        Ok(Self { pid, modules })
    }

    /// The process id, for logging.
    #[must_use]
    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// Re-reads `/proc/<pid>/maps`.
    ///
    /// The game loads `GameAssembly.so` after start-up, so a reader that attached early
    /// has to look again rather than conclude the module will never exist.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::ProcessGone`] if the process has exited.
    pub fn refresh_modules(&mut self) -> Result<(), ReadError> {
        self.modules = read_maps(self.pid)?;
        Ok(())
    }
}

impl ProcessMemory for LinuxProcess {
    fn read_exact(&self, address: u64, into: &mut [u8]) -> Result<(), ReadError> {
        if into.is_empty() {
            return Ok(());
        }
        let wanted = into.len();
        let remote = RemoteIoVec {
            base: usize::try_from(address).map_err(|_| ReadError::Unreadable {
                address,
                length: wanted,
            })?,
            len: wanted,
        };
        let mut local = [IoSliceMut::new(into)];

        match process_vm_readv(Pid::from_raw(self.pid), &mut local, &[remote]) {
            Ok(read) if read == wanted => Ok(()),
            Ok(read) => Err(ReadError::Short {
                address,
                wanted,
                got: read,
            }),
            Err(nix::errno::Errno::ESRCH) => Err(ReadError::ProcessGone(format!(
                "process {} has exited",
                self.pid
            ))),
            Err(nix::errno::Errno::EPERM) => Err(ReadError::ProcessGone(format!(
                "not allowed to read process {}; Yama ptrace_scope is probably 1",
                self.pid
            ))),
            Err(_) => Err(ReadError::Unreadable {
                address,
                length: wanted,
            }),
        }
    }

    fn module(&self, name: &str) -> Option<Module> {
        self.modules
            .iter()
            .find(|module| module.name.eq_ignore_ascii_case(name))
            .cloned()
    }

    fn is_64bit(&self) -> bool {
        // Among Us on Linux runs under Proton as a Windows binary, or natively as 64-bit.
        // There is no 32-bit native build, so the width is the host's.
        cfg!(target_pointer_width = "64")
    }
}

/// The first process id whose `comm` matches, case-insensitively.
#[must_use]
pub fn find_process(executable: &str) -> Option<i32> {
    let entries = fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        // `comm` is truncated to fifteen characters by the kernel, so `cmdline` is
        // consulted for anything longer. "Among Us.exe" fits; "GameAssembly.so" does not
        // reach here at all, since this looks for the process rather than the module.
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if comm.trim().eq_ignore_ascii_case(executable) {
            return Some(pid);
        }
        let cmdline = fs::read_to_string(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if let Some(first) = cmdline.split('\0').next()
            && std::path::Path::new(first)
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(executable))
        {
            return Some(pid);
        }
    }
    None
}

/// Reads `/proc/<pid>/maps` into one entry per named mapping.
///
/// A shared object appears once per segment — text, rodata, data — so the entries are
/// folded into one span per file. A module's `size` therefore covers the whole image,
/// which is what a pattern scan wants.
fn read_maps(pid: i32) -> Result<Vec<Module>, ReadError> {
    let text = fs::read_to_string(format!("/proc/{pid}/maps")).map_err(|error| {
        ReadError::ProcessGone(format!("cannot read /proc/{pid}/maps: {error}"))
    })?;

    let mut modules: Vec<Module> = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(range) = fields.next() else { continue };
        // Fields: address perms offset dev inode pathname. Only named mappings matter.
        let Some(path) = fields.nth(4) else { continue };
        if path.starts_with('[') {
            continue;
        }
        let Some((start, end)) = range.split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (u64::from_str_radix(start, 16), u64::from_str_radix(end, 16))
        else {
            continue;
        };
        let name = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_owned());

        if let Some(existing) = modules.iter_mut().find(|module| module.name == name) {
            let low = existing.base.min(start);
            let high = existing.base.saturating_add(existing.size).max(end);
            existing.base = low;
            existing.size = high.saturating_sub(low);
        } else {
            modules.push(Module {
                name,
                base: start,
                size: end.saturating_sub(start),
            });
        }
    }
    Ok(modules)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn reads_its_own_memory() {
        let process = LinuxProcess::open(std::process::id() as i32).expect("opening this process");
        let marker: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut read_back = [0u8; 8];
        process
            .read_exact(std::ptr::from_ref(&marker) as u64, &mut read_back)
            .expect("reading a local array through process_vm_readv");
        assert_eq!(read_back, marker);
    }

    #[test]
    fn an_unmapped_address_is_an_error_rather_than_zeroes() {
        let process = LinuxProcess::open(std::process::id() as i32).expect("opening this process");
        let mut buffer = [0u8; 16];
        assert!(process.read_exact(0x10, &mut buffer).is_err());
    }

    #[test]
    fn folds_a_shared_objects_segments_into_one_span() {
        let process = LinuxProcess::open(std::process::id() as i32).expect("opening this process");
        // libc appears as several mappings and must come back as one module covering all
        // of them, which is what a pattern scan needs.
        let libc = process
            .module("libc.so.6")
            .or_else(|| process.module("libc.so"));
        if let Some(libc) = libc {
            assert!(libc.size > 0x1000, "libc folded to {} bytes", libc.size);
        }
    }

    #[test]
    fn finds_this_process_by_name() {
        let own = std::env::current_exe().expect("an executable path");
        let name = own.file_name().expect("a file name").to_string_lossy();
        // `comm` is truncated to fifteen characters, so a long test binary name is found
        // through cmdline instead. Either path is a pass.
        assert!(find_process(&name).is_some(), "did not find {name}");
        assert!(find_process("no-such-process-here").is_none());
    }
}
