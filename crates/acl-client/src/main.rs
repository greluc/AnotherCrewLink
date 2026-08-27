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
        // Nothing to composite. The view's own drawing is better than a texture of the same
        // shapes: it is one draw call rather than an upload, and it recolours for free.
        if pieces.len() == 1 {
            return None;
        }
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
                let base = acl_ui::sprite::crewmate(
                    PORTRAIT_SPRITE,
                    acl_ui::sprite::Crewmate {
                        body: (body.r(), body.g(), body.b()),
                        shadow: (shadow.r(), shadow.g(), shadow.b()),
                        talking: false,
                        alive: true,
                    },
                );
                canvas.composite(&base, (0, 0), (PORTRAIT_SPRITE, PORTRAIT_SPRITE));
                continue;
            };
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

        let handle = context.load_texture(
            format!("portrait-{}-{}", appearance.colour, appearance.hat),
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

/// How large a main-window crewmate is rasterised.
///
/// Larger than the 52 points it is drawn at, so it survives a high-DPI display without
/// looking soft. Not larger still: it is composited on the CPU and uploaded, and the cost of
/// both is the square of this.
const PORTRAIT_SPRITE: i32 = 128;

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
            eprintln!("AnotherCrewLink: {}", occupant.message());
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
        // Frameless, because the title bar is drawn below.
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
    /// Whether the server has been told this client is the host.
    ///
    /// Kept so the claim is made on the *transition*. `setHost` every frame would be a
    /// message a second to a server that already agrees.
    claimed_host: bool,
    /// Who the voice layer believes is dead, by client id.
    ///
    /// Not the game's `is_dead`, and the difference is the whole point. See
    /// [`Self::follow_deaths`].
    dead: std::collections::BTreeMap<i64, bool>,
    /// The game state the death map was last updated for.
    last_game_state: Option<acl_game::GameState>,
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
}

impl Client {
    fn new(state_file: PathBuf, paths: &Paths) -> Self {
        let settings = settings_page::Page::open(paths.config_file());
        let catalogue = load_catalogue(&settings);
        Self {
            state_file,
            hats: hat_store::Loader::start(paths.hat_cache()),
            portraits: Portraits::default(),
            local_talking: false,
            hearable: std::collections::BTreeSet::new(),
            claimed_host: false,
            dead: std::collections::BTreeMap::new(),
            last_game_state: None,
            settings,
            link: net::Link::start(),
            audio: audio::Audio::start(),
            speaking: std::collections::BTreeSet::new(),
            joined: None,
            mods: None,
            page: Screen::Main,
            catalogue,
            // A reader that will not start is not a reason to refuse to open: the window is
            // where somebody would find out about it, so it opens and says so.
            reader: reader::Reader::start().ok(),
            last_seen: None,
            overlay_shown: false,
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
            impostor_radio: None,
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
                let bitmap = acl_ui::sprite::crewmate(
                    OVERLAY_SPRITE,
                    acl_ui::sprite::Crewmate {
                        body: (body.r(), body.g(), body.b()),
                        shadow: (shadow.r(), shadow.g(), shadow.b()),
                        talking: entry.talking,
                        alive: entry.alive,
                    },
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
            let bitmap = acl_ui::sprite::crewmate(
                side,
                acl_ui::sprite::Crewmate {
                    body: (body.r(), body.g(), body.b()),
                    shadow: (shadow.r(), shadow.g(), shadow.b()),
                    talking: true,
                    alive: !player.is_dead,
                },
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
        for packet in self.audio.take_encoded() {
            self.link.send_audio(packet);
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
                None,
            );
            // Per-player volume and mute. `voice_params` deliberately does not know about
            // them, because `Voice.tsx` applies them outside `calculateVoiceAudio` too --
            // the rule and the reason it is keyed on the name hash are in
            // `acl_ui::config::per_player_gain`, which is tested without a game.
            let Some(gain) = acl_ui::config::per_player_gain(
                self.settings.config(),
                player.name_hash,
                params.gain,
            ) else {
                // Muted, so nothing is placed for them at all -- cheaper than mixing
                // silence, and the same thing `gain <= 0` does below.
                continue;
            };

            if gain <= 0.0 {
                // Silent, and `placed` false means the panner was not given a position
                // either -- the Electron original leaves the graph alone in that case, and
                // a peer left out of the map is a peer the mixer does not mix.
                continue;
            }
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
            if host_now && !self.claimed_host {
                let client_id = state
                    .as_ref()
                    .and_then(|state| state.players.iter().find(|player| player.is_local))
                    .and_then(|player| player.client_id)
                    .map_or(-1, i64::from);
                if client_id >= 0 {
                    self.link.say_host(client_id);
                    self.claimed_host = true;
                }
            } else if !host_now {
                // Reset, so a second promotion in the same session is claimed again.
                self.claimed_host = false;
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
                self.claimed_host = is_host;
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
                // The server the settings name, which is 1.x's `serverURL` -- the same key
                // in the same file, so a player who changed it keeps their change.
                let url = self.settings.config().text_at("serverURL");
                ui.label(format!("Connecting to {url}…"));
                self.link.connect(&url);
                self.link.watch_lobbies(true);
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
            net::State::Connected(_) => {}
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
            // Nothing but the body: leave it alone rather than copying it through a blank
            // canvas for no reason. This is the common case -- most players wear nothing.
            if pieces.len() == 1 {
                continue;
            }

            let mut canvas = acl_ui::sprite::Bitmap::blank(body.width, body.height);
            for piece in &pieces {
                let Some(url) = piece.url.as_deref() else {
                    // The body, at its own size and origin.
                    canvas.composite(body, (0, 0), (body.width, body.height));
                    continue;
                };
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
            *body = canvas;
        }
    }

    /// Draws the settings, and does whatever they asked for.
    ///
    /// The device lists are empty until the audio pipeline lands in this process: a
    /// picker with nothing in it still shows what is stored, which is the truth about a
    /// client that is not yet listening to anything. Wiring it to `acl_audio::device`
    /// belongs with the pipeline that will use the answer.
    fn show_settings(&mut self, ui: &mut egui::Ui) {
        let catalogue = self.catalogue.as_ref();
        let translate = move |key: &str| {
            catalogue.map_or_else(|| key.to_owned(), |catalogue| catalogue.t(key).to_owned())
        };
        let locales = settings_page::locales();
        let context = acl_ui::views::settings::Context {
            t: &translate,
            microphones: &[],
            speakers: &[],
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
    fn title_bar(ui: &mut egui::Ui, ctx: &egui::Context, page: &mut Screen) {
        let bar = egui::Rect::from_min_size(
            ui.max_rect().min,
            egui::vec2(ui.max_rect().width(), TITLE_BAR),
        );
        let response = ui.interact(
            bar,
            ui.id().with("title-bar"),
            egui::Sense::click_and_drag(),
        );
        if response.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("AnotherCrewLink").strong());
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
                });
            });
        });
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
        self.follow_the_lobby();
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
        let dressed = self.before_painting(&ctx);
        egui::CentralPanel::default().show(ui, |ui| {
            let mut page = self.page;
            Self::title_bar(ui, &ctx, &mut page);
            self.page = page;
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

            ui.horizontal(|ui| {
                ui.label("Game reader:");
                ui.label(egui::RichText::new(format!("{:?}", reader.state())).strong());
                // How many peers are actually reachable, which is a different question from
                // how many players the game reports. A lobby of six with one connection is
                // the shape of a problem, and it is invisible without a number.
                ui.label(format!(
                    "· {} peer(s) connected",
                    self.link.connected_peers()
                ));
            });
            if let Some(trouble) = reader.trouble() {
                ui.colored_label(egui::Color32::from_rgb(230, 140, 90), trouble);
            }
            // The artwork is not why anybody opened this window, so it says so in the same
            // place and the same colour rather than in one of its own: a hat that did not
            // arrive is worth knowing about and is not worth a second panel.
            if let Some(trouble) = self.hats.trouble() {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 140, 90),
                    format!("Hats: {trouble}"),
                );
            }
            // No microphone or no speaker is a working client with half its voice, and it
            // is the first thing somebody will ask about when nobody can hear them.
            if let Some(trouble) = self.audio.trouble() {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 140, 90),
                    format!("Audio: {trouble}"),
                );
            }

            ui.horizontal(|ui| {
                if ui.button("Start").clicked() {
                    reader.ask_to_start();
                }
                if ui.button("Stop").clicked() {
                    reader.ask_to_stop();
                }
            });

            ui.separator();
            let Some(state) = reader.latest() else {
                ui.label("No frame yet. Start the reader with Among Us running.");
                return;
            };

            ui.label(format!("{} — {:?}", state.lobby_code, state.game_state));
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
                impostor_radio: None,
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
            acl_ui::views::main::draw(ui, &portraits);
        });
    }
}
