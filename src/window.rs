use crate::{
    colors::BLACK,
    error::Result,
    state::{GLOBAL_STATE, custom_wndproc, is_cursor_in_circle, toggle_fullscreen},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, error, trace};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    platform::windows::WindowAttributesExtWindows,
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};
use winsafe::{HWND, WNDPROC, co};

pub struct App {
    window: Option<Window>,
    last_frame_time: Instant,
    bitmap_buffer: Vec<u8>,
    title: String,
    hover: bool,
    phase: f32,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    fps: Arc<AtomicU64>,
    size_sync: Option<mpsc::Sender<PhysicalSize<u32>>>,
    frame_sync: Option<mpsc::Receiver<ffmpeg_next::frame::Video>>,
    frame: ffmpeg_next::frame::Video,
    transparency: u8,
}

impl App {
    pub fn new() -> Self {
        Self {
            window: None,
            last_frame_time: Instant::now(),
            bitmap_buffer: Vec::new(),
            title: String::from("AmazingWidget"),
            hover: false,
            phase: 0.0,
            position: PhysicalPosition::new(900, 100),
            size: PhysicalSize::new(400, 300),
            fps: Arc::new(AtomicU64::new(30.0f64.to_bits())),
            size_sync: None,
            frame_sync: None,
            frame: ffmpeg_next::frame::Video::empty(),
            transparency: 128, // Default to 50% transparency
        }
    }

    pub fn new_with_stream(
        size_sync: mpsc::Sender<PhysicalSize<u32>>,
        frame_sync: mpsc::Receiver<ffmpeg_next::frame::Video>,
        fps: Arc<AtomicU64>,
    ) -> Self {
        let mut app = Self::new();
        app.size_sync = Some(size_sync);
        app.frame_sync = Some(frame_sync);
        app.fps = fps;
        app
    }

    pub fn center(&self) -> PhysicalPosition<f64> {
        PhysicalPosition {
            x: f64::from(self.size.width) / 2.0,
            y: f64::from(self.size.height) / 2.0,
        }
    }

    fn frame_interval(&self) -> Duration {
        let fps = f64::from_bits(self.fps.load(Ordering::Relaxed));
        if fps > 0.0 {
            Duration::from_secs_f64(1.0 / fps)
        } else {
            Duration::from_millis(16) // Default to ~60 FPS if fps is not set
        }
    }

    fn hwnd(&self) -> HWND {
        match self
            .window
            .as_ref()
            .expect("tried to access window handle before window creation")
            .window_handle()
            .expect("tried to access window handle on another thread")
            .as_raw()
        {
            // Safety: We are accessing a valid HWND pointer from the window handle
            RawWindowHandle::Win32(handle) => unsafe { HWND::from_ptr(handle.hwnd.get() as _) },
            _ => unimplemented!("Unsupported platform"),
        }
    }

    fn draw_gdi(&mut self) -> Result<()> {
        let hdc_screen = self.hwnd().GetDC()?;
        let hdc_mem = hdc_screen.CreateCompatibleDC()?;
        trace!(
            "Drawing at position ({}, {}), size ({}, {})",
            self.position.x, self.position.y, self.size.width, self.size.height,
        );

        if let Some(frame_rx) = &self.frame_sync
            && let Ok(frame) = frame_rx.try_recv()
        {
            self.frame = frame;
        }

        // Safety: we initialized ffmpeg (and the frame) properly
        let frame_matches_size = !unsafe { self.frame.is_empty() }
            && self.frame.width() == self.size.width
            && self.frame.height() == self.size.height;

        let bits: &mut [u8] = if frame_matches_size {
            // Fast path: the decoded frame is already the right size, so write the
            // transparency key straight into its own buffer and hand that to GDI
            // directly instead of copying it into bitmap_buffer first.
            self.frame.data_mut(0)
        } else {
            let buffer_size = (self.size.width * self.size.height * 4) as usize;
            self.bitmap_buffer.resize(buffer_size, 0);

            // Fill the bitmap buffer with BGRA pixel data.
            // Safety: we initialized ffmpeg properly
            if unsafe { self.frame.is_empty() } {
                draw_gradient(
                    &mut self.bitmap_buffer,
                    self.size,
                    self.phase,
                    self.transparency,
                );
            } else {
                trace!(
                    "Frame size mismatch: {}x{} vs window {}x{}",
                    self.frame.width(),
                    self.frame.height(),
                    self.size.width,
                    self.size.height
                );

                let min_height = self.frame.height().min(self.size.height);
                let min_width = self.frame.width().min(self.size.width);
                for y in 0..min_height {
                    let src_offset = (y * self.frame.width() * 4) as usize;
                    let dst_offset = (y * self.size.width * 4) as usize;
                    let line_size = (min_width * 4) as usize;

                    if src_offset + line_size <= self.frame.data(0).len()
                        && dst_offset + line_size <= self.bitmap_buffer.len()
                    {
                        self.bitmap_buffer[dst_offset..dst_offset + line_size].copy_from_slice(
                            &self.frame.data(0)[src_offset..src_offset + line_size],
                        );
                    }
                }
            }

            &mut self.bitmap_buffer
        };

        // Apply transparency to alpha channel
        for pixel in bits.chunks_mut(4) {
            pixel[3] = self.transparency;
        }

        let width = self.size.width as i32;
        let height = self.size.height as i32;
        let bitmap = hdc_screen.CreateCompatibleBitmap(width, height)?;
        let mut bmi = winsafe::BITMAPINFO::default();
        bmi.bmiHeader.biWidth = width;
        bmi.bmiHeader.biHeight = -height; // Negative for top-down bitmap
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = co::BI::RGB;
        hdc_mem.SetDIBits(&bitmap, 0, height as u32, bits, &bmi, co::DIB::RGB_COLORS)?;
        let _bmp_guard = hdc_mem.SelectObject(&*bitmap)?;

        draw_pulsing_circle(self.phase, self.center(), &hdc_mem)?;

        // Blit to screen
        hdc_screen.BitBlt(
            winsafe::POINT::new(),
            winsafe::SIZE::with(width, height),
            &hdc_mem,
            winsafe::POINT::new(),
            co::ROP::SRCCOPY,
        )?;
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // Window already created
        }

        let window = event_loop.create_window(
            WindowAttributes::default()
                .with_title(&self.title)
                .with_inner_size(self.size)
                .with_position(self.position)
                .with_decorations(false)
                .with_transparent(true)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_skip_taskbar(true),
        );
        let window = match window {
            Ok(window) => window,
            Err(err) => {
                error!("Failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        let hwnd = self.hwnd();

        // A layered window must opt into both the color-key transparency and the
        // transparent hit-test behavior. Otherwise the OS still treats the top-level
        // overlay as receiving mouse input even when the underlying pixels are black.
        // WS_EX::LAYERED makes the black pixels transparent to clicks
        // WS_EX::LAYERED | WS_EX::TRANSPARENT makes the entire window transparent to clicks
        hwnd.set_style_ex(hwnd.style_ex() | co::WS_EX::LAYERED);
        if let Err(err) =
            hwnd.SetLayeredWindowAttributes(BLACK, self.transparency, co::LWA::COLORKEY)
        {
            error!("SetLayeredWindowAttributes failed: {err}");
        }

        // Store the original window procedure
        let old_proc = hwnd.GetWindowLongPtr(co::GWLP::WNDPROC);
        // Safety: This is known to be a valid WNDPROC pointer,
        // and we are storing it in a safe way to call later.
        GLOBAL_STATE.lock().unwrap().old_proc =
            Some(unsafe { std::mem::transmute::<isize, WNDPROC>(old_proc) });
        // Set our custom window procedure
        // Safety: the function we are casting has a valid signature for a window procedure,
        // and we are ensuring that the original window procedure is stored and called correctly.
        unsafe {
            hwnd.SetWindowLongPtr(
                co::GWLP::WNDPROC,
                custom_wndproc as *const () as usize as isize,
            )
        };
        // Send the initial window size to the FFmpeg thread if the channel is available
        if let Some(size_tx) = &self.size_sync {
            let _ = size_tx.send(self.size);
        }
        debug!("Window created");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                debug!("Close requested");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.draw_gdi() {
                    error!("draw_gdi failed: {err}");
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // Position is window-relative, so it can be compared directly with the
                // circle center. The hover state should update whenever the pointer is
                // anywhere on the transparent window client area, not just inside the
                // black circle hole.
                let next_hover = is_cursor_in_circle(self.center(), self.phase, position);
                if self.hover != next_hover {
                    debug!("Cursor hovered circle: {next_hover} at {:?}", position);
                }
                self.hover = next_hover;
            }
            WindowEvent::MouseInput {
                state: element_state,
                button: MouseButton::Left,
                ..
            } => {
                if element_state == ElementState::Pressed && self.hover {
                    debug!("Circle clicked!");
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if let Some(window) = &self.window {
                    let mut movement = PhysicalPosition::new(0, 0);
                    match key {
                        KeyCode::Escape => {
                            debug!("Escape pressed");
                            event_loop.exit();
                        }
                        KeyCode::KeyW => movement.y = -10,
                        KeyCode::KeyS => movement.y = 10,
                        KeyCode::KeyA => movement.x = -10,
                        KeyCode::KeyD => movement.x = 10,
                        KeyCode::KeyF => toggle_fullscreen(&self.hwnd()).expect("fullscreen"),
                        KeyCode::Equal | KeyCode::NumpadAdd => {
                            self.transparency = self.transparency.saturating_add(10);
                        }
                        KeyCode::Minus | KeyCode::NumpadSubtract => {
                            self.transparency = self.transparency.saturating_sub(10);
                        }
                        _ => {}
                    }

                    if movement.x != 0 || movement.y != 0 {
                        self.position = PhysicalPosition::new(
                            self.position.x + movement.x,
                            self.position.y + movement.y,
                        );
                        window.set_outer_position(self.position);
                    }
                }
            }
            WindowEvent::Moved(position) => {
                self.position = position;
            }
            WindowEvent::Resized(size) => {
                self.size = size;
                if let Some(size_tx) = &self.size_sync {
                    let _ = size_tx.send(size);
                }
            }
            event => trace!("Unhandled window event: {event:?}"),
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now.duration_since(self.last_frame_time) >= self.frame_interval() {
            self.last_frame_time = now;
            self.phase += 0.05;
            GLOBAL_STATE.lock().unwrap().phase = self.phase; // TODO: This shouldn't be two copies
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

fn draw_gradient(bitmap_buffer: &mut [u8], size: PhysicalSize<u32>, phase: f32, transparency: u8) {
    let red = ((phase % 1.0) * 255.0) as u8;
    for y in 0..size.height {
        for x in 0..size.width {
            let i = ((y * size.width + x) * 4) as usize;
            bitmap_buffer[i] = (x % 255) as u8;
            bitmap_buffer[i + 1] = (y % 255) as u8;
            bitmap_buffer[i + 2] = red;
            bitmap_buffer[i + 3] = transparency;
        }
    }
    trace!(
        "No frame data available, drawing {} gradient",
        bitmap_buffer.len()
    );
}

fn draw_pulsing_circle(
    phase: f32,
    center: PhysicalPosition<f64>,
    hdc_mem: &winsafe::guard::DeleteDCGuard,
) -> winsafe::SysResult<()> {
    let brush = winsafe::HBRUSH::CreateSolidBrush(BLACK)?;
    let _brush_guard = hdc_mem.SelectObject(&*brush)?;
    let radius = (60.0 + (phase.sin() * 30.0)) as i32;
    let ellipse_rect = winsafe::RECT {
        left: center.x as i32 - radius,
        top: center.y as i32 - radius,
        right: center.x as i32 + radius,
        bottom: center.y as i32 + radius,
    };
    hdc_mem.Ellipse(ellipse_rect)
}
