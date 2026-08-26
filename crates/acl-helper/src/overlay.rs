//! The overlay window: a layered window, updated from a bitmap, with no toolkit under it.
//!
//! §4.7 puts the overlay in this process because UIPI blocks window manipulation across
//! integrity levels, so an unelevated overlay stops following an elevated game — the exact
//! configuration the README instructs users into.
//!
//! # Why there is no GUI framework here
//!
//! §6's checklist says what this process is allowed to be: "no listening socket, no HTTP
//! client, no image decoder and no GPU context". An overlay drawn with a GUI toolkit has
//! the last of those, and a GPU context is a driver in an elevated address space — which
//! is also the availability argument that put the overlay in its own `BrowserWindow`
//! today, since a driver fault there does not drop the call.
//!
//! The rest of the documents already describe the alternative without naming it as one:
//! §3.3 calls it a *layered window* throughout, and §4.7 says it "receives pre-rasterised
//! sprites over the IPC and never fetches or decodes an image". A layered window updated
//! with `UpdateLayeredWindow` from a premultiplied bitmap needs no toolkit, no renderer and
//! no GPU, and pre-rasterised is exactly what it wants.
//!
//! `experiments/overlay-probe` uses eframe, and that is not a contradiction — it answered
//! whether such a window is available at all, in P1, using the framework candidate that
//! was to hand. Its answer transfers; its implementation does not.
//!
//! # The thread
//!
//! A window belongs to the thread that created it: messages are delivered there and
//! nowhere else. So the overlay owns one, with its own message loop, and everything else
//! reaches it through a channel. The loop polls rather than blocking in `GetMessage`,
//! because it has two things to wait on — the queue and the channel — and polling two
//! cheap calls at the rate an overlay redraws anyway is smaller than the handshake needed
//! to wake a blocking loop from outside.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;

/// Where the overlay should be, in screen coordinates.
///
/// The same four numbers `acl-core::game_window::Bounds` carries. They are repeated rather
/// than shared because that type lives in the unelevated half and this process does not
/// depend on it; what crosses the pipe is four integers either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    /// Screen x of the top-left corner.
    pub x: i32,
    /// Screen y of the top-left corner.
    pub y: i32,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}

/// One frame to show, already rasterised.
///
/// **Premultiplied BGRA, bottom-up is not required** — the bitmap is created with a
/// negative height so the rows are top-down, which is the order everything that produces
/// pixels uses.
///
/// Premultiplied because `UpdateLayeredWindow` with `AC_SRC_ALPHA` requires it and does not
/// check: straight alpha produces a picture that looks approximately right and has bright
/// fringes wherever anything is partly transparent, which is every antialiased edge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
    /// `width * height * 4` bytes, B, G, R, A, premultiplied.
    pub pixels: Vec<u8>,
}

impl Frame {
    /// A fully transparent frame of a given size.
    #[must_use]
    pub fn blank(width: i32, height: i32) -> Self {
        let count = usize::try_from(width.max(0))
            .unwrap_or(0)
            .saturating_mul(usize::try_from(height.max(0)).unwrap_or(0))
            .saturating_mul(4);
        Self {
            width,
            height,
            pixels: vec![0; count],
        }
    }

    /// Whether the buffer is the size the dimensions claim.
    ///
    /// Checked before it is handed to the kernel: `UpdateLayeredWindow` reads
    /// `width * height * 4` bytes from the bitmap it is given, and a short buffer is not a
    /// wrong picture but a read past the end of an allocation.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        let expected = usize::try_from(self.width.max(0))
            .unwrap_or(0)
            .saturating_mul(usize::try_from(self.height.max(0)).unwrap_or(0))
            .saturating_mul(4);
        self.width > 0 && self.height > 0 && self.pixels.len() == expected
    }
}

/// One pre-rasterised sprite and where it goes on the canvas.
///
/// Premultiplied BGRA, top row first, the same as [`Frame`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sprite {
    /// Its left edge, relative to the overlay's own top-left.
    pub x: i32,
    /// Its top edge.
    pub y: i32,
    /// The picture.
    pub frame: Frame,
}

impl Frame {
    /// Blends one sprite into this frame, source-over.
    ///
    /// Premultiplied on both sides, so the blend is `dst = src + dst * (1 - src.a)` per
    /// channel with no division by alpha anywhere — which is the whole reason premultiplied
    /// is the format this pipe carries. Straight alpha would need a divide per pixel and
    /// would produce bright fringes on every antialiased edge if anybody forgot one.
    ///
    /// Clipped rather than wrapped. A sprite partly off the edge is ordinary — an avatar at
    /// the corner of the screen is exactly that — and wrapping would draw its remainder on
    /// the opposite side of the overlay.
    pub fn blend(&mut self, sprite: &Sprite) {
        if !sprite.frame.is_consistent() || !self.is_consistent() {
            return;
        }
        let (width, height) = (self.width, self.height);
        for row in 0..sprite.frame.height {
            let target_y = sprite.y + row;
            if target_y < 0 || target_y >= height {
                continue;
            }
            for column in 0..sprite.frame.width {
                let target_x = sprite.x + column;
                if target_x < 0 || target_x >= width {
                    continue;
                }
                let from = usize::try_from((row * sprite.frame.width + column) * 4).unwrap_or(0);
                let into = usize::try_from((target_y * width + target_x) * 4).unwrap_or(0);
                // As an array rather than indexed four times: the slice is four bytes by
                // construction, and saying so once beats four bounds checks the compiler
                // cannot see are the same one.
                let Some(source) = sprite
                    .frame
                    .pixels
                    .get(from..from + 4)
                    .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                else {
                    continue;
                };
                let Some(destination) = self.pixels.get_mut(into..into + 4) else {
                    continue;
                };
                let inverse = u32::from(255 - source[3]);
                for (channel, byte) in destination.iter_mut().enumerate() {
                    let Some(over) = source.get(channel).copied() else {
                        continue;
                    };
                    // Rounded rather than truncated: repeated blends of the same
                    // translucent sprite otherwise drift darker one step at a time.
                    let kept = (u32::from(*byte) * inverse + 127) / 255;
                    *byte = u8::try_from(u32::from(over) + kept).unwrap_or(u8::MAX);
                }
            }
        }
    }
}

/// What the overlay thread is asked to do.
#[derive(Clone, Debug)]
enum Command {
    Place(Placement),
    Clear,
    Blit(Sprite),
    Present,
    Show(bool),
    Stop,
}

/// A running overlay window.
#[derive(Debug)]
pub struct Overlay {
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Overlay {
    /// Moves and resizes it.
    pub fn place(&self, placement: Placement) {
        let _ = self.commands.send(Command::Place(placement));
    }

    /// Wipes the canvas to transparent. The start of a frame.
    pub fn clear(&self) {
        let _ = self.commands.send(Command::Clear);
    }

    /// Blends one sprite into the canvas.
    ///
    /// An inconsistent sprite is dropped rather than drawn: see [`Frame::is_consistent`]
    /// for why that is not merely tidiness.
    pub fn blit(&self, sprite: Sprite) {
        if sprite.frame.is_consistent() {
            let _ = self.commands.send(Command::Blit(sprite));
        }
    }

    /// Puts the canvas on the screen. The end of a frame.
    ///
    /// Separate from the sprites so a frame appears at once: presenting after each would
    /// show the overlay half-composed, which on a talking indicator is a flicker every
    /// time somebody speaks.
    pub fn present(&self) {
        let _ = self.commands.send(Command::Present);
    }

    /// Shows or hides it.
    pub fn show(&self, visible: bool) {
        let _ = self.commands.send(Command::Show(visible));
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The window class, and the title a test finds it by.
///
/// Distinctive on purpose. `overlay-probe` looked its own window up by title because a
/// click-through window with no taskbar button never becomes the foreground one, and the
/// same is true here.
pub const WINDOW_CLASS: &str = "AnotherCrewLinkOverlay";

/// The window's title.
pub const WINDOW_TITLE: &str = "AnotherCrewLink overlay";

#[cfg(windows)]
mod platform {
    use super::{
        Command, Frame, Overlay, Placement, Receiver, RecvTimeoutError, Sprite, WINDOW_CLASS,
        WINDOW_TITLE, channel,
    };
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
        CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
        HBITMAP, ReleaseDC, SelectObject,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
        PeekMessageW, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER,
        SetWindowPos, ShowWindow, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSEXW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        WS_POPUP,
    };

    /// How often the loop looks at its queue and its channel while visible.
    ///
    /// Sixty a second, which is the rate an overlay redraws at anyway. Two cheap calls per
    /// wake.
    const AWAKE: Duration = Duration::from_millis(16);

    /// And while hidden. Nothing is being drawn, so the only thing to notice is a command.
    const IDLE: Duration = Duration::from_millis(100);

    /// A null-terminated UTF-16 copy.
    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Does nothing, on purpose.
    ///
    /// The window has no input to handle: `WS_EX_TRANSPARENT` means the mouse never
    /// reaches it, and it never takes focus. Everything it shows arrives through
    /// `UpdateLayeredWindow` rather than through `WM_PAINT`, so there is no paint handler
    /// either -- a layered window is composited from the bitmap it was last given.
    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> LRESULT {
        // SAFETY: the documented default handler, with the arguments as received.
        unsafe { DefWindowProcW(window, message, w, l) }
    }

    /// Everything the overlay thread owns.
    struct Window {
        handle: HWND,
        placement: Placement,
        /// What the next `present` will show.
        ///
        /// Composed here rather than sent whole, because a whole picture does not fit the
        /// pipe: `acl_ipc::MAX_FRAME` is 64 KiB and a 2560x1440 overlay is 14.7 MB. §4.7
        /// says sprites for that reason, and this is where they become a frame.
        canvas: Frame,
    }

    impl Window {
        /// Creates the window, hidden.
        ///
        /// The extended styles are the whole point of it, and each one is load-bearing.
        /// `LAYERED` is what `UpdateLayeredWindow` requires; `TRANSPARENT` is what makes
        /// clicks pass through to the game rather than being swallowed by an invisible
        /// window; `TOPMOST` keeps it above the game; `TOOLWINDOW` keeps it out of the
        /// taskbar and out of Alt-Tab; `NOACTIVATE` stops it from ever taking focus, which
        /// would minimise a fullscreen game.
        fn create() -> Option<Self> {
            let class = wide(WINDOW_CLASS);
            let title = wide(WINDOW_TITLE);
            // SAFETY: a documented call with a null argument, which asks for this module.
            let instance = unsafe { GetModuleHandleW(ptr::null()) };

            let descriptor = WNDCLASSEXW {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the struct's own field is u32 and the struct is far smaller"
                )]
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: ptr::null_mut(),
                hCursor: ptr::null_mut(),
                hbrBackground: ptr::null_mut(),
                lpszMenuName: ptr::null(),
                lpszClassName: class.as_ptr(),
                hIconSm: ptr::null_mut(),
            };
            // SAFETY: every pointer in the descriptor outlives the call. Registering a
            // class that already exists fails, which is not an error here: a second overlay
            // in one process would reuse it, and `CreateWindowExW` below is what actually
            // reports a problem.
            unsafe { RegisterClassExW(&raw const descriptor) };

            // SAFETY: the class and title are null-terminated and outlive the call; every
            // other argument is a documented constant or null.
            let handle = unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOPMOST
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE,
                    class.as_ptr(),
                    title.as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    instance,
                    ptr::null(),
                )
            };
            (!handle.is_null()).then_some(Self {
                handle,
                placement: Placement::default(),
                canvas: Frame::default(),
            })
        }

        fn place(&mut self, placement: Placement) {
            self.placement = placement;
            // Resized with the window, and wiped by the resize: a canvas of the old size
            // blitted into a window of the new one is a picture stretched or clipped, and
            // the caller is about to redraw anyway.
            self.canvas = Frame::blank(placement.width, placement.height);
            // SAFETY: a valid window handle; `NOACTIVATE` because taking focus would
            // minimise a fullscreen game, and `NOZORDER` because the topmost style already
            // decides where it sits.
            unsafe {
                SetWindowPos(
                    self.handle,
                    ptr::null_mut(),
                    placement.x,
                    placement.y,
                    placement.width,
                    placement.height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }

        fn show(&self, visible: bool) {
            // SAFETY: a valid window handle and a documented constant. `SHOWNOACTIVATE`
            // for the same reason `SWP_NOACTIVATE` is used above.
            unsafe {
                ShowWindow(
                    self.handle,
                    if visible { SW_SHOWNOACTIVATE } else { SW_HIDE },
                );
            }
        }

        /// Wipes the canvas.
        fn clear(&mut self) {
            self.canvas = Frame::blank(self.placement.width, self.placement.height);
        }

        /// Blends one sprite into the canvas.
        fn blit(&mut self, sprite: &Sprite) {
            self.canvas.blend(sprite);
        }

        /// Puts the canvas on the screen.
        fn present(&self) -> bool {
            self.draw(&self.canvas)
        }

        /// Puts a bitmap on the screen.
        ///
        /// One call does the resize, the position and the pixels together, which is what
        /// `UpdateLayeredWindow` is for: setting them separately makes a frame in which the
        /// window has the new size and the old picture, and on a moving window that is
        /// visible as a tear.
        fn draw(&self, frame: &Frame) -> bool {
            if !frame.is_consistent() {
                return false;
            }
            // SAFETY: a documented call with a null argument, which asks for the screen.
            let screen = unsafe { GetDC(ptr::null_mut()) };
            if screen.is_null() {
                return false;
            }
            // SAFETY: a valid device context from the call above.
            let memory = unsafe { CreateCompatibleDC(screen) };
            if memory.is_null() {
                // SAFETY: the handle came from GetDC and is released once.
                unsafe { ReleaseDC(ptr::null_mut(), screen) };
                return false;
            }

            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "the struct's own field is u32 and the struct is far smaller"
                    )]
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: frame.width,
                    // Negative, so the rows are top-down. A positive height means the
                    // first row in the buffer is the bottom of the picture, which is the
                    // convention nothing that produces pixels uses and the reason an
                    // overlay comes out upside down.
                    biHeight: -frame.height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [unsafe { std::mem::zeroed() }],
            };

            let mut bits: *mut core::ffi::c_void = ptr::null_mut();
            // SAFETY: the info describes the bitmap and `bits` receives the pointer the
            // kernel allocated for it.
            let bitmap: HBITMAP = unsafe {
                CreateDIBSection(
                    memory,
                    &raw const info,
                    DIB_RGB_COLORS,
                    &raw mut bits,
                    ptr::null_mut(),
                    0,
                )
            };
            let drawn = if bitmap.is_null() || bits.is_null() {
                false
            } else {
                // SAFETY: `bits` points at a buffer the kernel sized from the header above,
                // which is `width * height * 4` bytes, and `is_consistent` has already
                // checked that the source is exactly that many.
                unsafe {
                    ptr::copy_nonoverlapping(
                        frame.pixels.as_ptr(),
                        bits.cast(),
                        frame.pixels.len(),
                    );
                }
                // SAFETY: both handles are valid; the previous selection is restored below.
                let previous = unsafe { SelectObject(memory, bitmap) };
                let mut source = POINT { x: 0, y: 0 };
                let mut position = POINT {
                    x: self.placement.x,
                    y: self.placement.y,
                };
                let mut size = windows_sys::Win32::Foundation::SIZE {
                    cx: frame.width,
                    cy: frame.height,
                };
                let blend = BLENDFUNCTION {
                    // Both are single-byte constants declared as `u32` in the bindings, so
                    // the conversion cannot fail and `try_from` here would be error
                    // handling for a case the header rules out.
                    BlendOp: u8::try_from(AC_SRC_OVER).unwrap_or_default(),
                    BlendFlags: 0,
                    // Fully opaque *as a whole*; the per-pixel alpha in the bitmap is what
                    // does the work, and `AC_SRC_ALPHA` is what says to look at it.
                    SourceConstantAlpha: 255,
                    AlphaFormat: u8::try_from(AC_SRC_ALPHA).unwrap_or_default(),
                };
                // SAFETY: every pointer is to a live local, and the bitmap is selected into
                // the device context being passed.
                let ok = unsafe {
                    UpdateLayeredWindow(
                        self.handle,
                        screen,
                        &raw mut position,
                        &raw mut size,
                        memory,
                        &raw mut source,
                        0,
                        &raw const blend,
                        ULW_ALPHA,
                    )
                };
                // SAFETY: restoring what SelectObject returned, as documented.
                unsafe { SelectObject(memory, previous) };
                ok != 0
            };

            if !bitmap.is_null() {
                // SAFETY: created above and deleted once, after being deselected.
                unsafe { DeleteObject(bitmap) };
            }
            // SAFETY: created and obtained above, each released once.
            unsafe {
                DeleteDC(memory);
                ReleaseDC(ptr::null_mut(), screen);
            }
            drawn
        }

        /// Drains whatever the window has been sent.
        ///
        /// It handles nothing, and still has to run: a window whose thread never pumps is
        /// one the system marks as hung, and `IsHungAppWindow` on it is how the *other*
        /// side of this client decides an application is not answering.
        ///
        /// Associated rather than a method: messages are queued per *thread*, not per
        /// window, so this drains everything the overlay thread has been sent and does not
        /// need to know which window it belongs to.
        fn pump() {
            let mut message = MSG {
                hwnd: ptr::null_mut(),
                message: 0,
                wParam: 0,
                lParam: 0,
                time: 0,
                pt: POINT { x: 0, y: 0 },
            };
            // SAFETY: `message` is a live local for the duration of each call.
            while unsafe { PeekMessageW(&raw mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
                unsafe {
                    TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
            }
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: created in `create` and destroyed once, on the thread that made it.
            unsafe { DestroyWindow(self.handle) };
        }
    }

    /// Starts the overlay on its own thread.
    ///
    /// # Errors
    ///
    /// Whatever spawning the thread said. A window that cannot be created is reported the
    /// same way: the thread ends immediately and the first command is dropped, which is
    /// the behaviour an overlay on a machine with no interactive desktop should have.
    pub fn start() -> std::io::Result<Overlay> {
        let (commands, inbox) = channel();
        let thread = std::thread::Builder::new()
            .name("overlay".to_owned())
            .spawn(move || run(&inbox))?;
        Ok(Overlay {
            commands,
            thread: Some(thread),
        })
    }

    /// The overlay thread.
    fn run(inbox: &Receiver<Command>) {
        let Some(mut window) = Window::create() else {
            return;
        };
        let mut visible = false;
        loop {
            Window::pump();

            // Everything waiting, and then one wait. One command per iteration was the
            // first version, and at the idle rate it made a show-then-place take two
            // tenths of a second to become true -- which is not a test artefact but the
            // latency a player would see whenever the overlay was told two things at once.
            let mut acted = false;
            loop {
                match inbox.try_recv() {
                    Ok(command) => {
                        acted = true;
                        if !apply(&mut window, &mut visible, command) {
                            return;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                }
            }
            if acted {
                continue;
            }

            match inbox.recv_timeout(if visible { AWAKE } else { IDLE }) {
                Ok(command) => {
                    if !apply(&mut window, &mut visible, command) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Does one command. `false` means stop.
    fn apply(window: &mut Window, visible: &mut bool, command: Command) -> bool {
        match command {
            Command::Place(placement) => window.place(placement),
            Command::Clear => window.clear(),
            Command::Blit(sprite) => window.blit(&sprite),
            Command::Present => {
                window.present();
            }
            Command::Show(wanted) => {
                *visible = wanted;
                window.show(wanted);
            }
            Command::Stop => return false,
        }
        true
    }
}

#[cfg(windows)]
pub use platform::start;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Frame, Sprite};

    /// A short buffer is not a wrong picture. `UpdateLayeredWindow` reads
    /// `width * height * 4` bytes from the bitmap it is given, so the check is what stands
    /// between a caller's arithmetic mistake and a read past the end of an allocation.
    #[test]
    fn a_frame_whose_buffer_does_not_match_its_size_is_refused() {
        assert!(Frame::blank(4, 4).is_consistent());
        assert!(
            !Frame {
                width: 4,
                height: 4,
                pixels: vec![0; 4 * 4 * 4 - 1],
            }
            .is_consistent()
        );
        assert!(
            !Frame {
                width: 4,
                height: 4,
                pixels: Vec::new(),
            }
            .is_consistent()
        );
    }

    /// A zero-sized frame is refused too. It is what a minimised game produces, and there
    /// is nothing to draw on it.
    #[test]
    fn a_frame_with_no_pixels_is_refused() {
        assert!(!Frame::blank(0, 0).is_consistent());
        assert!(!Frame::blank(100, 0).is_consistent());
        assert!(!Frame::blank(-1, 10).is_consistent());
    }

    /// One opaque sprite replaces what was under it, exactly.
    ///
    /// Its alpha is 255, so the inverse is zero and nothing of the destination survives —
    /// which is the case a rounding mistake in the blend would show up in first.
    #[test]
    fn an_opaque_sprite_replaces_what_is_under_it() {
        let mut canvas = Frame::blank(2, 1);
        canvas
            .pixels
            .copy_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);
        canvas.blend(&Sprite {
            x: 0,
            y: 0,
            frame: Frame {
                width: 1,
                height: 1,
                pixels: vec![1, 2, 3, 255],
            },
        });
        assert_eq!(canvas.pixels, vec![1, 2, 3, 255, 50, 60, 70, 80]);
    }

    /// A fully transparent sprite changes nothing at all. Premultiplied, every channel of
    /// it is zero, so the destination is kept whole.
    #[test]
    fn a_transparent_sprite_changes_nothing() {
        let mut canvas = Frame::blank(1, 1);
        canvas.pixels.copy_from_slice(&[10, 20, 30, 40]);
        canvas.blend(&Sprite {
            x: 0,
            y: 0,
            frame: Frame {
                width: 1,
                height: 1,
                pixels: vec![0, 0, 0, 0],
            },
        });
        assert_eq!(canvas.pixels, vec![10, 20, 30, 40]);
    }

    /// Half-transparent white over black, premultiplied: the source is 128 in every
    /// channel, and the destination keeps `(0 * 127 + 127) / 255 = 0`.
    #[test]
    fn a_half_transparent_sprite_blends_rather_than_replaces() {
        let mut canvas = Frame::blank(1, 1);
        canvas.pixels.copy_from_slice(&[0, 0, 0, 255]);
        canvas.blend(&Sprite {
            x: 0,
            y: 0,
            frame: Frame {
                width: 1,
                height: 1,
                pixels: vec![128, 128, 128, 128],
            },
        });
        // The colour is the source's; the alpha is source plus what is left of the
        // destination's, which for an opaque background is full again.
        assert_eq!(canvas.pixels[3], 255);
        assert_eq!(&canvas.pixels[0..3], &[128, 128, 128]);
    }

    /// A sprite hanging off the edge draws the part that fits and nothing else. Wrapping
    /// would put an avatar's other half on the opposite side of the screen.
    #[test]
    fn a_sprite_off_the_edge_is_clipped_and_not_wrapped() {
        let mut canvas = Frame::blank(2, 2);
        canvas.blend(&Sprite {
            x: 1,
            y: 1,
            frame: Frame {
                width: 2,
                height: 2,
                pixels: vec![255; 2 * 2 * 4],
            },
        });
        // Only the bottom-right pixel is inside.
        assert_eq!(&canvas.pixels[0..12], &[0; 12]);
        assert_eq!(&canvas.pixels[12..16], &[255, 255, 255, 255]);
    }

    /// A negative position is the same case from the other side.
    #[test]
    fn a_sprite_before_the_origin_is_clipped_too() {
        let mut canvas = Frame::blank(2, 1);
        canvas.blend(&Sprite {
            x: -1,
            y: 0,
            frame: Frame {
                width: 2,
                height: 1,
                pixels: vec![255; 2 * 4],
            },
        });
        assert_eq!(&canvas.pixels[0..4], &[255, 255, 255, 255]);
        assert_eq!(&canvas.pixels[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn a_blank_frame_is_transparent_everywhere() {
        let frame = Frame::blank(2, 3);
        assert_eq!(frame.pixels.len(), 2 * 3 * 4);
        assert!(frame.pixels.iter().all(|byte| *byte == 0));
    }
}
