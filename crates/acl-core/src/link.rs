//! The driver that owns the helper: start it, talk to it, notice when it goes.
//!
//! Every piece of this existed before this module and none of them were joined up.
//! [`crate::helper`] says what state the helper is in and when it may be asked for,
//! [`crate::launch`] starts it, [`acl_ipc::pipe`] carries the conversation, and
//! `acl-helper` is at the other end. §4.6 says where the joining belongs, about its own
//! half of the same problem: "the driver that owns a socket, a membership and a set of
//! connections at once ... belongs with `acl-core` in P5".
//!
//! # What it is responsible for
//!
//! Exactly one thing that none of the parts can do alone: keeping
//! [`crate::helper::HelperState`] true. A state that says `Running` while the helper is a
//! zombie is worse than no state at all, because the UI then reports proximity working
//! when nothing is being read.
//!
//! # Blocking
//!
//! [`Link::start`] blocks — it launches a process and waits for it to answer, which behind
//! a UAC prompt is as long as somebody takes to read a dialog. It belongs on a worker
//! thread. [`Link::poll`] does not block in the ordinary case: it peeks before it reads,
//! for the reason recorded on [`acl_ipc::pipe::PipeConnection::available`]. It can still
//! wait out the tail of a frame whose first bytes have arrived, which is bounded by the
//! helper finishing a write it has already begun.

use std::path::Path;
use std::time::Duration;

use acl_game::AmongUsState;
use acl_ipc::pipe::{PipeConnection, connect, pipe_name};
use acl_ipc::stream::StreamTransport;
use acl_ipc::{CoreMessage, HelperMessage, Transport};

use crate::helper::{HelperState, VersionMismatch, check_protocol};
use crate::launch::{Elevation, Helper, LaunchError};

/// What the helper said, in terms the rest of the client cares about.
#[derive(Debug)]
pub enum Event {
    /// One frame of the game.
    GameState(Box<AmongUsState>),
    /// The helper stopped, and why.
    ///
    /// Its own words, for the log. A player is told [`HelperState`] instead.
    Stopped(String),
    /// Something arrived that could not be made sense of.
    ///
    /// Reported rather than swallowed, and not fatal: one unreadable frame is not a reason
    /// to tear down a helper that is otherwise answering.
    Undecodable(String),
}

/// Why the link could not be established.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// The helper would not start.
    #[error(transparent)]
    Launch(#[from] LaunchError),
    /// It started but never answered on the pipe.
    #[error("the helper did not answer: {0}")]
    Pipe(#[from] std::io::Error),
    /// A frame could not be written or read.
    #[error(transparent)]
    Frame(#[from] acl_ipc::FrameError),
    /// It answered with a protocol this client does not speak.
    #[error(transparent)]
    Protocol(#[from] VersionMismatch),
    /// It answered with something other than the greeting.
    #[error("the helper's first message was not a greeting")]
    NotAGreeting,
}

/// How long [`Link::stop`] waits for the helper to exit.
///
/// Long enough for a process whose only shutdown work is closing a pipe and a process
/// handle; short enough that a helper which has hung does not hold up the client.
#[cfg(windows)]
const SHUTDOWN_PATIENCE: Duration = Duration::from_secs(2);

/// The offsets bundles handed to the helper.
///
/// Both, and that is not belt and braces. Among Us ships as 32- and 64-bit, the bundles are
/// different files, and which one applies depends on the process the helper finds — which
/// the core cannot see. Sending one made this side guess, and a wrong guess is not an error
/// anywhere: every pointer chain resolves to nothing and the game reads as absent.
#[derive(Clone, Copy, Debug)]
pub struct Offsets<'a> {
    /// The bundle for a 32-bit game.
    pub for_32bit: &'a [u8],
    /// The bundle for a 64-bit game.
    pub for_64bit: &'a [u8],
}

/// The helper, and the conversation with it.
#[derive(Debug, Default)]
pub struct Link {
    state: HelperState,
    #[cfg(windows)]
    helper: Option<Helper>,
    transport: Option<StreamTransport<PipeConnection>>,
}

impl Link {
    /// A link that has not asked for anything yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What to tell the rest of the client.
    #[must_use]
    pub const fn state(&self) -> HelperState {
        self.state
    }

    /// Starts the helper and completes the handshake.
    ///
    /// The offsets are sent here rather than later because the helper cannot read anything
    /// without them, and a helper that is running but idle is a state with no use and one
    /// more way to be wrong.
    ///
    /// # Errors
    ///
    /// [`LinkError`], and the state is left where the failure put it — `Refused` when the
    /// user declined, `Lost` otherwise. Both are things to report rather than to retry
    /// immediately: see [`crate::helper::may_prompt`] for when asking again is allowed.
    #[cfg(windows)]
    pub fn start(
        &mut self,
        executable: &Path,
        elevation: Elevation,
        offsets: Offsets<'_>,
    ) -> Result<(), LinkError> {
        self.stop();
        self.state = HelperState::Starting;

        let core = acl_ipc::pipe::this_process_id();
        let helper = match crate::launch::start(executable, core, elevation) {
            Ok(helper) => helper,
            Err(error) => {
                // A declined prompt is not a failure to recover from, and the state has to
                // say which of the two it was: `may_prompt` refuses to ask a second time
                // after a refusal unless the user asks for it themselves.
                self.state = match error {
                    LaunchError::Refused => HelperState::Refused,
                    LaunchError::Io(_) => HelperState::Lost,
                };
                return Err(error.into());
            }
        };

        let result = self.shake_hands(&helper, core, offsets);
        if result.is_ok() {
            self.helper = Some(helper);
            self.state = HelperState::Running;
        } else {
            self.state = HelperState::Lost;
            self.transport = None;
        }
        result
    }

    /// Connects, checks who answered, and agrees on a protocol.
    #[cfg(windows)]
    fn shake_hands(
        &mut self,
        helper: &Helper,
        core: u32,
        offsets: Offsets<'_>,
    ) -> Result<(), LinkError> {
        let connection = connect(&pipe_name(core))?;
        // The other half of the mutual check. The helper refuses a client that is not the
        // id on its command line; this refuses a pipe server that is not the process the
        // launch returned, so a name taken by something else is talked to by neither.
        connection.expect_peer(helper.process_id(), false)?;

        let mut transport = StreamTransport::new(connection);
        match transport.recv::<HelperMessage>()? {
            Some(HelperMessage::Ready { protocol }) => check_protocol(protocol)?,
            _ => return Err(LinkError::NotAGreeting),
        }

        // Offsets before the instruction to read, because the helper has nothing to read
        // the game with until they arrive. Both of them: which applies is decided by the
        // process the helper finds, and only the helper can see that.
        transport.send(&CoreMessage::SetOffsets {
            is_64bit: false,
            bundle: offsets.for_32bit.to_vec(),
        })?;
        transport.send(&CoreMessage::SetOffsets {
            is_64bit: true,
            bundle: offsets.for_64bit.to_vec(),
        })?;
        transport.send(&CoreMessage::StartReading)?;
        self.transport = Some(transport);
        Ok(())
    }

    /// Everything the helper has said since the last call.
    ///
    /// Also the place the helper's death is noticed. A pipe that has closed is the usual
    /// signal; the process handle is checked as well, because a helper that was killed
    /// without its pipe being torn down would otherwise look alive until something was
    /// written to it.
    #[cfg(windows)]
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(transport) = self.transport.as_mut() else {
            return events;
        };

        loop {
            // `try_recv`, which consults this transport's own buffer before the pipe. A
            // loop that peeked only at the pipe would drop every frame that arrived in the
            // same read as the one before it -- see `StreamTransport::try_recv`.
            let message = match transport.try_recv::<HelperMessage>() {
                Ok(Some(message)) => message,
                Ok(None) => break,
                // A clean close, a torn frame, or a pipe that is gone.
                Err(_) => {
                    self.lost();
                    return events;
                }
            };
            match message {
                HelperMessage::GameState(payload) => {
                    match postcard::from_bytes::<AmongUsState>(&payload) {
                        Ok(state) => events.push(Event::GameState(Box::new(state))),
                        // One bad frame, not a broken helper. The alternative is a client
                        // that goes silent because of a single malformed packet from a
                        // process it started itself.
                        Err(error) => {
                            events.push(Event::Undecodable(format!("a game state: {error}")));
                        }
                    }
                }
                HelperMessage::Stopping { reason } => {
                    events.push(Event::Stopped(reason));
                    self.lost();
                    return events;
                }
                // `Ready` again, or a variant a newer helper knows about. Neither is an
                // error: the enum is `non_exhaustive` so that this can happen.
                _ => {}
            }
        }

        // Nothing was said, which is the ordinary case. The process is still worth asking
        // about: a helper killed from Task Manager leaves a pipe that reads as empty
        // rather than as closed until something is written to it.
        if self
            .helper
            .as_ref()
            .is_some_and(|helper| !helper.is_running())
        {
            self.lost();
        }
        events
    }

    /// Puts the overlay where the game is.
    ///
    /// The core is what can see it: reading another process's window rectangle is a read,
    /// and UIPI filters manipulation rather than reads. Moving the window is the half that
    /// has to happen in the elevated process, which is why the numbers cross the pipe
    /// instead of the window handle.
    ///
    /// Silently does nothing when there is no helper. Every caller of this already has to
    /// cope with a client that has none — that is what [`crate::helper::Capabilities`] is
    /// for — and making the overlay the one thing that reports it would put a failure path
    /// in front of a feature that is already documented as optional.
    #[cfg(windows)]
    pub fn place_overlay(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.tell(&CoreMessage::PlaceOverlay {
            x,
            y,
            width,
            height,
        });
    }

    /// Wipes the overlay's canvas. The start of a frame.
    #[cfg(windows)]
    pub fn clear_overlay(&mut self) {
        self.tell(&CoreMessage::ClearOverlay);
    }

    /// Hands the overlay one pre-rasterised sprite.
    ///
    /// Premultiplied BGRA, top row first, positioned relative to the overlay's own
    /// top-left. Rasterised here rather than there because §4.7 keeps every image decoder
    /// out of the elevated process -- and sent as a sprite rather than a frame because a
    /// frame does not fit: `acl_ipc::MAX_FRAME` is 64 KiB and an overlay covering a
    /// 2560x1440 screen is 14.7 MB.
    #[cfg(windows)]
    pub fn draw_sprite(&mut self, x: i32, y: i32, width: i32, height: i32, pixels: Vec<u8>) {
        self.tell(&CoreMessage::DrawSprite {
            x,
            y,
            width,
            height,
            pixels,
        });
    }

    /// Puts the composed canvas on the screen. The end of a frame.
    #[cfg(windows)]
    pub fn present_overlay(&mut self) {
        self.tell(&CoreMessage::PresentOverlay);
    }

    /// Shows or hides the overlay.
    #[cfg(windows)]
    pub fn show_overlay(&mut self, visible: bool) {
        self.tell(&CoreMessage::SetOverlayVisible(visible));
    }

    /// Sends one message.
    ///
    /// A dead pipe is the helper being lost. **A message too large is not** — it is a
    /// mistake on this side, and tearing down a working helper for it turns a caller's
    /// arithmetic error into a lost game reader and a UAC prompt to get it back. The first
    /// version of this made no distinction, and an oversized overlay frame took the whole
    /// helper down with it.
    #[cfg(windows)]
    fn tell(&mut self, message: &CoreMessage) {
        let Some(transport) = self.transport.as_mut() else {
            return;
        };
        match transport.send(message) {
            Ok(()) => {}
            Err(acl_ipc::FrameError::TooLarge(bytes)) => {
                tracing::error!(
                    bytes,
                    limit = acl_ipc::MAX_FRAME,
                    "refused to send an oversized message to the helper"
                );
            }
            Err(_) => self.lost(),
        }
    }

    /// Asks the helper to stop, waits briefly for it to, and forgets it either way.
    ///
    /// The request is best-effort: a helper that is already gone cannot be told anything,
    /// and its own watchdog ends it when this process does.
    ///
    /// **The wait is not politeness.** The helper's pipe is created with
    /// `FILE_FLAG_FIRST_PIPE_INSTANCE` and named after this process, so a replacement
    /// started while its predecessor is still shutting down cannot create the pipe at all,
    /// and this side then waits out the whole connect timeout for a name that never
    /// appears. Returning from `stop` before the process has gone makes the retry after
    /// `HelperState::Lost` fail for a reason unrelated to whatever lost it.
    ///
    /// Bounded, because a helper that will not exit must not hold up the client. Its
    /// watchdog is what ends that case, when this process does.
    pub fn stop(&mut self) {
        if let Some(transport) = self.transport.as_mut() {
            let _ = transport.send(&CoreMessage::Shutdown);
        }
        self.transport = None;
        #[cfg(windows)]
        {
            if let Some(helper) = self.helper.take()
                && !helper.wait(SHUTDOWN_PATIENCE)
            {
                // Nothing to do about it here. Said rather than swallowed, because the next
                // symptom is a connect that times out and looks like something else.
                tracing::warn!(
                    process = helper.process_id(),
                    "the helper did not exit when asked; a replacement may not get its pipe"
                );
            }
        }
        // Only from a state that was ever up. Stopping something that was refused must not
        // rewrite that into `Lost`, because `may_prompt` treats the two differently.
        if self.state == HelperState::Running || self.state == HelperState::Starting {
            self.state = HelperState::NotRequested;
        }
    }

    /// The helper went away without being asked to.
    fn lost(&mut self) {
        self.transport = None;
        #[cfg(windows)]
        {
            self.helper = None;
        }
        self.state = HelperState::Lost;
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::Link;
    use crate::helper::HelperState;

    #[test]
    fn a_link_that_has_asked_for_nothing_says_so() {
        let link = Link::new();
        assert_eq!(link.state(), HelperState::NotRequested);
    }

    /// Stopping a link that was refused must leave it refused. `may_prompt` allows a
    /// second prompt after `Lost` and refuses one after `Refused` unless the user asks, so
    /// rewriting one into the other is how a declined dialog comes back uninvited.
    #[test]
    fn stopping_does_not_erase_a_refusal() {
        let mut link = Link::new();
        link.state = HelperState::Refused;
        link.stop();
        assert_eq!(link.state(), HelperState::Refused);
    }

    /// And a link that was never started has nothing to say when polled, rather than
    /// deciding it has been lost.
    #[cfg(windows)]
    #[test]
    fn polling_before_starting_is_not_a_loss() {
        let mut link = Link::new();
        assert!(link.poll().is_empty());
        assert_eq!(link.state(), HelperState::NotRequested);
    }
}
