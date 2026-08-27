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
//! What goes with it: `eprintln!` now writes nowhere. That is acceptable for the diagnostic
//! lines -- a user launching from the Start menu never saw them either -- and is *not*
//! acceptable for the one message that exists to be read, which is a second copy telling
//! you the first is already running. That one is a message box now.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! The window.
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

mod audio;
mod controls;
mod hat_store;
mod net;
mod reader;
mod settings_page;

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

/// How tall the title bar is drawn.
const TITLE_BAR: f32 = 32.0;

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
}

impl Devices {
    /// How stale the list may be.
    const FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(2);

    /// What the machine has, enumerating again if it has been a while.
    fn current(&mut self) -> &[acl_audio::device::Device] {
        let due = self
            .refreshed
            .is_none_or(|last| last.elapsed() >= Self::FRESH_FOR);
        if due {
            use acl_audio::device::Backend as _;
            // A failure leaves the last good list standing rather than emptying the picker:
            // a device that was there a moment ago is a better answer than none, and the
            // one that is stored is still what the client is using.
            if let Ok(found) = acl_audio::device::system::Cpal::new().devices() {
                self.held = found;
            }
            self.refreshed = Some(std::time::Instant::now());
        }
        &self.held
    }
}

/// Says what the window frame just did, when asked to.
///
/// Behind `ACL_CHROME_LOG` because it is a line per click and nobody needs that by default.
/// It exists at all because a frameless window's move and resize are the client's own job --
/// there is no system title bar to blame -- and when they do not work there is nothing to
/// look at. The closure is not called unless the variable is set.
fn chrome_log(what: impl FnOnce() -> String) {
    if std::env::var_os("ACL_CHROME_LOG").is_some() {
        eprintln!("AnotherCrewLink: {}", what());
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
            eprintln!("AnotherCrewLink: {error}");
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
        eprintln!("AnotherCrewLink: 1.x's settings could not be read; starting with defaults");
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

    let file = paths.window_state_file();
    let saved = read_state(&file);
    let opening = restore(saved, &displays(), MIN_WIDTH, MIN_HEIGHT);

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
        Box::new(move |_| Ok(Box::new(Client::new(file, &paths)))),
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
    /// What the window was last seen at, for saving on the way out.
    last_seen: Option<WindowState>,
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
    controls: controls::Controls,
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
    /// The last public-lobby listing sent, so it is sent only when it changes.
    ///
    /// The server rate-limits this handler, and the listing is derived from a frame that
    /// arrives five times a second — most of which say the same thing. `Voice.tsx` sends it
    /// on a change too, from a `useEffect`.
    listed: Option<serde_json::Value>,
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
        Self {
            state_file,
            hats: hat_store::Loader::start(paths.hat_cache()),
            portraits: Portraits::default(),
            local_talking: false,
            hearable: std::collections::BTreeSet::new(),
            announced: Announced::default(),
            dead: std::collections::BTreeMap::new(),
            last_game_state: None,
            controls: {
                let saved = settings.config();
                controls::Controls::new(
                    &saved.text_at("muteShortcut"),
                    &saved.text_at("deafenShortcut"),
                    &saved.text_at("pushToTalkShortcut"),
                    &saved.text_at("impostorRadioShortcut"),
                )
            },
            settings,
            link: net::Link::start(),
            audio: audio::Audio::start(capture),
            speaking: std::collections::BTreeSet::new(),
            joined: None,
            mods: None,
            page: Screen::Main,
            catalogue,
            // A reader that will not start is not a reason to refuse to open: the window is
            // where somebody would find out about it, so it opens and says so.
            reader,
            last_seen: None,
            overlay_shown: false,
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
        use acl_core::game_window;

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

        let game = acl_game::windows::find_process("Among Us.exe");
        let bounds = game.and_then(game_window::content_bounds);
        let bounds = bounds.filter(|bounds| bounds.is_drawable());
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
            Self::compose_meeting(&mut self.overlay_shown, reader, &bounds, state);
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
    fn write_window_state(&self) {
        let Some(state) = self.last_seen else {
            return;
        };
        let mut stored = std::fs::read_to_string(&self.state_file)
            .ok()
            .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
            .unwrap_or_default();
        stored.set(acl_ui::window_state::MAIN_WINDOW, state);
        if let Ok(text) = serde_json::to_string(&stored) {
            let _ = std::fs::write(&self.state_file, text);
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
            // Nothing knows who is talking yet, so no seat is marked. The shape is here and
            // the audio is what fills it in; drawing every seat lit would be worse than
            // drawing none, because it would say something false rather than nothing.
            let talking = false;
            if !talking {
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
        for packet in self.link.take_arrived() {
            self.audio.receive(packet);
        }
        // The three switches, before anything is sent. Rebound first, because somebody
        // may have just changed one on the settings page and the next press should use it.
        let mode = {
            let saved = self.settings.config();
            self.controls.rebind(
                &saved.text_at("muteShortcut"),
                &saved.text_at("deafenShortcut"),
                &saved.text_at("pushToTalkShortcut"),
                &saved.text_at("impostorRadioShortcut"),
            );
            // `pushToTalkMode`, which is what the settings screen writes. It used to read
            // `pushToTalk` -- a boolean that nothing in this project has ever written, so it
            // was always false and every client was in voice activity whatever the screen
            // said. Push-to-mute did not exist at all.
            controls::Mode::from_setting(saved.number_at("pushToTalkMode"))
        };
        let switches = self.controls.poll(&acl_core::keys::AsyncKeyState);
        let transmitting = switches.transmitting(mode);

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

        for packet in self.audio.take_encoded() {
            // Drained either way. The encoder runs whatever the switches say, and a queue
            // nobody empties while somebody is muted is a queue that plays their last
            // minute at whoever is listening when they unmute.
            if transmitting {
                self.link.send_audio(packet);
            }
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
                lobby.max_distance,
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
            let Some(gain) = acl_ui::config::after_the_rules(
                self.settings.config(),
                acl_ui::config::Listener {
                    speaker_name_hash: player.name_hash,
                    is_dead: me.is_dead,
                    speaker_is_dead: player.is_dead,
                },
                params.gain,
            ) else {
                // Muted, silenced by the master volume, or turned all the way down.
                // Nothing is placed for them at all: the Electron original leaves the graph
                // alone in that case, and a peer left out of the map is a peer the mixer
                // does not mix -- cheaper than mixing silence.
                //
                // `after_the_rules` returns `None` for every gain at or below zero, so
                // there is no second check after this one. There was until 2026-08-27, and
                // it could not fire.
                continue;
            };

            placements.insert(
                socket.to_owned(),
                audio::Placement {
                    gain,
                    source: acl_audio::panner::Position {
                        x: params.pan.x,
                        y: 0.0,
                        // The game is flat and the listener faces along `z`, so the game's
                        // `y` is the panner's depth. Negative in front, which is the Web
                        // Audio convention the Electron client already works in.
                        z: -params.pan.y,
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
        if self.listed.as_ref() == Some(&lobby) {
            return;
        }
        self.listed = Some(lobby.clone());
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
                // The server the settings name, which is 1.x's `serverURL` -- the same key
                // in the same file, so a player who changed it keeps their change.
                let url = self.settings.config().text_at("serverURL");
                self.link.connect(&url);
            }
            // A subscription belongs to a socket. When the socket goes, so does it.
            net::State::Connecting | net::State::Failed(_) => self.watching_lobbies = false,
            net::State::Connected(_) => {}
        }
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
        match &wanted {
            Some(code) => {
                let me = state
                    .as_ref()
                    .and_then(|state| state.players.iter().find(|player| player.is_local));
                let player_id = me.map_or(-1, |player| i64::from(player.id));
                let client_id = me.and_then(|player| player.client_id).map_or(-1, i64::from);
                let is_host = state.as_ref().is_some_and(|state| state.is_host);
                self.link.join(code, player_id, client_id, is_host);
                // `join` carries it, so a claim on top would be the same statement twice.
                self.announced.host = is_host;
            }
            None => self.link.leave(),
        }
        self.joined = wanted;
    }

    /// Draws the public lobby browser.
    ///
    /// Opening the page is what connects. A session held open for a window nobody is
    /// looking at is a socket the server has to keep, a heartbeat to answer and a
    /// reconnect to attempt, for nothing; the voice pipeline will want one for longer and
    /// will say so when it exists.
    fn show_lobbies(&mut self, ui: &mut egui::Ui) {
        // Before the catalogue is borrowed below, because detecting takes `&mut self`.
        let installed = self.installed_mod();
        let catalogue = self.catalogue.as_ref();
        let translate = move |key: &str| {
            catalogue.map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };

        match self.link.state().clone() {
            net::State::Idle => {
                ui.label("Connecting…");
                return;
            }
            net::State::Connecting => {
                ui.spinner();
                return;
            }
            net::State::Failed(why) => {
                ui.colored_label(egui::Color32::from_rgb(230, 140, 90), why);
                if ui.button("Try again").clicked() {
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
        let context = acl_ui::views::settings::Context {
            t: &translate,
            microphones: &microphones,
            speakers: &speakers,
            locales: &locales,
            // Both false until this process has a session: nobody is host of a lobby it
            // has not joined. Saying so is what puts the "not in a lobby" explanation on
            // the rules rather than leaving them silently dead.
            host_may_change: false,
            in_menu_or_lobby: false,
            capturing: self.settings.capturing(),
        };
        let effects = egui::ScrollArea::vertical()
            .show(ui, |ui| self.settings.show(ui, &context))
            .inner;

        for effect in effects {
            match effect {
                settings_page::Effect::RestoreDefaults => {
                    self.settings.restore_defaults();
                    self.catalogue = load_catalogue(&self.settings);
                }
                settings_page::Effect::LanguageChanged => {
                    self.catalogue = load_catalogue(&self.settings);
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
    fn title_bar(ui: &mut egui::Ui, ctx: &egui::Context, page: &mut Screen) -> bool {
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

        ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.horizontal_centered(|ui| {
                // Everything in one right-to-left row, controls added first so they take
                // their space from the right and the *name* is what gives way. Laid out the
                // other way round -- name first -- a 250-point window pushed the buttons off
                // the end and clipped the title mid-word as well.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕").on_hover_text("Close").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("—").on_hover_text("Minimise").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    // One button rather than two, and it says where it goes rather than
                    // where you are: a gear on the settings page reads as "settings are
                    // here", which is where you already were.
                    let (glyph, hint) = match page {
                        Screen::Main => ("⚙", "Settings"),
                        Screen::Settings | Screen::Lobbies => ("⏴", "Back"),
                    };
                    if ui.button(glyph).on_hover_text(hint).clicked() {
                        *page = match page {
                            Screen::Main => Screen::Settings,
                            Screen::Settings | Screen::Lobbies => Screen::Main,
                        };
                    }
                    if *page == Screen::Main
                        && ui.button("🌐").on_hover_text("Public lobbies").clicked()
                    {
                        *page = Screen::Lobbies;
                    }
                    reload = ui
                        .button("⟳")
                        .on_hover_text("Reload: start the game reader again")
                        .clicked();
                    // The version beside the name, as the shipped client shows it: it is
                    // the first thing anybody is asked for when they report something.
                    ui.label(
                        egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                            .weak()
                            .small(),
                    );
                    // Last, so it fills what is left and truncates there.
                    ui.add(
                        egui::Label::new(egui::RichText::new("AnotherCrewLink").strong())
                            .truncate(),
                    );
                });
            });
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
    /// The lobby's code and what the game is doing, as one line.
    ///
    /// `hideCode` is applied here: `Voice.tsx` line 277 replaces the code with the word
    /// LOBBY rather than blanking it, so somebody streaming shows that there *is* a lobby
    /// without showing which one. The menu keeps its own name — there is no code to give
    /// away, and a menu labelled LOBBY would be a lie rather than a redaction.
    fn lobby_line(&self, state: &acl_game::AmongUsState) -> String {
        let code =
            if self.settings.config().bool_at("hideCode") && state.lobby_code.trim() != "MENU" {
                "LOBBY"
            } else {
                state.lobby_code.as_str()
            };
        format!("{code} — {:?}", state.game_state)
    }

    /// The line of things that are wrong, and the one number that says whether it works.
    ///
    /// Four sources in one strip rather than four panels: none of them is why anybody
    /// opened this window, and each is worth knowing about when it applies. A client with
    /// no microphone is a working client with half its voice, and it is the first thing
    /// somebody asks about when nobody can hear them.
    fn status_strip(&self, ui: &mut egui::Ui, reader: &reader::Reader) {
        const TROUBLE: egui::Color32 = egui::Color32::from_rgb(230, 140, 90);

        ui.horizontal(|ui| {
            ui.label("Game reader:");
            ui.label(egui::RichText::new(format!("{:?}", reader.state())).strong());
            // The server, beside the reader. Both are things that can be down, and only one
            // of them used to be sayable here -- a connection that never happened showed as
            // fifteen crewmates wearing a "no connection" badge and nothing that said why.
            ui.label("· Server:");
            let (word, colour) = match self.link.state() {
                net::State::Connected(_) => ("connected", ui.visuals().text_color()),
                net::State::Connecting => ("connecting…", ui.visuals().weak_text_color()),
                net::State::Idle => ("not connected", TROUBLE),
                net::State::Failed(_) => ("failed", TROUBLE),
            };
            ui.colored_label(colour, egui::RichText::new(word).strong());
            // How many peers are actually reachable, which is a different question from how
            // many players the game reports. A lobby of six with one connection is the shape
            // of a problem, and it is invisible without a number.
            ui.label(format!(
                "· {} peer(s) connected",
                self.link.connected_peers()
            ));
        });
        let link_trouble = match self.link.state() {
            net::State::Failed(why) => Some(why.as_str()),
            _ => None,
        };
        for (what, trouble) in [
            (None, reader.trouble()),
            (Some("Server"), link_trouble),
            (Some("Hats"), self.hats.trouble()),
            (Some("Audio"), self.audio.trouble()),
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
        controls: &controls::Controls,
        connected: bool,
        local_talking: bool,
        dressed: &std::collections::BTreeMap<usize, egui::TextureId>,
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
        );
        ui.separator();
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

        // Remembered every frame and written once, on the way out. The shipped keeper
        // debounces instead, because it is reacting to move and resize events; this is
        // already awake, so there is nothing to debounce.
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
            {
                self.last_seen = Some(WindowState {
                    width: outer.width() as i32,
                    height: outer.height() as i32,
                    x: Some(outer.min.x as i32),
                    y: Some(outer.min.y as i32),
                });
            }
        }

        // Composed on the same cadence the frames arrive at, which is the helper's five a
        // second: there is nothing new to draw between them.
        #[cfg(windows)]
        if let Some(state) = self
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

        // The game state arrives five times a second and nothing else moves, so there is no
        // reason to redraw faster than that.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }

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
            let reload = Self::title_bar(ui, &ctx, &mut page);
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
                // And the audio, which is what applies the three capture settings that can
                // only be given when the device is opened. `Settings.tsx` raises an
                // "unsaved" count for exactly those and asks for a reconnect; this is that
                // reconnect. Dropping the old handle stops its streams.
                self.audio = audio::Audio::start(capture_settings(self.settings.config()));
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

            let Some(reader) = self.reader.as_ref() else {
                ui.label("The game reader could not be started.");
                return;
            };

            self.status_strip(ui, reader);

            ui.separator();
            let Some(state) = reader.latest() else {
                ui.label("No frame yet. Waiting for Among Us.");
                return;
            };

            ui.label(self.lobby_line(state));
            ui.add_space(4.0);

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
            Self::draw_you(
                ui,
                state,
                controls,
                connected_to_server,
                local_talking,
                &dressed,
            );
            acl_ui::views::main::draw(ui, &portraits);
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
