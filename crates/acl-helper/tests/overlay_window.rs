#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! The overlay window, asked about by the operating system rather than looked at.
//!
//! `experiments/overlay-probe` established in P1 that a transparent, click-through,
//! always-on-top window is available on Windows, and it did so by reading the styles back
//! rather than by reporting that something appeared. This does the same for the real
//! component, and for the same reason: a window that is merely invisible looks exactly like
//! one that is transparent, and one that swallows clicks looks exactly like one that does
//! not until somebody tries to play through it.
//!
//! Every assertion here is a property whose absence is a specific failure:
//!
//! * **layered** — `UpdateLayeredWindow` refuses a window without it, so nothing is drawn;
//! * **transparent** — clicks land on the overlay instead of the game;
//! * **topmost** — the overlay is behind the game and never seen;
//! * **tool window** — it appears in the taskbar and in Alt-Tab, which no overlay should;
//! * **no activation** — showing it takes focus, and a fullscreen game minimises.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use acl_helper::overlay::{Frame, Placement, Sprite, WINDOW_TITLE, start};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GWL_EXSTYLE, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

/// One overlay at a time.
///
/// They are found by title, and the title is the product's rather than each test's --
/// deliberately, because the title is what a real caller would search for. Run in parallel,
/// one test finds another's window, and `dropping_it_closes_the_window` fails because
/// somebody else's is still open. Serialising is the honest fix; a per-test title would
/// test a name nothing uses.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serially() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether somebody else's overlay is already on screen.
///
/// These tests find their window by title, because a click-through window with no taskbar
/// button never becomes the foreground one -- and a title is not exclusive. A helper
/// belonging to a client the developer happens to be running owns a window with this exact
/// title, so the test would place *its* overlay, watch it not move, and report a bug in
/// code that is working.
///
/// Skipping is the honest answer: the alternative is a suite that goes red whenever
/// somebody has the thing they are working on open, which is a suite people learn to
/// ignore. Cost one false lead on 2026-08-27.
fn somebody_elses_overlay() -> bool {
    if find().is_some() {
        eprintln!("skipping: an overlay window already exists, so a client is running");
        return true;
    }
    false
}

/// Finds the overlay by title.
///
/// By title, and not `GetForegroundWindow`: a click-through window with no taskbar button
/// never becomes the foreground one, which is how the probe first came to read the
/// console's styles and report them as the overlay's.
fn find() -> Option<HWND> {
    let mut title: Vec<u16> = WINDOW_TITLE.encode_utf16().collect();
    title.push(0);
    // SAFETY: a documented call with a null class and a null-terminated title.
    let handle = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    (!handle.is_null()).then_some(handle)
}

/// Waits for the overlay thread to have created its window.
fn wait_for_window() -> HWND {
    for _ in 0..100 {
        if let Some(handle) = find() {
            return handle;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the overlay never created a window");
}

/// One opaque pixel, so the compositor has something to keep.
fn one_pixel() -> Sprite {
    Sprite {
        x: 0,
        y: 0,
        frame: Frame {
            width: 1,
            height: 1,
            // Premultiplied BGRA: opaque white.
            pixels: vec![255, 255, 255, 255],
        },
    }
}

#[test]
fn the_window_has_every_style_that_makes_it_an_overlay() {
    let _serially = serially();
    if somebody_elses_overlay() {
        return;
    }
    let overlay = start().expect("the overlay thread starts");
    let window = wait_for_window();

    // The extended styles are a 32-bit word; the call returns pointer-sized because it is
    // shared with `GWLP_USERDATA` and friends, so the high half is nothing.
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "a 32-bit style word returned in a pointer-sized, signed slot"
    )]
    let styles = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;

    for (bit, name, consequence) in [
        (
            WS_EX_LAYERED,
            "layered",
            "UpdateLayeredWindow draws nothing",
        ),
        (
            WS_EX_TRANSPARENT,
            "transparent",
            "clicks land on the overlay instead of the game",
        ),
        (WS_EX_TOPMOST, "topmost", "it sits behind the game"),
        (
            WS_EX_TOOLWINDOW,
            "tool window",
            "it appears in the taskbar and in Alt-Tab",
        ),
        (
            WS_EX_NOACTIVATE,
            "no activation",
            "showing it takes focus and minimises a fullscreen game",
        ),
    ] {
        assert!(
            styles & bit != 0,
            "the overlay is not {name}: {consequence} (exstyle 0x{styles:08x})"
        );
    }

    drop(overlay);
}

#[test]
fn it_goes_where_it_is_put_and_hides_when_told() {
    let _serially = serially();
    if somebody_elses_overlay() {
        return;
    }
    let overlay = start().expect("the overlay thread starts");
    let window = wait_for_window();

    // `UpdateLayeredWindow` sets the position, the size and the pixels together, so the
    // frame is what actually moves it -- `SetWindowPos` alone leaves a layered window
    // showing its previous bitmap at its previous size.
    let placement = Placement {
        x: 120,
        y: 80,
        width: 320,
        height: 200,
    };
    overlay.place(placement);
    overlay.clear();
    overlay.blit(one_pixel());
    overlay.present();
    overlay.show(true);

    let rect = wait_for(window, |rect| {
        rect.left == placement.x
            && rect.top == placement.y
            && rect.right - rect.left == placement.width
            && rect.bottom - rect.top == placement.height
    });
    assert_eq!(
        (
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top
        ),
        (placement.x, placement.y, placement.width, placement.height)
    );
    assert!(
        wait_until(|| unsafe { IsWindowVisible(window) } != 0),
        "it never became visible"
    );

    overlay.show(false);
    assert!(
        wait_until(|| unsafe { IsWindowVisible(window) } == 0),
        "it did not hide when told"
    );

    drop(overlay);
}

/// Dropping the handle takes the window with it.
///
/// An overlay left on screen after the client has gone is the failure a user cannot fix
/// without Task Manager, and this window has no close button, no taskbar entry and no way
/// to be focused.
#[test]
fn dropping_it_closes_the_window() {
    let _serially = serially();
    if somebody_elses_overlay() {
        return;
    }
    {
        let overlay = start().expect("the overlay thread starts");
        wait_for_window();
        overlay.show(true);
    }
    for _ in 0..100 {
        if find().is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the window outlived the overlay that owned it");
}

/// Polls a condition until it holds, or gives up.
///
/// Every command crosses a channel and is applied by another thread, so nothing here is
/// true the instant the call returns.
///
/// # Safety
///
/// The closures passed here call Win32 functions on a handle the caller keeps valid for
/// the whole test.
fn wait_until(mut holds: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if holds() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Polls the window's rectangle until it matches, or gives up.
///
/// The commands cross a channel and are applied by another thread, so the change is not
/// there the instant `place` returns.
fn wait_for(window: HWND, mut matches: impl FnMut(RECT) -> bool) -> RECT {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    for _ in 0..200 {
        // SAFETY: a valid window handle and a live local for the answer.
        unsafe { GetWindowRect(window, &raw mut rect) };
        if matches(rect) {
            return rect;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    rect
}
