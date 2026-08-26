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

/// The whole link, driven by the code the client will actually use.
///
/// The other tests here speak the protocol by hand, which is what makes them useful when
/// the protocol is what broke. This one goes through `acl_core::link::Link` instead, so
/// that the four pieces are exercised the way the client joins them: launch, connect,
/// check who answered, agree a protocol, send offsets, start reading.
///
/// No game is running, so no frame arrives. That is the point of the assertion being about
/// the state: a helper that cannot find the game is a helper that is working correctly, and
/// a link that dropped to `Lost` over it would take proximity down every time somebody
/// alt-tabbed out of a game that had not started yet.
#[test]
fn the_link_starts_the_helper_and_keeps_it() {
    let _serially = serially();

    let mut link = acl_core::link::Link::new();
    link.start(
        std::path::Path::new(env!("CARGO_BIN_EXE_acl-helper")),
        acl_core::launch::Elevation::AsIs,
        OFFSETS,
    )
    .expect("the helper starts and answers");
    assert_eq!(link.state(), acl_core::helper::HelperState::Running);

    // Polled over a stretch that spans several of the helper's own sample intervals, so
    // that a helper which fell over on the first attempt to attach to a game that is not
    // there would be caught rather than missed.
    for _ in 0..10 {
        for event in link.poll() {
            assert!(
                !matches!(event, acl_core::link::Event::Stopped(_)),
                "the helper stopped while nothing was wrong: {event:?}"
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        link.state(),
        acl_core::helper::HelperState::Running,
        "the link lost a helper that is alive and simply has no game to read"
    );

    link.stop();
}

/// The whole chain, with a game at the end of it.
///
/// Ignored, because it needs Among Us running. It is the only test in this repository that
/// crosses every part at once: the helper attaches to the game, the reader produces an
/// `AmongUsState`, postcard encodes it, the pipe carries it, and `Link` decodes it back
/// into the same type. Each of those is tested on its own; none of the unit tests would
/// notice if two of them disagreed about a format.
///
/// Start Among Us -- the menu is enough -- then:
///
/// ```text
/// cargo test -p acl-helper -- --ignored the_link_reads
/// ```
#[test]
#[ignore = "needs Among Us to be running"]
fn the_link_reads_a_real_game() {
    let _serially = serially();

    let mut link = acl_core::link::Link::new();
    link.start(
        std::path::Path::new(env!("CARGO_BIN_EXE_acl-helper")),
        acl_core::launch::Elevation::AsIs,
        OFFSETS,
    )
    .expect("the helper starts and answers");

    // Long enough to cover a failed first attach and the retry after it: the helper backs
    // off 7.5 s when it cannot reach the game, the same as `hook.ts` does, so a shorter
    // deadline could only ever see the first attempt -- and would report "the chain is
    // broken" for a game that was merely still loading.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut frames = 0usize;
    let mut last = None;
    while std::time::Instant::now() < deadline {
        for event in link.poll() {
            match event {
                acl_core::link::Event::GameState(state) => {
                    frames += 1;
                    last = Some(state);
                }
                other => panic!("unexpected: {other:?}"),
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let state =
        last.unwrap_or_else(|| panic!("no frame arrived; is the game running and readable?"));
    eprintln!(
        "{frames} frames; last: state={:?} map={} players={}",
        state.game_state,
        state.map,
        state.players.len()
    );
    // Five a second, so ten seconds is forty-odd even allowing for the attach.
    assert!(frames > 20, "only {frames} frames");
    link.stop();
}

/// The overlay, driven from the core through the pipe.
///
/// The overlay's own tests build one in-process and check what the operating system says
/// about it. This checks the other half: that the commands survive the crossing and reach
/// the window in a *different* process, which is where the interesting failure lives —
/// four integers that arrive transposed put the overlay off-screen, and nothing in either
/// process would notice.
#[test]
fn the_core_can_place_and_show_the_helper_overlay() {
    let _serially = serially();

    let mut link = acl_core::link::Link::new();
    link.start(
        std::path::Path::new(env!("CARGO_BIN_EXE_acl-helper")),
        acl_core::launch::Elevation::AsIs,
        OFFSETS,
    )
    .expect("the helper starts and answers");

    let (x, y, width, height) = (140, 90, 300, 180);
    link.place_overlay(x, y, width, height);
    // A sprite rather than a whole picture, and not for tidiness: `acl_ipc::MAX_FRAME` is
    // 64 KiB, so a 300x180 frame at four bytes a pixel is already three times the limit
    // and a full-screen one is two hundred times it. The overlay composes.
    let sprite: i32 = 32;
    link.clear_overlay();
    link.draw_sprite(
        4,
        4,
        sprite,
        sprite,
        vec![255u8; usize::try_from(sprite * sprite * 4).unwrap_or_default()],
    );
    link.present_overlay();
    link.show_overlay(true);

    let placed = wait_for_overlay(|rect| {
        rect.left == x
            && rect.top == y
            && rect.right - rect.left == width
            && rect.bottom - rect.top == height
    });
    assert!(
        placed,
        "the overlay never reached {x},{y} {width}x{height} in the helper process"
    );

    link.stop();
}

/// Polls the helper's overlay window until its rectangle matches.
///
/// Found by title across the process boundary, which is the only handle this side has:
/// the window belongs to the helper.
fn wait_for_overlay(mut matches: impl FnMut(windows_sys::Win32::Foundation::RECT) -> bool) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};

    let mut title: Vec<u16> = acl_helper::overlay::WINDOW_TITLE.encode_utf16().collect();
    title.push(0);
    for _ in 0..200 {
        // SAFETY: a documented call with a null class and a null-terminated title.
        let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if !window.is_null() {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: a valid window handle and a live local for the answer.
            unsafe { GetWindowRect(window, &raw mut rect) };
            if matches(rect) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Bundles that parse, which is all this needs: the helper rejects one that does not and
/// stops, and a test that fed it rubbish would be testing that path instead.
///
/// The embedded floor for both architectures, so the test carries no fixture of its own,
/// cannot drift from the shape the reader expects, and exercises the pair the client
/// actually sends.
const OFFSETS: acl_core::link::Offsets<'static> = acl_core::link::Offsets {
    for_32bit: include_bytes!("../../acl-game/assets/offsets-x86.json"),
    for_64bit: include_bytes!("../../acl-game/assets/offsets-x64.json"),
};

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
