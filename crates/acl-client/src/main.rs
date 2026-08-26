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

mod reader;

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

/// How large one crewmate is in the overlay, and how far apart they sit.
///
/// Fixed rather than scaled to the game window: a 4K screen and a 1080p one want the same
/// physical size, and scaling by the window would make the overlay twice as large on the
/// larger monitor for no reason anybody asked for.
const OVERLAY_SPRITE: i32 = 56;
/// See [`OVERLAY_SPRITE`].
const OVERLAY_GAP: i32 = 8;

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

    eframe::run_native(
        "anothercrewlink",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(move |_| Ok(Box::new(Client::new(file)))),
    )
}

/// Reads the saved geometry, falling back to the older flat file.
///
/// Both, because §4.10 has the 2.0 build read what 1.x wrote — and somebody upgrading from
/// further back than that has only `window-state.json`.
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
    /// Whether the overlay is currently meant to be on screen.
    ///
    /// Kept so that show and hide are sent on the edges rather than every frame: the
    /// helper would act on either correctly, and a command five times a second for a state
    /// that has not changed is a pipe carrying nothing.
    overlay_shown: bool,
}

impl Client {
    fn new(state_file: PathBuf) -> Self {
        Self {
            state_file,
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
        let game = acl_game::windows::find_process("Among Us.exe");
        let bounds = game.and_then(game_window::content_bounds);
        let Some(bounds) = bounds.filter(|bounds| bounds.is_drawable()) else {
            // No game, or nothing to draw over. Hidden rather than left showing the last
            // frame over whatever the player switched to.
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
        let shown = overlay(&seats, &voice, false);

        let sprites: Vec<(i32, i32, acl_ui::sprite::Bitmap)> = shown
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
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
                let at = i32::try_from(slot).unwrap_or(0);
                Some((
                    OVERLAY_GAP + at * (OVERLAY_SPRITE + OVERLAY_GAP),
                    OVERLAY_GAP,
                    bitmap,
                ))
            })
            .collect();

        reader.draw_overlay((bounds.x, bounds.y, bounds.width, bounds.height), sprites);
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

    /// The title bar: a drag handle and the two buttons the shipped window has.
    ///
    /// There is no maximise button, matching `maximizable: false`. Dragging is
    /// `StartDrag`, which hands the move to the window manager rather than repositioning
    /// the window per frame — the difference is visible as smoothness and as whether snap
    /// layouts work at all.
    fn title_bar(ui: &mut egui::Ui, ctx: &egui::Context) {
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
            Self::title_bar(ui, &ctx);
            ui.add_space(TITLE_BAR);
            ui.separator();

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
