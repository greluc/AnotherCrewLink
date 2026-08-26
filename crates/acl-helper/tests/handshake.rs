//! The helper, started for real, over a real pipe.
//!
//! Everything else about this binary is unit-testable in the crates it draws on — the
//! framing in `acl-ipc`, the reader in `acl-game`, the launch decisions in `acl-core`. What
//! is not is whether the four of them line up: the command line the core sends against the
//! one the helper parses, the pipe name each derives, the process check at both ends, and
//! the handshake. Every one of those is a place where two correct halves disagree, and
//! none of them fails until the binary actually runs.
//!
//! No game and no elevation. The helper is started at this process's own integrity level
//! and never told to read anything, so what is under test is the boundary and nothing
//! behind it.

#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use acl_ipc::pipe::{connect, pipe_name};
use acl_ipc::stream::StreamTransport;
use acl_ipc::{CoreMessage, HelperMessage, PROTOCOL_VERSION, Transport};

/// One helper at a time.
///
/// The pipe name is derived from the core's process id, deliberately: one core, one
/// helper, one name. Inside a test binary that makes every test here contend for the same
/// name, and `FILE_FLAG_FIRST_PIPE_INSTANCE` means the loser does not queue -- it fails to
/// create the pipe at all, and the test that started it waits for something that will
/// never appear. Serialising is the honest fix; renaming per test would test a name the
/// product does not use.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Takes the lock, and does not care that a failing test poisoned it.
fn serially() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts the binary this crate builds, the way the core starts it.
///
/// `CARGO_BIN_EXE_` rather than a path: the test then runs against what was just compiled,
/// including on a machine that has an older one installed.
fn start_helper(core_process_id: u32) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_acl-helper"))
        .args(acl_core::launch::arguments(core_process_id))
        .spawn()
        .expect("the helper binary starts")
}

#[test]
fn it_connects_announces_itself_and_stops_when_told() {
    let _serially = serially();
    let us = std::process::id();
    let mut helper = start_helper(us);
    let helper_id = helper.id();

    within(
        30,
        &[
            "connected",
            "identified",
            "ready",
            "shutdown sent",
            "stopping",
        ],
        move |step| {
            // The name is derived on both sides from the one number on the command line,
            // so deriving it here a third time is the check: if the helper computed a
            // different one, nothing is listening on this.
            let connection = connect(&pipe_name(us)).expect("the helper's pipe appears");
            step.send("connected").unwrap();

            connection
                .expect_peer(helper_id, false)
                .expect("the pipe server is the helper we started");
            step.send("identified").unwrap();

            let mut transport = StreamTransport::new(connection);
            let ready: HelperMessage = transport
                .recv()
                .expect("a frame arrives")
                .expect("and it is not a clean close");
            assert_eq!(
                ready,
                HelperMessage::Ready {
                    protocol: PROTOCOL_VERSION
                }
            );
            step.send("ready").unwrap();

            transport.send(&CoreMessage::Shutdown).unwrap();
            step.send("shutdown sent").unwrap();

            let stopping: HelperMessage = transport
                .recv()
                .expect("a frame arrives")
                .expect("and it is not a clean close");
            assert!(
                matches!(stopping, HelperMessage::Stopping { .. }),
                "expected a reason for stopping, got {stopping:?}"
            );
            step.send("stopping").unwrap();
        },
    );

    let status = wait_briefly(&mut helper).expect("the helper exits after being told to");
    assert!(status.success(), "the helper exited with {status}");
}

/// Closing the pipe is enough. An elevated process holding debug-level access to another
/// process's memory must not outlive the thing that asked for it, and the core crashing is
/// exactly the case where nobody is left to send `Shutdown`.
#[test]
fn it_exits_when_the_core_goes_away() {
    let _serially = serially();
    let us = std::process::id();
    let mut helper = start_helper(us);

    let connection = connect(&pipe_name(us)).expect("the helper's pipe appears");
    let mut transport = StreamTransport::new(connection);
    let _: HelperMessage = transport.recv().unwrap().unwrap();
    drop(transport);

    let status = wait_briefly(&mut helper).expect("the helper exits when the pipe closes");
    assert!(status.success(), "the helper exited with {status}");
}

/// Started by hand rather than by the core, which is the shape of somebody having found
/// the binary in the installation directory and double-clicked it.
#[test]
fn it_refuses_to_start_without_being_told_who_started_it() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_acl-helper"))
        .output()
        .expect("the helper binary runs");
    assert!(!output.status.success());
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(
        complaint.contains("--core-pid"),
        "the complaint should name what is missing, got {complaint:?}"
    );
}

/// Runs the exchange on a worker thread and gives up on it.
///
/// A test that hangs reports nothing, and the first version of this file hung: the helper
/// held a duplicated pipe handle, the reader blocked, and the write behind it never
/// completed. What was visible was "has been running for over 60 seconds" and no clue
/// which step. The worker reports each step it finishes, so a hang names the one after.
fn within(
    seconds: u64,
    steps: &'static [&'static str],
    body: impl FnOnce(&std::sync::mpsc::Sender<&'static str>) + Send + 'static,
) {
    let (progress, reached) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || body(&progress));
    let mut done: Vec<&str> = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        match reached.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
            Ok(step) => done.push(step),
            // Every step reported and the sender dropped: the worker is finishing.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let next = steps
                    .get(done.len())
                    .copied()
                    .unwrap_or("(past the last step)");
                panic!(
                    "stuck at: {next}
  finished: {}",
                    if done.is_empty() {
                        "nothing".to_owned()
                    } else {
                        done.join(", ")
                    }
                );
            }
        }
    }
    worker.join().expect("the exchange did not panic");
    assert_eq!(done, steps, "the exchange did not reach every step");
}

/// Waits for the process, without waiting forever if it hangs.
///
/// `try_wait` in a loop rather than `wait`: a helper that never exits would otherwise take
/// the whole test run down with it, and a test that hangs reports nothing at all.
fn wait_briefly(child: &mut std::process::Child) -> Option<std::process::ExitStatus> {
    for _ in 0..100 {
        if let Some(status) = child.try_wait().expect("the child is waitable") {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    None
}
