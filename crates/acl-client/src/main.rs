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

/// How large one crewmate is in the overlay.
///
/// How far apart they sit is `acl_ui::overlay_layout::GAP`, which is also what the strip
/// is sized from -- one number, in the module that does the arithmetic.
///
/// Fixed rather than scaled to the game window: a 4K screen and a 1080p one want the same
/// physical size, and scaling by the window would make the overlay twice as large on the
/// larger monitor for no reason anybody asked for.
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
    #[cfg(windows)]
    let _instance = match single_instance::claim(paths.user_data()) {
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
    /// The settings, and everything that happens to them.
    settings: settings_page::Page,
    /// The signalling session, on a thread of its own.
    link: net::Link,
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
            settings,
            link: net::Link::start(),
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
        // `meetingOverlay` is read by nothing yet. It switches on a second overlay that
        // draws the players over the meeting table, which needs the meeting hud's
        // on-screen geometry out of the game -- a reader question rather than a drawing
        // one, and not one this has an answer to. The setting is kept and honoured as soon
        // as there is something for it to switch on.
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

        let voice = Voice {
            talking: &|_| false,
            dead: &|_| false,
            connected: &|_| true,
            audible: &|_| false,
            local_talking: false,
            local_alive: !state.players.iter().any(|p| p.is_local && p.is_dead),
            impostor_radio: None,
            local_is_impostor: false,
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

        // Which crewmate wears what, collected while the sprites are built and applied
        // afterwards: the artwork lives behind `&mut self` and the closure below is
        // already borrowing the reader's state.
        let mut wearing: Vec<(usize, String, String)> = Vec::new();
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
                wearing.push((at, player.hat_id.clone(), player.visor_id.clone()));
            }
        }
        Self::dress(&mut self.hats, &mut sprites, &wearing);

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

    /// Puts each crewmate's cosmetics on their sprite.
    ///
    /// A hat that has not been fetched yet is simply not drawn: `Loader::image` asks for it
    /// and answers `None`, and a later frame has it. A cosmetic arriving a frame late is not
    /// worth a stalled window.
    ///
    /// The order is [`acl_ui::cosmetics::PAINT_ORDER`]'s, minus the layers this does not
    /// have yet -- the back of a hat goes behind the player, which needs the base sprite
    /// split into two passes, and the skin sits between them.
    ///
    /// Takes the loader rather than `&self` so that the reader's borrow and the artwork's
    /// can coexist: they are disjoint fields, and the borrow checker only knows that when
    /// they are named separately.
    #[cfg(windows)]
    fn dress(
        hats: &mut hat_store::Loader,
        sprites: &mut [(i32, i32, acl_ui::sprite::Bitmap)],
        wearing: &[(usize, String, String)],
    ) {
        for (at, hat, visor) in wearing {
            let Some((_, _, canvas)) = sprites.get_mut(*at) else {
                continue;
            };
            for id in [visor, hat] {
                let Some(found) = hats.collection().find(id, acl_ui::hats::BASE) else {
                    continue;
                };
                let geometry = found.geometry;
                let Some(url) = found.image_url(acl_types::cosmetics::HAT_COLLECTION_URL, false)
                else {
                    continue;
                };
                let Some(artwork) = hats.image(&url) else {
                    continue;
                };
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_precision_loss,
                    reason = "sprite and artwork dimensions in pixels, both far below f32's                               exact integer range"
                )]
                let rect = {
                    let size = OVERLAY_SPRITE as f32;
                    let width = geometry.width * size;
                    (
                        (geometry.left * size) as i32,
                        (geometry.top * size) as i32,
                        width as i32,
                        // The artwork is square-ish and the geometry gives only a width, so
                        // the height follows the file's own proportions -- which is what
                        // `width` alone means in the stylesheet this is ported from.
                        (width * artwork.height as f32 / artwork.width.max(1) as f32) as i32,
                    )
                };
                let artwork = artwork.clone();
                canvas.composite(&artwork, (rect.0, rect.1), (rect.2, rect.3));
            }
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

impl eframe::App for Client {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(reader) = self.reader.as_mut() {
            reader.pump();
        }
        self.hats.pump();
        self.link.pump();

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
            let voice = Voice {
                talking: &|_| false,
                dead: &|_| false,
                connected: &|_| true,
                audible: &|_| false,
                local_talking: false,
                local_alive: !state.players.iter().any(|p| p.is_local && p.is_dead),
                impostor_radio: None,
                local_is_impostor: false,
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
                    })
                })
                .collect();
            acl_ui::views::main::draw(ui, &portraits);
        });
    }
}
