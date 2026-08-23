use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::{Duration, Instant};
use tracing::{debug, error, trace};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    platform::windows::WindowAttributesExtWindows,
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};
use winsafe::{
    BITMAPINFO, GetSystemMetrics, HBRUSH, HPEN, HWND, HwndPlace, POINT, RECT, SIZE, SysResult,
    WNDPROC, co, guard::DeleteDCGuard,
};

mod error;
use error::Result;
mod ff;
use ff::FrameStream;
mod state;
use state::{FRAME_SYNC, WINDOW_STATE, WindowState};
mod colors;
use colors::{BLACK, BLUE, GREEN};

const TRANSPARENCY: u8 = 100;

struct App {
    window: Option<Window>,
    last_frame_time: Instant,
    frame_interval: Duration,
    bitmap_buffer: Vec<u8>,
}

impl App {
    fn new() -> Self {
        let fps = WINDOW_STATE.lock().unwrap().fps;
        Self {
            window: None,
            last_frame_time: Instant::now(),
            frame_interval: Duration::from_millis((1000 / fps) as u64),
            bitmap_buffer: Vec::new(),
        }
    }

    fn cursor_in_circle(state: &WindowState, cursor_pos: PhysicalPosition<f64>) -> bool {
        let dx = cursor_pos.x - state.center().x;
        let dy = cursor_pos.y - state.center().y;
        let radius = 60.0 + (f64::from(state.phase.sin()) * 30.0);
        dx * dx + dy * dy < radius * radius
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let (title, size, position) = {
            let state = WINDOW_STATE.lock().unwrap();
            (state.title.clone(), state.size, state.position)
        };
        let window = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title(&title)
                    .with_inner_size(size)
                    .with_position(position)
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_window_level(WindowLevel::AlwaysOnTop)
                    .with_skip_taskbar(true),
            )
            .unwrap();

        // Set up click-through for non-circle areas
        setup_click_through(&window);

        self.window = Some(window);
        debug!("Window created");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                debug!("Close requested");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Some(window) = &self.window
                    && let Err(err) = draw_gdi(window, &mut self.bitmap_buffer)
                {
                    error!("draw_gdi failed: {err}");
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let mut state = WINDOW_STATE.lock().unwrap();
                // position is window-relative, so we can use it directly for hover detection
                state.hover = Self::cursor_in_circle(&state, position);
            }
            WindowEvent::MouseInput {
                state: element_state,
                button: MouseButton::Left,
                ..
            } => {
                if element_state == ElementState::Pressed {
                    let state = WINDOW_STATE.lock().unwrap();
                    if state.hover {
                        debug!("Circle clicked!");
                    }
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
                        _ => {}
                    }

                    if movement.x != 0 || movement.y != 0 {
                        let new_pos = {
                            let mut state = WINDOW_STATE.lock().unwrap();
                            state.position = PhysicalPosition::new(
                                state.position.x + movement.x,
                                state.position.y + movement.y,
                            );
                            state.position
                        }; // Release the lock before calling set_outer_position
                        window.set_outer_position(new_pos);
                    }
                }
            }
            WindowEvent::Moved(position) => {
                let mut state = WINDOW_STATE.lock().unwrap();
                state.position = position;
            }
            WindowEvent::Resized(size) => {
                let mut state = WINDOW_STATE.lock().unwrap();
                state.size = size;
                state.rescale_needed = true;
            }
            event => trace!("Unhandled window event: {event:?}"),
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now.duration_since(self.last_frame_time) >= self.frame_interval {
            self.last_frame_time = now;

            {
                let mut state = WINDOW_STATE.lock().unwrap();
                state.phase += 0.05;
                FRAME_SYNC.notify_one();
            }

            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    ffmpeg_next::init()?;

    if let Some(filename) = std::env::args().nth(1) {
        std::thread::spawn(move || {
            if let Err(e) = FrameStream::new(&filename).and_then(|mut s| {
                WINDOW_STATE.lock().unwrap().fps = s.fps;
                s.read_frames()
            }) {
                error!("Error in FFmpeg thread: {}", e);
            }
        });
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    event_loop.run_app(&mut app)?;

    Ok(())
}

fn draw_gradient(bitmap_buffer: &mut [u8], size: PhysicalSize<u32>, phase: f32) {
    let red = ((phase % 1.0) * 255.0) as u8;
    for y in 0..size.height {
        for x in 0..size.width {
            let i = ((y * size.width + x) * 4) as usize;
            bitmap_buffer[i] = (x % 255) as u8;
            bitmap_buffer[i + 1] = (y % 255) as u8;
            bitmap_buffer[i + 2] = red;
            bitmap_buffer[i + 3] = TRANSPARENCY;
        }
    }
    trace!(
        "No frame data available, drawing {} gradient",
        bitmap_buffer.len()
    );
}

fn draw_pulsing_circle(
    hover: bool,
    phase: f32,
    center: PhysicalPosition<f64>,
    hdc_mem: &DeleteDCGuard,
) -> SysResult<()> {
    let pen_color = if hover { GREEN } else { BLUE };
    let hpen = HPEN::CreatePen(co::PS::SOLID, 3, pen_color)?;
    let _pen_guard = hdc_mem.SelectObject(&*hpen)?;
    let brush = HBRUSH::CreateSolidBrush(BLACK)?;
    let _brush_guard = hdc_mem.SelectObject(&*brush)?;
    let radius = (60.0 + (phase.sin() * 30.0)) as i32;
    let ellipse_rect = RECT {
        left: center.x as i32 - radius,
        top: center.y as i32 - radius,
        right: center.x as i32 + radius,
        bottom: center.y as i32 + radius,
    };
    hdc_mem.Ellipse(ellipse_rect)?;
    Ok(())
}

fn draw_gdi(window: &Window, bitmap_buffer: &mut Vec<u8>) -> Result<()> {
    let hwnd = match window
        .window_handle()
        .expect("tried to access window handle on another thread")
        .as_raw()
    {
        // Safety: We are accessing a valid HWND pointer from the window handle
        RawWindowHandle::Win32(handle) => unsafe { HWND::from_ptr(handle.hwnd.get() as _) },
        _ => unimplemented!("Unsupported platform"),
    };
    let hdc_screen = hwnd.GetDC()?;
    let hdc_mem = hdc_screen.CreateCompatibleDC()?;
    let state = WINDOW_STATE.lock().expect("window state lock");
    trace!(
        "Drawing at position ({}, {}), size ({}, {}) frame: {}x{}",
        state.position.x,
        state.position.y,
        state.size.width,
        state.size.height,
        state.frame.width(),
        state.frame.height()
    );

    let buffer_size = (state.size.width * state.size.height * 4) as usize;
    bitmap_buffer.resize(buffer_size, 0);

    // Fill the bitmap buffer with BGRA pixel data.
    if unsafe { state.frame.is_empty() } {
        draw_gradient(bitmap_buffer, state.size, state.phase);
    } else if state.frame.width() == state.size.width && state.frame.height() == state.size.height {
        let copy_size = buffer_size.min(state.frame.data(0).len());
        if copy_size > 0 {
            bitmap_buffer[..copy_size].copy_from_slice(&state.frame.data(0)[..copy_size]);
        }
    } else {
        trace!(
            "Frame size mismatch: {}x{} vs window {}x{}",
            state.frame.width(),
            state.frame.height(),
            state.size.width,
            state.size.height
        );

        let min_height = state.frame.height().min(state.size.height);
        let min_width = state.frame.width().min(state.size.width);
        for y in 0..min_height {
            let src_offset = (y * state.frame.width() * 4) as usize;
            let dst_offset = (y * state.size.width * 4) as usize;
            let line_size = (min_width * 4) as usize;

            if src_offset + line_size <= state.frame.data(0).len()
                && dst_offset + line_size <= bitmap_buffer.len()
            {
                bitmap_buffer[dst_offset..dst_offset + line_size]
                    .copy_from_slice(&state.frame.data(0)[src_offset..src_offset + line_size]);
            }
        }
    }

    for pixel in bitmap_buffer.chunks_mut(4) {
        pixel[3] = TRANSPARENCY;
    }

    let width = state.size.width as i32;
    let height = state.size.height as i32;
    let bitmap = hdc_screen.CreateCompatibleBitmap(width, height)?;
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -height; // Negative for top-down bitmap
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = co::BI::RGB;
    hdc_mem.SetDIBits(
        &bitmap,
        0,
        height as u32,
        bitmap_buffer,
        &bmi,
        co::DIB::RGB_COLORS,
    )?;
    let _bmp_guard = hdc_mem.SelectObject(&*bitmap)?;

    draw_pulsing_circle(state.hover, state.phase, state.center(), &hdc_mem)?;

    // Blit to screen
    hdc_screen.BitBlt(
        POINT::new(),
        SIZE::with(width, height),
        &hdc_mem,
        POINT::new(),
        co::ROP::SRCCOPY,
    )?;
    Ok(())
}

fn custom_wndproc(hwnd: HWND, msg: co::WM, wparam: usize, lparam: isize) -> isize {
    if msg == co::WM::NCHITTEST {
        // Get the cursor position from lparam (screen coordinates)
        let screen_x = i32::from((lparam & 0xFFFF) as i16);
        let screen_y = i32::from(((lparam >> 16) & 0xFFFF) as i16);

        // Get window position to convert to window coordinates
        if let Ok(rect) = hwnd.GetWindowRect() {
            let window_x = screen_x - rect.left;
            let window_y = screen_y - rect.top;

            // Check if the cursor is over the circle
            let state = WINDOW_STATE.lock().unwrap();
            let cursor_pos = PhysicalPosition::new(f64::from(window_x), f64::from(window_y));

            if App::cursor_in_circle(&state, cursor_pos) {
                return co::HT::TRANSPARENT.raw() as isize; // Circle is click-through
            }
            return co::HT::CAPTION.raw() as isize; // Background is draggable
        }
    }

    // Intercept maximize command and double-click to go fullscreen instead
    if msg == co::WM::NCLBUTTONDBLCLK
        || (msg == co::WM::SYSCOMMAND && (wparam & 0xFFF0) == co::SC::MAXIMIZE.raw() as usize)
    {
        let _ = toggle_fullscreen(&hwnd);
        return 0;
    }

    // Call the original window procedure
    let old_proc = {
        let state = WINDOW_STATE.lock().unwrap();
        state.old_proc
    };
    if let Some(old_proc) = old_proc {
        old_proc(hwnd, msg, wparam, lparam)
    } else {
        0
    }
}

fn toggle_fullscreen(hwnd: &HWND) -> Result<()> {
    let (point, size) = {
        // The mutex needs to be dropped before going across `SetWindowPos`
        // because the Moved/Resized events will try to lock it again.
        let mut state = WINDOW_STATE.lock().unwrap();
        if state.is_fullscreen {
            // Restore to old position and size
            state.position = state.old_position;
            state.size = state.old_size;
            (
                POINT::with(state.position.x, state.position.y),
                SIZE::with(state.size.width as i32, state.size.height as i32),
            )
        } else {
            // Save current position and size before going fullscreen
            state.old_position = state.position;
            state.old_size = state.size;
            // Get full screen dimensions (including taskbar)
            (
                POINT::new(),
                SIZE::with(
                    GetSystemMetrics(co::SM::CXSCREEN),
                    GetSystemMetrics(co::SM::CYSCREEN),
                ),
            )
        }
    };

    debug!(
        "Setting window position to ({}, {}), size ({}x{})",
        point.x, point.y, size.cx, size.cy
    );
    let resize_flags: co::SWP = co::SWP::FRAMECHANGED | co::SWP::NOACTIVATE;
    hwnd.SetWindowPos(HwndPlace::None, point, size, resize_flags)?;
    let mut state = WINDOW_STATE.lock().unwrap();
    state.is_fullscreen = !state.is_fullscreen;
    state.rescale_needed = true;
    Ok(())
}

fn setup_click_through(window: &Window) {
    let hwnd = match window
        .window_handle()
        .expect("tried to access window handle on another thread")
        .as_raw()
    {
        // Safety: We are accessing a valid HWND pointer from the window handle
        RawWindowHandle::Win32(handle) => unsafe { HWND::from_ptr(handle.hwnd.get() as _) },
        _ => unimplemented!("Unsupported platform"),
    };
    let mut state = WINDOW_STATE.lock().unwrap();

    // Store the original window procedure
    let old_proc = hwnd.GetWindowLongPtr(co::GWLP::WNDPROC);
    let old_proc = unsafe { std::mem::transmute::<isize, WNDPROC>(old_proc) };
    state.old_proc = Some(old_proc);

    // Set our custom window procedure
    unsafe {
        hwnd.SetWindowLongPtr(
            co::GWLP::WNDPROC,
            custom_wndproc as *const () as usize as isize,
        )
    };

    debug!("Click-through enabled");
}
