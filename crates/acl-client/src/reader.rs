//! The game reader, on a thread of its own.
//!
//! [`acl_core::link::Link::start`] blocks — it launches a process and waits for it to
//! answer, which behind a UAC prompt is as long as somebody takes to read a dialog. Called
//! from a paint function that would be the window freezing while a dialog nobody can see
//! yet waits for an answer.
//!
//! So the link lives here, on its own thread, and the window sees two channels: commands
//! going out and frames coming back.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use acl_core::helper::HelperState;
use acl_core::launch::Elevation;
use acl_core::link::{Event, Link};
use acl_game::AmongUsState;

/// What the window asks for.
pub(crate) enum Command {
    /// Start the helper and read the game.
    Start,
    /// Stop reading and let the helper go.
    Stop,
    /// Show or hide the overlay.
    ShowOverlay(bool),
    /// One frame of the overlay: where it goes, and what is on it.
    ///
    /// Composed here rather than in the helper because §4.7 keeps every image decoder out
    /// of the elevated process. What crosses the pipe is bytes and coordinates.
    Overlay {
        /// Where the overlay window belongs, in screen coordinates.
        placement: (i32, i32, i32, i32),
        /// The sprites, each with its position inside the overlay.
        sprites: Vec<(i32, i32, acl_ui::sprite::Bitmap)>,
    },
}

/// What the reader thread reports.
pub(crate) enum Report {
    /// The helper's state changed.
    State(HelperState),
    /// A frame of the game.
    Frame(Box<AmongUsState>),
    /// Something worth putting in front of a person.
    Trouble(String),
}

/// A handle on the reader thread.
pub(crate) struct Reader {
    commands: Sender<Command>,
    reports: Receiver<Report>,
    state: HelperState,
    latest: Option<Box<AmongUsState>>,
    trouble: Option<String>,
}

impl Reader {
    /// Starts the thread. It does nothing until asked.
    ///
    /// # Errors
    ///
    /// Whatever spawning the thread said.
    pub(crate) fn start() -> std::io::Result<Self> {
        let (commands, orders) = channel();
        let (reports, inbox) = channel();
        std::thread::Builder::new()
            .name("game-reader".to_owned())
            .spawn(move || run(&orders, &reports))?;
        Ok(Self {
            commands,
            reports: inbox,
            state: HelperState::NotRequested,
            latest: None,
            trouble: None,
        })
    }

    /// Takes in everything the thread has said since the last look.
    ///
    /// Called once a frame, and never blocks.
    pub(crate) fn pump(&mut self) {
        loop {
            match self.reports.try_recv() {
                Ok(Report::State(state)) => {
                    // On the transition, not per report: the thread says the same thing
                    // repeatedly while nothing changes, and a log that repeats itself is
                    // one nobody reads to the end of.
                    if self.state != state {
                        acl_core::log_info!("reader", "helper is now {state:?}");
                    }
                    self.state = state;
                    if state == HelperState::Running {
                        self.trouble = None;
                    }
                }
                Ok(Report::Frame(state)) => self.latest = Some(state),
                Ok(Report::Trouble(what)) => {
                    // Everything the elevated half has to say arrives here. It cannot write
                    // to this file itself -- see the note at the top of `acl-helper` -- so
                    // this is where what it said is kept.
                    acl_core::log_warn!("reader", "{what}");
                    self.trouble = Some(what);
                }
                // Nothing waiting, or the thread has gone. The second is not recoverable
                // here and the state it last reported stands rather than being rewritten
                // into something cheerier, so both stop the same way.
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    /// What to tell the person about the helper.
    #[must_use]
    pub(crate) const fn state(&self) -> HelperState {
        self.state
    }

    /// The last frame, if there has been one.
    #[must_use]
    pub(crate) fn latest(&self) -> Option<&AmongUsState> {
        self.latest.as_deref()
    }

    /// Anything that went wrong and has not been superseded.
    #[must_use]
    pub(crate) fn trouble(&self) -> Option<&str> {
        self.trouble.as_deref()
    }

    /// Asks for the game reader.
    pub(crate) fn ask_to_start(&self) {
        let _ = self.commands.send(Command::Start);
    }

    /// Asks it to stop.
    pub(crate) fn ask_to_stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    /// Shows or hides the overlay.
    pub(crate) fn show_overlay(&self, visible: bool) {
        let _ = self.commands.send(Command::ShowOverlay(visible));
    }

    /// Hands one overlay frame across.
    pub(crate) fn draw_overlay(
        &self,
        placement: (i32, i32, i32, i32),
        sprites: Vec<(i32, i32, acl_ui::sprite::Bitmap)>,
    ) {
        let _ = self.commands.send(Command::Overlay { placement, sprites });
    }
}

/// How often the thread looks for frames when it is not doing anything else.
///
/// The helper samples five times a second, so this is comfortably below the rate anything
/// arrives at and costs a wake with nothing in it the rest of the time.
const TICK: Duration = Duration::from_millis(50);

/// The reader thread.
fn run(orders: &Receiver<Command>, reports: &Sender<Report>) {
    let mut link = Link::new();
    loop {
        match orders.recv_timeout(TICK) {
            Ok(Command::Start) => start(&mut link, reports),
            Ok(Command::Stop) => {
                link.stop();
                let _ = reports.send(Report::State(link.state()));
            }
            #[cfg(windows)]
            Ok(Command::ShowOverlay(visible)) => link.show_overlay(visible),
            #[cfg(windows)]
            Ok(Command::Overlay { placement, sprites }) => {
                let (x, y, width, height) = placement;
                link.place_overlay(x, y, width, height);
                // Clear, then every sprite, then present -- one frame appears at once
                // rather than half-composed, which on a talking ring would be a flicker
                // every time somebody spoke.
                link.clear_overlay();
                for (at_x, at_y, bitmap) in sprites {
                    link.draw_sprite(at_x, at_y, bitmap.width, bitmap.height, bitmap.pixels);
                }
                link.present_overlay();
            }
            #[cfg(not(windows))]
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // The window has gone. Stopping the link takes the helper with it, which is
            // the whole reason it is worth doing on the way out rather than leaving to the
            // helper's own watchdog.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                link.stop();
                return;
            }
        }

        #[cfg(windows)]
        for event in link.poll() {
            match event {
                Event::GameState(state) => {
                    let _ = reports.send(Report::Frame(state));
                }
                Event::Stopped(reason) => {
                    let _ = reports.send(Report::Trouble(reason));
                    let _ = reports.send(Report::State(link.state()));
                }
                // One frame that would not decode is not worth interrupting somebody
                // about, and the link has already decided it is not fatal.
                Event::Undecodable(_) => {}
            }
        }
    }
}

/// Starts the helper, unelevated first.
///
/// §6: the core "starts the helper on demand, unelevated, and re-launches it through UAC
/// only when the game's integrity level denies the read". The second half of that sentence
/// is not here yet — nothing has asked the helper to read a game it could not — so this
/// tries the first and reports what happened.
#[cfg(windows)]
fn start(link: &mut Link, reports: &Sender<Report>) {
    let _ = reports.send(Report::State(HelperState::Starting));
    let executable = match acl_core::launch::helper_beside_this_one() {
        Ok(path) => path,
        Err(error) => {
            let _ = reports.send(Report::Trouble(format!(
                "could not work out where the helper is: {error}"
            )));
            let _ = reports.send(Report::State(HelperState::Lost));
            return;
        }
    };
    if !acl_core::launch::is_plausible_helper(&executable) {
        let _ = reports.send(Report::Trouble(format!(
            "{} is not something to start elevated",
            executable.display()
        )));
        let _ = reports.send(Report::State(HelperState::Lost));
        return;
    }

    match link.start(&executable, Elevation::AsIs, OFFSETS) {
        Ok(()) => {}
        Err(error) => {
            let _ = reports.send(Report::Trouble(error.to_string()));
        }
    }
    let _ = reports.send(Report::State(link.state()));
}

#[cfg(not(windows))]
fn start(_link: &mut Link, reports: &Sender<Report>) {
    let _ = reports.send(Report::Trouble(
        "the game reader is a Windows binary and there is no other implementation".to_owned(),
    ));
}

/// The offsets the helper is given, both architectures of them.
///
/// Both, because which applies depends on the process the helper finds and only the helper
/// can see that. The compiled-in floor, and only that for now: fetching a newer bundle is
/// `acl_game::store`'s business and it is HTTP, which belongs on this side of the pipe —
/// §6 keeps every HTTP client out of the elevated process — but wiring the store in is a
/// change with its own failure modes, and a shell that opens a window does not need it.
#[cfg(windows)]
const OFFSETS: acl_core::link::Offsets<'static> = acl_core::link::Offsets {
    for_32bit: include_bytes!("../../acl-game/assets/offsets-x86.json"),
    for_64bit: include_bytes!("../../acl-game/assets/offsets-x64.json"),
};
