//! P1+ experiment 1: a transparent, click-through, always-on-top window.
//!
//! The plan calls this out as something that must be answered before the GUI phase is
//! planned around it, not during it: eframe's transparency has renderer-specific
//! failures, and the known workarounds are mutually exclusive with a single-process
//! design. Today the overlay is its own Electron `BrowserWindow`, so whatever replaces it
//! has to do the same three things at once.
//!
//! It does not print "looks right". It asks the OS what it actually did: mouse
//! passthrough is `WS_EX_TRANSPARENT | WS_EX_LAYERED` on the window's extended style, and
//! if those bits are missing the window swallows clicks meant for the game however
//! transparent it looks. The answer was `layered=true transparent=true topmost=true`,
//! `exstyle=0x000c0138`.
//!
//! There was a second arm here until 2026-08-25, run under `xvfb` with `llvmpipe`. It
//! could only report `RESULT arch=linux windowed=true` — reading an X11 input region back
//! needs an X connection of the probe's own — and it went with the client's Linux
//! support, along with the CI job that ran it.

use std::time::{Duration, Instant};

use eframe::egui;

/// How long the window stays up. Long enough for a compositor to settle, short enough not
/// to sit on top of someone's game.
const LIFETIME: Duration = Duration::from_millis(2500);

struct Experiment {
    started: Instant,
    reported: bool,
}

impl eframe::App for Experiment {
    /// Fully transparent. A window that reports itself transparent but clears to opaque
    /// black is the failure this experiment is looking for.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    /// eframe 0.36 splits the old `update` in two: `logic` runs before the pass and may
    /// not paint, `ui` paints. The viewport commands belong in `logic`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.reported {
            self.reported = true;
            report(ctx);
        }
        if self.started.elapsed() > LIFETIME {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Transparency and passthrough are compositor state, not paint state; keep
        // repainting so a failure that only appears after the first frame still appears.
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 96, 255),
                    "ACL overlay experiment",
                );
            });
    }
}

fn report(_ctx: &egui::Context) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GWL_EXSTYLE, GetWindowLongPtrW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    };

    // By title, not `GetForegroundWindow`. A click-through window with no taskbar button
    // never becomes the foreground one, so the first version of this experiment was
    // reading the console's styles and reporting them as the overlay's.
    let mut title: Vec<u16> = "acl-overlay-experiment".encode_utf16().collect();
    title.push(0);
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if hwnd.is_null() {
        println!("RESULT hwnd=none");
        return;
    }
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    println!(
        "RESULT arch={} layered={} transparent={} topmost={} toolwindow={} exstyle=0x{:08x}",
        if cfg!(target_pointer_width = "32") {
            "i686"
        } else {
            "x86_64"
        },
        ex & WS_EX_LAYERED != 0,
        ex & WS_EX_TRANSPARENT != 0,
        ex & WS_EX_TOPMOST != 0,
        ex & WS_EX_TOOLWINDOW != 0,
        ex
    );
}

fn main() -> eframe::Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([320.0, 96.0])
        // Away from the middle of the screen, so it does not sit over a game.
        .with_position([48.0, 48.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_mouse_passthrough(true)
        .with_taskbar(false);

    eframe::run_native(
        "acl-overlay-experiment",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|_cc| {
            Ok(Box::new(Experiment {
                started: Instant::now(),
                reported: false,
            }))
        }),
    )
}
