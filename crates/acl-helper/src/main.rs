//! No console window.
//!
//! The client became a windows-subsystem application on 2026-08-27 to stop a terminal
//! opening beside it. That alone would have moved the problem rather than fixed it: a
//! console-subsystem child spawned by a GUI parent gets a console of its own, so the one
//! window would have become one window that appears a moment later.
//!
//! The two diagnostics below are for somebody who ran this by hand, and they still reach a
//! shell that redirected them -- which is how `it_refuses_to_start_without_being_told_who_
//! started_it` reads them.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! The elevated half of the client.
//!
//! §4.7 of `docs/rust-port/04-implementation-plan.md` splits the client in two, and §6 of
//! `docs/rust-port/06-security.md` says what that buys: "a process with no listening
//! socket, no HTTP client, no image decoder and no GPU context". This is that process, and
//! the list is a specification rather than a description — every dependency here is
//! checked against it.
//!
//! What it does is read the game and say what it saw. It does not fetch the offsets
//! bundle, because fetching is HTTP and HTTP is on the list; the core fetches, validates
//! and sends it. It does not decode an image, because the overlay will receive
//! pre-rasterised sprites. It does not open a socket.
//!
//! # Starting it
//!
//! ```text
//! acl-helper --core-pid <pid>
//! ```
//!
//! The core starts it, elevating only when the game's integrity level denies the read.
//! Both ends derive the pipe name from that one number, so there is one source of truth
//! for it, and both check that the process at the other end is the one they expect.
//!
//! # Why it exits when the core does
//!
//! An elevated process holding debug-level access to another process's memory should not
//! outlive the reason it was started. There is no service and no resident component: when
//! the pipe closes, this stops.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use acl_game::offsets::Offsets;
use acl_game::reader::{ReadContext, read_state};
use acl_game::resolve::resolve_offsets;
use acl_helper::overlay::{Frame, Placement, Sprite};
use acl_ipc::stream::StreamTransport;
use acl_ipc::{CoreMessage, HelperMessage, PROTOCOL_VERSION, Transport};

/// The executable the reader attaches to.
const GAME_EXECUTABLE: &str = "Among Us.exe";

/// The module the offsets are relative to.
const GAME_MODULE: &str = "GameAssembly.dll";

/// How often the game is sampled.
///
/// `1000 / 5` in `src/main/hook.ts`, and the same number for the same reason: proximity
/// audio is recomputed from each frame, and five a second is what the Electron client has
/// been shipping. Faster costs a cross-process read per field for no audible gain.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(200);

/// How long to wait before trying the game again after a failed read.
///
/// `setTimeout(frame, 7500)` in `hook.ts`. Long, and deliberately: the usual cause is a
/// game that is starting or shutting down, and retrying five times a second against a
/// process in that state is how a log fills with the same line.
const RETRY_INTERVAL: Duration = Duration::from_millis(7500);

/// Why the helper stopped.
#[derive(Debug, thiserror::Error)]
enum Fatal {
    /// The command line was not what the core sends.
    #[error("{0}")]
    Arguments(String),
    /// The pipe could not be created, or the core never arrived on it.
    #[error("the pipe to the core: {0}")]
    Pipe(#[from] std::io::Error),
    /// A frame could not be written.
    #[error("writing to the core: {0}")]
    Frame(#[from] acl_ipc::FrameError),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Plain stderr, and only here. Everything after the pipe is open is reported
            // over it, because that is the channel that reaches the core -- an elevated
            // process started through `runas` does not inherit the parent's handles, so
            // nothing written here is read by anybody in the case this binary exists for.
            // The workspace has no tracing subscriber to install instead; choosing one is
            // a packaging decision and belongs with the phase that packages this.
            eprintln!("acl-helper: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The core's process id, from the command line.
fn core_process_id() -> Result<u32, Fatal> {
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--core-pid" {
            let value = arguments
                .next()
                .ok_or_else(|| Fatal::Arguments("--core-pid takes a value".to_owned()))?;
            return value
                .parse()
                .map_err(|_| Fatal::Arguments(format!("--core-pid {value} is not a process id")));
        }
    }
    Err(Fatal::Arguments(
        "no --core-pid; this binary is started by the client, not by hand".to_owned(),
    ))
}

/// Nothing to do here, and saying so beats a linker error.
///
/// The crate is in the workspace so that formatting, licence and advisory jobs see it;
/// those run on Linux runners, which is a runner choice and not a supported platform.
#[cfg(not(windows))]
fn run() -> Result<(), Fatal> {
    Err(Fatal::Arguments(
        "the helper reads Windows process memory and has no other implementation".to_owned(),
    ))
}

/// How long the helper waits for the core to connect before giving up.
///
/// `ConnectNamedPipe` has no timeout of its own, so without this an elevated process
/// holding debug-level access to another process's memory waits for a client that may
/// never come -- a core that crashed between starting it and connecting leaves it resident
/// for the rest of the session, with no window and nothing to notice it by. Found by the
/// first run of the handshake test, which left one behind.
///
/// A minute, because the wait includes a UAC prompt somebody may be reading.
#[cfg(windows)]
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(60);

/// Ends this process when the core is gone, or was never coming.
///
/// A thread rather than a check in the loop: the two things it waits on -- a process
/// handle and a deadline -- are both blocking, and the main thread is blocked on the pipe
/// for the whole window this covers.
///
/// `exit` rather than an unwind. There is nothing to clean up that the kernel will not do,
/// and the failure being guarded against is precisely the one where the orderly paths are
/// not running.
#[cfg(windows)]
fn watch_the_core(core: u32, connected: &std::sync::atomic::AtomicBool) {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    // `SYNCHRONIZE` is declared under file access rights rather than under process ones.
    // It is the standard right of that name and applies to any waitable object; the module
    // it happens to live in in these bindings is not a hint about what it is for.
    use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

    // SAFETY: a documented call with a constant rights mask and no pointer arguments.
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, core) };
    if handle.is_null() {
        // The core is already gone, or is not something this process may wait on. Either
        // way there is nobody to serve.
        eprintln!("acl-helper: cannot wait on process {core}; exiting");
        std::process::exit(1);
    }

    // First the accept window, then forever. Splitting the wait is what distinguishes "the
    // core never connected" from "the core connected and later exited": the second is
    // ordinary and silent, the first is worth a line.
    // SAFETY: a valid handle from OpenProcess above.
    let waited = unsafe {
        WaitForSingleObject(
            handle.cast(),
            u32::try_from(ACCEPT_TIMEOUT.as_millis()).unwrap_or(u32::MAX),
        )
    };
    if waited == WAIT_TIMEOUT && !connected.load(Ordering::Acquire) {
        eprintln!("acl-helper: the core never connected; exiting");
        std::process::exit(1);
    }
    if waited != WAIT_TIMEOUT {
        std::process::exit(0);
    }

    // SAFETY: still the same valid handle; INFINITE is the documented no-timeout value.
    unsafe { WaitForSingleObject(handle.cast(), u32::MAX) };
    // SAFETY: closed once, on the only path that reaches here.
    unsafe { CloseHandle(handle) };
    std::process::exit(0);
}

#[cfg(windows)]
fn run() -> Result<(), Fatal> {
    let core = core_process_id()?;

    let connected = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watching = std::sync::Arc::clone(&connected);
    std::thread::Builder::new()
        .name("core-watchdog".to_owned())
        .spawn(move || watch_the_core(core, &watching))
        .map_err(Fatal::Pipe)?;

    let name = acl_ipc::pipe::pipe_name(core);
    let server = acl_ipc::pipe::PipeServer::create(&name)?;
    let connection = server.accept()?;
    // Before a single message is exchanged. A pipe this process created can still be
    // connected to by anything running as this user, and the only thing that makes the far
    // end the core is that its process id is the one on the command line.
    connection.expect_peer(core, true)?;
    connected.store(true, std::sync::atomic::Ordering::Release);

    let mut transport = StreamTransport::new(connection);
    transport.send(&HelperMessage::Ready {
        protocol: PROTOCOL_VERSION,
    })?;

    pump(&mut transport)
}

/// The main loop: answer the core, and sample the game while it wants that.
///
/// One thread for both directions. The obvious alternative -- a reader thread on a
/// duplicated handle -- deadlocks, for the reason recorded on
/// [`acl_ipc::pipe::PipeConnection::available`]; this loop peeks instead, which it can
/// afford because it is already awake every [`SAMPLE_INTERVAL`].
#[cfg(windows)]
fn pump(transport: &mut StreamTransport<acl_ipc::pipe::PipeConnection>) -> Result<(), Fatal> {
    // Started once, kept for the life of the helper, and hidden until the core asks. A
    // machine with no interactive desktop -- which is not a case this client runs in, but
    // is one a test harness can be -- gets `None` and every overlay command is ignored
    // rather than being a reason to stop reading the game.
    let overlay = acl_helper::overlay::start().ok();
    let mut offsets = Bundles::default();
    let mut sampler: Option<Sampler> = None;
    let mut reading = false;
    let mut next_attempt = Instant::now();

    loop {
        let tick = Instant::now() + SAMPLE_INTERVAL;

        // Everything the core has said since the last tick. A command therefore takes at
        // most one interval to be acted on, which is below anything a person notices about
        // a push-to-talk or an overlay toggle.
        //
        // `try_recv`, which consults the transport's own buffer before the pipe. A loop
        // that peeked only at the pipe dropped every frame that arrived in the same read
        // as the one before it -- and that is not hypothetical: it is why this helper took
        // its offsets and then never read the game. `StartReading` came in with them.
        loop {
            let message = match transport.try_recv::<CoreMessage>() {
                Ok(Some(message)) => message,
                // Nothing waiting. Not a close: a live peer with nothing to say looks
                // exactly like this, and it is the ordinary case.
                Ok(None) => break,
                // A clean close, a torn frame, or a pipe that is gone. All three mean the
                // core is not there, so there is nothing to report and nobody to report to.
                Err(_) => return Ok(()),
            };
            match message {
                CoreMessage::SetOffsets { is_64bit, bundle } => {
                    match serde_json::from_slice::<Offsets>(&bundle) {
                        Ok(parsed) => {
                            if is_64bit {
                                offsets.for_64bit = Some(parsed);
                            } else {
                                offsets.for_32bit = Some(parsed);
                            }
                            // The old one described a different build. Dropping the sampler
                            // makes the next tick re-resolve signatures against the new
                            // bundle rather than keep reading through the previous one's
                            // addresses.
                            sampler = None;
                            next_attempt = Instant::now();
                        }
                        Err(error) => {
                            stop(
                                transport,
                                &format!("the offsets bundle did not parse: {error}"),
                            );
                            return Ok(());
                        }
                    }
                }
                CoreMessage::StartReading => reading = true,
                CoreMessage::SetOverlayVisible(wanted) => {
                    if let Some(overlay) = overlay.as_ref() {
                        overlay.show(wanted);
                    }
                }
                CoreMessage::PlaceOverlay {
                    x,
                    y,
                    width,
                    height,
                } => {
                    if let Some(overlay) = overlay.as_ref() {
                        overlay.place(Placement {
                            x,
                            y,
                            width,
                            height,
                        });
                    }
                }
                CoreMessage::ClearOverlay => {
                    if let Some(overlay) = overlay.as_ref() {
                        overlay.clear();
                    }
                }
                CoreMessage::DrawSprite {
                    x,
                    y,
                    width,
                    height,
                    pixels,
                } => {
                    if let Some(overlay) = overlay.as_ref() {
                        overlay.blit(Sprite {
                            x,
                            y,
                            frame: Frame {
                                width,
                                height,
                                pixels,
                            },
                        });
                    }
                }
                CoreMessage::PresentOverlay => {
                    if let Some(overlay) = overlay.as_ref() {
                        overlay.present();
                    }
                }
                CoreMessage::Shutdown => {
                    stop(transport, "the core asked it to");
                    return Ok(());
                }
                // Anything this build does not know about. `CoreMessage` is
                // `non_exhaustive` so that a newer core can send one, and ignoring it is
                // the whole point of that.
                _ => {}
            }
        }

        if reading && Instant::now() >= next_attempt && offsets.any() {
            sample_once(transport, &offsets, &mut sampler, &mut next_attempt)?;
        }

        // What is left of the interval after the work, rather than a flat sleep: a sample
        // that took 40 ms should not push the next one out to 240.
        std::thread::sleep(tick.saturating_duration_since(Instant::now()));
    }
}

/// The offsets bundles the core has sent, one per architecture of the game.
///
/// Both, because which applies is decided by the process this helper finds rather than by
/// anything the core can see. See `acl_ipc::CoreMessage::SetOffsets`.
#[cfg(windows)]
#[derive(Default)]
struct Bundles {
    for_32bit: Option<Offsets>,
    for_64bit: Option<Offsets>,
}

#[cfg(windows)]
impl Bundles {
    /// Whether there is anything to try at all.
    fn any(&self) -> bool {
        self.for_32bit.is_some() || self.for_64bit.is_some()
    }

    /// The one for a game of this width.
    fn pick(&self, is_64bit: bool) -> Option<&Offsets> {
        if is_64bit {
            self.for_64bit.as_ref()
        } else {
            self.for_32bit.as_ref()
        }
    }
}

/// Attaches if needed, reads one frame, and sends it.
#[cfg(windows)]
fn sample_once(
    transport: &mut StreamTransport<acl_ipc::pipe::PipeConnection>,
    offsets: &Bundles,
    sampler: &mut Option<Sampler>,
    next_attempt: &mut Instant,
) -> Result<(), Fatal> {
    if sampler.is_none() {
        let Some(attached) = Sampler::attach(offsets) else {
            *next_attempt = Instant::now() + RETRY_INTERVAL;
            return Ok(());
        };
        *sampler = Some(attached);
    }
    let Some(active) = sampler.as_mut() else {
        return Ok(());
    };
    let Some(state) = active.sample() else {
        // The game went away, or a chain that must resolve did not. Both are recovered
        // from by attaching again, not by giving up: a player restarting the game should
        // get their proximity back without restarting the client.
        *sampler = None;
        *next_attempt = Instant::now() + RETRY_INTERVAL;
        return Ok(());
    };
    match postcard::to_allocvec(&state) {
        Ok(payload) => transport.send(&HelperMessage::GameState(payload))?,
        Err(error) => stop(transport, &format!("a frame did not encode: {error}")),
    }
    Ok(())
}

/// Says why, then stops.
///
/// The message is for the core's log and not for a player, and a failure to deliver it is
/// not worth reporting to anybody: the only channel it could be reported on is the one
/// that just failed.
#[cfg(windows)]
fn stop(transport: &mut StreamTransport<acl_ipc::pipe::PipeConnection>, reason: &str) {
    let _ = transport.send(&HelperMessage::Stopping {
        reason: reason.to_owned(),
    });
}

/// An attached game, and everything the reader needs to keep reading it.
#[cfg(windows)]
struct Sampler {
    process: acl_game::windows::WindowsProcess,
    resolved: Offsets,
    context: ReadContext,
}

#[cfg(windows)]
impl Sampler {
    /// Finds the game and resolves the bundle's signatures against it.
    ///
    /// `None` rather than an error, for every reason it can fail. The game not running is
    /// the ordinary case and is not worth a type; the game running elevated while this
    /// process is not is the case the elevation prompt exists for, and the core already
    /// knows what to do about it.
    fn attach(offsets: &Bundles) -> Option<Self> {
        use acl_game::ProcessMemory;

        let process = acl_game::windows::WindowsProcess::open_by_name(GAME_EXECUTABLE).ok()?;
        let module = process.module(GAME_MODULE)?;
        // The bundle is chosen here, where the game's width is known. Choosing on the
        // other side of the pipe would be guessing, and a wrong guess resolves every
        // pointer chain to nothing -- which reads as a game that is not running.
        let bundle = offsets.pick(process.is_64bit())?;
        let resolved = resolve_offsets(&process, &module, bundle).ok()?.offsets;
        let context = ReadContext::new(module.base, acl_game::mods::Mod::None);
        Some(Self {
            process,
            resolved,
            context,
        })
    }

    /// One frame.
    fn sample(&mut self) -> Option<acl_game::AmongUsState> {
        let state = read_state(&self.process, &self.resolved, &mut self.context).ok()?;
        // Carried, because two fields are defined against the frame before: `oldGameState`
        // and `lightRadiusChanged`. The menu hold in the reader carries more than that,
        // and it lives in the context for the same reason.
        self.context.previous = Some(state.clone());
        Some(state)
    }
}
