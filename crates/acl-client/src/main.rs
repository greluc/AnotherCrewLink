//! No console window.
//!
//! Reported against 2.0.0-alpha.1: a terminal opened beside the client. A Rust binary is a
//! console-subsystem application unless it says otherwise, so Windows gives it a console --
//! and one that nobody asked for, in front of a proximity chat, is a window a user has to
//! work out is safe to ignore.
//!
//! Unconditional rather than `cfg_attr(not(debug_assertions))`. A debug build that behaves
//! differently from the shipped one in the window department is a difference that hides
//! exactly this class of bug until a release, which is where this one was found.
//!
//! What went with it: `eprintln!` now writes nowhere at all, because a windows-subsystem
//! process has no standard error to write to. Every diagnostic in this binary was going to
//! a handle that does not exist. They go to `acl_core::logging` instead, which writes the
//! file under the player's profile that `logFile.ts` has always written -- same shape, same
//! four-mebibyte cap, same single previous file, so a support conversation can read a 1.x
//! and a 2.x log side by side.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! §4.8 item 1 continues below.
//!
//! §4.8 item 1: the shell, a custom title bar, and window state persistence. It is also the
//! first thing in this port that assembles the rest — the single-instance lock, the paths,
//! the restored geometry, the helper and the reader all meet here, and until they did,
//! every one of them was a library nobody called.
//!
//! # What it looks like to the operating system
//!
//! Frameless, resizable, not maximisable and not full-screenable, minimum 250 by 350 —
//! which is `new BrowserWindow({ frame: false, ... })` in `src/main/index.ts`, number for
//! number. The defaults are the minimum because that is what the shipped client passes.
//!
//! # The renderer
//!
//! `glow`, and §3.3 chooses `wgpu`. That choice comes with §4.8 item 6 — the GPU fallback
//! chain — and the dependency review that belongs to it; a shell that has to open a window
//! does not need to start either. `experiments/gui-spike` measured this rung, so the
//! performance number already on record is the one this runs on.

use acl_ui::views::theme;

mod audio;
mod controls;
mod hat_store;
mod net;
mod reader;
mod settings_page;
mod updates;

use std::path::PathBuf;

use acl_core::paths::{Environment, Paths};
use acl_core::single_instance;
use acl_ui::roster::{Roster, Voice, main_view, overlay};
use acl_ui::views::main::Portrait;
use acl_ui::window_state::{Rect, Stored, WindowState, restore, worth_saving};
use eframe::egui;

/// The window's minimum, and its default.
///
/// `MAIN_WINDOW_MIN_WIDTH` and `MAIN_WINDOW_MIN_HEIGHT` in `src/main/index.ts`, where the
/// defaults are the minimums too.
const MIN_WIDTH: i32 = 250;
/// See [`MIN_WIDTH`].
const MIN_HEIGHT: i32 = 350;

/// The size a window opens at when nothing has been saved yet.
///
/// **Not the minimum**, which is what stood here and is what `src/main/index.ts` passes
/// `windowStateKeeper` as its default. A floor is not a choice: 250 by 350 is the smallest
/// this window is allowed to be, and opening a fresh installation there means every new
/// player meets the product at its most cramped.
///
/// 400 by 520 is the size the design system draws the client at -- every mockup under
/// `design/mockups/` sets it, and `reference.json` names 400 as the typical width against
/// 250 as the minimum. Somebody who has sized the window keeps their size; this is only
/// for the first run and for a `windows.json` that has gone missing.
const DEFAULT_WIDTH: i32 = 400;
/// See [`DEFAULT_WIDTH`].
const DEFAULT_HEIGHT: i32 = 520;

/// How long the geometry has to hold still before it is written down.
///
/// Long enough that a drag is one write rather than a hundred, short enough that letting
/// go and pulling the plug keeps the size. The window repaints five times a second when
/// nothing is happening, so this is noticed within a frame of expiring.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(1);

/// How often the overlay is recomposed, whatever the window is repainting at.
///
/// [`Client::compose_overlay`] opens with `find_process("Among Us.exe")`, and that is a
/// `CreateToolhelp32Snapshot` over every process on the machine. Measured on 2026-08-28:
/// **18.3ms**, against 0.9ms for the window lookup after it. Eighteen milliseconds is the
/// whole budget of a 50Hz frame, spent before anything is drawn.
///
/// It ran once per repaint, under a comment saying it ran at the helper's five a second.
/// Those are the same number only while nothing moves: egui repaints on every mouse event,
/// so dragging the window put a hundred process-table snapshots a second in front of the
/// paint, and the window juddered under the pointer that was moving it.
const OVERLAY_TICK: std::time::Duration = std::time::Duration::from_millis(200);

/// Something that happens at most this often, however often it is asked.
///
/// A timestamp rather than a counter of frames, because the thing it paces is a wall-clock
/// cadence -- the game reports five times a second -- and the frames it is asked from are
/// however many the mouse generates.
#[derive(Default)]
struct Cadence(Option<std::time::Instant>);

impl Cadence {
    /// Whether it is due, and marks it done when it is.
    fn due(&mut self, now: std::time::Instant, period: std::time::Duration) -> bool {
        if self.0.is_some_and(|last| now.duration_since(last) < period) {
            return false;
        }
        self.0 = Some(now);
        true
    }
}

/// How tall the title bar is drawn.
const TITLE_BAR: f32 = acl_ui::views::theme::TITLEBAR_H;

/// The smallest rectangle containing both.
///
/// The meeting overlay's window is only as large as the seats that are drawn into it, and
/// which those are changes with who is speaking.
#[cfg(windows)]
fn union(
    left: acl_ui::overlay_layout::Rect,
    right: acl_ui::overlay_layout::Rect,
) -> acl_ui::overlay_layout::Rect {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    acl_ui::overlay_layout::Rect {
        x,
        y,
        width: (left.x + left.width).max(right.x + right.width) - x,
        height: (left.y + left.height).max(right.y + right.height) - y,
    }
}

/// How large one crewmate is in the overlay.
///
/// How far apart they sit is `acl_ui::overlay_layout::GAP`, which is also what the strip
/// is sized from -- one number, in the module that does the arithmetic.
///
/// Fixed rather than scaled to the game window: a 4K screen and a 1080p one want the same
/// physical size, and scaling by the window would make the overlay twice as large on the
/// larger monitor for no reason anybody asked for.
/// What a dressed crewmate looks like, as everything that changes the picture.
///
/// The cache key. Colour and the three cosmetics and nothing else: whether a player is
/// speaking or dead is drawn *over* the sprite by the view, so two players in the same
/// outfit share one texture however differently they are behaving.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Appearance {
    colour: i32,
    hat: String,
    skin: String,
    visor: String,
    /// Which of the two masters the body comes from.
    ///
    /// Part of the key rather than a tint applied afterwards, because the ghost is a
    /// different drawing — it has a tail where the crewmate has legs.
    alive: bool,
}

/// The dressed crewmates the main window is showing, as GPU textures.
///
/// # Why a cache and not a build per frame
///
/// Building one is a composite of up to five layers into a bitmap and an upload. At sixty
/// frames a second for fifteen players that is the most expensive thing in the window, to
/// produce the same picture every time.
///
/// # Why it cannot grow
///
/// Everything not asked for during a frame is dropped at the end of it. A lobby holds
/// fifteen players, so the live set is small and bounded — but a session spans many lobbies
/// and many outfits, and a map that only ever inserted would hold a texture for everybody
/// the user has played with today.
#[derive(Default)]
struct Portraits {
    held: std::collections::BTreeMap<Appearance, egui::TextureHandle>,
    /// What this frame asked for. Cleared at the start of each frame rather than allocated
    /// per frame.
    seen: std::collections::BTreeSet<Appearance>,
}

impl Portraits {
    /// The texture for one player, building it if the artwork is there.
    ///
    /// `None` while the cosmetics are still being fetched, which the view draws as shapes —
    /// deliberately, and not as a placeholder: artwork can fail to arrive at all, and a
    /// window that showed nothing then would look broken for a reason nobody can see.
    fn of(
        &mut self,
        context: &egui::Context,
        hats: &mut hat_store::Loader,
        appearance: Appearance,
    ) -> Option<egui::TextureId> {
        self.seen.insert(appearance.clone());
        if let Some(held) = self.held.get(&appearance) {
            return Some(held.id());
        }

        let pieces = acl_ui::worn::pieces(
            hats.collection(),
            acl_ui::worn::Worn {
                hat: &appearance.hat,
                skin: &appearance.skin,
                visor: &appearance.visor,
            },
            acl_types::cosmetics::HAT_COLLECTION_URL,
            acl_ui::hats::BASE,
        );
        // No early return for a player wearing nothing. There used to be one -- the view
        // draws its own crewmate and that seemed cheaper than a texture of the same shapes
        // -- but the shapes are not the same: `views::main::shapes` paints two circles and
        // a visor, and the body is a drawing. So a lobby of players in default outfits was
        // a row of coloured discs next to the shipped client's crewmates. The fallback
        // stays for the case it was written for, which is artwork that will not decode.
        //
        // Every layer, or none. A half-dressed crewmate cached now would stay half-dressed
        // until the outfit changed, because nothing here revisits a texture it already has.
        if pieces
            .iter()
            .filter_map(|piece| piece.url.as_deref())
            .any(|url| hats.image(url).is_none())
        {
            return None;
        }

        let (body, shadow) = acl_ui::views::colour::crew(appearance.colour);
        let mut canvas = acl_ui::sprite::Bitmap::blank(PORTRAIT_SPRITE, PORTRAIT_SPRITE);
        for piece in &pieces {
            let Some(url) = piece.url.as_deref() else {
                // The body. Plain: no speaking ring and no fading, because the view draws
                // both of those over whatever body it has and would otherwise draw them
                // twice.
                //
                // Low and slightly wider than its box, which is where every cosmetic's
                // geometry is measured from -- see `worn::BASE_TOP`.
                let base = acl_ui::body::recoloured(
                    appearance.alive,
                    (body.r(), body.g(), body.b()),
                    (shadow.r(), shadow.g(), shadow.b()),
                )?;
                let (at, size) = acl_ui::worn::base_placement(PORTRAIT_SPRITE);
                canvas.composite(&base, at, size);
                continue;
            };
            // A ghost wears nothing. `Avatar.tsx` sets `display: none` on all three
            // cosmetic layers for a dead player -- the hat does not follow you out.
            if !appearance.alive {
                continue;
            }
            let Some(artwork) = hats.image(url) else {
                continue;
            };
            let artwork = artwork.clone();
            let (at, size) = acl_ui::worn::placement(
                piece.geometry,
                PORTRAIT_SPRITE,
                (artwork.width, artwork.height),
            );
            canvas.composite(&artwork, at, size);
        }

        // The round window the shipped client's `border-radius: 50%` gives it. Last, so it
        // takes the cosmetics with it -- a hat wide enough to leave the circle is cropped
        // there rather than sticking out of a round avatar.
        acl_ui::sprite::clip_to_circle(&mut canvas);

        // Named after everything it is built from. It was named after the colour and the
        // hat, which is not a key: two players in one colour and one hat, one of them in a
        // skin, would have shared a name in the texture debugger while being two textures.
        let handle = context.load_texture(
            format!(
                "portrait-{}-{}-{}-{}-{}",
                appearance.colour,
                appearance.hat,
                appearance.skin,
                appearance.visor,
                if appearance.alive { "alive" } else { "ghost" },
            ),
            acl_ui::sprite::to_image(&canvas),
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.held.insert(appearance, handle);
        Some(id)
    }

    /// Drops everything this frame did not ask for.
    ///
    /// Dropping the handle is what frees the texture: egui keeps it alive exactly as long as
    /// somebody holds one.
    fn sweep(&mut self) {
        self.held.retain(|key, _| self.seen.contains(key));
        self.seen.clear();
    }
}

/// The capture settings that are fixed when the microphone opens.
///
/// Read here rather than in `audio`, because the settings file belongs to this half and
/// `audio` should not have to know what a key is called. The three of them are
/// `getUserMedia` constraints on the shipped client and are applied the same way: at open
/// time, and again when the reload button reopens the audio.
/// Whether it is time to try the signalling server again, and what to remember.
///
/// Returns the attempt number when a connection should be made now, and the schedule to
/// hold until the next call. Pure, because the alternative is a reconnect policy that can
/// only be checked by unplugging a network cable.
///
/// The doubling is `acl_net::reconnect`'s, which is the same schedule the peer connections
/// use. `initiates_reconnect`'s asymmetry does not apply: there is no second end to
/// collide with here, only a server, so the answering grace is not added.
fn reconnect_due(
    retry: Option<(std::time::Instant, u32)>,
    now: std::time::Instant,
) -> (Option<u32>, Option<(std::time::Instant, u32)>) {
    let (due, attempt) = retry.unwrap_or((now + acl_net::reconnect::reconnect_delay(1, true), 1));
    if now < due {
        return (None, Some((due, attempt)));
    }
    let next = attempt.saturating_add(1);
    (
        Some(attempt),
        Some((now + acl_net::reconnect::reconnect_delay(next, true), next)),
    )
}

fn capture_settings(stored: &acl_ui::config::Config) -> audio::Capture {
    audio::Capture {
        echo_cancellation: stored.bool_at("echoCancellation"),
        noise_suppression: stored.bool_at("noiseSuppression"),
        voice_detection: stored.bool_at("vadEnabled"),
        fixed_rate: stored.bool_at("oldSampleDebug"),
    }
}

/// The capture settings that change while it is open, as `audio::Audio::tune` wants them.
///
/// `microphoneGain` is a percentage and the gain is a multiplier, which is the shipped
/// client's own `settings.microphoneGain / 100`. Each is `None`-equivalent when its own
/// checkbox is off; see `tune` for why they are independent here and coupled there.
fn live_settings(stored: &acl_ui::config::Config) -> (f32, Option<f64>) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a percentage between zero and three hundred, divided by a hundred"
    )]
    let gain = if stored.bool_at("microphoneGainEnabled") {
        (stored.number_at("microphoneGain") / 100.0) as f32
    } else {
        1.0
    };
    let floor = stored
        .bool_at("micSensitivityEnabled")
        .then(|| stored.number_at("micSensitivity"));
    (gain, floor)
}

/// The pickable devices for one direction, as the pairs the picker shows.
///
/// The device the settings already name is in the list whether or not the machine can see
/// it. A headset that is unplugged is still what the client is set to use, and a picker that
/// silently drops it looks like the setting was lost — worse, picking anything else to make
/// the list sensible would change the setting. It is shown by the label stored beside it,
/// which is what `microphoneLabel` and `speakerLabel` are for: Windows changes a device's id
/// when it moves to another port, and the label is what survives that.
fn named(
    devices: &[acl_audio::device::Device],
    direction: acl_audio::device::Direction,
    stored: &acl_ui::config::Config,
    key: &str,
) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = devices
        .iter()
        .filter(|device| device.direction == direction)
        .map(|device| (device.id.clone(), device.name.clone()))
        .collect();

    let chosen = stored.text_at(key);
    if !chosen.is_empty() && !pairs.iter().any(|(id, _)| *id == chosen) {
        let label = stored.text_at(&format!("{key}Label"));
        let shown = if label.is_empty() {
            chosen.clone()
        } else {
            label
        };
        pairs.insert(0, (chosen, shown));
    }
    pairs
}

/// A platform's fields, owned.
///
/// `start_game::Described` borrows, and these are borrowed from `self` -- which the button
/// press then needs mutably. Owning them for one frame is what lets those two not overlap.
struct Startable {
    run_path: String,
    is_uri: bool,
    execute: Vec<String>,
}

/// The machine's microphones and speakers, as the settings screen needs them.
///
/// Cached, because enumerating is a trip through WASAPI and the screen repaints five times
/// a second — but not cached for the session, because a headset plugged in while the
/// settings are open should appear in the list. Two seconds is below noticing and far above
/// the repaint rate.
#[derive(Default)]
struct Devices {
    held: Vec<acl_audio::device::Device>,
    refreshed: Option<std::time::Instant>,
    /// A refresh that is running on a thread of its own. See [`Devices::current`].
    asking: Option<std::sync::mpsc::Receiver<Vec<acl_audio::device::Device>>>,
}

/// Where the game's window is, without walking the process table to find out.
///
/// `find_process("Among Us.exe")` is a `CreateToolhelp32Snapshot` over every process on the
/// machine, and it cost **18.3ms** measured on 2026-08-28. Held to five times a second it
/// still took a whole overlay tick to **15 to 24ms**, five times a second, on the thread
/// that draws -- which is a dropped frame every fifth of a second, and the last thing
/// standing between a scroll and sixty frames.
///
/// A process id does not change while a process lives, so it is worth remembering. The
/// liveness check is the window lookup that has to happen anyway: `content_bounds` walks
/// the top-level windows, costs **0.9ms**, and a game that has gone has no window to find.
/// Only when that fails is the table walked again, and that walk happens on a thread.
///
/// **Process ids are reused**, so in principle a recycled id belonging to something else
/// with a window could be followed. In practice this is only reached while the reader is
/// still reporting game state -- which means the game is still running -- and the id is
/// looked up by name again the moment its window stops being found.
#[derive(Default)]
struct GameWindow {
    /// The game's process, once something has found it.
    known: Option<u32>,
    /// A search running on a thread of its own.
    looking: Option<std::sync::mpsc::Receiver<Option<u32>>>,
    /// When the last search finished, so a machine with no game does not start one a frame.
    searched: Option<std::time::Instant>,
}

impl GameWindow {
    /// How long to wait before looking for the game again after not finding it.
    const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_secs(1);

    /// Where the game is drawing, if it is.
    #[cfg(windows)]
    fn bounds(&mut self) -> Option<acl_core::game_window::Bounds> {
        match self
            .looking
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv)
        {
            Some(Ok(found)) => {
                self.known = found;
                self.looking = None;
                self.searched = Some(std::time::Instant::now());
            }
            // The thread ended without sending, which cannot happen unless it panicked.
            // Treated as "not found" so the back-off applies rather than a search a frame.
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.looking = None;
                self.searched = Some(std::time::Instant::now());
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => {}
        }

        if let Some(process) = self.known {
            if let Some(bounds) = acl_core::game_window::content_bounds(process) {
                return Some(bounds);
            }
            // No window under that id any more, so as far as this is concerned the game has
            // gone. Looking again is the next block.
            self.known = None;
        }

        let due = self
            .searched
            .is_none_or(|last| last.elapsed() >= Self::LOOK_AGAIN);
        if due && self.looking.is_none() {
            let (send, receive) = std::sync::mpsc::channel();
            // Detached: nothing waits for it, and the answer is picked up whenever it lands.
            std::thread::spawn(move || {
                let _ = send.send(acl_game::windows::find_process("Among Us.exe"));
            });
            self.looking = Some(receive);
        }
        None
    }
}

impl Devices {
    /// How stale the list may be.
    const FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(2);

    /// What the machine has, asking again if it has been a while.
    ///
    /// **The asking happens on a thread.** Measured on 2026-08-28: enumerating the
    /// machine's audio devices costs **11 to 14 milliseconds**, and it used to happen
    /// inside the paint. Every two seconds one frame took fourteen times its budget, which
    /// is a visible hitch in a list somebody is scrolling -- and the settings screen is the
    /// only place this is called from, so the settings screen was the only place it showed.
    ///
    /// The first list is fetched here and now, because a picker that is empty for two
    /// frames while a thread starts is worse than one frame that takes fourteen
    /// milliseconds on the way in. Every refresh after that is asked for in the background
    /// and collected whenever it arrives.
    fn current(&mut self) -> &[acl_audio::device::Device] {
        use acl_audio::device::Backend as _;

        if self.refreshed.is_none() {
            // A failure leaves the last good list standing rather than emptying the picker:
            // a device that was there a moment ago is a better answer than none, and the
            // one that is stored is still what the client is using.
            if let Ok(found) = acl_audio::device::system::Cpal::new().devices() {
                self.held = found;
            }
            self.refreshed = Some(std::time::Instant::now());
            return &self.held;
        }

        match self
            .asking
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv)
        {
            Some(Ok(found)) => {
                self.held = found;
                self.asking = None;
                self.refreshed = Some(std::time::Instant::now());
            }
            // The thread ended without sending, which is the enumeration having failed.
            // The clock is restarted so this is tried again on the usual interval rather
            // than every frame.
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.asking = None;
                self.refreshed = Some(std::time::Instant::now());
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => {}
        }

        let due = self
            .refreshed
            .is_none_or(|last| last.elapsed() >= Self::FRESH_FOR);
        if due && self.asking.is_none() {
            let (send, receive) = std::sync::mpsc::channel();
            // Detached: nothing waits for it, and dropping the sender is how a failure is
            // reported. cpal initialises COM on whatever thread it is called from.
            std::thread::spawn(move || {
                if let Ok(found) = acl_audio::device::system::Cpal::new().devices() {
                    let _ = send.send(found);
                }
            });
            self.asking = Some(receive);
        }
        &self.held
    }
}

/// One icon in the window chrome.
///
/// Frameless and `#777`, which is what the design system means by an icon button: "Icons
/// in chrome are `#777` and nothing else." The 2px white border the style gives every
/// other button belongs to the outline buttons -- the launch control and reload -- and on
/// a 24px strip it turns three icons into three boxes.
fn chrome_icon(ui: &mut egui::Ui, glyph: &str, hint: &str) -> egui::Response {
    let text = egui::RichText::new(glyph)
        .font(acl_ui::views::theme::icon_font(18.0))
        .color(acl_ui::views::theme::ICON_QUIET);
    ui.add(egui::Button::new(text).frame(false))
        .on_hover_text(hint)
}

/// Says what the window frame just did, when asked to.
///
/// Behind `ACL_CHROME_LOG` because it is a line per click and nobody needs that by default.
/// It exists at all because a frameless window's move and resize are the client's own job --
/// there is no system title bar to blame -- and when they do not work there is nothing to
/// look at. The closure is not called unless the variable is set.
fn chrome_log(what: impl FnOnce() -> String) {
    if std::env::var_os("ACL_CHROME_LOG").is_some() {
        acl_core::log_info!("chrome", "{}", what());
    }
}

/// One overlay crewmate: the body artwork in its box, with the speaking ring around it.
///
/// The same body the main window draws, from the same master and at the same offset, because
/// `Overlay.tsx` renders the very same `Avatar` component the player list does. Keeping them
/// on one geometry is also what lets `worn::placement` mean one thing: it measures from the
/// body, and both surfaces now put the body in the same place.
///
/// Falls back to the drawn shape if the vendored artwork does not decode, which is a broken
/// build rather than anything a running client can reach.
#[cfg(windows)]
fn overlay_body(
    size: i32,
    talking: bool,
    alive: bool,
    body: (u8, u8, u8),
    shadow: (u8, u8, u8),
) -> acl_ui::sprite::Bitmap {
    let Some(artwork) = acl_ui::body::recoloured(alive, body, shadow) else {
        return acl_ui::sprite::crewmate(
            size,
            acl_ui::sprite::Crewmate {
                body,
                shadow,
                talking,
                alive,
            },
        );
    };
    let mut bitmap = acl_ui::sprite::Bitmap::blank(size, size);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a sprite size in pixels, far below f32's exact integer range"
    )]
    let extent = size as f32;
    // Before the body, and at the same radius `sprite::crewmate` uses, so a sprite does not
    // change size when somebody starts speaking.
    if talking {
        bitmap.ring(
            (extent / 2.0, extent / 2.0),
            extent / 2.0 - extent * 0.04,
            extent * 0.06,
            acl_ui::sprite::TALKING,
        );
    }
    let (at, placed) = acl_ui::worn::base_placement(size);
    bitmap.composite(&artwork, at, placed);
    bitmap
}

/// How large a main-window crewmate is rasterised.
///
/// Larger than the 52 points it is drawn at, so it survives a high-DPI display without
/// looking soft. Not larger still: it is composited on the CPU and uploaded, and the cost of
/// both is the square of this.
const PORTRAIT_SPRITE: i32 = 128;

/// What the lobby has already been told, so it is not told again every frame.
///
/// Both of these are claims made on a *transition*: the state they describe is a level and
/// the wire wants edges. `setHost` every frame would be a message a second to a server that
/// already agrees, and the radio key is held rather than tapped.
///
/// One struct rather than two fields because they are the same kind of thing, and because
/// three loose booleans on a client this size is where nobody can tell which are state and
/// which are memory of what was sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Announced {
    /// The server has been told this client is the game host.
    host: bool,
    /// The lobby has been told this player is on the impostor radio.
    radio: bool,
}

/// The three cosmetic ids a player is wearing.
///
/// Named fields rather than a tuple of three `String`s: they are the same type, and nothing
/// would catch two of them being swapped -- which shows up as a visor worn as a hat, on
/// somebody else's screen, with no error anywhere.
#[cfg(windows)]
struct Wearing {
    hat: String,
    skin: String,
    visor: String,
    /// Whether they are alive, because a ghost wears nothing.
    ///
    /// `Avatar.tsx` sets `display: none` on all three cosmetic layers for a dead player, and
    /// the overlay renders that same component.
    alive: bool,
}

const OVERLAY_SPRITE: i32 = 56;

fn main() -> eframe::Result<()> {
    let paths = match Paths::resolve(Environment {
        app_data: std::env::var("APPDATA").ok().as_deref(),
    }) {
        Ok(paths) => paths,
        Err(error) => {
            // Before a window exists, so there is nowhere to show it but here. A client
            // that cannot work out where its files go cannot start, and saying so beats a
            // window with no settings in it.
            acl_core::log_error!("client", "{error}");
            return Ok(());
        }
    };

    // Before anything else takes a resource. Two clients against one game means two
    // keyboard hooks, two memory readers and two overlays -- see `single_instance`, which
    // also explains why the check is a window rather than the mutex §4.7 first named.
    // 1.x's settings, brought forward on the first run and never written back. Before the
    // window, because the settings decide which renderer it opens with and which language
    // it opens in.
    let carried = acl_core::paths::import::settings_forward(
        &paths.config_file(),
        paths.legacy_config_file().as_deref(),
    );
    if carried == acl_core::paths::import::Outcome::Failed {
        // Not a reason to refuse to start: a first run with defaults is a working client.
        acl_core::log_warn!(
            "settings",
            "1.x's settings could not be read; starting with defaults"
        );
    }

    #[cfg(windows)]
    let _instance = match single_instance::claim(paths.user_data(), paths.legacy_user_data()) {
        Ok(guard) => guard,
        Err(occupant) => {
            // A message box, because there is no console to print to and this is the one
            // line a user has to see: without it a second copy simply does nothing, which
            // reads as a client that will not start.
            tell_the_user(occupant.message());
            return Ok(());
        }
    };

    // Before anything else has anything to say. Every diagnostic in this binary goes here
    // and nowhere else, because a windows-subsystem process has no standard error.
    acl_core::logging::open(&paths.log_file());
    acl_core::log_info!(
        "client",
        "AnotherCrewLink {} starting",
        env!("CARGO_PKG_VERSION")
    );

    let file = paths.window_state_file();
    let saved = read_state(&file);
    let opening = restore(saved, &displays(), DEFAULT_WIDTH, DEFAULT_HEIGHT);

    #[expect(
        clippy::cast_precision_loss,
        reason = "window dimensions in pixels, far below f32's exact integer range"
    )]
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("AnotherCrewLink")
        // Frameless, because the title bar is drawn below. The edges are hit-tested by
        // `acl_ui::edges` for the same reason -- see that module for why `with_resizable`
        // on its own leaves a window that cannot be dragged larger.
        //
        // **The style word is not how to check this.** Measured on 2026-08-26: the window
        // keeps `WS_CAPTION | WS_THICKFRAME` -- style `0x16cf0000` -- and is undecorated all
        // the same. winit removes the non-client area with `WM_NCCALCSIZE` rather than by
        // dropping the styles, which is what keeps snap layouts and the resize borders
        // working on a frameless window. What says it worked is the client rectangle
        // equalling the window rectangle, which it does: 458x351 both ways, a one-pixel
        // inset at the top.
        .with_decorations(false)
        .with_resizable(true)
        // Neither is offered by the shipped client, and the window state keeper skips
        // saving in both -- so allowing them here would produce a state nothing restores.
        .with_maximize_button(false)
        .with_inner_size([opening.width as f32, opening.height as f32])
        .with_min_inner_size([MIN_WIDTH as f32, MIN_HEIGHT as f32]);
    // The taskbar button had nothing to draw. Windows looks for a window's own icon first
    // and falls back to the executable's, and this client had set neither -- so what people
    // saw beside a 1.x client with a proper icon was a blank sheet. Left unset if it will
    // not decode: a client that starts without its icon is still a client.
    if let Some(icon) = acl_ui::icon::window() {
        viewport = viewport.with_icon(icon);
    }
    if let Some(rect) = opening.rect() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "screen coordinates in pixels, far below f32's exact integer range"
        )]
        {
            viewport = viewport.with_position([rect.x as f32, rect.y as f32]);
        }
    }

    // §4.8 item 6. The setting is 1.x's own `hardware_acceleration`, read straight out of
    // the file it already writes rather than from a key invented here: a renamed key reads
    // as absent, and an absent key gives acceleration back to every player who turned it
    // off -- on the machines least able to run it.
    let accelerated = std::fs::read_to_string(paths.config_file()).map_or(true, |text| {
        acl_ui::config::Config::read(&text).bool_at(acl_ui::renderer::HARDWARE_ACCELERATION_KEY)
    });

    eframe::run_native(
        "anothercrewlink",
        eframe::NativeOptions {
            viewport,
            renderer: eframe::Renderer::Wgpu,
            wgpu_options: renderer_options(acl_ui::renderer::chain(accelerated)),
            ..Default::default()
        },
        Box::new(move |creation| {
            // Before the first frame. Fonts and the palette are the window's whole
            // identity, and a frame drawn with egui's defaults would be a flash of a
            // different product.
            acl_ui::views::theme::apply(&creation.egui_ctx);
            Ok(Box::new(Client::new(file, &paths)))
        }),
    )
}

/// Reads the saved geometry, falling back to the older flat file.
///
/// Both, because §4.10 has the 2.0 build read what 1.x wrote — and somebody upgrading from
/// further back than that has only `window-state.json`.
/// Loads the strings for whichever language the settings name.
///
/// The stored default is `unkown` — the shipped spelling, and a sentinel meaning "ask the
/// operating system" rather than a language tag. It is left alone rather than corrected,
/// because correcting it would make every existing installation look like a fresh one; a
/// value that is not a locale this build ships falls back to English, which is what the
/// catalogue is anyway.
fn load_catalogue(settings: &settings_page::Page) -> Option<acl_i18n::Catalogue> {
    let wanted = settings.config().text_at("language");
    let locale = if acl_i18n::name_of(&wanted).is_some() {
        wanted
    } else {
        "en".to_owned()
    };
    let root = locale_root()?;
    acl_i18n::Catalogue::load(&root, &locale)
        .or_else(|_| acl_i18n::Catalogue::load(&root, "en"))
        .ok()
}

/// Where the locale tree is.
///
/// Beside the executable in an installed build, and up two directories from `target/debug`
/// in a development one. Both are tried rather than one being configured: a client that
/// cannot find its strings shows keys, and finding out which layout it is in costs two
/// `exists` calls.
fn locale_root() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let beside = executable.parent()?.join("static").join("locales");
    if beside.is_dir() {
        return Some(beside);
    }
    let repository = executable
        .parent()?
        .parent()?
        .parent()?
        .join("static")
        .join("locales");
    repository.is_dir().then_some(repository)
}

/// The wgpu setup, with the fallback chain as its adapter selector.
///
/// eframe asks for one adapter and the chain names two, so the walk happens inside the
/// selector rather than around `run_native`: by the time enumeration has happened the
/// answer is a list, and picking from a list is not a reason to tear a window down and
/// build another.
///
/// **DX12 only.** It is the backend §4.8 names, it is what WARP is reached through, and it
/// is the one Windows guarantees. Leaving Vulkan enabled would let wgpu pick a vendor
/// Vulkan driver on some machines and DX12 on others, which turns one renderer into two
/// and makes a bug report about "the software renderer" ambiguous.
fn renderer_options(
    rungs: Vec<acl_ui::renderer::Renderer>,
) -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, wgpu};

    let mut setup = match WgpuConfiguration::default().wgpu_setup {
        WgpuSetup::CreateNew(setup) => setup,
        // eframe's default is `CreateNew`. An `Existing` here would mean a device was
        // handed to us, and there is nothing to select between.
        existing @ WgpuSetup::Existing(_) => {
            return WgpuConfiguration {
                wgpu_setup: existing,
                ..WgpuConfiguration::default()
            };
        }
    };
    setup.instance_descriptor.backends = wgpu::Backends::DX12;
    setup.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, _surface| {
        let kinds: Vec<acl_ui::renderer::Adapter> = adapters
            .iter()
            .map(|adapter| match adapter.get_info().device_type {
                wgpu::DeviceType::Cpu => acl_ui::renderer::Adapter::Cpu,
                // `VirtualGpu` is a passed-through device in a virtual machine and `Other`
                // is a driver that did not say. Neither is WARP, so both are hardware as
                // far as the choice goes.
                _ => acl_ui::renderer::Adapter::Gpu,
            })
            .collect();
        rungs
            .iter()
            .find_map(|rung| acl_ui::renderer::choose(*rung, &kinds))
            .and_then(|at| adapters.get(at).cloned())
            .inspect(|adapter| {
                // Once, at start-up. Which rung the client ended up on is the first thing
                // worth knowing about a report of a slow window, and it is not otherwise
                // visible from inside the running client.
                let info = adapter.get_info();
                eprintln!(
                    "AnotherCrewLink: rendering on {:?} \"{}\" ({:?})",
                    info.device_type, info.name, info.backend
                );
            })
            .ok_or_else(|| {
                // Reached only if DX12 enumerated nothing at all, WARP included -- which
                // means the graphics stack itself is missing rather than the card.
                format!("no Direct3D 12 adapter among {}", adapters.len())
            })
    }));
    WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..WgpuConfiguration::default()
    }
}

fn read_state(file: &PathBuf) -> Option<WindowState> {
    if let Ok(text) = std::fs::read_to_string(file)
        && let Ok(stored) = serde_json::from_str::<Stored>(&text)
        && let Some(state) = stored.get(acl_ui::window_state::MAIN_WINDOW)
    {
        return Some(state);
    }
    let legacy = file.with_file_name(acl_ui::window_state::LEGACY_FILE);
    std::fs::read_to_string(legacy)
        .ok()
        .as_deref()
        .and_then(acl_ui::window_state::from_legacy)
}

/// The screens, as rectangles.
///
/// Empty when they cannot be asked for, which [`restore`] reads as "restore nothing" — the
/// safe direction, since the alternative is opening at coordinates nothing draws.
#[cfg(windows)]
fn displays() -> Vec<Rect> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    // The virtual screen: the bounding box of every monitor, as one rectangle. Coarser than
    // enumerating them -- a window in the gap of an L-shaped arrangement reads as visible --
    // and it catches the case this exists for, which is a monitor that is simply gone.
    // Enumerating properly wants `EnumDisplayMonitors` and a callback, and is worth doing
    // when the arrangement rather than the count is what changed.
    // SAFETY: four documented calls taking a constant and returning an integer.
    let rect = unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    };
    if rect.width > 0 && rect.height > 0 {
        vec![rect]
    } else {
        Vec::new()
    }
}

#[cfg(not(windows))]
fn displays() -> Vec<Rect> {
    Vec::new()
}

/// Which page the window is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    /// Who is in the lobby.
    Main,
    /// The settings.
    Settings,
    /// The public lobbies.
    Lobbies,
}

/// The reader's state, as the voice rules want it.
///
/// Two crates that describe the same game and do not know about each other: `acl-game`
/// reads it out of memory and `acl-audio` decides what it sounds like. This is the seam,
/// and it is a translation rather than a conversion -- `voice_params` reads six fields and
/// this hands over six fields.
#[cfg(windows)]
fn as_voice_state(state: &acl_game::AmongUsState) -> acl_audio::voice::State {
    acl_audio::voice::State {
        game_state: match state.game_state {
            acl_game::GameState::Lobby => acl_audio::voice::GameState::Lobby,
            acl_game::GameState::Tasks => acl_audio::voice::GameState::Tasks,
            acl_game::GameState::Discussion => acl_audio::voice::GameState::Discussion,
            // `Unknown` is treated as the menu: it is what the reader reports before it has
            // read anything, and silence is the right answer to "I do not know yet".
            acl_game::GameState::Menu | acl_game::GameState::Unknown => {
                acl_audio::voice::GameState::Menu
            }
        },
        // The reader reports the game's own number; `from_game` is what turns it into a
        // map, and `Unknown` for anything it does not know -- which the collider lookup
        // then treats as "no walls" rather than panicking, deliberately.
        map: acl_types::map::MapType::from_game(state.map),
        closed_doors: state.closed_doors.clone(),
        coms_sabotaged: state.coms_sabotaged,
        current_camera: acl_types::map::CameraLocation::from_state(state.current_camera),
        // The reader tracks this, so nothing here has to remember the previous frame.
        light_radius_changed: state.light_radius_changed,
    }
}

/// One player, as the voice rules want them.
#[cfg(windows)]
fn as_voice_player(player: &acl_game::Player) -> acl_audio::voice::Player {
    acl_audio::voice::Player {
        client_id: player.client_id.unwrap_or_default(),
        position: acl_types::map::Vector2 {
            x: player.x,
            y: player.y,
        },
        is_dead: player.is_dead,
        is_impostor: player.is_impostor,
        in_vent: player.in_vent,
        disconnected: player.disconnected,
        // Absent means "the reader could not tell", and a dummy that is treated as a person
        // is a person nobody can hear -- the safer way round for a freeplay lobby.
        is_dummy: player.is_dummy.unwrap_or(false),
    }
}

/// One player of the reader's state, as the roster wants to see them.
///
/// A wrapper rather than an implementation on `acl_game::Player`, because the trait belongs
/// to `acl-ui` and the type to `acl-game`, and neither should have to know about the other
/// to satisfy it.
struct Seat<'a>(&'a acl_game::Player);

impl Roster for Seat<'_> {
    fn id(&self) -> u8 {
        self.0.id
    }
    fn client_id(&self) -> i64 {
        // Absent when the reader could not read it, which is what `Player::client_id` is an
        // `Option` for. A player with no client id matches no voice stream, and -1 is an id
        // the server never issues.
        self.0.client_id.map_or(-1, i64::from)
    }
    fn is_local(&self) -> bool {
        self.0.is_local
    }
    fn disconnected(&self) -> bool {
        self.0.disconnected
    }
    fn in_vent(&self) -> bool {
        self.0.in_vent
    }
    fn bugged(&self) -> bool {
        self.0.bugged
    }
    fn is_dead(&self) -> bool {
        self.0.is_dead
    }
}

struct Client {
    state_file: PathBuf,
    reader: Option<reader::Reader>,
    /// What the window was last seen at.
    last_seen: Option<WindowState>,
    /// What is already in the file, so a settled window is not rewritten every frame.
    written: Option<WindowState>,
    /// When the geometry last changed. `None` once it has been written.
    moved_at: Option<std::time::Instant>,
    /// Holds the overlay to [`OVERLAY_TICK`]. See what it costs.
    overlay_due: Cadence,
    /// Where the game's window is. See [`GameWindow`] for what it saves.
    game_window: GameWindow,
    /// The hat artwork, fetched and decoded on a thread of its own.
    hats: hat_store::Loader,
    /// The dressed crewmates the main window is showing. See [`Portraits`].
    portraits: Portraits,
    /// Whether this player is speaking, as this end's own detector last said.
    ///
    /// Kept here because the detector reports *changes* and the window paints levels: a
    /// frame that saw no transition still has to draw the indicator it had.
    local_talking: bool,
    /// The sockets whose gain is above zero this frame. See where it is filled.
    hearable: std::collections::BTreeSet<String>,
    /// What the lobby has already been told about this client. See [`Announced`].
    announced: Announced,
    /// Who the voice layer believes is dead, by client id.
    ///
    /// Not the game's `is_dead`, and the difference is the whole point. See
    /// [`Self::follow_deaths`].
    dead: std::collections::BTreeMap<i64, bool>,
    /// The game state the death map was last updated for.
    last_game_state: Option<acl_game::GameState>,
    /// Mute, deafen and push-to-talk. See [`controls`].
    ///
    /// Watched on a thread of its own rather than polled from the paint. The paint runs at
    /// five hertz whenever the pointer is not over the window, and an ordinary tap of a key
    /// is pressed and released inside two hundred milliseconds -- so a mute that was polled
    /// here was a mute that frequently did not happen.
    controls: controls::Switchboard,
    /// The settings, and everything that happens to them.
    settings: settings_page::Page,
    /// The signalling session, on a thread of its own.
    link: net::Link,
    /// The microphone, the speaker, and everything between them.
    audio: audio::Audio,
    /// Who has been heard since the last frame.
    ///
    /// Taken from the link once a frame and held for the drawing, because the roster asks
    /// about each player in turn and taking it per question would answer the first one and
    /// nobody else.
    speaking: std::collections::BTreeSet<i64>,
    /// The lobby the session has been asked to join, so the ask happens on the edges.
    ///
    /// A join sent every frame is a join the server rate-limits, and `within_limit` in the
    /// server's `on_join` is not a suggestion.
    joined: Option<String>,
    /// When to try the signalling server again, and how many times it has been tried.
    ///
    /// A dropped socket used to be the end of voice for the life of the process:
    /// `keep_connected` reconnected from `Idle` and treated `Failed` as terminal, so a
    /// server restart, or three seconds of unplugged ethernet, left a client that still
    /// painted its lobby list and could no longer hear anybody. Nothing told the player,
    /// because from their side nothing had visibly changed.
    ///
    /// The schedule is `acl_net::reconnect`'s, which is the same one the peer connections
    /// use and was written for exactly this shape of problem -- doubling from two seconds
    /// to a thirty-second ceiling, so a server that is down costs one attempt every half
    /// minute rather than a tight loop against a machine that is trying to come back up.
    retry: Option<(std::time::Instant, u32)>,
    /// Which mod is installed beside the game, and the process it was found for.
    ///
    /// Remembered per process id, because detecting walks a directory: doing that on every
    /// frame would be a `readdir` five times a second for an answer that changes when the
    /// player restarts the game. A new process id is a new answer.
    mods: Option<(u32, acl_game::mods::Mod)>,
    /// Which page is showing.
    page: Screen,
    /// The strings, in whichever language the settings name.
    ///
    /// `None` when the locale tree cannot be found — an unusual installation, or a
    /// development run from somewhere unexpected. Every lookup then answers with the key,
    /// which is `t`'s documented behaviour and is legible enough to work from.
    catalogue: Option<acl_i18n::Catalogue>,
    /// Whether the overlay is currently meant to be on screen.
    ///
    /// Kept so that show and hide are sent on the edges rather than every frame: the
    /// helper would act on either correctly, and a command five times a second for a state
    /// that has not changed is a pipe carrying nothing.
    overlay_shown: bool,
    /// The window level the viewport has been told about.
    ///
    /// Remembered rather than sent every frame: a viewport command is a round trip to
    /// winit, and telling it four times a second that nothing changed is four hundred
    /// messages for the one that matters. `None` until the first frame, which is what makes
    /// the setting take effect on the way up as well as when it is changed.
    on_top: Option<bool>,
    /// The name being typed into the custom-platform editor's add box.
    ///
    /// Held here rather than in the view, because the view is redrawn from scratch every
    /// frame and a half-typed name would not survive one.
    adding_platform: String,
    /// Whether there is a newer version, once the check has said.
    updates: updates::Updates,
    /// Why the game would not start, if somebody pressed the button and it did not.
    ///
    /// Kept rather than shown once: the button is on a screen that repaints five times a
    /// second, and a message drawn only on the frame of the click is one nobody reads.
    launch_trouble: Option<String>,
    /// The last public-lobby listing sent, so it is sent only when it changes.
    ///
    /// The server rate-limits this handler, and the listing is derived from a frame that
    /// arrives five times a second — most of which say the same thing. `Voice.tsx` sends it
    /// on a change too, from a `useEffect`.
    listed: Option<(String, serde_json::Value)>,
    /// Whether the server has been asked for public-lobby updates.
    ///
    /// Told once, like `overlay_shown`: `watch_lobbies` is a message, and asking every frame
    /// is a message a second for a subscription the server already has. Cleared when the
    /// connection goes, so a reconnected session subscribes again rather than believing a
    /// subscription that went with the socket.
    watching_lobbies: bool,
    /// What the machine has to listen and speak through.
    devices: Devices,
}

impl Client {
    fn new(state_file: PathBuf, paths: &Paths) -> Self {
        let settings = settings_page::Page::open(paths.config_file());
        let catalogue = load_catalogue(&settings);
        // Reading the game is the whole point of the window, so it starts reading. The
        // shipped client has no start button either -- it launches its reader on the way up
        // -- and a client that opens showing nothing until you press something reads as one
        // that does not work.
        // Before `settings` is moved into the struct below.
        let capture = capture_settings(settings.config());
        let reader = reader::Reader::start().ok();
        if let Some(reader) = reader.as_ref() {
            reader.ask_to_start();
        }
        // The media path, end to end, with the window nowhere in it. Arriving packets go
        // from the signalling worker straight to the mixer; encoded frames go from the
        // capture callback straight back to the worker. Both used to travel through this
        // window's paint, whose floor is two hundred milliseconds when the pointer is not
        // over it.
        let (link, packets) = net::Link::start();
        // Before the switch watcher, which writes the microphone gate into the capture
        // callback's `Tuning` and therefore needs one to write into.
        let audio = audio::Audio::start(capture, packets, &link.audio_sink());
        Self {
            state_file,
            hats: hat_store::Loader::start(paths.hat_cache()),
            portraits: Portraits::default(),
            local_talking: false,
            hearable: std::collections::BTreeSet::new(),
            announced: Announced::default(),
            dead: std::collections::BTreeMap::new(),
            last_game_state: None,
            settings,
            link,
            controls: controls::Switchboard::start(audio.tuning()),
            audio,
            speaking: std::collections::BTreeSet::new(),
            joined: None,
            retry: None,
            mods: None,
            page: Screen::Main,
            catalogue,
            // A reader that will not start is not a reason to refuse to open: the window is
            // where somebody would find out about it, so it opens and says so.
            reader,
            last_seen: None,
            written: None,
            moved_at: None,
            overlay_due: Cadence::default(),
            game_window: GameWindow::default(),
            overlay_shown: false,
            adding_platform: String::new(),
            updates: updates::Updates::start(env!("CARGO_PKG_VERSION")),
            launch_trouble: None,
            listed: None,
            on_top: None,
            watching_lobbies: false,
            devices: Devices::default(),
        }
    }

    /// Composes one overlay frame and sends it across.
    ///
    /// Every part of this is decided elsewhere: `game_window` says where the game is and
    /// whether it can be followed, `roster::overlay` says who is audible, and `sprite` turns
    /// each of them into bytes. What is here is the arrangement -- a row along the top of
    /// the game, which is where `Overlay.tsx` puts it by default.
    #[cfg(windows)]
    fn compose_overlay(&mut self, state: &acl_game::AmongUsState) {
        // Asked for before the reader is borrowed, because it needs `self` mutably: it
        // remembers the game's process id rather than looking it up again every time. See
        // [`GameWindow`].
        let found = self.game_window.bounds();

        let Some(reader) = self.reader.as_ref() else {
            return;
        };
        // The three settings that decide whether there is an overlay at all, read every
        // frame rather than cached: they are changed on the settings page, which is this
        // same process, and a cached copy is one more thing to invalidate.
        let settings = self.settings.config();
        let enabled = settings.bool_at("enableOverlay");
        // While a meeting is up the players go over the seats of the meeting table instead
        // of into a corner, which is what `meetingOverlay` switches on.
        let meeting = settings.bool_at("meetingOverlay");
        let position =
            acl_ui::overlay_layout::Position::parse(&settings.text_at("overlayPosition"));
        let compact = settings.bool_at("compactOverlay") || position.forces_compact();

        let bounds = found.filter(|bounds| bounds.is_drawable());
        let Some(bounds) = bounds.filter(|_| enabled) else {
            // No game, nothing to draw over, or the overlay is switched off. Hidden rather
            // than left showing the last frame over whatever the player switched to.
            if self.overlay_shown {
                self.overlay_shown = false;
                reader.show_overlay(false);
            }
            return;
        };

        // Every one of these was a stub until 2026-08-27, so the overlay showed everybody
        // connected, nobody audible and nobody speaking -- a strip of crewmates that never
        // changed. The main window had two of them wired and the overlay had none, which is
        // the same lobby described two ways on one screen.
        let link = &self.link;
        let heard = &self.speaking;
        let local_talking = self.local_talking;
        let hearable = &self.hearable;
        let believed_dead = &self.dead;
        let can_hear = |client_id: i64| {
            link.socket_of(client_id)
                .is_some_and(|socket| hearable.contains(socket))
        };
        let voice = Voice {
            talking: &|client_id| link.talking(client_id) && can_hear(client_id),
            dead: &|client_id| believed_dead.get(&client_id).copied().unwrap_or(false),
            connected: &|client_id| link.hears(client_id),
            audible: &|client_id| heard.contains(&client_id),
            local_talking,
            local_alive: !state.players.iter().any(|p| p.is_local && p.is_dead),
            // 1.x carries this over the WebRTC data channel, which this client does not
            // have. §4.13 recorded the blocker as *moving* the claim to the socket, which
            // would break 1.x peers; a second route breaks nobody, so `impostorRadio` is a
            // 2.x socket event and a mixed lobby degrades exactly as far as it did before.
            impostor_radio: link.on_radio(),
            local_is_impostor: state
                .players
                .iter()
                .any(|player| player.is_local && player.is_impostor),
        };
        let seats: Vec<Seat<'_>> = state.players.iter().map(Seat).collect();
        let shown = overlay(&seats, &voice, compact);

        // `Menu` is not "the menu layout" -- it is no overlay at all. `Overlay.tsx`
        // returns null on it before it reaches the layout, so the `gamestate_menu` styling
        // it carries is reachable only from `Unknown`, which is the reader attached and
        // the state not yet readable. Following the class name rather than the early
        // return would put a strip over the main menu that the shipped client never shows.
        if state.game_state == acl_game::GameState::Menu {
            if self.overlay_shown {
                self.overlay_shown = false;
                reader.show_overlay(false);
            }
            return;
        }
        let in_menu = state.game_state == acl_game::GameState::Unknown;
        if meeting && state.game_state == acl_game::GameState::Discussion {
            Self::compose_meeting(
                &mut self.overlay_shown,
                reader,
                &bounds,
                state,
                voice.talking,
                local_talking,
            );
            return;
        }
        let laid = acl_ui::overlay_layout::lay_out(
            position,
            in_menu,
            acl_ui::overlay_layout::Rect {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
            shown.len(),
            OVERLAY_SPRITE,
        );
        let Some(laid) = laid else {
            // Hidden, or nobody to draw. An empty strip is still a rectangle of nothing
            // sitting over the game.
            if self.overlay_shown {
                self.overlay_shown = false;
                reader.show_overlay(false);
            }
            return;
        };

        let sprites = Self::strip_sprites(&mut self.hats, &shown, state, &laid);

        reader.draw_overlay(
            (
                laid.placement.x,
                laid.placement.y,
                laid.placement.width,
                laid.placement.height,
            ),
            sprites,
        );
        if !self.overlay_shown {
            self.overlay_shown = true;
            reader.show_overlay(true);
        }
    }

    /// Writes the geometry back, keeping every other window's.
    ///
    /// Named for what it does rather than `App::save`, which eframe calls with its own
    /// storage and on its own schedule -- a different thing that happens to share a verb.
    fn write_window_state(&mut self) {
        self.moved_at = None;
        let Some(state) = self.last_seen else {
            return;
        };
        // Every other window's entry is read back and put again, so this writes one key
        // rather than the file: the overlay keeps its own, and so does anything added
        // later.
        if self.written == Some(state) {
            return;
        }
        let mut stored = std::fs::read_to_string(&self.state_file)
            .ok()
            .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
            .unwrap_or_default();
        stored.set(acl_ui::window_state::MAIN_WINDOW, state);
        if let Ok(text) = serde_json::to_string(&stored)
            && std::fs::write(&self.state_file, text).is_ok()
        {
            self.written = Some(state);
        }
    }

    /// The crewmates of the corner strip, dressed and placed.
    ///
    /// Its own function because `compose_overlay` was doing four things: reading the
    /// settings, deciding whether there is anything to draw, laying it out, and drawing
    /// it. This is the last one.
    #[cfg(windows)]
    fn strip_sprites(
        hats: &mut hat_store::Loader,
        shown: &[acl_ui::roster::Shown],
        state: &acl_game::AmongUsState,
        laid: &acl_ui::overlay_layout::Layout,
    ) -> Vec<(i32, i32, acl_ui::sprite::Bitmap)> {
        // Which crewmate wears what, collected while the sprites are built and applied
        // afterwards: the artwork lives behind `&mut self` and the closure below is
        // already borrowing the reader's state.
        let mut wearing: Vec<(usize, Wearing)> = Vec::new();
        let mut sprites: Vec<(i32, i32, acl_ui::sprite::Bitmap)> = shown
            .iter()
            .zip(laid.sprites.iter())
            .filter_map(|(entry, (x, y))| {
                let player = state.players.get(entry.at)?;
                let (body, shadow) =
                    acl_ui::views::colour::crew(i32::try_from(player.color_id).unwrap_or(-1));
                let bitmap = overlay_body(
                    OVERLAY_SPRITE,
                    entry.talking,
                    entry.alive,
                    (body.r(), body.g(), body.b()),
                    (shadow.r(), shadow.g(), shadow.b()),
                );
                Some((*x, *y, bitmap))
            })
            .collect();
        for (at, (entry, _)) in shown.iter().zip(laid.sprites.iter()).enumerate() {
            let Some(player) = state.players.get(entry.at) else {
                continue;
            };
            if at < sprites.len() {
                wearing.push((
                    at,
                    Wearing {
                        hat: player.hat_id.clone(),
                        skin: player.skin_id.clone(),
                        visor: player.visor_id.clone(),
                        alive: !player.is_dead,
                    },
                ));
            }
        }
        Self::dress(hats, &mut sprites, &wearing);
        sprites
    }

    /// Draws the players over the seats of the meeting table.
    ///
    /// The seats are worked out by [`acl_ui::overlay_layout::meeting`] from the game
    /// window's shape -- nothing is read out of memory for it but `old_meeting_hud`, which
    /// says which of two tables this build has.
    ///
    /// **Everybody gets a seat, in the game's own order.** This is not the corner strip:
    /// there, the roster hides the dead and sorts the disconnected to the end, because the
    /// strip is a list. A seat is a *place*, and it has to be the place the game drew that
    /// player at, so the order is the game's and nobody is left out of it. Only the ring is
    /// conditional -- a player who is not speaking has an empty seat drawn over them, which
    /// is nothing at all.
    ///
    /// Takes the reader and the flag rather than `&self`, so that the reader's borrow and
    /// the flag's can coexist: they are disjoint fields, and the borrow checker only knows
    /// that when they are named separately.
    #[cfg(windows)]
    fn compose_meeting(
        shown: &mut bool,
        reader: &reader::Reader,
        bounds: &acl_core::game_window::Bounds,
        state: &acl_game::AmongUsState,
        talking: &dyn Fn(i64) -> bool,
        local_talking: bool,
    ) {
        let game = acl_ui::overlay_layout::Rect {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        };
        let seats = acl_ui::overlay_layout::meeting::seats(
            game,
            state.old_meeting_hud,
            state.players.len(),
        );

        let mut placement: Option<acl_ui::overlay_layout::Rect> = None;
        let mut sprites: Vec<(i32, i32, acl_ui::sprite::Bitmap)> = Vec::new();
        for (player, seat) in state.players.iter().zip(seats.iter()) {
            // `let talking = false;` stood here, hard-coded, from before the audio was
            // wired -- so the meeting overlay computed every seat and then drew none of
            // them, for ever. The predicate it was waiting for has existed since
            // 2026-08-27 and is the same one the corner strip uses ten lines above the
            // call; nothing connected the two.
            //
            // The local player is asked separately for the same reason `Voice` carries
            // `local_talking` separately: this client's own speech comes from its own
            // detector rather than from a `VAD` event it does not send itself.
            let speaking = if player.is_local {
                local_talking
            } else {
                player.client_id.is_some_and(|id| talking(i64::from(id)))
            };
            if !speaking {
                continue;
            }
            let (body, shadow) =
                acl_ui::views::colour::crew(i32::try_from(player.color_id).unwrap_or(-1));
            let side = seat.width.min(seat.height).max(1);
            let bitmap = overlay_body(
                side,
                true,
                !player.is_dead,
                (body.r(), body.g(), body.b()),
                (shadow.r(), shadow.g(), shadow.b()),
            );
            let grown = placement.map_or(*seat, |so_far| union(so_far, *seat));
            placement = Some(grown);
            sprites.push((seat.x, seat.y, bitmap));
        }

        let Some(placement) = placement else {
            // Nobody is speaking, so there is nothing over the table. Hidden rather than
            // left showing whoever spoke last.
            if *shown {
                *shown = false;
                reader.show_overlay(false);
            }
            return;
        };

        // The sprites were placed in screen coordinates; the helper wants them relative to
        // the window it is about to draw into.
        let sprites = sprites
            .into_iter()
            .map(|(x, y, bitmap)| (x - placement.x, y - placement.y, bitmap))
            .collect();
        reader.draw_overlay(
            (placement.x, placement.y, placement.width, placement.height),
            sprites,
        );
        if !*shown {
            *shown = true;
            reader.show_overlay(true);
        }
    }

    /// Which mod is installed beside the running game.
    ///
    /// [`acl_game::mods::detect_mod`] is the port of `getInstalledMods` and has been in the
    /// tree since the reader was written; what was missing was the executable's path to
    /// hand it. The answer is what a lobby's `mods` field is compared against, so a wrong
    /// one shows as a join button that refuses for a reason the player cannot act on.
    ///
    /// [`acl_game::mods::Mod::None`] when the game is not running, which is also the right
    /// answer for a browser opened from the menu: an unmodded client is what most lobbies
    /// advertise.
    #[cfg(windows)]
    fn installed_mod(&mut self) -> acl_game::mods::Mod {
        let Some(pid) = acl_game::windows::find_process("Among Us.exe") else {
            // Forgotten rather than kept: the next game to start may have a different mod,
            // and a stale answer is worse than no answer because nothing looks wrong.
            self.mods = None;
            return acl_game::mods::Mod::None;
        };
        if let Some((known, which)) = self.mods
            && known == pid
        {
            return which;
        }
        let which = acl_game::windows::executable_path(pid)
            .map_or(acl_game::mods::Mod::None, |path| {
                acl_game::mods::detect_mod(&path)
            });
        self.mods = Some((pid, which));
        which
    }

    /// Off Windows there is no process to look beside.
    #[cfg(not(windows))]
    fn installed_mod(&mut self) -> acl_game::mods::Mod {
        acl_game::mods::Mod::None
    }

    /// Moves audio in both directions, and works out what each peer should sound like.
    ///
    /// The placement is the whole of what this decides, and it decides none of it:
    /// `acl_audio::voice::voice_params` applies every rule — distance, walls, vision, the
    /// dead, the vents, the impostor radio — and this turns its answer into the two numbers
    /// the mixer needs.
    ///
    /// Once a frame, which is five times a second. That is far slower than the audio, and
    /// deliberately: a player moves at the game's pace, and recomputing a gain fifty times a
    /// second would be forty-five recomputations of the same answer.
    fn carry_audio(&mut self) {
        // What the settings say, handed to the watcher rather than acted on here. It polls
        // at thirty milliseconds and writes the microphone gate straight into the capture
        // callback; this only has to keep it told.
        //
        // `pushToTalkMode` is what the settings screen writes. It used to read
        // `pushToTalk` -- a boolean that nothing in this project has ever written, so it
        // was always false and every client was in voice activity whatever the screen
        // said. Push-to-mute did not exist at all.
        {
            let saved = self.settings.config();
            self.controls.configure(
                controls::Mode::from_setting(saved.number_at("pushToTalkMode")),
                [
                    saved.text_at("muteShortcut"),
                    saved.text_at("deafenShortcut"),
                    saved.text_at("pushToTalkShortcut"),
                    saved.text_at("impostorRadioShortcut"),
                ],
            );
        }
        let switches = self.controls.state();

        // The impostor radio, on the transition. Three conditions this end has to check
        // before claiming it, and `Voice.tsx` checks the same three at line 902: an
        // impostor, alive, and a lobby that allows it. The receiving end checks again --
        // `voice_params` only lifts the distance rule when both are impostors -- so a
        // client that lied would be believed by nobody.
        let wants_radio = switches.on_radio
            && self
                .settings
                .config()
                .bool_at("localLobbySettings.impostorRadioEnabled")
            && self
                .reader
                .as_ref()
                .and_then(|reader| reader.latest())
                .is_some_and(|state| {
                    state
                        .players
                        .iter()
                        .any(|player| player.is_local && player.is_impostor && !player.is_dead)
                });
        if wants_radio != self.announced.radio {
            self.announced.radio = wants_radio;
            self.link.say_on_radio(wants_radio);
        }

        let Some(state) = self
            .reader
            .as_ref()
            .and_then(|reader| reader.latest().cloned())
        else {
            // No game, no positions, nothing to place. The mixer plays what it has at the
            // gain it last had, which is right for the fraction of a second between two
            // frames and wrong for anything longer -- so it is emptied instead.
            self.audio.place(std::collections::BTreeMap::new());
            return;
        };

        let settings = self.settings.config();
        let lobby = acl_audio::voice::LobbySettings {
            max_distance: settings.number_at("localLobbySettings.maxDistance"),
            haunting: settings.bool_at("localLobbySettings.haunting"),
            coms_sabotage: settings.bool_at("localLobbySettings.commsSabotage"),
            hear_impostors_in_vents: settings.bool_at("localLobbySettings.hearImpostorsInVents"),
            impostors_hear_impostors_in_vent: settings
                .bool_at("localLobbySettings.impostersHearImpostersInvent"),
            impostor_radio_enabled: settings.bool_at("localLobbySettings.impostorRadioEnabled"),
            dead_only: settings.bool_at("localLobbySettings.deadOnly"),
            meeting_ghost_only: settings.bool_at("localLobbySettings.meetingGhostOnly"),
            vision_hearing: settings.bool_at("localLobbySettings.visionHearing"),
            hear_through_cameras: settings.bool_at("localLobbySettings.hearThroughCameras"),
            walls_block_audio: settings.bool_at("localLobbySettings.wallsBlockAudio"),
        };
        // Two fields, not five. `masterVolume` and `crewVolumeAsGhost` are applied
        // elsewhere in the Electron graph -- one on the output node and one inside the
        // ghost rule -- and `voice_params` takes only what its own rules read. Passing them
        // here would be applying them twice.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a percentage from the settings file, which the schema bounds at 100"
        )]
        let client = acl_audio::voice::ClientSettings {
            ghost_volume_as_impostor: settings.number_at("ghostVolumeAsImpostor") as f32,
            enable_spatial_audio: settings.bool_at("enableSpatialAudio"),
        };

        let placements = self.placements(&state, &lobby, client);
        // Who is actually audible this frame, kept before the map is handed away.
        //
        // `Voice.tsx` line 1598: `nowTalking = otherVAD[clientId] === true && gain > 0`.
        // The speaking indicator is gated on being able to hear them, and that is not a
        // nicety -- in a game, showing that somebody across the map is talking tells you
        // they are alive and where they are not. 1.x has always gated it; this client
        // showed the raw `VAD` until 2026-08-27.
        self.hearable = placements
            .iter()
            .filter(|(_, placement)| placement.gain > 0.0)
            .map(|(socket, _)| socket.clone())
            .collect();
        self.audio.place(placements);
    }

    /// What every peer in the lobby should sound like.
    ///
    /// Separate from `carry_audio` because that one moves bytes and this one applies rules,
    /// and the rules are the part worth reading on their own.
    fn placements(
        &self,
        state: &acl_game::AmongUsState,
        lobby: &acl_audio::voice::LobbySettings,
        client: acl_audio::voice::ClientSettings,
    ) -> std::collections::BTreeMap<String, audio::Placement> {
        let mut placements = std::collections::BTreeMap::new();
        let on_radio = self
            .link
            .on_radio()
            .and_then(|client_id| u32::try_from(client_id).ok());
        let Some(me) = state.players.iter().find(|player| player.is_local) else {
            return placements;
        };
        let voice_state = as_voice_state(state);
        let listener = as_voice_player(me);
        // The panner's `maxDistance`, rewritten each frame from the hearing range --
        // `Voice.tsx` does exactly this, and it is why `vision_hearing` can change how far
        // you hear without changing any gain directly.
        let hearing =
            acl_audio::voice::hearing_range(lobby, &listener, f64::from(state.light_radius));

        for player in &state.players {
            if player.is_local {
                continue;
            }
            let Some(client_id) = player.client_id.map(i64::from) else {
                continue;
            };
            let Some(socket) = self.link.socket_of(client_id) else {
                continue;
            };
            let params = acl_audio::voice::voice_params(
                &voice_state,
                &client,
                lobby,
                &listener,
                &as_voice_player(player),
                // `hearing`, not `lobby.max_distance`, and they are not the same number.
                // `Voice.tsx:408` passes `maxDistanceRef.current` to `noteVoice`, and
                // `maxDistanceRef` is the vision-hearing figure worked out at line 1505 --
                // the light radius plus half a unit for a crewmate in a lobby with the
                // toggle on, and a floor of 1 below 0.6.
                //
                // The panner four lines below was already given `hearing`, so with the
                // toggle on the cutoff and the roll-off disagreed: a player shown in range
                // and cut to silence, or heard past where the ring says they are.
                hearing,
                // Who the lobby says is on the radio. `voice_params` has implemented the
                // rule since P3+ -- skip the distance check, add the highpass muffle -- and
                // was passed `None` until 2026-08-27, so it never once fired.
                on_radio,
            );
            // Deafened silences everybody, which is the same `gain = 0` the per-player
            // mute below takes and is checked in the same place for the same reason:
            // `Voice.tsx` line 1584 tests `deafened || isMuted` as one condition.
            if self.controls.state().deafened {
                continue;
            }
            // Per-player volume and mute. `voice_params` deliberately does not know about
            // them, because `Voice.tsx` applies them outside `calculateVoiceAudio` too --
            // the rule and the reason it is keyed on the name hash are in
            // `acl_ui::config::per_player_gain`, which is tested without a game.
            let after_the_rules = acl_ui::config::after_the_rules(
                self.settings.config(),
                acl_ui::config::Listener {
                    speaker_name_hash: player.name_hash,
                    is_dead: me.is_dead,
                    speaker_is_dead: player.is_dead,
                },
                params.gain,
            );
            let Some(gain) = after_the_rules.or_else(|| params.reverb.then_some(0.0)) else {
                // Muted, silenced by the master volume, or turned all the way down.
                // Nothing is placed for them at all: the Electron original leaves the graph
                // alone in that case, and a peer left out of the map is a peer the mixer
                // does not mix -- cheaper than mixing silence.
                //
                // `after_the_rules` returns `None` for every gain at or below zero, so
                // there is no second check after this one. There was until 2026-08-27, and
                // it could not fire.
                //
                // A haunting ghost is the exception, and is placed at zero rather than left
                // out. In the Electron client the convolver stays connected while the gain
                // goes to zero, so it keeps being fed and its three-second tail rings out;
                // dropping the peer here would take the convolver with it and cut the tail
                // the moment the ghost stepped out of range.
                continue;
            };

            placements.insert(
                socket.to_owned(),
                audio::Placement {
                    gain,
                    // The vent's low pass, the camera's, or the impostor radio's high
                    // pass. Decided here and applied in the mixing thread, which is the
                    // only place a filter's own state can live.
                    muffle: params.muffle,
                    // Whether an impostor is being haunted, which is the one rule that also
                    // stops walls blocking a voice. Decided here and applied in the mixing
                    // thread, beside the muffle and for the same reason: three seconds of
                    // convolver is state, and state cannot live in a value the window
                    // rebuilds every frame.
                    reverb: params.reverb,
                    // Exactly what `Voice.tsx:618-620` writes into the `PannerNode`:
                    // `positionX = panPos[0]`, `positionY = panPos[1]`, and a fixed
                    // `positionZ = -0.5`.
                    //
                    // The game's `y` goes to the panner's *elevation*, not its depth. That
                    // reads backwards -- the game is flat and elevation ought to be
                    // meaningless -- and it is what the client this is a port of does, so
                    // it is what players have been hearing since 1.0.0. It matters twice.
                    // The azimuth is `atan2(x, -z)`, which with a fixed `z` collapses to
                    // `atan2(x, 0.5)`: the stereo image comes from the sideways offset
                    // alone, and a player two units to the right is hard right whether they
                    // are level with you or across the map. Reading the game's `y` as depth
                    // instead put that same player at 26 degrees rather than 76.
                    //
                    // And the fixed half unit is a floor on the distance. Without it two
                    // players standing on the same tile are zero apart, which the panner
                    // answers by centring -- correct, and not what 1.x does.
                    source: acl_audio::panner::Position {
                        x: params.pan.x,
                        y: params.pan.y,
                        z: -0.5,
                    },
                    panner: acl_audio::panner::Panner {
                        max_distance: hearing,
                        ..acl_audio::panner::Panner::default()
                    },
                    spatial: client.enable_spatial_audio,
                },
            );
        }
        placements
    }

    /// Applies "always on top", which is a setting that had nowhere to go.
    ///
    /// `alwaysOnTop` was stored, defaulted, translated and tested, and never reached a
    /// window: the only mentions of it outside the settings model were its own tests. It is
    /// the first checkbox on the overlay page, and it did nothing at all.
    ///
    /// `WindowLevel` rather than the builder's `with_always_on_top`, because the setting can
    /// be turned off again and a window built on top would stay there for the session.
    fn keep_on_top(&mut self, ctx: &egui::Context) {
        let wanted = self.settings.config().bool_at("alwaysOnTop");
        if self.on_top == Some(wanted) {
            return;
        }
        self.on_top = Some(wanted);
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if wanted {
            egui::viewport::WindowLevel::AlwaysOnTop
        } else {
            egui::viewport::WindowLevel::Normal
        }));
    }

    /// Lists this lobby in the public browser, when its host asked for that.
    ///
    /// `Voice.tsx`'s `updateLobby`, with its four guards: a game state, being the host, a
    /// lobby code that is not the menu, and players. Only the host, because the listing
    /// claims a player count and a host name that only the host can speak for.
    ///
    /// Sent on a change rather than per frame. The listing is derived from a frame that
    /// arrives five times a second and mostly says the same thing, and the server rate-
    /// limits this handler — a client that ignored that would have its own listing dropped.
    ///
    /// Switching `publicLobby_on` off sends the same message with `isPublic` false, which
    /// is how the server is told to remove a listing: one command, both directions.
    fn keep_listed(&mut self) {
        // Before the reader is borrowed: `installed_mod` takes `&mut self`.
        let mods = self.installed_mod().id().to_owned();
        let Some(state) = self
            .reader
            .as_ref()
            .and_then(|reader| reader.latest())
            .filter(|state| state.is_host && !state.players.is_empty())
        else {
            return;
        };
        let code = state.lobby_code.trim().to_owned();
        if code.is_empty() || code == "MENU" {
            return;
        }

        let stored = self.settings.config();
        let host = state
            .players
            .iter()
            .find(|player| player.is_local)
            .map_or_else(String::new, |player| player.name.clone());
        let lobby = serde_json::json!({
            // The server assigns the real one; `Voice.tsx` sends this and so does this.
            "id": -1,
            "title": stored.text_at("localLobbySettings.publicLobby_title"),
            "host": host,
            "current_players": state.players.len(),
            "max_players": state.max_players,
            "server": state.current_server,
            "language": stored.text_at("localLobbySettings.publicLobby_language"),
            "mods": mods,
            "isPublic": stored.bool_at("localLobbySettings.publicLobby_on"),
            "gameState": state.game_state as i32,
        });
        // The code is part of what is compared, and it was not until 2026-08-29. The
        // payload alone does not name the lobby -- the code is the separate first argument
        // to `advertise` -- so a host whose next lobby happened to have the same title,
        // language, player count and game state as the last one was compared equal to it
        // and never advertised. Their lobby was simply absent from the public browser,
        // with nothing to say so.
        if self
            .listed
            .as_ref()
            .is_some_and(|(was, before)| was.as_str() == code.as_str() && before == &lobby)
        {
            return;
        }
        self.listed = Some((code.clone(), lobby.clone()));
        self.link.advertise(&code, lobby);
    }

    /// Connects to the voice server, and stays connected.
    ///
    /// On the way up rather than on the way into a screen. Until 2026-08-27 the only
    /// `connect` call was inside the public-lobby browser, so a client that never opened
    /// that screen never joined the server: no peers, no voice, and every crewmate in the
    /// window wearing the "no connection" badge -- correctly, which is how it was found.
    ///
    /// `Failed` is left alone. A connection that was refused is retried when somebody asks
    /// for it, not four times a second: the settings screen has the server address and the
    /// browser has a button.
    fn keep_connected(&mut self) {
        match self.link.state() {
            net::State::Idle => {
                self.retry = None;
                self.connect_now();
            }
            // A subscription belongs to a socket. When the socket goes, so does it.
            //
            // `retry` is deliberately *not* cleared here. An attempt that is in flight is
            // still an attempt: clearing it would reset the backoff to two seconds on
            // every try, and a server that is down would be hammered every two seconds for
            // as long as it stayed down instead of every thirty.
            net::State::Connecting => self.watching_lobbies = false,
            net::State::Failed(_) => {
                self.watching_lobbies = false;
                let (now_or_wait, next) = reconnect_due(self.retry, std::time::Instant::now());
                self.retry = next;
                if let Some(attempt) = now_or_wait {
                    acl_core::log_info!("net", "reconnecting, attempt {attempt}");
                    // Here rather than when the failure was first seen, and the ordering
                    // matters. The lobby went with the socket, so `follow_the_lobby` has
                    // to re-emit the join -- otherwise the client reconnects to the
                    // *server* and not to the lobby: the code has not changed, so the edge
                    // that sends `join` never comes round again and the player sits in a
                    // lobby the server does not know they are in.
                    //
                    // Cleared on the same frame as the connect, because `Command::Connect`
                    // empties the worker's deferred queue. A join queued while the socket
                    // was down would be thrown away by the connect that was meant to carry
                    // it, and `joined` would already be set again so nothing would re-send
                    // it. `logic` calls `follow_the_lobby` on the line after this one.
                    self.joined = None;
                    self.connect_now();
                }
            }
            net::State::Connected(_) => self.retry = None,
        }
    }

    /// Opens a session to the server the settings name.
    ///
    /// `serverURL` is 1.x's key in 1.x's file, so a player who changed it keeps their
    /// change.
    fn connect_now(&mut self) {
        let url = self.settings.config().text_at("serverURL");
        acl_core::log_info!("net", "connecting to {url}");
        self.link.connect(&url);
    }

    /// Joins and leaves the lobby the game is in.
    ///
    /// On the edges rather than every frame: a join sent five times a second is a join the
    /// server rate-limits, and `on_join`'s `within_limit` is not a suggestion.
    ///
    /// The code is what the reader read out of the game, so this follows the game rather
    /// than the other way round — which is why there is no "join" button anywhere. A player
    /// who is in a lobby is in it.
    fn follow_the_lobby(&mut self) {
        let state = self
            .reader
            .as_ref()
            .and_then(|reader| reader.latest().cloned());
        let wanted = state.as_ref().and_then(|state| {
            // An empty code is the menu, and `MENU` is what the reader reports when it has
            // one and the game is not in a lobby.
            let code = state.lobby_code.trim();
            (!code.is_empty() && code != "MENU").then(|| code.to_owned())
        });

        if wanted == self.joined {
            // Still in the same lobby, so nothing to join -- but the host can change under
            // us. Among Us promotes somebody when the host leaves, and the server goes on
            // routing host-dependent decisions to a socket that is gone until it is told.
            let host_now = state.as_ref().is_some_and(|state| state.is_host);
            if host_now && !self.announced.host {
                let client_id = state
                    .as_ref()
                    .and_then(|state| state.players.iter().find(|player| player.is_local))
                    .and_then(|player| player.client_id)
                    .map_or(-1, i64::from);
                if client_id >= 0 {
                    self.link.say_host(client_id);
                    self.announced.host = true;
                }
            } else if !host_now {
                // Reset, so a second promotion in the same session is claimed again.
                self.announced.host = false;
            }
            return;
        }
        if let Some(code) = &wanted {
            let me = state
                .as_ref()
                .and_then(|state| state.players.iter().find(|player| player.is_local));
            let player_id = me.map_or(-1, |player| i64::from(player.id));
            let client_id = me.and_then(|player| player.client_id).map_or(-1, i64::from);
            let is_host = state.as_ref().is_some_and(|state| state.is_host);
            acl_core::log_info!(
                "lobby",
                "joining {code} as player {player_id}, client {client_id}, host {is_host}"
            );
            self.link.join(code, player_id, client_id, is_host);
            // `join` carries it, so a claim on top would be the same statement twice.
            self.announced.host = is_host;
        } else {
            acl_core::log_info!("lobby", "leaving");
            // So the next lobby is advertised even if it looks exactly like this one.
            // `keep_listed` compares against what was last sent, and a value left behind
            // across a lobby change is a comparison against a lobby that no longer exists.
            self.listed = None;
            self.link.leave();
        }
        self.joined = wanted;
    }

    /// Draws the public lobby browser.
    ///
    /// Opening the page is what *subscribes*, not what connects. It used to be both, and
    /// the connection moved to the way up on 2026-08-27 -- a voice client that has not
    /// joined its server is not a voice client, whatever screen it is showing. What is
    /// still bound to this page is the lobby-list subscription: a session left watching
    /// receives every change to every public lobby for as long as it is connected, which is
    /// traffic for a window nobody is looking at.
    fn show_lobbies(&mut self, ui: &mut egui::Ui) {
        // Before the catalogue is borrowed below, because detecting takes `&mut self`.
        let installed = self.installed_mod();
        let catalogue = self.catalogue.as_ref();
        let translate = move |key: &str| {
            catalogue.map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };

        match self.link.state().clone() {
            net::State::Idle => {
                ui.label(translate("client.lobby.connecting"));
                return;
            }
            net::State::Connecting => {
                ui.spinner();
                return;
            }
            net::State::Failed(why) => {
                // A retired protocol is the one server error worth replacing: it is not a
                // fault to report but a sentence telling the player to update, and it is
                // theirs to read in their own language. Everything else passes through --
                // a translated guess over a real error hides the thing they need.
                ui.colored_label(
                    egui::Color32::from_rgb(230, 140, 90),
                    acl_net::retirement::message(&why, translate),
                );
                if ui.button(translate("client.buttons.try_again")).clicked() {
                    // Back to idle, which is what makes the arm above connect again.
                    self.link.disconnect();
                }
                return;
            }
            net::State::Connected(_) => {
                // Once, on arriving here connected. It used to be sent beside `connect`,
                // which was in the `Idle` arm above -- so a browser opened on an
                // already-connected session subscribed to nothing and listed nothing.
                if !self.watching_lobbies {
                    self.link.watch_lobbies(true);
                    self.watching_lobbies = true;
                }
            }
        }

        let listings: Vec<acl_ui::views::lobby_browser::Listing<'_>> = self
            .link
            .lobbies()
            .map(|lobby| acl_ui::views::lobby_browser::Listing {
                id: i64::try_from(lobby.id).unwrap_or(i64::MAX),
                title: &lobby.title,
                host: &lobby.host,
                mods: &lobby.mods,
                language: &lobby.language,
                row: acl_ui::lobby_list::LobbyRow {
                    // Zero is the waiting state on both sides: `GameState::Lobby` here, and
                    // `GAME_STATE_LOBBY` in the server, which is also what it checks before
                    // handing out a code.
                    waiting: lobby.game_state == 0,
                    players: u32::try_from(lobby.current_players).unwrap_or(0),
                    capacity: u32::try_from(lobby.max_players).unwrap_or(0),
                },
            })
            .collect();

        let answer = self.link.answer().map(ToOwned::to_owned);
        let browser = acl_ui::views::lobby_browser::Browser {
            t: &translate,
            mods: installed.id(),
            language_name: &|tag| acl_i18n::name_of(tag).unwrap_or(tag).to_owned(),
            answer: answer.as_deref(),
        };

        match acl_ui::views::lobby_browser::draw(ui, &listings, &browser) {
            Some(acl_ui::views::lobby_browser::Action::Join(id)) => {
                self.link.join_lobby(u64::try_from(id).unwrap_or(0));
            }
            Some(acl_ui::views::lobby_browser::Action::Close) => {
                // Closing stops the updates as well as hiding them: a session left watching
                // receives every change to every public lobby for as long as it is
                // connected.
                self.link.watch_lobbies(false);
                self.watching_lobbies = false;
                self.page = Screen::Main;
            }
            None => {}
        }
    }

    /// Paints every layer a player is wearing onto a fresh sprite.
    ///
    /// Onto a fresh one, and that is the change of 2026-08-27. It used to composite onto the
    /// body, which works for anything that goes *over* it and makes the hat's back
    /// impossible -- there is nothing under a canvas. `acl_ui::worn::pieces` returns all five
    /// layers with the body among them, so the body becomes one paste like the others.
    ///
    /// The skin was simply missing before: `AmongUsState` has carried `skin_id` all along and
    /// the list this takes had room for two ids.
    ///
    /// Takes the loader rather than `&self` so that the reader's borrow and the artwork's can
    /// coexist: they are disjoint fields, and the borrow checker only knows that when they
    /// are named separately.
    #[cfg(windows)]
    fn dress(
        hats: &mut hat_store::Loader,
        sprites: &mut [(i32, i32, acl_ui::sprite::Bitmap)],
        wearing: &[(usize, Wearing)],
    ) {
        for (at, worn) in wearing {
            let Some((_, _, body)) = sprites.get_mut(*at) else {
                continue;
            };
            let pieces = acl_ui::worn::pieces(
                hats.collection(),
                acl_ui::worn::Worn {
                    hat: &worn.hat,
                    skin: &worn.skin,
                    visor: &worn.visor,
                },
                acl_types::cosmetics::HAT_COLLECTION_URL,
                acl_ui::hats::BASE,
            );
            let mut canvas = acl_ui::sprite::Bitmap::blank(body.width, body.height);
            for piece in &pieces {
                let Some(url) = piece.url.as_deref() else {
                    // The body, at its own size and origin.
                    canvas.composite(body, (0, 0), (body.width, body.height));
                    continue;
                };
                if !worn.alive {
                    continue;
                }
                let Some(artwork) = hats.image(url) else {
                    // Not fetched yet, or not fetchable. The layer is skipped and the rest
                    // are drawn -- a missing hat is a player without a hat, not a player
                    // without a body.
                    continue;
                };
                let artwork = artwork.clone();
                let (at, size) = acl_ui::worn::placement(
                    piece.geometry,
                    OVERLAY_SPRITE,
                    (artwork.width, artwork.height),
                );
                canvas.composite(&artwork, at, size);
            }
            // Round, like the main window's and like the `Avatar` the shipped overlay
            // renders. Here rather than in `overlay_body` so it happens once: clipping a
            // feathered edge twice thins it.
            acl_ui::sprite::clip_to_circle(&mut canvas);
            *body = canvas;
        }
    }

    /// Draws the settings, and does whatever they asked for.
    ///
    /// The device pickers list what the machine has, by name. They showed the stored *id*
    /// until 2026-08-27 -- a forty-character hash, twice, where two device names belong --
    /// because the lists were left empty while the audio pipeline was still elsewhere. It
    /// is in this process now, so they are filled from it.
    fn show_settings(&mut self, ui: &mut egui::Ui) {
        let catalogue = self.catalogue.as_ref();
        let translate = move |key: &str| {
            catalogue.map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };
        let locales = settings_page::locales();
        // Scoped, because `show` below wants the settings mutably and these only need to
        // read them.
        let (microphones, speakers) = {
            let stored = self.settings.config();
            let found = self.devices.current();
            (
                named(
                    found,
                    acl_audio::device::Direction::Input,
                    stored,
                    "microphone",
                ),
                named(
                    found,
                    acl_audio::device::Direction::Output,
                    stored,
                    "speaker",
                ),
            )
        };
        let microphones: Vec<acl_ui::views::settings::Entry<'_>> = microphones
            .iter()
            .map(|(id, label)| acl_ui::views::settings::Entry { id, label })
            .collect();
        let speakers: Vec<acl_ui::views::settings::Entry<'_>> = speakers
            .iter()
            .map(|(id, label)| acl_ui::views::settings::Entry { id, label })
            .collect();
        let level = self.audio.input_level();
        let testing = self.audio.testing_speaker();
        let (may_change, menu_or_lobby) = self.who_may_change_the_rules();
        let context = acl_ui::views::settings::Context {
            t: &translate,
            input_level: level,
            testing_speaker: testing,
            microphones: &microphones,
            speakers: &speakers,
            locales: &locales,
            // From the game, not hard-coded. Both were `false` -- with a comment saying
            // that is what a client with no session looks like -- and the client has had a
            // session and a reader for a while: every lobby rule on this screen was shown
            // as unavailable to its host, permanently, with the "not in a lobby"
            // explanation on it while they were in one.
            //
            // `Settings.tsx`'s two conditions, at line 348: in the menu, or host of a
            // lobby that has not started.
            host_may_change: may_change,
            in_menu_or_lobby: menu_or_lobby,
            capturing: self.settings.capturing(),
        };
        let entries = self.custom_platforms();
        let chosen = self.settings.config().text_at("launchPlatform");
        let mut adding = std::mem::take(&mut self.adding_platform);
        let (effects, edits) = egui::ScrollArea::vertical()
            .show(ui, |ui| {
                let effects = self.settings.show(ui, &context);
                ui.separator();
                let edits = acl_ui::views::platforms::draw(
                    ui,
                    &entries,
                    &mut acl_ui::views::platforms::Context {
                        t: &translate,
                        chosen: &chosen,
                        adding: &mut adding,
                    },
                );
                (effects, edits)
            })
            .inner;
        self.adding_platform = adding;
        for edit in edits {
            self.apply_platform_edit(&edit);
        }

        for effect in effects {
            match effect {
                settings_page::Effect::RestoreDefaults => {
                    self.settings.restore_defaults();
                    self.catalogue = load_catalogue(&self.settings);
                }
                settings_page::Effect::LanguageChanged => {
                    self.catalogue = load_catalogue(&self.settings);
                }
                settings_page::Effect::TestSpeaker => {
                    // The same button both ways, which is what the two catalogue keys are
                    // for: one of them was never used because the tone could only start.
                    if self.audio.testing_speaker() {
                        acl_core::log_info!("audio", "stopping the test tone");
                        self.audio.stop_testing_speaker();
                    } else {
                        acl_core::log_info!("audio", "playing a test tone");
                        self.audio.test_speaker();
                    }
                }
                settings_page::Effect::ResetOffsets => {
                    // The offsets belong to the helper, which owns the file and the
                    // fetch. Nothing here can throw them away, and pretending otherwise
                    // would leave the client showing a reset that did not happen.
                    if let Some(reader) = self.reader.as_ref() {
                        reader.ask_to_stop();
                    }
                }
            }
        }
    }

    /// The title bar: a drag handle and the two buttons the shipped window has.
    ///
    /// There is no maximise button, matching `maximizable: false`. Dragging is
    /// `StartDrag`, which hands the move to the window manager rather than repositioning
    /// the window per frame — the difference is visible as smoothness and as whether snap
    /// layouts work at all.
    fn title_bar(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &mut Screen,
        say: &dyn Fn(&str) -> String,
    ) -> bool {
        let bar = egui::Rect::from_min_size(
            ui.max_rect().min,
            egui::vec2(ui.max_rect().width(), TITLE_BAR),
        );
        let mut reload = false;
        let response = ui.interact(
            bar,
            ui.id().with("title-bar"),
            egui::Sense::click_and_drag(),
        );
        if response.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            chrome_log(|| "title bar: move started".to_owned());
        }

        // `#1d1a23`, and the whole strip is the drag region.
        ui.painter()
            .rect_filled(bar, egui::CornerRadius::ZERO, theme::BG_TITLEBAR);

        ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
            // Settings and reload flush left, close flush right, the name centred between
            // them. Three separate passes over the same strip rather than one flow,
            // because a centred label cannot be centred by a layout that has already
            // spent the row on buttons.
            let mut page_now = *page;
            let mut used_left = 0.0_f32;
            let mut used_right = 0.0_f32;
            ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
                let group = ui.horizontal_centered(|ui| {
                    ui.add_space(4.0);
                    // One button rather than two, and it says where it goes rather than
                    // where you are: a gear on the settings page reads as "settings are
                    // here", which is where you already were. Leaving the settings is what
                    // applies the three capture settings a device can only be opened with,
                    // so the arrow carries `buttons.exit`.
                    let (glyph, hint) = match page_now {
                        Screen::Main => (theme::icon::SETTINGS, say("settings.title")),
                        Screen::Settings | Screen::Lobbies => {
                            (theme::icon::ARROW_BACK, say("buttons.exit"))
                        }
                    };
                    if chrome_icon(ui, glyph, &hint).clicked() {
                        page_now = match page_now {
                            Screen::Main => Screen::Settings,
                            Screen::Settings | Screen::Lobbies => Screen::Main,
                        };
                    }
                    reload = chrome_icon(ui, theme::icon::REFRESH, &say("client.buttons.reload"))
                        .clicked();
                    if page_now == Screen::Main
                        && chrome_icon(ui, theme::icon::PUBLIC, &say("buttons.public_lobby"))
                            .clicked()
                    {
                        page_now = Screen::Lobbies;
                    }
                });
                used_left = group.response.rect.width();
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
                let group =
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(4.0);
                        if chrome_icon(ui, theme::icon::CLOSE, &say("buttons.close")).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if chrome_icon(ui, theme::icon::MINIMIZE, &say("client.buttons.minimise"))
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                used_right = group.response.rect.width();
            });
            *page = page_now;

            // The name and version, centred *in what the icons left*, in the accent. The
            // version is beside it because it is the first thing anybody is asked for when
            // they report something.
            //
            // Centred on the bar rather than on the gap, it sits under the icons at 250
            // points -- which is the width this window has to work at, not a corner case.
            // So the two groups are measured and the name is painted between them, clipped
            // rather than overlapping: a name that runs out of room gives way, and the
            // controls do not.
            let name = concat!("AnotherCrewLink v", env!("CARGO_PKG_VERSION"));
            let gap = egui::Rect::from_min_max(
                egui::pos2(bar.left() + used_left + 8.0, bar.top()),
                egui::pos2(bar.right() - used_right - 8.0, bar.bottom()),
            );
            if gap.width() > 24.0 {
                // No wrap. A 24-point strip has room for one line, and a name that wraps
                // in it is a name drawn over the row below.
                let galley = ui.painter().layout_no_wrap(
                    name.to_owned(),
                    egui::FontId::proportional(14.0),
                    theme::PURPLE,
                );
                ui.painter().with_clip_rect(gap).galley(
                    egui::pos2(
                        gap.center().x - galley.size().x / 2.0,
                        gap.center().y - galley.size().y / 2.0,
                    ),
                    galley,
                    theme::PURPLE,
                );
            }
        });
        reload
    }
}

impl Client {
    /// Builds this frame's crewmate textures, keyed by the player's index in the state.
    ///
    /// Every player the game reports rather than only the ones the roster shows: a lobby
    /// holds fifteen, the cache makes the second frame free, and deciding who is visible is
    /// the view's job rather than this one's.
    ///
    /// Returns ids and not handles, because the view paints and does not own. What owns them
    /// is [`Portraits`], which drops at the end of this call everything the call did not ask
    /// for -- so a session that visits many lobbies does not accumulate a texture per outfit
    /// it has ever seen.
    fn dress_portraits(
        &mut self,
        context: &egui::Context,
    ) -> std::collections::BTreeMap<usize, egui::TextureId> {
        let Some(state) = self
            .reader
            .as_ref()
            .and_then(|reader| reader.latest().cloned())
        else {
            // No frame, so nothing to dress -- and nothing to keep, either. A reader that
            // stopped should not leave the last lobby's textures resident.
            self.portraits.sweep();
            return std::collections::BTreeMap::new();
        };

        let mut dressed = std::collections::BTreeMap::new();
        for (at, player) in state.players.iter().enumerate() {
            let appearance = Appearance {
                colour: i32::try_from(player.color_id).unwrap_or(-1),
                hat: player.hat_id.clone(),
                skin: player.skin_id.clone(),
                visor: player.visor_id.clone(),
                alive: !player.is_dead,
            };
            if let Some(id) = self.portraits.of(context, &mut self.hats, appearance) {
                dressed.insert(at, id);
            }
        }
        self.portraits.sweep();
        dressed
    }
}

impl Client {
    /// What the window shows while there is no game.
    ///
    /// `Menu.tsx`: the sentence, and under it a way to start the game. Until 2026-08-27
    /// this was one label. `acl_types::platform` had carried the store identifiers since
    /// the port began, with a test comparing every string against `GamePlatform.ts`, and
    /// nothing used them — the client could tell you it was waiting and could not do the one
    /// thing that would end the wait.
    ///
    /// The platform is `launchPlatform`, the same key in the same file 1.x writes, so
    /// somebody who chose Epic there is still on Epic here.
    fn waiting_for_the_game(&mut self, ui: &mut egui::Ui) {
        // Translated up front rather than through a closure that borrows `self`: pressing
        // the button needs `self` mutably, and the two cannot overlap.
        let say = |key: &str| {
            self.catalogue
                .as_ref()
                .map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };
        let waiting = say("game.waiting");
        let no_platform = say("game.error_platform");
        let open_via = say("game.open");
        let open_error = say("game.open_error");
        ui.label(waiting);

        let stored = self.settings.config().text_at("launchPlatform");
        let Some((label, startable)) = self.how_to_start(&stored) else {
            // No platform, one this build does not know, and nothing in `customPlatforms`
            // describing it either. `Menu.tsx` says exactly this rather than offering a
            // button that cannot work.
            ui.label(no_platform);
            return;
        };
        // A built-in platform hands back a translation key; a custom one hands back the
        // title its owner typed, which is already the words to show.
        let name = if label.starts_with("platform.") {
            say(&label)
        } else {
            label
        };

        let mut pressed = false;
        ui.horizontal(|ui| {
            ui.label(open_via);
            pressed = ui.button(name).clicked();
        });
        // Outside the closure, which borrows `self` for the translator above.
        if pressed {
            self.launch_trouble = None;
            self.start_the_game(&stored, &startable);
        }
        if let Some(why) = self.launch_trouble.clone() {
            ui.colored_label(egui::Color32::from_rgb(230, 140, 90), why);
            ui.label(open_error);
        }
    }

    /// Starts the game on a platform, and says so either way.
    ///
    /// The Microsoft Store entry is a program rather than a URI and its path is `none`
    /// until somebody sets one, so this can refuse — and refusing has to be visible.
    /// `Menu.tsx` shows `game.open_error`, which tells the player to start it themselves,
    /// and that is better than a button that appears to do nothing.
    fn start_the_game(&mut self, key: &str, startable: &Startable) {
        let outcome = acl_core::start_game::plan(acl_core::start_game::Described {
            run_path: &startable.run_path,
            is_uri: startable.is_uri,
            execute: &startable.execute,
        })
        .and_then(|what| acl_core::start_game::start(&what));
        match outcome {
            Ok(()) => acl_core::log_info!("game", "asked {key} to start Among Us"),
            Err(why) => {
                acl_core::log_warn!("game", "could not start Among Us on {key}: {why}");
                self.launch_trouble = Some(why.to_string());
            }
        }
    }

    /// What `launchPlatform` names, as a label and the fields that start it.
    ///
    /// One of the three the client knows, or an entry in `customPlatforms` under that same
    /// key. The second is not an extra: a player who set one up in 1.x has it in the file
    /// this client reads, and refusing to start it would make this client worse than the
    /// one they had. Adding a new one is not offered here yet -- only using one.
    ///
    /// The label is a translation key for a built-in and the player's own title for a
    /// custom entry, and the caller tells them apart by the prefix.
    fn how_to_start(&self, key: &str) -> Option<(String, Startable)> {
        if let Some(platform) = acl_types::platform::Platform::from_key(key) {
            return Some((
                platform.translate_key().to_owned(),
                Startable {
                    run_path: platform.run_path().to_owned(),
                    is_uri: matches!(platform.run_type(), acl_types::platform::RunType::Uri),
                    execute: platform
                        .executable()
                        .map(|name| vec![name.to_owned()])
                        .unwrap_or_default(),
                },
            ));
        }

        let stored = self.settings.config();
        let run_path = stored.text_at(&format!("customPlatforms.{key}.runPath"));
        if run_path.is_empty() {
            return None;
        }
        // `PlatformRunType` is a string enum -- `'URI'` and `'EXE'` -- rather than the
        // numbers it looks like it should be. Anything else is treated as a program, which
        // is the branch that can fail visibly; guessing URI would hand an unknown string to
        // the shell.
        let is_uri = stored
            .text_at(&format!("customPlatforms.{key}.launchType"))
            .eq_ignore_ascii_case("URI");
        Some((
            key.to_owned(),
            Startable {
                run_path,
                is_uri,
                execute: stored.strings_at(&format!("customPlatforms.{key}.execute")),
            },
        ))
    }

    /// Whether the lobby rules may be changed, and whether a lobby exists to explain it.
    ///
    /// `Settings.tsx` line 348: the rules are the host's while the round has not started,
    /// and anybody's in the menu -- where a change is a preference for the next lobby
    /// rather than an edit to one people are in. The second flag picks which of the two
    /// explanations a disabled rule gives.
    ///
    /// No reader, or no frame yet, reads as the menu: a client that cannot see the game
    /// should let somebody set their preferences rather than lock the page.
    fn who_may_change_the_rules(&self) -> (bool, bool) {
        let Some(state) = self.reader.as_ref().and_then(|reader| reader.latest()) else {
            return (true, true);
        };
        let in_menu = state.game_state == acl_game::GameState::Menu;
        let in_lobby = state.game_state == acl_game::GameState::Lobby;
        (in_menu || (state.is_host && in_lobby), in_menu || in_lobby)
    }

    /// The custom platforms the settings hold, in the shape the editor shows them.
    fn custom_platforms(&self) -> Vec<acl_ui::platforms::Entry> {
        let stored = self.settings.config();
        let Some(serde_json::Value::Object(map)) = stored.get("customPlatforms") else {
            return Vec::new();
        };
        // Sorted, because a JSON object has no order a person can rely on and a list that
        // rearranges itself between frames is one nobody can click in.
        let mut names: Vec<&String> = map.keys().collect();
        names.sort();
        names
            .into_iter()
            .map(|name| {
                let is_uri = stored
                    .text_at(&format!("customPlatforms.{name}.launchType"))
                    .eq_ignore_ascii_case("URI");
                acl_ui::platforms::to_entry(
                    name,
                    is_uri,
                    &acl_ui::platforms::Stored {
                        run_path: stored.text_at(&format!("customPlatforms.{name}.runPath")),
                        execute: stored.strings_at(&format!("customPlatforms.{name}.execute")),
                    },
                )
            })
            .collect()
    }

    /// Writes one edit through to the settings file.
    ///
    /// Every field of an entry is written together, because they are one thing: a
    /// `launchType` that changed without its `runPath` describes a URI platform holding a
    /// directory, which starts nothing and reads as a broken entry rather than a half-saved
    /// one.
    fn apply_platform_edit(&mut self, edit: &acl_ui::views::platforms::Edit) {
        use acl_ui::views::platforms::Edit;
        match edit {
            Edit::Add(name) => {
                acl_core::log_info!("platform", "adding {name}");
                self.write_platform(&acl_ui::platforms::Entry {
                    name: name.clone(),
                    ..acl_ui::platforms::Entry::default()
                });
            }
            Edit::Update(entry) => self.write_platform(entry),
            Edit::Remove(name) => {
                acl_core::log_info!("platform", "removing {name}");
                self.settings.forget(&format!("customPlatforms.{name}"));
                // A platform that is gone cannot stay selected, or the launch button would
                // name something the file no longer describes.
                if self.settings.config().text_at("launchPlatform") == *name {
                    self.settings
                        .put("launchPlatform", serde_json::json!("STEAM"));
                }
            }
            Edit::Use(name) => {
                acl_core::log_info!("platform", "launching through {name} from now on");
                self.settings.put("launchPlatform", serde_json::json!(name));
            }
        }
    }

    /// Stores one entry under its own name.
    fn write_platform(&mut self, entry: &acl_ui::platforms::Entry) {
        let stored = acl_ui::platforms::to_stored(entry);
        let name = &entry.name;
        // `key` and `translateKey` are both the title, which is what
        // `CustomPlatformSettings.tsx` writes -- a 1.x client reads them and would show a
        // nameless entry without them.
        for (field, value) in [
            ("key", serde_json::json!(name)),
            ("translateKey", serde_json::json!(name)),
            ("default", serde_json::json!(false)),
            (
                "launchType",
                serde_json::json!(if entry.is_uri { "URI" } else { "EXE" }),
            ),
            ("runPath", serde_json::json!(stored.run_path)),
            ("execute", serde_json::json!(stored.execute)),
        ] {
            self.settings
                .put(&format!("customPlatforms.{name}.{field}"), value);
        }
    }

    /// The two lobby rules that are worth saying out loud on the main screen.
    ///
    /// `Voice.tsx` puts both here, and the reason is support rather than decoration: with
    /// "only the dead talk" or "only ghosts in meetings" on, a living player hears silence
    /// and has no way to tell that from a broken microphone. A line saying which rule is in
    /// force is the difference between a rule and a fault.
    ///
    /// Only when on. A list of rules that are off is a list nobody reads.
    fn say_what_the_lobby_allows(&self, ui: &mut egui::Ui) {
        let stored = self.settings.config();
        let say = |key: &str| {
            self.catalogue
                .as_ref()
                .map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };
        for (setting, key) in [
            (
                "localLobbySettings.deadOnly",
                "settings.lobbysettings.ghost_only_warning2",
            ),
            (
                "localLobbySettings.meetingGhostOnly",
                "settings.lobbysettings.meetings_only_warning2",
            ),
        ] {
            if stored.bool_at(setting) {
                ui.label(egui::RichText::new(say(key)).small().weak());
            }
        }
    }

    /// The lobby's code and what the game is doing, as one line.
    ///
    /// `hideCode` is applied here: `Voice.tsx` line 277 replaces the code with the word
    /// LOBBY rather than blanking it, so somebody streaming shows that there *is* a lobby
    /// without showing which one. The menu keeps its own name — there is no code to give
    /// away, and a menu labelled LOBBY would be a lie rather than a redaction.
    fn lobby_code(&self, state: &acl_game::AmongUsState) -> String {
        let say = |key: &str| {
            self.catalogue
                .as_ref()
                .map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };
        if state.lobby_code.trim() == "MENU" {
            // The reader reports the word MENU, and the catalogue has it: it is a word on
            // the screen like any other, and both locales translate it.
            say("game.menu")
        } else if self.settings.config().bool_at("hideCode") {
            "LOBBY".to_owned()
        } else {
            state.lobby_code.clone()
        }
    }

    /// The one line an update gets, and the button that takes it.
    ///
    /// Drawn before the game reader is asked anything, for two reasons. It is not part of
    /// the reader's status -- a waiting update is a good thing to see, and belongs above the
    /// list of what is wrong rather than under it -- and `status_strip` holds a borrow of
    /// the reader, which is what pressing this cannot have.
    fn offer_the_update(&mut self, ui: &mut egui::Ui) {
        let Some(updates::Offer::Ready(version)) = self.updates.offer() else {
            return;
        };
        let version = version.clone();
        let say = |key: &str| {
            self.catalogue
                .as_ref()
                .map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };
        let say_with = |key: &str, args: &[(&str, &str)]| {
            self.catalogue.as_ref().map_or_else(
                || key.to_owned(),
                |catalogue| catalogue.t_with(key, args).into_owned(),
            )
        };
        let mut pressed = false;
        ui.horizontal(|ui| {
            ui.label(say_with(
                "client.update.available",
                &[("version", &version)],
            ));
            pressed = ui.button(say("client.update.install")).clicked();
        });
        if !pressed {
            return;
        }
        match updates::install() {
            Ok(()) => {
                acl_core::log_info!("update", "started the updater for {version}");
                // An installer cannot write over files this process is holding open, so
                // leaving is part of installing rather than a courtesy.
                self.write_window_state();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(why) => {
                acl_core::log_warn!("update", "could not start the updater: {why}");
                self.launch_trouble = Some(why);
            }
        }
    }

    /// The line of things that are wrong, and the one number that says whether it works.
    ///
    /// Four sources in one strip rather than four panels: none of them is why anybody
    /// opened this window, and each is worth knowing about when it applies. A client with
    /// no microphone is a working client with half its voice, and it is the first thing
    /// somebody asks about when nobody can hear them.
    fn status_lines(&self, ui: &mut egui::Ui, reader: &reader::Reader) {
        const TROUBLE: egui::Color32 = egui::Color32::from_rgb(230, 140, 90);

        let say = |key: &str| {
            self.catalogue
                .as_ref()
                .map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };
        let say_with = |key: &str, args: &[(&str, &str)]| {
            self.catalogue.as_ref().map_or_else(
                || key.to_owned(),
                |catalogue| catalogue.t_with(key, args).into_owned(),
            )
        };

        // One line each, stacked, rather than the single row this was until 2026-08-28.
        // Three phrases and a window that is 250px wide at its minimum: the row ran off the
        // right edge and the peer count -- the one number here that answers "can anybody
        // hear me" -- was the half that fell off it.
        ui.horizontal(|ui| {
            ui.label(say("client.status.game_reader"));
            ui.label(egui::RichText::new(format!("{:?}", reader.state())).strong());
        });
        // The server, under the reader. Both are things that can be down, and only one of
        // them used to be sayable here -- a connection that never happened showed as fifteen
        // crewmates wearing a "no connection" badge and nothing that said why.
        ui.horizontal(|ui| {
            ui.label(say("client.status.server"));
            let (word, colour) = match self.link.state() {
                net::State::Connected(_) => ("client.status.connected", ui.visuals().text_color()),
                net::State::Connecting => {
                    ("client.status.connecting", ui.visuals().weak_text_color())
                }
                net::State::Idle => ("client.status.not_connected", TROUBLE),
                net::State::Failed(_) => ("client.status.failed", TROUBLE),
            };
            ui.colored_label(colour, egui::RichText::new(say(word)).strong());
        });
        // How many peers are actually reachable, which is a different question from how many
        // players the game reports. A lobby of six with one connection is the shape of a
        // problem, and it is invisible without a number.
        ui.label(say_with(
            "client.status.peers",
            &[("count", &self.link.connected_peers().to_string())],
        ));
    }

    /// Everything that is currently wrong, one line each.
    ///
    /// Separate from [`Self::status_lines`] because they go in different places: the three
    /// status lines sit in the column beside your crewmate, and a fault is full width under
    /// the divider, where a sentence has room to be read.
    fn trouble_lines(&self, ui: &mut egui::Ui, reader: &reader::Reader) {
        const TROUBLE: egui::Color32 = egui::Color32::from_rgb(230, 140, 90);

        let say = |key: &str| {
            self.catalogue
                .as_ref()
                .map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };
        let retired;
        let link_trouble = match self.link.state() {
            net::State::Failed(why) => {
                let catalogue = self.catalogue.as_ref();
                retired = acl_net::retirement::message(why, |key| {
                    catalogue
                        .map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
                });
                Some(retired.as_str())
            }
            _ => None,
        };
        let update_trouble = match self.updates.offer() {
            Some(updates::Offer::Trouble(why)) => Some(why.as_str()),
            _ => None,
        };
        for (what, trouble) in [
            (None, reader.trouble()),
            (Some(say("client.status.server")), link_trouble),
            (Some(say("client.status.update")), update_trouble),
            (Some(say("client.status.hats")), self.hats.trouble()),
            (Some(say("client.status.audio")), self.audio.trouble()),
        ] {
            let Some(trouble) = trouble else {
                continue;
            };
            ui.colored_label(
                TROUBLE,
                what.map_or_else(|| trouble.to_owned(), |name| format!("{name}: {trouble}")),
            );
        }
    }

    /// You, above everybody else, which is where `Voice.tsx` puts you.
    ///
    /// `main_view` filters the local player out on purpose -- it answers "who else is
    /// here" -- so this is built separately rather than by asking it for a row it will
    /// never return.
    ///
    /// Its own function because `ui` was over a hundred lines with it inline, and because
    /// what it draws is genuinely a different thing: everybody else's row says whether you
    /// can hear them, and this one says whether anybody can hear you.
    fn draw_you(
        ui: &mut egui::Ui,
        state: &acl_game::AmongUsState,
        controls: &controls::Switchboard,
        connected: bool,
        local_talking: bool,
        dressed: &std::collections::BTreeMap<usize, egui::TextureId>,
        say: &dyn Fn(&str) -> String,
    ) {
        let Some((at, me)) = state
            .players
            .iter()
            .enumerate()
            .find(|(_, player)| player.is_local)
        else {
            return;
        };
        let switches = controls.state();
        acl_ui::views::main::draw_own(
            ui,
            &acl_ui::views::main::Own {
                portrait: Portrait {
                    name: &me.name,
                    color_id: i32::try_from(me.color_id).unwrap_or(-1),
                    state: acl_ui::roster::Shown {
                        at,
                        talking: local_talking,
                        alive: !me.is_dead,
                        // The server connection rather than a peer's, which is what
                        // `Voice.tsx` shows here: it is the one connection that is yours,
                        // and losing it is why nobody can hear you.
                        link: if connected {
                            acl_ui::roster::Link::Connected
                        } else {
                            acl_ui::roster::Link::Disconnected
                        },
                        using_radio: false,
                    },
                    art: dressed.get(&at).copied(),
                },
                muted: switches.muted,
                deafened: switches.deafened,
            },
            say,
        );
    }

    /// Everything the frame needs settled before anything is painted.
    ///
    /// It is here rather than inside the panel because of borrowing, not tidiness: the
    /// closure that draws holds `self.reader` for its whole body, and each of these wants
    /// `&mut` on a different field. Hoisting them makes the borrows disjoint by being
    /// sequential.
    ///
    /// Returns the crewmate textures, keyed by the player's index in the state.
    fn before_painting(
        &mut self,
        context: &egui::Context,
    ) -> std::collections::BTreeMap<usize, egui::TextureId> {
        if let Some(state) = self
            .reader
            .as_ref()
            .and_then(|reader| reader.latest())
            .cloned()
        {
            self.follow_deaths(&state);
        }
        // Held as well as sent: the local player's own row reads it, and the detector
        // reports transitions rather than a level, so nothing else remembers it. Only on a
        // transition -- its hangover is what makes that a handful of messages a minute
        // rather than fifty a second.
        if let Some(speaking) = self.audio.take_voice_activity() {
            self.local_talking = speaking;
            self.link.say_speaking(speaking);
        }
        self.dress_portraits(context)
    }

    /// Updates who the voice layer believes is dead.
    ///
    /// The rule and its reason are `acl_ui::roster::follow_deaths`, which is tested without
    /// a game. What is here is the two things only this side knows: which of the reader's
    /// five states each of its three phases is, and that the whole thing runs on the
    /// transition rather than per frame.
    fn follow_deaths(&mut self, state: &acl_game::AmongUsState) {
        if self.last_game_state == Some(state.game_state) {
            return;
        }
        self.last_game_state = Some(state.game_state);

        // `Menu` and `Unknown` are `Elsewhere` deliberately: leaving a game is a moment when
        // what was secret stops being secret, and a map left standing would follow into the
        // next lobby.
        let phase = match state.game_state {
            acl_game::GameState::Lobby => acl_ui::roster::Phase::Lobby,
            acl_game::GameState::Tasks => acl_ui::roster::Phase::Round,
            _ => acl_ui::roster::Phase::Elsewhere,
        };
        let seats: Vec<Seat<'_>> = state.players.iter().map(Seat).collect();
        acl_ui::roster::follow_deaths(phase, &seats, &mut self.dead);
    }
}

impl eframe::App for Client {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(reader) = self.reader.as_mut() {
            reader.pump();
        }
        self.hats.pump();
        self.link.pump();
        self.updates.pump();
        // Once a frame, and it decays on its own: a peer who stops sending is not in the
        // next one, with nothing having to notice they went quiet.
        self.speaking = self.link.take_speaking();
        self.keep_on_top(ctx);
        // Cheap enough to do every frame: two atomic stores, and the alternative is another
        // thing to invalidate when the settings screen writes.
        let (gain, floor) = live_settings(self.settings.config());
        self.audio.tune(gain, floor);
        self.link
            .set_force_relay(self.settings.config().bool_at("natFix"));
        self.keep_connected();
        self.follow_the_lobby();
        self.keep_listed();
        self.carry_audio();

        // Remembered every frame, and written down once it has stopped moving.
        //
        // **Not only on the way out**, which is what this did until 2026-08-28. A client
        // that is killed, or that the driver takes down with it, closes without running
        // the line that writes -- and the next start has nothing to restore, so it opens
        // at `MIN_WIDTH` by `MIN_HEIGHT`. That is a floor, not a size anybody chose, and
        // it is how a window somebody had sized to their screen came back at 250x350.
        //
        // The shipped keeper debounces for the same reason, off move and resize events.
        // This is already awake every frame, so the debounce is a timestamp instead.
        let minimised = ctx.input(|input| input.viewport().minimized.unwrap_or(false));
        let maximised = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
        if worth_saving(minimised, maximised, fullscreen)
            && let Some(outer) = ctx.input(|input| input.viewport().outer_rect)
        {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "screen coordinates, rounded to the pixel they are stored as"
            )]
            let seen = WindowState {
                width: outer.width() as i32,
                height: outer.height() as i32,
                x: Some(outer.min.x as i32),
                y: Some(outer.min.y as i32),
            };
            if self.last_seen != Some(seen) {
                self.last_seen = Some(seen);
                self.moved_at = Some(std::time::Instant::now());
            }
        }
        if self.moved_at.is_some_and(|since| since.elapsed() >= SETTLE) {
            self.write_window_state();
        }

        // Five times a second, and now actually five times a second: see [`OVERLAY_TICK`]
        // for what one of these costs and what asking for it every repaint did. The tick is
        // taken before the state, because cloning the state is itself work this does not
        // need to do a hundred times a second either.
        #[cfg(windows)]
        if self
            .overlay_due
            .due(std::time::Instant::now(), OVERLAY_TICK)
            && let Some(state) = self
                .reader
                .as_ref()
                .and_then(|reader| reader.latest().cloned())
        {
            self.compose_overlay(&state);
        }

        // Every frame while a capture is running, because it reads the key state rather
        // than waiting for an event. Escape abandons it: the settings screen is the one
        // place where a key press means "this key" rather than "do the thing this key
        // does", so leaving needs a way out that is not a binding.
        if self.settings.capturing().is_some() {
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.settings.cancel_capture();
            } else {
                self.settings.poll_capture();
            }
            // A capture is the one thing here that is waiting on something faster than the
            // game's five frames a second.
            ctx.request_repaint();
        }

        if ctx.input(|input| input.viewport().close_requested()) {
            self.write_window_state();
        }

        // The game state arrives five times a second and nothing else moves, so when nobody
        // is looking at this window there is no reason to redraw faster than that.
        //
        // **While the pointer is over it there is.** egui scrolls smoothly by animating an
        // offset over several frames, and it can only do that in the frames it is given.
        // Measured on 2026-08-28 with the settings open and the wheel turning: 5 frames a
        // second and gaps of 146 to 239 milliseconds between them -- which is this fallback
        // firing, because between one wheel event and the next nothing asked for anything
        // sooner. A fifth of a second of held-still list, then a jump, is exactly what
        // "slow and juddery" describes.
        //
        // The cost is bounded by where the pointer is. This window's own work is 0.08ms a
        // frame -- measured the same day -- so drawing continuously while somebody is
        // actually pointing at it is cheap, and the moment they move away it goes back to
        // five a second.
        if ctx.input(|input| input.pointer.has_pointer()) {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one function because it is one window: the page dispatch and the main                   screen's order are the same decision, and splitting them would put the                   order in two places"
    )]
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Before the panel, because the edge has to beat whatever the panel draws under it.
        if let Some(direction) = acl_ui::edges::interact(&ctx) {
            chrome_log(|| format!("edge: resize started {direction:?}"));
        }
        // Where a press landed and how big the window thought it was. Two rounds of "I
        // cannot move or resize it" were spent guessing at this; one line per click is
        // cheap and turns the next one into a measurement.
        if ctx.input(|input| input.pointer.primary_pressed()) {
            chrome_log(|| {
                let pointer = ctx.input(|input| input.pointer.interact_pos());
                format!(
                    "press at {pointer:?} in {:?} (focused {})",
                    ctx.content_rect(),
                    ctx.input(|input| input.viewport().focused.unwrap_or(false)),
                )
            });
        }
        let dressed = self.before_painting(&ctx);
        egui::CentralPanel::default().show(ui, |ui| {
            let mut page = self.page;
            // Scoped, so the borrow of `self.catalogue` ends here: everything below wants
            // `self` mutably, and a translator that outlived the title bar would keep it.
            let reload = {
                let catalogue = self.catalogue.as_ref();
                let say = move |key: &str| {
                    catalogue
                        .map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
                };
                Self::title_bar(ui, &ctx, &mut page, &say)
            };
            self.page = page;
            // Stop then start, in that order and on one channel, so the thread does them in
            // that order: the shipped client's ⟳ reloads its renderer, and the nearest
            // thing here is letting the helper go and asking for a fresh one. That is what
            // re-fetches the offsets, which is what somebody pressing it usually wants.
            if reload {
                if let Some(reader) = self.reader.as_ref() {
                    reader.ask_to_stop();
                    reader.ask_to_start();
                }
                acl_core::log_info!("chrome", "reload: restarting the reader and the audio");
                // And the audio, which is what applies the three capture settings that can
                // only be given when the device is opened. `Settings.tsx` raises an
                // "unsaved" count for exactly those and asks for a reconnect; this is that
                // reconnect. Dropping the old handle stops its streams.
                //
                // The media path is rewired with it: the new pipeline has a new mixing
                // thread, and a worker still delivering to the old one would leave the
                // client deaf from the first rebuild onwards.
                let packets = self.link.rewire_audio();
                self.audio = audio::Audio::start(
                    capture_settings(self.settings.config()),
                    packets,
                    &self.link.audio_sink(),
                );
                self.controls = controls::Switchboard::start(self.audio.tuning());
            }
            ui.add_space(TITLE_BAR);
            ui.separator();

            match self.page {
                Screen::Settings => {
                    self.show_settings(ui);
                    return;
                }
                Screen::Lobbies => {
                    self.show_lobbies(ui);
                    return;
                }
                Screen::Main => {}
            }

            self.offer_the_update(ui);

            let Some(reader) = self.reader.as_ref() else {
                let catalogue = self.catalogue.as_ref();
                ui.label(catalogue.map_or_else(
                    || "client.status.reader_failed".to_owned(),
                    |catalogue| catalogue.t("client.status.reader_failed").to_owned(),
                ));
                return;
            };

            let Some(state) = reader.latest() else {
                // No game yet, so no crewmate to put the status beside. It keeps its own
                // block here, which is also where it is most worth reading: this is the
                // screen somebody stares at when the server will not come up.
                self.status_lines(ui, reader);
                self.trouble_lines(ui, reader);
                ui.separator();
                self.waiting_for_the_game(ui);
                return;
            };

            // The roster decides who is shown; this only draws them. Nothing here knows
            // anything about audio yet, so the voice layer is answered with the truth as
            // it stands: nobody is talking, nobody has been heard to die, and every player
            // the game reports is treated as reachable but silent.
            // `connected` is the one this can answer now: the mesh reports which peers
            // have a connection that is up, which is the difference `views::main` draws
            // between a player who has arrived and one who can be heard. The rest still
            // waits on audio moving.
            let link = &self.link;
            let speaking = &self.speaking;
            // This end's own detector, which `roster` folds into the local player's row.
            let local_talking = self.local_talking;
            let hearable = &self.hearable;
            let believed_dead = &self.dead;
            let controls = &self.controls;
            let connected_to_server = matches!(self.link.state(), net::State::Connected(_));
            // `Voice.tsx` line 1598, and the reason is in the comment where `hearable` is
            // filled: a peer is shown as speaking only if this client can hear them.
            let can_hear = |client_id: i64| {
                link.socket_of(client_id)
                    .is_some_and(|socket| hearable.contains(socket))
            };
            let voice = Voice {
                // Two different questions, and they used to be one. `talking` is whether a
                // peer's stream carries speech, which is the `VAD` the server relays;
                // `audible` is whether audio is arriving at all. A peer can be audible and
                // silent -- that is most of a lobby, most of the time -- and one can be
                // talking with nothing arriving, which is the shape of a broken connection.
                talking: &|client_id| link.talking(client_id) && can_hear(client_id),
                dead: &|client_id| believed_dead.get(&client_id).copied().unwrap_or(false),
                connected: &|client_id| link.hears(client_id),
                audible: &|client_id| speaking.contains(&client_id),
                local_talking,
                local_alive: !state.players.iter().any(|p| p.is_local && p.is_dead),
                // `impostor_radio` is §4.13's one genuinely blocked item, and this is where
                // it shows. 1.x claims the radio over the *data channel* -- `Voice.tsx` 913 and
                // 1290 -- and this client has none by design: `the_offer_carries_audio_and_no_
                // data_channel` asserts the SDP has no `m=application`. Moving the claim to the
                // socket is the change §4.12's rollout forbids while both generations share a
                // lobby, so it stays `None` until 1.x is switched off.
                //
                // `local_is_impostor` is not blocked and is read from the game. On its own it
                // changes nothing -- `roster` needs both -- but a hard `false` where a fact is
                // available is a line that stops looking like a stub.
                impostor_radio: link.on_radio(),
                local_is_impostor: state
                    .players
                    .iter()
                    .any(|player| player.is_local && player.is_impostor),
            };
            let seats: Vec<Seat<'_>> = state.players.iter().map(Seat).collect();
            let portraits: Vec<Portrait<'_>> = main_view(&seats, &voice)
                .iter()
                .filter_map(|entry| {
                    let player = state.players.get(entry.at)?;
                    Some(Portrait {
                        name: &player.name,
                        color_id: i32::try_from(player.color_id).unwrap_or(-1),
                        state: *entry,
                        // `None` while the cosmetics are still arriving, which the view
                        // draws as shapes rather than as nothing.
                        art: dressed.get(&entry.at).copied(),
                    })
                })
                .collect();
            // One translator for both views. Built here rather than passed down from the
            // panel's own, because that one's borrow of `self.catalogue` ended with the
            // title bar -- see the scope there.
            let catalogue = self.catalogue.as_ref();
            let say_here = move |key: &str| {
                catalogue.map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
            };
            // The spec's top row: your crewmate on the left, and stacked beside it your
            // name, the lobby code, and where this client stands. §2, minus the mute and
            // deafen buttons on the right, which are not built yet.
            let me = state.players.iter().find(|player| player.is_local);
            let switches = controls.state();
            let mut pressed = None;
            ui.horizontal(|ui| {
                Self::draw_you(
                    ui,
                    state,
                    controls,
                    connected_to_server,
                    local_talking,
                    &dressed,
                    &say_here,
                );
                // 30px of button and the spec's 5px of padding beside it, taken off the
                // right before the column is given the rest. A `vertical` in a row takes
                // the whole remainder, so built the other way round there is no width left
                // for them to be in.
                let column = (ui.available_width() - 35.0).max(0.0);
                ui.allocate_ui(egui::vec2(column, ui.available_height()), |ui| {
                    ui.vertical(|ui| {
                        // **Your own name is not drawn**, which is a deviation from §2 --
                        // "your name (20px, ellipsised) and the lobby code stacked centre"
                        // -- decided by the maintainer on 2026-08-28. It is the one label
                        // on this screen whose reader already knows what it says, and the
                        // crewmate beside it is yours whether or not it is written out.
                        acl_ui::views::main::lobby_code(
                            ui,
                            &self.lobby_code(state),
                            me.map_or(-1, |me| i32::try_from(me.color_id).unwrap_or(-1)),
                        );
                        self.status_lines(ui, reader);
                    });
                });
                // Against the right edge. `allocate_ui` gives back only what the column
                // actually used, not what it was offered, so without this the pair sits
                // against the longest line of text instead of against the window.
                ui.add_space((ui.available_width() - 30.0).max(0.0));
                pressed = acl_ui::views::main::draw_switches(
                    ui,
                    switches.muted,
                    switches.deafened,
                    &say_here,
                );
            });

            ui.separator();
            self.say_what_the_lobby_allows(ui);
            self.trouble_lines(ui, reader);
            ui.add_space(4.0);
            acl_ui::views::main::draw(ui, &portraits, &say_here);

            // Applied here rather than where it was pressed, which is the rule the settings
            // screen states and the same reason: the rules of these two are not the view's.
            // Mute while deafened clears both, and a button that wrote its own boolean
            // would be a second set of rules to keep in step with the keys.
            match pressed {
                Some(acl_ui::views::main::Switch::Mute) => self.controls.toggle_mute(),
                Some(acl_ui::views::main::Switch::Deafen) => self.controls.toggle_deafen(),
                None => {}
            }
        });
    }
}

/// Shows one line to somebody who has no console to read it in.
///
/// `MessageBoxW` rather than a toolkit window: this runs before eframe has started, and the
/// cases that need it are exactly the ones where the client is about to exit without ever
/// drawing anything. A window that never appears is the failure this replaces.
#[cfg(windows)]
fn tell_the_user(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};

    let mut body: Vec<u16> = message.encode_utf16().collect();
    body.push(0);
    let mut title: Vec<u16> = "AnotherCrewLink".encode_utf16().collect();
    title.push(0);
    // SAFETY: two null-terminated wide strings that outlive the call, and a null owner
    // window, which is what a process with no window of its own has.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(test)]
mod tests {
    /// The line that keeps the console window shut is still in this file.
    ///
    /// Not a style check. `#![windows_subsystem = "windows"]` was added in 558db643, and
    /// 65c72329 deleted it while rewriting the paragraph above it -- the prose that
    /// explains the fix survived, the line that *is* the fix did not, and the console came
    /// back in front of a proximity chat that had shipped without one.
    ///
    /// Read out of the source rather than asserted about the process, because a test
    /// binary is a console application whatever this file says: the attribute applies to
    /// the executable it is compiled into, and that is not this one. What can be checked
    /// here is that nobody has removed the line again, which is how it was lost.
    /// A cadence lets the first through and holds the rest until its period is up.
    ///
    /// Driven with made-up instants rather than by sleeping, so it says something about the
    /// arithmetic rather than about how busy the machine was.
    /// The signalling connection comes back, on a schedule that grows.
    ///
    /// Until 2026-08-29 a dropped socket was the end of voice for the life of the process:
    /// `keep_connected` reconnected only from `Idle` and treated `Failed` as terminal. A
    /// server restart, or three seconds of unplugged ethernet, left a client that still
    /// painted its lobby list and could no longer hear anybody, with nothing on screen to
    /// say so.
    #[test]
    fn a_dropped_socket_is_retried_on_a_growing_delay() {
        use acl_net::reconnect::{BASE_DELAY, MAX_DELAY, reconnect_delay};

        let start = std::time::Instant::now();

        // The first sight of a failure schedules rather than connecting. Reconnecting the
        // instant a server closes the socket races the restart that closed it.
        // `reconnect_due` always hands a schedule back -- there is no state in which the
        // client stops trying to reach its server -- so this unwrapping is the assertion.
        let schedule = |held, now| match super::reconnect_due(held, now) {
            (fired, Some(next)) => (fired, next),
            (_, None) => panic!("reconnect_due must always leave a schedule behind"),
        };

        let (now, (due, attempt)) = schedule(None, start);
        assert!(now.is_none(), "the first failure waits");
        assert_eq!(attempt, 1);
        assert_eq!(due - start, BASE_DELAY);

        // Called again before the delay is up, it holds the schedule rather than replacing
        // it. Every frame calls this, so a schedule that reset itself would never fire.
        let (now, held) = schedule(Some((due, attempt)), start + BASE_DELAY / 2);
        assert!(now.is_none());
        assert_eq!(
            held,
            (due, attempt),
            "the schedule survives being looked at"
        );

        // And at the deadline it fires and doubles.
        let (now, (second_due, second)) = schedule(Some((due, attempt)), due);
        assert_eq!(now, Some(1), "the first attempt");
        assert_eq!(second, 2);
        assert_eq!(second_due - due, reconnect_delay(2, true));

        // Doubling to a ceiling, so a server that is down costs one attempt every thirty
        // seconds rather than a tight loop against a machine trying to come back up.
        let mut held = (second_due, second);
        let mut when = second_due;
        for _ in 0..12 {
            let (fired, next) = schedule(Some(held), when);
            assert!(fired.is_some());
            held = next;
            when = next.0;
        }
        let (_, far_out) = schedule(Some(held), when);
        assert_eq!(far_out.0 - when, MAX_DELAY, "the delay is bounded");
    }

    #[test]
    fn a_cadence_holds_everything_between_its_ticks() {
        use std::time::Duration;

        let period = Duration::from_millis(200);
        let start = std::time::Instant::now();
        let mut cadence = super::Cadence::default();

        assert!(cadence.due(start, period), "the first is always due");
        assert!(!cadence.due(start + Duration::from_millis(1), period));
        assert!(!cadence.due(start + Duration::from_millis(199), period));
        assert!(cadence.due(start + period, period));
        // The next window is measured from when it last fired, not from the first ask --
        // otherwise a burst of repaints walks the deadline forward and it never fires.
        assert!(!cadence.due(start + period + Duration::from_millis(1), period));
        assert!(cadence.due(start + period + period, period));
    }

    #[test]
    fn the_console_window_is_still_switched_off() {
        let source = include_str!("main.rs");
        assert!(
            source.contains(r#"#![cfg_attr(windows, windows_subsystem = "windows")]"#),
            "the windows_subsystem attribute is gone: this build opens a console"
        );
    }
}
