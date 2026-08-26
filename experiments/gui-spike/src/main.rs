//! P6 item 1: the framework spike.
//!
//! §4.8 sets the bar and it is deliberately not "hello world": "The spike must produce
//! more than three text controls — a lobby-browser table with sortable columns and one
//! composited animating avatar". Those two were chosen because they are the shapes the
//! React client actually has that a retained-mode toolkit finds easy and an immediate-mode
//! one might not: a table whose rows are reordered by a header click, and a sprite built
//! from several layers that changes every frame.
//!
//! It does not print "looks right". It draws both, every frame, for a fixed number of
//! frames, and reports what they cost:
//!
//! ```text
//! RESULT frames=… rows=64 avatars=12 work_*_ms=… interval_*_ms=…
//! ```
//!
//! **Two numbers, because the obvious one is about the display.** `interval` is frame to
//! frame and is vsync-bound: on a 60 Hz screen it reads 16.7 ms whatever the work costs,
//! which the first version of this file reported on its own and which would have been read
//! as "egui costs 16 ms a frame". `work` is the time spent building the frame, and it is
//! the one with headroom in it.
//!
//! # What the numbers are for
//!
//! §4.8's last milestone is "the GPU fallback chain and the performance baseline the
//! footprint claims are currently asserted without". This is the first half of that
//! baseline, taken before anything is built on top, so that a later regression has
//! something to be a regression against.
//!
//! The p95 matters more than the median. A client that renders in 3 ms and stalls for 40
//! every second is a client that stutters, and a median hides exactly that.
//!
//! # What it does not answer
//!
//! Whether the wgpu rung of the fallback chain behaves the same — that is a separate
//! measurement with a separate feature flag. And whether the window can be transparent and
//! click-through, which `overlay-probe` answered in P1+, before this phase was planned
//! around it.

use std::time::{Duration, Instant};

use acl_ui::lobby_list::{LobbyRow, sort};
use eframe::egui;

/// How many frames to measure over.
///
/// Ten seconds at sixty. Long enough for the first-frame costs — shader compilation, font
/// atlas, texture upload — to stop dominating, short enough to sit through.
const FRAMES: usize = 600;

/// How many rows the table holds.
///
/// The lobby browser shows what the server advertises, and the server's cap is what makes
/// this a number rather than a guess. Sixty-four is comfortably above any real listing,
/// which is the point: a table that is fine at eight rows and repaints in twenty
/// milliseconds at sixty-four has told you nothing at eight.
const ROWS: usize = 64;

/// How many avatars are composited each frame.
///
/// One per player in a full lobby, plus the local one. §4.8 asks for one; a full lobby is
/// what the main view actually draws, and the difference between one and twelve is the
/// difference between "it works" and "it works at the size we need".
const AVATARS: usize = 12;

/// Which column the table is ordered by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortBy {
    /// The ordering the shipped client uses, from `acl_ui::lobby_list`.
    ///
    /// Not one of the columns, and that is the point of having it: the default order is a
    /// three-key rule about joinability, not a field, and a table that could only sort by
    /// a column could not express it.
    Recommended,
    Code,
    Players,
    Map,
}

/// One row, as the table draws it.
///
/// `LobbyRow` carries only what the ordering reads; everything else here is display, which
/// is exactly the split `acl-ui` documents. The spike keeps them side by side rather than
/// widening the model, because widening the model to suit a table is how the ordering
/// stopped being testable in the client this replaces.
struct Row {
    ordering: LobbyRow,
    code: String,
    map: &'static str,
}

/// A plausible listing, deterministically generated.
///
/// Deterministic because a spike that reports a different number every run cannot be
/// compared with itself, and because §4.8 wants a baseline rather than an anecdote.
fn listing() -> Vec<Row> {
    const MAPS: [&str; 5] = ["The Skeld", "Mira HQ", "Polus", "Airship", "Fungle"];
    const ALPHABET: &[u8] = b"QWXRTYLPESDFGHUJKZOCVBINMA";

    (0..ROWS)
        .map(|index| {
            let code: String = (0..6)
                .map(|position| {
                    let at = (index * 7 + position * 13) % ALPHABET.len();
                    char::from(ALPHABET[at])
                })
                .collect();
            let capacity = if index % 3 == 0 { 15 } else { 10 };
            Row {
                ordering: LobbyRow {
                    waiting: index % 5 != 0,
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "index is bounded by ROWS"
                    )]
                    players: ((index * 3) % (capacity as usize + 1)) as u32,
                    capacity,
                },
                code,
                map: MAPS[index % MAPS.len()],
            }
        })
        .collect()
}

struct Spike {
    rows: Vec<Row>,
    sort_by: SortBy,
    started: Instant,
    frame_started: Instant,
    /// End of one frame to the end of the next.
    ///
    /// Vsync-bound, so on a 60 Hz display this reads 16.7 ms whatever the work costs. It
    /// answers "does it keep up" and nothing else, which is why [`Self::work`] exists
    /// beside it.
    intervals: Vec<Duration>,
    /// How long building the frame took.
    ///
    /// The number with headroom in it. The first version of this file measured only the
    /// interval, reported 16.66 ms, and would have been read as "egui costs 16 ms a
    /// frame" -- when what it had measured was the display.
    work: Vec<Duration>,
    reported: bool,
}

impl Spike {
    fn new() -> Self {
        let mut spike = Self {
            rows: listing(),
            sort_by: SortBy::Recommended,
            started: Instant::now(),
            frame_started: Instant::now(),
            intervals: Vec::with_capacity(FRAMES),
            work: Vec::with_capacity(FRAMES),
            reported: false,
        };
        spike.reorder();
        spike
    }

    /// Applies the current ordering.
    ///
    /// The recommended arm goes through `acl_ui::lobby_list::sort`, which is the shipped
    /// rule; the column arms are the spike's own, because per-column sorting is a feature
    /// the lobby browser does not have yet and this is where it would be tried out.
    fn reorder(&mut self) {
        match self.sort_by {
            SortBy::Recommended => {
                // Sorted through the real function, on the real type, so the spike is
                // exercising the model rather than a copy of its behaviour.
                let mut ordering: Vec<LobbyRow> =
                    self.rows.iter().map(|row| row.ordering).collect();
                sort(&mut ordering);
                // Reassembled by matching each sorted entry back to a row. Quadratic, and
                // it does not matter: this runs on a header click over sixty-four rows,
                // and writing an index sort here would be optimising the spike instead of
                // measuring the toolkit.
                let mut taken = vec![false; self.rows.len()];
                let mut reordered = Vec::with_capacity(self.rows.len());
                for wanted in ordering {
                    if let Some(at) = self
                        .rows
                        .iter()
                        .enumerate()
                        .position(|(index, row)| !taken[index] && row.ordering == wanted)
                    {
                        taken[at] = true;
                        reordered.push(at);
                    }
                }
                let mut rows: Vec<Option<Row>> = self.rows.drain(..).map(Some).collect();
                self.rows = reordered
                    .into_iter()
                    .filter_map(|at| rows[at].take())
                    .collect();
            }
            SortBy::Code => self.rows.sort_by(|a, b| a.code.cmp(&b.code)),
            // Descending, so `Reverse` rather than a flipped comparison: clippy is right
            // that a key sort says it better, and the key has to carry the direction.
            SortBy::Players => self
                .rows
                .sort_by_key(|row| std::cmp::Reverse(row.ordering.players)),
            SortBy::Map => self.rows.sort_by(|a, b| a.map.cmp(b.map)),
        }
    }

    /// A crewmate, composited from layers, animating.
    ///
    /// Four layers over a body, which is what the client's own avatars are: a coloured
    /// body, a visor, a hat and a talking ring. Drawn with shapes rather than textures
    /// because the question is whether the toolkit can composite this many translucent
    /// pieces per frame, not whether it can load a PNG.
    fn avatar(&self, painter: &egui::Painter, centre: egui::Pos2, index: usize, phase: f32) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "an index below AVATARS, used only to offset a phase"
        )]
        let own_phase = phase + index as f32 * 0.5;
        // The bob a crewmate has when walking, which is why this animates at all: a static
        // sprite would be a texture test and not a compositing one.
        let bob = own_phase.sin() * 3.0;
        let at = egui::pos2(centre.x, centre.y + bob);
        let hue = (index % 12) as f32 / 12.0;
        // Three phase-shifted sines around the colour wheel, which is a cheap way to get
        // twelve distinguishable body colours without shipping the game's palette into a
        // spike.
        let channel = |offset: f32| -> u8 {
            (127.0f32).mul_add(((hue + offset) * std::f32::consts::TAU).sin(), 128.0) as u8
        };
        let body = egui::Color32::from_rgb(channel(0.0), channel(0.33), channel(0.66));

        // The talking ring, under everything and translucent, because that is the layer
        // that makes this a compositing test rather than four opaque circles.
        let talking = (own_phase * 1.7).sin().mul_add(0.5, 0.5);
        painter.circle_filled(
            at,
            22.0 + talking * 6.0,
            egui::Color32::from_rgba_unmultiplied(120, 220, 140, (60.0 * talking) as u8),
        );
        painter.circle_filled(at, 16.0, body);
        painter.circle_filled(
            egui::pos2(at.x + 5.0, at.y - 4.0),
            7.0,
            egui::Color32::from_rgba_unmultiplied(190, 230, 245, 235),
        );
        painter.circle_filled(
            egui::pos2(at.x, at.y - 14.0),
            8.0,
            egui::Color32::from_rgba_unmultiplied(230, 90, 90, 220),
        );
    }
}

impl eframe::App for Spike {
    /// eframe 0.36 splits the old `update` in two: `logic` runs before the pass and may
    /// not paint, `ui` paints. The repaint request and the stopping condition belong here.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Continuous, not on demand. A spike that only repaints on input measures an idle
        // client, and the main view repaints five times a second at rest and every frame
        // while anybody is talking.
        ctx.request_repaint();

        if self.intervals.len() >= FRAMES && !self.reported {
            self.reported = true;
            self.report();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let building = Instant::now();
        let phase = self.started.elapsed().as_secs_f32() * 3.0;

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Lobby browser");
                ui.separator();
                for (label, by) in [
                    ("Recommended", SortBy::Recommended),
                    ("Code", SortBy::Code),
                    ("Players", SortBy::Players),
                    ("Map", SortBy::Map),
                ] {
                    if ui
                        .selectable_label(self.sort_by == by, label)
                        .on_hover_text("Sort the table by this column")
                        .clicked()
                    {
                        self.sort_by = by;
                        self.reorder();
                    }
                }
            });

            let avatars = ui.available_rect_before_wrap();
            let strip = egui::Rect::from_min_size(avatars.min, egui::vec2(avatars.width(), 60.0));
            let painter = ui.painter_at(strip);
            for index in 0..AVATARS {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "an index below AVATARS, used as a screen offset"
                )]
                let x = strip.min.x + 34.0 + index as f32 * 52.0;
                self.avatar(&painter, egui::pos2(x, strip.center().y), index, phase);
            }
            ui.allocate_space(egui::vec2(strip.width(), 64.0));

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("lobbies")
                    .num_columns(4)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Code");
                        ui.label("Map");
                        ui.label("Players");
                        ui.label("Status");
                        ui.end_row();
                        for row in &self.rows {
                            ui.monospace(&row.code);
                            ui.label(row.map);
                            ui.label(format!(
                                "{}/{}",
                                row.ordering.players, row.ordering.capacity
                            ));
                            ui.label(if !row.ordering.waiting {
                                "in progress"
                            } else if row.ordering.is_full() {
                                "full"
                            } else {
                                "open"
                            });
                            ui.end_row();
                        }
                    });
            });
        });

        // Two numbers, because one of them is about the display rather than the toolkit.
        // `work` is this function: laying out sixty-four rows and compositing twelve
        // avatars. `intervals` is frame to frame, which on a vsync-limited display is the
        // refresh rate and says only whether anything was dropped.
        self.work.push(building.elapsed());
        self.intervals.push(self.frame_started.elapsed());
        self.frame_started = Instant::now();
    }
}

impl Spike {
    fn report(&self) {
        // The first ten frames are dropped, and that is not flattering the number. Shader
        // compilation, the font atlas and the first texture upload all land there and none
        // of them happens again; leaving them in makes the worst case a fact about
        // start-up rather than about drawing.
        let percentile = |samples: &[Duration], fraction: f64| -> f64 {
            let mut sorted: Vec<Duration> = samples.iter().skip(10).copied().collect();
            sorted.sort_unstable();
            let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
            sorted.get(index).copied().unwrap_or_default().as_secs_f64() * 1000.0
        };
        println!(
            "RESULT frames={} rows={} avatars={} work_median_ms={:.2} work_p95_ms={:.2} work_worst_ms={:.2} interval_median_ms={:.2} interval_p95_ms={:.2} interval_worst_ms={:.2}",
            self.intervals.len().saturating_sub(10),
            self.rows.len(),
            AVATARS,
            percentile(&self.work, 0.5),
            percentile(&self.work, 0.95),
            percentile(&self.work, 1.0),
            percentile(&self.intervals, 0.5),
            percentile(&self.intervals, 0.95),
            percentile(&self.intervals, 1.0),
        );
    }
}

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "acl-gui-spike",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 700.0]),
            ..Default::default()
        },
        Box::new(|_| Ok(Box::new(Spike::new()))),
    )
}
