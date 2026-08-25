use crate::{
    colors::BLACK,
    error::Result,
    ff::{DecodedFrame, FrameStream, PlaybackCommand},
    overlay::{OverlayText, PulsingCircle, is_cursor_in_circle},
    state::{AtomicF64, GLOBAL_STATE, custom_wndproc, toggle_fullscreen},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};
use tracing::{debug, error, trace, warn};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    platform::windows::WindowAttributesExtWindows,
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};
use winsafe::{HWND, WNDPROC, co};

/// The main application state and logic.
pub struct App {
    /// The window handle, which is created in the `resumed` method on first render.
    window: Option<Window>,
    /// The last time a frame was drawn, used to control the frame rate.
    last_frame_time: Instant,
    /// The persistent buffer used to store the bitmap data for the current frame.
    /// Only meant to be used when we have no video frame
    bitmap_buffer: Vec<u8>,
    /// The title of the window, which is set when the window is created.
    title: String,
    /// The state of the pulsing circle, will probably be removed later
    pulse: PulsingCircle,
    /// The current position of the window on the screen, updated on `Moved` or `Resized` events.
    position: PhysicalPosition<i32>,
    /// The current size of the window, updated on `Resized` events.
    size: PhysicalSize<u32>,
    /// The channel used to send the current window size to the `FFmpeg` thread.
    size_sync: Option<mpsc::Sender<PhysicalSize<u32>>>,
    /// The channel used to receive decoded frames from the `FFmpeg` thread.
    frame_sync: Option<mpsc::Receiver<DecodedFrame>>,
    /// The channel used to send playback commands to the `FFmpeg` thread.
    command_tx: Option<mpsc::Sender<PlaybackCommand>>,
    /// The current frames per second, might be updated by the `FFmpeg` thread.
    fps: Arc<AtomicF64>,
    /// The total duration of the video in seconds, might be updated by the `FFmpeg` thread.
    duration: Arc<AtomicF64>,
    /// The current video frame and its presentation timestamp in seconds.
    video: DecodedFrame,
    /// The name of the video file being played, used for overlay text.
    file_name: String,
    /// Whether the video playback is currently paused.
    paused: bool,
    /// The transparency level of the window, from 0 (fully transparent) to 255 (fully opaque).
    transparency: u8,
    /// The overlay text, including the reusable buffer
    overlay_text: OverlayText,
}

impl App {
    pub fn new() -> Result<Self> {
        Ok(Self {
            window: None,
            last_frame_time: Instant::now(),
            bitmap_buffer: Vec::new(),
            title: String::from("AmazingWidget"),
            pulse: PulsingCircle::default(),
            position: PhysicalPosition::new(900, 100),
            size: PhysicalSize::new(800, 450),
            fps: Arc::new(AtomicF64::new(30.0)),
            duration: Arc::default(),
            size_sync: None,
            frame_sync: None,
            command_tx: None,
            video: DecodedFrame::default(),
            file_name: String::new(),
            paused: false,
            transparency: 128, // Default to 50% transparency
            overlay_text: OverlayText::new()?,
        })
    }

    pub fn new_with_stream(path: impl AsRef<Path>) -> Result<Self> {
        let (size_tx, size_rx) = mpsc::channel();
        // Bounded so the decoder blocks until the window consumes a frame,
        // pacing decoding to real playback speed instead of racing ahead.
        let (frame_tx, frame_rx) = mpsc::sync_channel(2);
        let (command_tx, command_rx) = mpsc::channel();
        let fps = Arc::new(AtomicF64::new(0.0));
        let duration = Arc::new(AtomicF64::new(0.0));
        let mut app = Self::new()?;
        app.size_sync = Some(size_tx);
        app.frame_sync = Some(frame_rx);
        app.command_tx = Some(command_tx);
        app.fps = fps.clone();
        app.duration = duration.clone();
        app.file_name = path
            .as_ref()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut stream = FrameStream::new(
            path.as_ref().into(),
            size_rx,
            frame_tx,
            command_rx,
            fps,
            duration,
        );
        ffmpeg_next::init()?;
        thread::spawn(move || {
            if let Err(e) = stream.read_frames() {
                error!("Error in FFmpeg thread: {e}");
            }
        });
        Ok(app)
    }

    pub fn run(&mut self) -> Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::Poll);
        event_loop.run_app(self).map_err(Into::into)
    }

    pub fn center(&self) -> PhysicalPosition<f64> {
        PhysicalPosition {
            x: f64::from(self.size.width) / 2.0,
            y: f64::from(self.size.height) / 2.0,
        }
    }

    fn frame_interval(&self) -> Duration {
        let fps = self.fps.load(Ordering::Relaxed);
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
            && let Ok(decoded) = frame_rx.try_recv()
        {
            self.video = decoded;
        }

        // Safety: we initialized ffmpeg (and the frame) properly
        let frame_matches_size = !unsafe { self.video.frame.is_empty() }
            && self.video.frame.width() == self.size.width
            && self.video.frame.height() == self.size.height;

        let bits: &mut [u8] = if frame_matches_size {
            // Fast path: the decoded frame is already the right size, so write the
            // transparency key straight into its own buffer and hand that to GDI
            // directly instead of copying it into bitmap_buffer first.
            self.video.frame.data_mut(0)
        } else {
            let buffer_size = (self.size.width * self.size.height * 4) as usize;
            self.bitmap_buffer.resize(buffer_size, 0);

            // Fill the bitmap buffer with BGRA pixel data.
            // Safety: we initialized ffmpeg properly
            if unsafe { self.video.frame.is_empty() } {
                draw_gradient(
                    &mut self.bitmap_buffer,
                    self.size,
                    self.pulse.phase,
                    self.transparency,
                );
            } else {
                trace!(
                    "Frame size mismatch: {}x{} vs window {}x{}",
                    self.video.frame.width(),
                    self.video.frame.height(),
                    self.size.width,
                    self.size.height
                );

                let min_height = self.video.frame.height().min(self.size.height);
                let min_width = self.video.frame.width().min(self.size.width);
                for y in 0..min_height {
                    let src_offset = (y * self.video.frame.width() * 4) as usize;
                    let dst_offset = (y * self.size.width * 4) as usize;
                    let line_size = (min_width * 4) as usize;

                    if src_offset + line_size <= self.video.frame.data(0).len()
                        && dst_offset + line_size <= self.bitmap_buffer.len()
                    {
                        self.bitmap_buffer[dst_offset..dst_offset + line_size].copy_from_slice(
                            &self.video.frame.data(0)[src_offset..src_offset + line_size],
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

        self.pulse.draw(&hdc_mem, self.center())?;
        self.overlay_text.draw(
            &hdc_mem,
            &self.file_name,
            self.video.pts_seconds,
            self.duration.load(Ordering::Relaxed),
        )?;

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

    fn handle_key_input(&mut self, event_loop: &ActiveEventLoop, key: KeyCode) {
        if let Some(window) = &self.window {
            match key {
                KeyCode::Escape => {
                    debug!("Escape pressed");
                    event_loop.exit();
                }
                KeyCode::KeyW => self.position.y -= 10,
                KeyCode::KeyS => self.position.y += 10,
                KeyCode::KeyA => self.position.x -= 10,
                KeyCode::KeyD => self.position.x += 10,
                KeyCode::KeyH => self.overlay_text.show = !self.overlay_text.show,
                KeyCode::KeyF => {
                    if let Err(err) = toggle_fullscreen(&self.hwnd()) {
                        error!("Failed to toggle fullscreen: {err}");
                    }
                }
                KeyCode::Space => {
                    self.paused = !self.paused;
                    if let Some(command_tx) = &self.command_tx {
                        let _ = command_tx.send(PlaybackCommand::TogglePause);
                    }
                }
                KeyCode::ArrowLeft | KeyCode::ArrowRight => {
                    self.overlay_text.show = true; // Show overlay when seeking
                    let duration_secs = self.duration.load(Ordering::Relaxed);
                    let step = 5.0;
                    let mut target = self.video.pts_seconds;
                    if key == KeyCode::ArrowLeft {
                        target -= step;
                    } else {
                        target += step;
                    }
                    target = target.clamp(0.0, duration_secs.max(0.0));
                    self.video.pts_seconds = target;
                    if let Some(frame_rx) = &self.frame_sync {
                        while frame_rx.try_recv().is_ok() {}
                    }
                    if let Some(command_tx) = &self.command_tx {
                        let _ = command_tx.send(PlaybackCommand::Seek(target));
                    }
                }
                KeyCode::Equal | KeyCode::NumpadAdd => {
                    self.transparency = self.transparency.saturating_add(10);
                }
                KeyCode::Minus | KeyCode::NumpadSubtract => {
                    self.transparency = self.transparency.saturating_sub(10);
                }
                _ => {}
            }
            if matches!(
                key,
                KeyCode::KeyW | KeyCode::KeyS | KeyCode::KeyA | KeyCode::KeyD
            ) {
                window.set_outer_position(self.position);
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            warn!("Window already created, ignoring resumed event");
            return;
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

    #[allow(clippy::too_many_lines)]
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
                let next_hover = is_cursor_in_circle(self.center(), self.pulse.phase, position);
                if self.pulse.hover != next_hover {
                    debug!("Cursor hovered circle: {next_hover} at {:?}", position);
                }
                self.pulse.hover = next_hover;
            }
            WindowEvent::MouseInput {
                state: element_state,
                button: MouseButton::Left,
                ..
            } => {
                if element_state == ElementState::Pressed && self.pulse.hover {
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
            } => self.handle_key_input(event_loop, key),
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
            self.pulse.phase += 0.05;
            GLOBAL_STATE.lock().unwrap().phase = self.pulse.phase; // TODO: This shouldn't be two copies
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
