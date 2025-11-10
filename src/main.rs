use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::{Duration, Instant};
use tracing::{debug, error, trace};
use winsafe::{self as w, co, prelude::*};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    platform::windows::WindowAttributesExtWindows,
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};

mod error;
mod ff;
mod state;
use error::Result;
use ff::FrameStream;
use state::{WindowState, FRAME_SYNC, WINDOW_STATE};

struct App {
    window: Option<Window>,
    last_frame_time: Instant,
    frame_interval: Duration,
}

impl App {
    fn new() -> Self {
        let fps = WINDOW_STATE.lock().unwrap().fps;
        Self {
            window: None,
            last_frame_time: Instant::now(),
            frame_interval: Duration::from_millis((1000 / fps) as u64),
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

        let state = WINDOW_STATE.lock().unwrap();
        let window = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title(&state.title)
                    .with_inner_size(state.size)
                    .with_position(state.position)
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
                if let Some(window) = &self.window {
                    draw_gdi(window);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let mut state = WINDOW_STATE.lock().unwrap();
                state.cursor = position;

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
                    let mut state = WINDOW_STATE.lock().unwrap();
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
                        let new_pos = PhysicalPosition::new(
                            state.position.x + movement.x,
                            state.position.y + movement.y,
                        );
                        window.set_outer_position(new_pos);
                        state.position = new_pos;
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
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now.duration_since(self.last_frame_time) >= self.frame_interval {
            self.last_frame_time = now;

            let mut state = WINDOW_STATE.lock().unwrap();
            state.phase += 0.05;
            FRAME_SYNC.notify_one();

            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

fn main() -> Result<()> {
    let filename = std::env::args().nth(1).unwrap_or_default();
    tracing_subscriber::fmt::init();
    ffmpeg_next::init()?;

    std::thread::spawn(move || {
        if let Err(e) = FrameStream::new(&filename).and_then(|mut s| {
            WINDOW_STATE.lock().unwrap().fps = s.fps;
            s.read_frames()
        }) {
            error!("Error in FFmpeg thread: {}", e);
        }
    });

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    event_loop.run_app(&mut app)?;

    Ok(())
}

fn draw_pulsing_circle(state: &WindowState, hdc_mem: &w::HDC) {
    let pen_color = if state.hover {
        co::COLORREF::new(0, 255, 0) // Green when hovered
    } else {
        co::COLORREF::new(255, 0, 0) // Red otherwise (note: BGR order)
    };
    
    let hpen = w::HPEN::CreatePen(co::PS::SOLID, 3, pen_color).unwrap();
    let old_pen = hdc_mem.SelectObjectPen(&hpen).unwrap();
    
    let brush = w::HBRUSH::CreateSolidBrush(co::COLORREF::new(0, 0, 0)).unwrap();
    let old_brush = hdc_mem.SelectObjectBrush(&brush).unwrap();
    
    let radius = (60.0 + (state.phase.sin() * 30.0)) as i32;
    let center = state.center();
    
    let _ = hdc_mem.Ellipse(
        center.x as i32 - radius,
        center.y as i32 - radius,
        center.x as i32 + radius,
        center.y as i32 + radius,
    );
    
    hdc_mem.SelectObjectPen(&old_pen).ok();
    hdc_mem.SelectObjectBrush(&old_brush).ok();
}

fn draw_gdi(window: &Window) {
    let hwnd = match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Win32(handle) => w::HWND::from_ptr(handle.hwnd.get() as _),
        _ => {
            error!("Unsupported platform");
            return;
        }
    };

    let hdc_screen = match hwnd.GetDC() {
        Ok(dc) => dc,
        Err(e) => {
            error!("Failed to get DC: {}", e);
            return;
        }
    };
    
    let hdc_mem = match hdc_screen.CreateCompatibleDC() {
        Ok(dc) => dc,
        Err(e) => {
            error!("Failed to create compatible DC: {}", e);
            return;
        }
    };

    let state = WINDOW_STATE.lock().unwrap();
    let width = state.size.width as i32;
    let height = state.size.height as i32;

    trace!(
        "Drawing at position ({}, {}), size ({}, {}) frame: {}x{}",
        state.position.x,
        state.position.y,
        width,
        height,
        state.frame.width(),
        state.frame.height()
    );

    let bmi = w::BITMAPINFO {
        bmiHeader: w::BITMAPINFOHEADER {
            biSize: std::mem::size_of::<w::BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: co::BI::RGB.raw(),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    
    // CreateDIBSection requires unsafe due to raw pointer manipulation
    let hbitmap = unsafe {
        match hdc_mem.CreateDIBSection(
            &bmi,
            co::DIB::RGB_COLORS,
            &mut bits_ptr as *mut _,
            None,
            0,
        ) {
            Ok(bmp) => bmp,
            Err(e) => {
                error!("Failed to create DIB section: {}", e);
                return;
            }
        }
    };
    
    let old_bmp = hdc_mem.SelectObjectBitmap(&hbitmap).unwrap();

    if !bits_ptr.is_null() {
        let buffer_size = (width * height * 4) as usize;
        // SAFETY: bits_ptr is guaranteed to be valid by CreateDIBSection
        let dst = unsafe { std::slice::from_raw_parts_mut(bits_ptr.cast::<u8>(), buffer_size) };

        // Fill with arbitrary image data (BGRA)
        if state.frame.is_empty() {
            trace!("No frame data available");
            let red = ((state.phase % 1.0) * 255.0) as u8;
            for y in 0..height {
                for x in 0..width {
                    let i = ((y * width + x) * 4) as usize;
                    dst[i] = (x % 255) as u8; // Blue
                    dst[i + 1] = (y % 255) as u8; // Green
                    dst[i + 2] = red;
                }
            }
        } else {
            let frame_data = state.frame.data(0);
            let frame_width = state.frame.width() as i32;
            let frame_height = state.frame.height() as i32;

            // If frame dimensions match window, direct copy
            if frame_width == width && frame_height == height {
                let copy_size = buffer_size.min(frame_data.len());
                if copy_size > 0 {
                    dst[..copy_size].copy_from_slice(&frame_data[..copy_size]);
                }
            } else {
                // Frame size mismatch - fill with black and copy what we can
                trace!(
                    "Frame size mismatch: {}x{} vs window {}x{}",
                    frame_width,
                    frame_height,
                    width,
                    height
                );
                dst.fill(0);

                // Copy line by line to handle size mismatch
                let min_height = frame_height.min(height);
                let min_width = frame_width.min(width);
                for y in 0..min_height {
                    let src_offset = (y * frame_width * 4) as usize;
                    let dst_offset = (y * width * 4) as usize;
                    let line_size = (min_width * 4) as usize;

                    if src_offset + line_size <= frame_data.len()
                        && dst_offset + line_size <= dst.len()
                    {
                        dst[dst_offset..dst_offset + line_size]
                            .copy_from_slice(&frame_data[src_offset..src_offset + line_size]);
                    }
                }
            }
        }
    }

    draw_pulsing_circle(&state, &hdc_mem);

    // Blit to screen
    let _ = hdc_screen.BitBlt(
        w::POINT::new(0, 0),
        w::SIZE::new(width, height),
        &hdc_mem,
        w::POINT::new(0, 0),
        co::ROP::SRCCOPY,
    );

    // Clean up (automatic via RAII in winsafe)
    hdc_mem.SelectObjectBitmap(&old_bmp).ok();
}

static mut OLD_WNDPROC: Option<isize> = None;

extern "system" fn custom_wndproc(
    hwnd: w::HWND,
    msg: co::WM,
    wparam: usize,
    lparam: isize,
) -> isize {
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
    if msg == co::WM::SYSCOMMAND && (wparam & 0xFFF0) == co::SC::MAXIMIZE.raw() as usize {
        toggle_fullscreen(hwnd);
        return 0;
    }

    if msg == co::WM::NCLBUTTONDBLCLK {
        toggle_fullscreen(hwnd);
        return 0;
    }

    // Call the original window procedure
    unsafe {
        if let Some(old_proc) = OLD_WNDPROC {
            w::CallWindowProc(old_proc, hwnd, msg.raw(), wparam, lparam)
        } else {
            0
        }
    }
}

static mut IS_FULLSCREEN: bool = false;
static mut SAVED_POSITION: (i32, i32) = (0, 0);
static mut SAVED_SIZE: (i32, i32) = (0, 0);

fn toggle_fullscreen(hwnd: w::HWND) {
    unsafe {
        if IS_FULLSCREEN {
            // Restore to normal size
            let _ = hwnd.SetWindowPos(
                w::HwndPlace::Top,
                SAVED_POSITION.0,
                SAVED_POSITION.1,
                SAVED_SIZE.0,
                SAVED_SIZE.1,
                co::SWP::FRAMECHANGED | co::SWP::NOACTIVATE,
            );
            IS_FULLSCREEN = false;

            // Update state
            let mut state = WINDOW_STATE.lock().unwrap();
            state.position = PhysicalPosition::new(SAVED_POSITION.0, SAVED_POSITION.1);
            state.size = PhysicalSize::new(SAVED_SIZE.0 as u32, SAVED_SIZE.1 as u32);
            state.rescale_needed = true;
        } else {
            // Save current position and size
            if let Ok(rect) = hwnd.GetWindowRect() {
                SAVED_POSITION = (rect.left, rect.top);
                SAVED_SIZE = (rect.right - rect.left, rect.bottom - rect.top);
            }

            // Get full screen dimensions (including taskbar)
            let screen_width = w::GetSystemMetrics(co::SM::CXSCREEN);
            let screen_height = w::GetSystemMetrics(co::SM::CYSCREEN);

            // Set to fullscreen
            let _ = hwnd.SetWindowPos(
                w::HwndPlace::Top,
                0,
                0,
                screen_width,
                screen_height,
                co::SWP::FRAMECHANGED | co::SWP::NOACTIVATE,
            );
            IS_FULLSCREEN = true;

            // Update state
            let mut state = WINDOW_STATE.lock().unwrap();
            state.position = PhysicalPosition::new(0, 0);
            state.size = PhysicalSize::new(screen_width as u32, screen_height as u32);
            state.rescale_needed = true;
        }
    }
}

fn setup_click_through(window: &Window) {
    let hwnd = match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Win32(handle) => w::HWND::from_ptr(handle.hwnd.get() as _),
        _ => {
            error!("Unsupported platform for click-through");
            return;
        }
    };

    unsafe {
        // Store the original window procedure
        if let Ok(old_proc) = hwnd.GetWindowLongPtr(co::GWLP::WNDPROC) {
            OLD_WNDPROC = Some(old_proc);
            
            // Set our custom window procedure
            let _ = hwnd.SetWindowLongPtr(
                co::GWLP::WNDPROC,
                custom_wndproc as usize as isize,
            );
            
            debug!("Click-through enabled");
        } else {
            error!("Failed to get window procedure");
        }
    }
}
