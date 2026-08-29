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
    /// The helper has gone, and the last frame it sent is no longer true.
    ///
    /// Separate from `State(Lost)` because the two do different things: one is what the
    /// window shows, and this is what the audio reads. `Reader::latest` kept serving the
    /// last frame after the helper died, so every player stayed placed where they stood at
    /// that moment -- proximity frozen rather than absent, which is worse, because
    /// somebody walking away stays audible.
    Lost,
}

/// A handle on the reader thread.
pub(crate) struct Reader {
    commands: Sender<Command>,
    reports: Receiver<Report>,
    state: HelperState,
    /// Whether the helper this is talking to is a different one from a moment ago.
    ///
    /// Read once and cleared. The overlay's visibility, its position and its contents all
    /// live in the helper's process, so a replacement starts with none of them -- and
    /// `Client::overlay_shown` is a latch that says "already told it", which meant the
    /// overlay never came back after a helper was replaced. It stayed told, to a process
    /// that had never heard.
    replaced: bool,
    latest: Option<Box<AmongUsState>>,
    trouble: Option<String>,
}

impl Reader {
    /// Starts the thread. It does nothing until asked.
    ///
    /// # Errors
    ///
    /// Whatever spawning the thread said.
    pub(crate) fn start(cache: std::path::PathBuf) -> std::io::Result<Self> {
        let (commands, orders) = channel();
        let (reports, inbox) = channel();
        std::thread::Builder::new()
            .name("game-reader".to_owned())
            .spawn(move || run(&orders, &reports, &cache))?;
        Ok(Self {
            commands,
            reports: inbox,
            state: HelperState::NotRequested,
            replaced: false,
            latest: None,
            trouble: None,
        })
    }

    /// Takes in everything the thread has said since the last look.
    ///
    /// Called once a frame, and never blocks.
    /// Whether the helper has been replaced since this was last asked, and forgets it.
    ///
    /// Everything the overlay is told lives in the helper's process: whether it is shown,
    /// where it is, and what is on it. A replacement knows none of it, so whoever composes
    /// the overlay has to say all three again -- and `overlay_shown` is a latch meaning
    /// "already told it", which is why the overlay never returned after a helper was
    /// replaced.
    pub(crate) fn take_replaced(&mut self) -> bool {
        std::mem::take(&mut self.replaced)
    }

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
                    // A helper that has just started knows nothing about an overlay: it
                    // has never been told to show one, never been given a position, and has
                    // nothing on its canvas. Whoever is composing the overlay has to be
                    // told to say all of that again.
                    if self.state == HelperState::Running && state != HelperState::Running {
                        self.replaced = true;
                    }
                    self.state = state;
                    if state == HelperState::Running {
                        self.trouble = None;
                    }
                }
                Ok(Report::Frame(state)) => {
                    // The palette travels on one frame per attach and nothing else reads
                    // it, so it is adopted here rather than carried further. See
                    // `acl_types::player_colors::adopt`: the avatar, the overlay and the
                    // recoloured sprites all have to agree about what colour seven is.
                    if let Some(colors) = state.player_colors.clone() {
                        acl_core::log_info!(
                            "reader",
                            "the game's palette has {} colours",
                            colors.len()
                        );
                        acl_types::player_colors::adopt(colors);
                    }
                    self.latest = Some(state);
                }
                // Cleared rather than left standing. A frame from a helper that has gone
                // places everybody where they were when it went, and a player who walked
                // away is then still audible from where they used to be.
                Ok(Report::Lost) => self.latest = None,
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

/// How long to wait before starting a helper that has been lost.
///
/// The helper's own retry interval for finding the game, which is the matching number: one
/// that died because the machine was busy starts on the next attempt, and one that dies
/// every time costs one process every seven and a half seconds rather than hundreds.
const RESTART_AFTER: Duration = Duration::from_millis(7_500);

/// The reader thread.
fn run(orders: &Receiver<Command>, reports: &Sender<Report>, cache: &std::path::Path) {
    let mut link = Link::new();
    // When to try again after the helper has gone.
    //
    // Nothing restarted it until 2026-08-29. `HelperState::Lost` was reported, the window
    // showed it, and there it stayed: no game state for the rest of the session, and
    // `Reader::latest` went on serving the last frame it had, so every player stayed placed
    // where they stood when the helper died. `may_prompt` says a lost helper may be started
    // again, and nothing ever asked.
    //
    // Restarted without a prompt, which is within §4.7 rather than around it: the prompt
    // exists for *elevation*, which asks the user for something. Starting an unelevated
    // helper they already asked for is resuming what they asked for.
    let mut restart_due: Option<std::time::Instant> = None;
    let mut wanted = false;
    // Which offsets file the helper is reading with, so a build that wants the same one
    // costs nothing. A second `SetOffsets` makes the helper drop its sampler and resolve
    // every signature again, which is a full set of pattern scans -- worth it for a
    // different build, wasted for the same one.
    #[cfg(windows)]
    let mut sent_file: Option<String> = None;
    // Where a build-keyed fetch lands. It runs on a thread of its own because it is HTTP
    // with a timeout, and this thread is the one draining game state.
    #[cfg(windows)]
    let (found_offsets, offsets_arriving) = std::sync::mpsc::channel::<(bool, String, Vec<u8>)>();
    loop {
        #[cfg(windows)]
        while let Ok((is_64bit, file, bundle)) = offsets_arriving.try_recv() {
            sent_file = Some(file);
            link.set_offsets(is_64bit, bundle);
        }

        match orders.recv_timeout(TICK) {
            Ok(Command::Start) => {
                wanted = true;
                restart_due = None;
                #[cfg(windows)]
                {
                    sent_file = start(&mut link, reports, cache);
                }
                #[cfg(not(windows))]
                let _ = start(&mut link, reports, cache);
            }
            Ok(Command::Stop) => {
                // Asked for, so it stays down. The restart below is for a helper that went
                // away on its own.
                wanted = false;
                restart_due = None;
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
                    // The frame the window is holding is now a lie, so it is cleared as
                    // well as reported.
                    let _ = reports.send(Report::Lost);
                }
                // One frame that would not decode is not worth interrupting somebody
                // about, and the link has already decided it is not fatal.
                Event::Undecodable(_) => {}
                Event::Attached { is_64bit, build } => {
                    // The moment the guess can be checked. Everything before this ran on
                    // what the mirror calls the current build; this is the game saying
                    // which build it actually is.
                    let Some(build) = build else {
                        acl_core::log_info!(
                            "reader",
                            "the helper found the game but not its build; keeping the                              offsets it was given"
                        );
                        continue;
                    };
                    let cache = cache.to_path_buf();
                    let already = sent_file.clone();
                    let found_offsets = found_offsets.clone();
                    // Detached on purpose. If it finishes after the helper has gone, the
                    // send finds a receiver that is still there -- the loop owns it -- and
                    // the bundle is handed to whatever helper is running then, which will
                    // report its own build and correct it if that was wrong.
                    std::thread::Builder::new()
                        .name("offsets".to_owned())
                        .spawn(move || {
                            if let Some((file, bundle)) =
                                refetch_for_build(&cache, build, already.as_deref(), is_64bit)
                            {
                                let _ = found_offsets.send((is_64bit, file, bundle));
                            }
                        })
                        .ok();
                }
            }
        }

        // And the restart. Checked every tick because the helper can go at any moment and
        // there is no event on the command channel to hang it off.
        #[cfg(windows)]
        if wanted && link.state() == HelperState::Lost {
            match restart_due {
                None => restart_due = Some(std::time::Instant::now() + RESTART_AFTER),
                Some(due) if std::time::Instant::now() >= due => {
                    restart_due = None;
                    acl_core::log_info!("reader", "the helper is gone; starting another");
                    sent_file = start(&mut link, reports, cache);
                }
                Some(_) => {}
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
fn start(link: &mut Link, reports: &Sender<Report>, cache: &std::path::Path) -> Option<String> {
    let _ = reports.send(Report::State(HelperState::Starting));
    let executable = match acl_core::launch::helper_beside_this_one() {
        Ok(path) => path,
        Err(error) => {
            let _ = reports.send(Report::Trouble(format!(
                "could not work out where the helper is: {error}"
            )));
            let _ = reports.send(Report::State(HelperState::Lost));
            return None;
        }
    };
    if !acl_core::launch::is_plausible_helper(&executable) {
        let _ = reports.send(Report::Trouble(format!(
            "{} is not something to start elevated",
            executable.display()
        )));
        let _ = reports.send(Report::State(HelperState::Lost));
        return None;
    }

    // Fetched here rather than held in a constant, and on this thread: it is two HTTP
    // requests with a timeout, and the thread it blocks is the reader's own, which has
    // nothing else to do until the helper is up.
    let guessed = offsets_for_the_helper(reports, cache);
    let offsets = acl_core::link::Offsets {
        for_32bit: &guessed.for_32bit,
        for_64bit: &guessed.for_64bit,
        patterns: guessed.patterns.as_deref(),
    };
    match link.start(&executable, Elevation::AsIs, offsets) {
        Ok(()) => {}
        Err(error) => {
            let _ = reports.send(Report::Trouble(error.to_string()));
        }
    }
    let _ = reports.send(Report::State(link.state()));
    guessed.file
}

#[cfg(not(windows))]
fn start(_link: &mut Link, reports: &Sender<Report>, _cache: &std::path::Path) -> Option<String> {
    let _ = reports.send(Report::Trouble(
        "the game reader is a Windows binary and there is no other implementation".to_owned(),
    ));
    None
}

/// The compiled-in floor, both architectures of it.
///
/// Both, because which applies depends on the process the helper finds and only the helper
/// can see that. Used when the mirror cannot be reached and nothing has been cached, which
/// is what a floor is for.
#[cfg(windows)]
const FLOOR: acl_core::link::Offsets<'static> = acl_core::link::Offsets {
    for_32bit: include_bytes!("../../acl-game/assets/offsets-x86.json"),
    for_64bit: include_bytes!("../../acl-game/assets/offsets-x64.json"),
    // The floor's own lookup carries patterns, but the floor is what is left when the
    // lookup could not be read at all -- and a build number is no use without the version
    // table that turns it into a file.
    patterns: None,
};

/// What the first round produced, and what a second round would have to beat.
#[cfg(windows)]
struct Guess {
    /// The 32-bit bundle that went across.
    for_32bit: Vec<u8>,
    /// The 64-bit one.
    for_64bit: Vec<u8>,
    /// The lookup's `patterns` object, for the helper's build scan.
    patterns: Option<Vec<u8>>,
    /// Which offsets file these came from, so a build that wants the same one costs
    /// nothing. `None` when the lookup could not be read and this is the floor.
    file: Option<String>,
}

/// The offsets to give the helper: the mirror's, the cache's, or the floor's.
///
/// **Nothing asked the store until 2026-08-29.** `acl_game::store` -- the lookup, the two
/// mirrors, the cache, the validation on every load, the rollback check -- had no
/// production caller at all, so the client ran on the compiled-in floor and nothing else.
/// That is not a degradation, it is the whole point of the store gone: Among Us moves its
/// fields on almost every update, and `offsetStore.ts` exists so a player gets working
/// proximity the day the mirror publishes rather than the day a new client ships.
///
/// HTTP on this side of the pipe, which is §6: no HTTP client in the elevated process.
///
/// # This is the guess, not the answer
///
/// The lookup is keyed by a build number compiled into `GameAssembly.dll`, and only a
/// process that has opened the game can read it. So this takes the `default` entry -- what
/// the mirror says the current build is -- and sends the lookup's byte patterns along with
/// it. The helper scans with them on its first attach and reports what it found; if that
/// build wants a different file, [`refetch_for_build`] fetches it and the core sends a
/// second `SetOffsets`.
///
/// The guess still has to be sent, and sent for both architectures: a helper with no
/// bundle for the game's width does not attach at all, and would never get far enough to
/// read a build.
#[cfg(windows)]
fn offsets_for_the_helper(reports: &Sender<Report>, cache: &std::path::Path) -> Guess {
    use acl_game::store::{HttpFetcher, OffsetStore};

    let floor = || Guess {
        for_32bit: FLOOR.for_32bit.to_vec(),
        for_64bit: FLOOR.for_64bit.to_vec(),
        patterns: None,
        file: None,
    };
    let store = OffsetStore::new(cache, env!("CARGO_PKG_VERSION"));
    let fetcher = HttpFetcher;

    let lookup = match store.load_lookup(&fetcher) {
        Ok(loaded) => {
            if let Some(why) = loaded.reason.as_deref() {
                acl_core::log_warn!(
                    "reader",
                    "the offsets lookup came from the {:?}: {why}",
                    loaded.source
                );
            }
            loaded.value
        }
        // The floor itself did not validate, which means this build shipped broken. There
        // is nothing left to fall back to, so the bytes go across unexamined and the helper
        // will say what it makes of them.
        Err(error) => {
            let _ = reports.send(Report::Trouble(format!(
                "the offsets lookup could not be read: {error}"
            )));
            return floor();
        }
    };

    let Some(entry) = lookup.entry_for("default") else {
        let _ = reports.send(Report::Trouble(
            "the offsets lookup names no default build; using the compiled-in offsets".to_owned(),
        ));
        return floor();
    };
    acl_core::log_info!(
        "reader",
        "offsets for {} from {}",
        entry.version,
        entry.file
    );

    // Both architectures, because the helper decides which it needs. One of the two
    // failing is not a reason to refuse the other: a 64-bit player is not helped by
    // withholding their offsets because the 32-bit file is missing.
    let one = |is_64bit: bool, fallback: &[u8]| -> Vec<u8> {
        match store.load_offsets(&fetcher, is_64bit, &entry.file) {
            Ok(loaded) => serde_json::to_vec(&loaded.value).unwrap_or_else(|_| fallback.to_vec()),
            Err(error) => {
                acl_core::log_warn!(
                    "reader",
                    "no {} offsets for {}: {error}; using the compiled-in ones",
                    if is_64bit { "x64" } else { "x86" },
                    entry.file
                );
                fallback.to_vec()
            }
        }
    };
    Guess {
        for_32bit: one(false, FLOOR.for_32bit),
        for_64bit: one(true, FLOOR.for_64bit),
        // Validated before it left the store -- `Lookup::validate` checks the broadcast
        // pattern on the same footing as the offsets themselves, because this is the one
        // thing a remote file contributes to what the elevated helper does.
        patterns: serde_json::to_vec(&lookup.patterns).ok(),
        file: Some(entry.file.clone()),
    }
}

/// The offsets for a build the helper has just reported, if they differ from the guess.
///
/// Runs on a thread of its own: it is two HTTP requests with a timeout, and the reader's
/// own thread is the one draining game state from the helper. Blocking it here would stop
/// proximity for as long as the mirror takes to answer, which on an unreachable mirror is
/// the full timeout.
///
/// Returns nothing when the build is the one already sent, when the lookup does not
/// describe it, or when the file cannot be had. **The last is deliberate.** The store
/// refuses to serve the compiled-in floor for a build it does not describe, and falling
/// back to it here would be the one thing that refusal exists to prevent: offsets for a
/// different game. The helper keeps reading with the guess, which is at least a bundle
/// somebody published for *some* build, and says so in the log.
#[cfg(windows)]
fn refetch_for_build(
    cache: &std::path::Path,
    build: i32,
    already_sent: Option<&str>,
    is_64bit: bool,
) -> Option<(String, Vec<u8>)> {
    use acl_game::store::{HttpFetcher, OffsetStore};

    let store = OffsetStore::new(cache, env!("CARGO_PKG_VERSION"));
    let fetcher = HttpFetcher;
    let lookup = store.load_lookup(&fetcher).ok()?.value;

    let key = build.to_string();
    // `entry_for` falls back to `default` for a build it does not know, which is what
    // makes an unrecognised build cost nothing rather than fail.
    let entry = lookup.entry_for(&key)?;
    if already_sent == Some(entry.file.as_str()) {
        return None;
    }

    match store.load_offsets(&fetcher, is_64bit, &entry.file) {
        Ok(loaded) => {
            let bundle = serde_json::to_vec(&loaded.value).ok()?;
            acl_core::log_info!(
                "reader",
                "the game reports build {key}; switching to {} from the {:?}",
                entry.file,
                loaded.source
            );
            Some((entry.file.clone(), bundle))
        }
        Err(error) => {
            acl_core::log_warn!(
                "reader",
                "the game reports build {key}, which wants {}, and it could not be had: \
                 {error}; keeping what the helper has",
                entry.file
            );
            None
        }
    }
}
