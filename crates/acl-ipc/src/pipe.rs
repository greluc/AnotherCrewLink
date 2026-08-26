//! The named pipe the two processes talk over.
//!
//! [`crate::stream::StreamTransport`] is generic over anything that reads and writes; this
//! is what it reads and writes on. Everything here is Windows, and everything here is a
//! consequence of one asymmetry: one end of this pipe may be elevated and the other never
//! is.
//!
//! # Which end creates it
//!
//! **The helper creates the pipe and the core connects to it**, even though the core is
//! what starts the helper. The other way round is an elevation vector rather than a style
//! choice: a pipe *server* can call `ImpersonateNamedPipeClient` and act as whoever
//! connected to it. If the unelevated core owned the server and the elevated helper
//! connected, then any process that could take that name before the core did would be
//! handed the helper's token. Servers impersonate clients, so the higher-privileged end
//! has to be the server.
//!
//! That leaves the mirror-image risk: the core connecting to a name somebody else claimed
//! first. Two things answer it. `FILE_FLAG_FIRST_PIPE_INSTANCE` makes the helper's own
//! creation fail loudly rather than silently join an existing name — a denial of service
//! instead of a compromise, which is the correct trade — and the core checks
//! `GetNamedPipeServerProcessId` against the process it actually started, so a squatter
//! that got there first is refused rather than talked to. The helper checks the client the
//! same way.
//!
//! # Why the security descriptor is not left to default
//!
//! A kernel object created by an elevated process carries a High mandatory label, and the
//! default mandatory policy is no-write-up: a medium-integrity process cannot write to it.
//! The core is medium integrity. With a default descriptor this pipe would be created
//! successfully, accept a connection, and then fail every write from the one process it
//! exists to talk to.
//!
//! So the label is set explicitly to Medium, and the DACL to the creating user alone.
//! Not `Authenticated Users`, not `Interactive`: on a machine with two people signed in,
//! either of those would let the other one's session open a pipe into a process holding
//! debug-level access to the game.

use std::ffi::OsStr;
use std::io;

/// The pipe name both ends agree on.
///
/// Under `\\.\pipe\`, which is the only namespace named pipes live in, and suffixed with
/// the core's process id. Per-process rather than fixed: two clients for two users on one
/// machine would otherwise contend for a single name, and the first to start would own it
/// for both.
#[must_use]
pub fn pipe_name(core_process_id: u32) -> String {
    format!(r"\\.\pipe\AnotherCrewLink.{core_process_id}")
}

#[cfg(windows)]
mod platform {
    use super::{OsStr, io, pipe_name};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::ptr;
    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE,
        HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
        PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, PeekNamedPipe,
        WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, OpenProcessToken,
    };

    /// One frame's worth of headroom, twice over.
    ///
    /// The kernel's own buffers, not ours: `StreamTransport` does its own framing. Sized
    /// so a whole maximum frame fits without the writer blocking on a reader that is busy.
    const BUFFER: u32 = 128 * 1024;
    // Written out rather than derived, because deriving it needs a cast and a cast is how
    // a buffer silently becomes too small. This says the same thing at compile time.
    const _: () = assert!(BUFFER as usize >= crate::MAX_FRAME);

    /// How long the core waits for the helper's pipe to exist.
    ///
    /// The helper is a process that was just started, so this is a start-up race and not a
    /// network timeout. Long enough to survive a cold start behind a UAC prompt the user
    /// takes a moment over; short enough that a helper which died on launch is reported
    /// rather than waited on.
    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    /// How long to wait between attempts while the name does not exist.
    ///
    /// Short, because the usual wait is the few milliseconds between `spawn` returning and
    /// the helper reaching `CreateNamedPipeW`, and a long first sleep would pay for a UAC
    /// prompt that is usually not there.
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(20);

    /// A null-terminated UTF-16 copy.
    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// The security descriptor's owner, as SDDL.
    ///
    /// The creating process's own user, read from its token rather than named: a constant
    /// would have to be one of the well-known aliases, and every alias broad enough to
    /// cover "whoever is running this" also covers somebody else signed in at the same
    /// time.
    fn current_user_sid() -> io::Result<String> {
        let mut token: HANDLE = ptr::null_mut();
        // SAFETY: a documented call; `token` is a live local receiving the handle.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcessToken succeeded, so this is a valid handle owned from here.
        let token = unsafe { OwnedHandle::from_raw_handle(token.cast()) };

        let mut needed = 0u32;
        // SAFETY: the documented two-call pattern -- this one is expected to fail with
        // ERROR_INSUFFICIENT_BUFFER and to write the size it wants into `needed`.
        unsafe {
            windows_sys::Win32::Security::GetTokenInformation(
                token.as_raw_handle().cast(),
                TokenUser,
                ptr::null_mut(),
                0,
                &raw mut needed,
            )
        };
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }

        // `u64` and not `u8`: what comes back is a `TOKEN_USER`, which holds a pointer, and
        // a `Vec<u8>` is only byte-aligned. Reading a struct out of one is a misaligned
        // read on paper and an architecture-dependent one in practice.
        let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
        // SAFETY: `buffer` is at least `needed` bytes, which is the size the call above
        // asked for.
        let queried = unsafe {
            windows_sys::Win32::Security::GetTokenInformation(
                token.as_raw_handle().cast(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &raw mut needed,
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: on success the buffer holds a TOKEN_USER, whose SID pointer points into
        // the same buffer and so lives as long as it does.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut text: *mut u16 = ptr::null_mut();
        // SAFETY: the SID came from the kernel and `text` is a live local receiving an
        // allocation this function frees.
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // A SID in text is under 200 characters; the bound is there so that a pointer to
        // something that is not a string ends the loop rather than walking the heap.
        let mut length = 0usize;
        while length < 1024 {
            // SAFETY: on success `text` is a null-terminated wide string, and the index
            // has not yet passed its terminator.
            if unsafe { *text.add(length) } == 0 {
                break;
            }
            length += 1;
        }
        // SAFETY: `length` is the count of units before the terminator.
        let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
        // SAFETY: `text` was allocated by ConvertSidToStringSidW, which documents LocalFree
        // as the way to release it.
        unsafe { LocalFree(text.cast()) };
        Ok(sid)
    }

    /// Owns a security descriptor for as long as the `CreateNamedPipeW` call needs it.
    struct Descriptor {
        raw: PSECURITY_DESCRIPTOR,
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW,
                // which documents LocalFree as the way to release it.
                unsafe { LocalFree(self.raw) };
            }
        }
    }

    impl Descriptor {
        /// The one this pipe is created with: the creating user, and a Medium label.
        ///
        /// `FA` rather than a narrower mask because both ends read and write. The label is
        /// the half that is easy to leave out and impossible to notice missing until the
        /// helper is elevated — see the module documentation.
        fn for_this_user() -> io::Result<Self> {
            let sddl = format!(
                "D:(A;;FA;;;{sid})(A;;FA;;;SY)S:(ML;;NW;;;ME)",
                sid = current_user_sid()?
            );
            let mut raw: PSECURITY_DESCRIPTOR = ptr::null_mut();
            // SAFETY: the string is null-terminated and outlives the call, which allocates
            // into `raw` on success.
            let converted = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide(OsStr::new(&sddl)).as_ptr(),
                    SDDL_REVISION_1,
                    &raw mut raw,
                    ptr::null_mut(),
                )
            };
            if converted == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { raw })
        }

        fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the struct's own field is u32 and the struct is far smaller"
                )]
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.raw,
                // Not inherited. Nothing this process starts should get a handle to it.
                bInheritHandle: 0,
            }
        }
    }

    /// An open pipe, from either end.
    ///
    /// `Read` and `Write` so it can be handed straight to
    /// [`crate::stream::StreamTransport`], which is the only thing that should be framing
    /// anything on it.
    #[derive(Debug)]
    pub struct PipeConnection {
        handle: OwnedHandle,
    }

    impl PipeConnection {
        /// The process at the other end, when this end is the client.
        ///
        /// # Errors
        ///
        /// Whatever the kernel says. A failure here has to be treated as a mismatch: the
        /// point of asking is to refuse a stranger, and an unanswered question is not a
        /// pass.
        pub fn server_process_id(&self) -> io::Result<u32> {
            let mut pid = 0u32;
            // SAFETY: a valid pipe handle and a live local for the answer.
            if unsafe {
                GetNamedPipeServerProcessId(self.handle.as_raw_handle().cast(), &raw mut pid)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(pid)
        }

        /// The process at the other end, when this end is the server.
        ///
        /// # Errors
        ///
        /// Whatever the kernel says; see [`Self::server_process_id`].
        pub fn client_process_id(&self) -> io::Result<u32> {
            let mut pid = 0u32;
            // SAFETY: a valid pipe handle and a live local for the answer.
            if unsafe {
                GetNamedPipeClientProcessId(self.handle.as_raw_handle().cast(), &raw mut pid)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(pid)
        }

        /// Refuses the connection unless the other end is the process expected.
        ///
        /// # Errors
        ///
        /// [`io::ErrorKind::PermissionDenied`] when it is somebody else, and whatever the
        /// kernel said when the question could not be asked.
        pub fn expect_peer(&self, expected: u32, this_end_is_the_server: bool) -> io::Result<()> {
            let actual = if this_end_is_the_server {
                self.client_process_id()?
            } else {
                self.server_process_id()?
            };
            if actual == expected {
                return Ok(());
            }
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("expected process {expected} at the other end of the pipe, found {actual}"),
            ))
        }
    }

    impl PipeConnection {
        /// How many bytes are waiting, without taking any of them.
        ///
        /// This exists so that one thread can do both directions, and that is not a
        /// preference — it is the only shape that works here.
        ///
        /// The obvious alternative was a second handle from `DuplicateHandle`, one thread
        /// reading and one writing. It deadlocks. A duplicated handle refers to the *same
        /// file object*, and a file object opened without `FILE_FLAG_OVERLAPPED` is
        /// synchronous: the I/O manager serialises every operation on it. The reader
        /// blocks in `ReadFile` holding that lock, the writer's `WriteFile` queues behind
        /// it, and neither ever finishes. It was written that way, it passed the
        /// first message in each direction, and it hung on the second.
        ///
        /// Overlapped I/O is the other way out and is a great deal more machinery for a
        /// process that already wakes up five times a second. Peek, then read what is
        /// there.
        ///
        /// # Errors
        ///
        /// Whatever `PeekNamedPipe` says. A closed pipe reports one.
        pub fn available(&self) -> io::Result<usize> {
            let mut waiting = 0u32;
            // SAFETY: a valid pipe handle; every buffer argument is null, which is the
            // documented way to ask only for the byte count.
            let ok = unsafe {
                PeekNamedPipe(
                    self.handle.as_raw_handle().cast(),
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &raw mut waiting,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(waiting as usize)
        }
    }

    impl crate::stream::Peek for PipeConnection {
        fn available(&self) -> io::Result<usize> {
            Self::available(self)
        }
    }

    impl io::Read for PipeConnection {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let mut read = 0u32;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "clamped to u32::MAX on the line above the cast"
            )]
            let wanted = buffer.len().min(u32::MAX as usize) as u32;
            // SAFETY: `buffer` is writable for `wanted` bytes, which is its own length.
            let ok = unsafe {
                ReadFile(
                    self.handle.as_raw_handle().cast(),
                    buffer.as_mut_ptr(),
                    wanted,
                    &raw mut read,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(read as usize)
        }
    }

    impl io::Write for PipeConnection {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let mut written = 0u32;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "clamped to u32::MAX on the line above the cast"
            )]
            let wanted = buffer.len().min(u32::MAX as usize) as u32;
            // SAFETY: `buffer` is readable for `wanted` bytes, which is its own length.
            let ok = unsafe {
                WriteFile(
                    self.handle.as_raw_handle().cast(),
                    buffer.as_ptr(),
                    wanted,
                    &raw mut written,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(written as usize)
        }

        fn flush(&mut self) -> io::Result<()> {
            // Nothing buffered on this side. `FlushFileBuffers` on a pipe blocks until the
            // reader has consumed everything, which is not what a flush is being asked for
            // here and would deadlock a caller that writes before reading.
            Ok(())
        }
    }

    /// A created pipe, waiting for the other end.
    #[derive(Debug)]
    pub struct PipeServer {
        handle: OwnedHandle,
    }

    impl PipeServer {
        /// Creates the pipe. This is the elevated end.
        ///
        /// # Errors
        ///
        /// Whatever the kernel says. `ERROR_ACCESS_DENIED` here means somebody already
        /// holds this name — see the module documentation for why that is refused rather
        /// than joined.
        pub fn create(name: &str) -> io::Result<Self> {
            let descriptor = Descriptor::for_this_user()?;
            let mut attributes = descriptor.attributes();
            // SAFETY: the name is null-terminated and the attributes outlive the call.
            let handle = unsafe {
                windows_sys::Win32::System::Pipes::CreateNamedPipeW(
                    wide(OsStr::new(name)).as_ptr(),
                    windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX
                        | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    // One instance, ever. A second connection to this name is a second
                    // core, and there is only ever one.
                    1,
                    BUFFER,
                    BUFFER,
                    0,
                    &raw mut attributes,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                // SAFETY: a valid handle from CreateNamedPipeW, owned from here.
                handle: unsafe { OwnedHandle::from_raw_handle(handle.cast()) },
            })
        }

        /// Waits for the other end to connect.
        ///
        /// # Errors
        ///
        /// Whatever the kernel says.
        pub fn accept(self) -> io::Result<PipeConnection> {
            // SAFETY: a valid pipe handle in blocking mode with no overlapped structure.
            let connected =
                unsafe { ConnectNamedPipe(self.handle.as_raw_handle().cast(), ptr::null_mut()) };
            if connected == 0 {
                let error = io::Error::last_os_error();
                // Not a failure. The client can connect between CreateNamedPipeW and this
                // call, and the kernel reports the race rather than the connection.
                if error.raw_os_error() != Some(ERROR_PIPE_CONNECTED.cast_signed()) {
                    return Err(error);
                }
            }
            Ok(PipeConnection {
                handle: self.handle,
            })
        }
    }

    /// Connects to a pipe the other end created. This is the unelevated end.
    ///
    /// Retries until the deadline, because the helper is a process that was started a
    /// moment ago and may still be behind a UAC prompt.
    ///
    /// The retry is a loop and not `WaitNamedPipeW`, which is the obvious call and the
    /// wrong one. It waits for an *instance* of an existing pipe to become free; if the
    /// name does not exist at all it fails immediately with `ERROR_FILE_NOT_FOUND`. The
    /// case this function exists for is precisely the one where the name does not exist
    /// yet, and the first version of it therefore failed instantly every time. `WaitNamedPipe`
    /// is still used, for the one thing it does do: waiting out a busy instance.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::TimedOut`] if the pipe never appears, and whatever the kernel says
    /// otherwise.
    pub fn connect(name: &str) -> io::Result<PipeConnection> {
        let wide_name = wide(OsStr::new(name));
        let deadline = std::time::Instant::now() + CONNECT_TIMEOUT;
        loop {
            // SAFETY: the name is null-terminated; every other argument is a documented
            // constant or null.
            let handle = unsafe {
                CreateFileW(
                    wide_name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(PipeConnection {
                    // SAFETY: a valid handle from CreateFileW, owned from here.
                    handle: unsafe { OwnedHandle::from_raw_handle(handle.cast()) },
                });
            }

            let error = io::Error::last_os_error();
            let code = error.raw_os_error().unwrap_or_default();
            if code == ERROR_PIPE_BUSY.cast_signed() {
                // The name exists and every instance is taken. This is what
                // `WaitNamedPipeW` is for, and the only thing it is for here.
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                let milliseconds = u32::try_from(remaining.as_millis()).unwrap_or(u32::MAX);
                // SAFETY: the name is null-terminated and outlives the call.
                unsafe { WaitNamedPipeW(wide_name.as_ptr(), milliseconds) };
            } else if code == ERROR_FILE_NOT_FOUND.cast_signed() {
                // Not there yet. The helper has been started and has not reached
                // `CreateNamedPipeW`, or the user is still looking at a UAC dialog.
                std::thread::sleep(RETRY_DELAY);
            } else {
                return Err(error);
            }

            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "no pipe named {name} accepted a connection within {} ms",
                        CONNECT_TIMEOUT.as_millis()
                    ),
                ));
            }
        }
    }

    /// This process's id, for the pipe name and for the checks at both ends.
    #[must_use]
    pub fn this_process_id() -> u32 {
        // SAFETY: a documented call with no arguments that cannot fail.
        unsafe { GetCurrentProcessId() }
    }

    /// The name this process would offer, for a core that has not started a helper yet.
    #[must_use]
    pub fn name_for_this_process() -> String {
        pipe_name(this_process_id())
    }
}

#[cfg(windows)]
pub use platform::{PipeConnection, PipeServer, connect, name_for_this_process, this_process_id};

#[cfg(all(test, windows))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{PipeServer, connect, pipe_name, this_process_id};
    use crate::Transport;
    use crate::stream::StreamTransport;
    use crate::{HelperMessage, PROTOCOL_VERSION};
    use std::io;

    /// A name nothing else in this test run will claim.
    fn unique(tag: &str) -> String {
        format!("{}.{tag}", pipe_name(this_process_id()))
    }

    /// The whole boundary, end to end: create, connect, frame a real message across it,
    /// and read it back as the same value.
    #[test]
    fn a_message_crosses_the_pipe_unchanged() {
        let name = unique("roundtrip");
        let server = PipeServer::create(&name).expect("the pipe is created");

        let client_name = name.clone();
        let client = std::thread::spawn(move || {
            let connection = connect(&client_name).expect("the client connects");
            let mut transport = StreamTransport::new(connection);
            transport
                .send(&HelperMessage::Ready {
                    protocol: PROTOCOL_VERSION,
                })
                .expect("the frame is written");
        });

        let connection = server.accept().expect("the server accepts");
        let mut transport = StreamTransport::new(connection);
        let received: HelperMessage = transport
            .recv()
            .expect("a frame arrives")
            .expect("and it is not a clean close");
        client.join().expect("the client thread finishes");

        assert_eq!(
            received,
            HelperMessage::Ready {
                protocol: PROTOCOL_VERSION
            }
        );
    }

    /// `FILE_FLAG_FIRST_PIPE_INSTANCE`, which is the difference between a squatter being
    /// refused and a squatter being joined.
    #[test]
    fn a_second_server_cannot_take_the_same_name() {
        let name = unique("first-instance");
        let _held = PipeServer::create(&name).expect("the first creation succeeds");
        assert!(
            PipeServer::create(&name).is_err(),
            "a second server took a name the first one already holds"
        );
    }

    /// Both ends can name the other, which is what makes the pid check possible at all.
    #[test]
    fn each_end_can_identify_the_other() {
        let name = unique("identity");
        let server = PipeServer::create(&name).expect("the pipe is created");

        let client_name = name.clone();
        let client = std::thread::spawn(move || connect(&client_name).expect("connects"));

        let accepted = server.accept().expect("the server accepts");
        let connected = client.join().expect("the client thread finishes");

        // One process on both ends here, so both answers are this one -- which is enough
        // to prove the calls work and that `expect_peer` compares what it claims to.
        let us = this_process_id();
        assert_eq!(accepted.client_process_id().unwrap(), us);
        assert_eq!(connected.server_process_id().unwrap(), us);
        accepted.expect_peer(us, true).expect("the client is us");
        connected.expect_peer(us, false).expect("the server is us");

        let wrong = accepted.expect_peer(us.wrapping_add(1), true).unwrap_err();
        assert_eq!(wrong.kind(), io::ErrorKind::PermissionDenied);
    }

    /// A name nobody created must time out rather than block forever or succeed.
    ///
    /// The timeout is long, so this asks for one that is not: the call under test is
    /// `WaitNamedPipeW`, and a pipe that does not exist is refused by it immediately.
    #[test]
    fn connecting_to_a_pipe_nobody_created_fails() {
        let error = connect(&unique("never-created")).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::NotFound
            ),
            "unexpected error for a pipe that does not exist: {error:?}"
        );
    }
}
