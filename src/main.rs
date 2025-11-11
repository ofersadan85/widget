use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::{Duration, Instant};
use tracing::{debug, error, trace};
use windows::Win32::{
    Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, CreatePen, CreateSolidBrush, DeleteDC,
        DeleteObject, Ellipse, GetDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HDC, PS_SOLID, SRCCOPY,
    },
    UI::WindowsAndMessaging::{
        CallWindowProcW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW,
        SetWindowPos, GWL_WNDPROC, HTCAPTION, HTTRANSPARENT, HWND_TOP, SC_MAXIMIZE, SM_CXSCREEN,
        SM_CYSCREEN, SWP_FRAMECHANGED, SWP_NOACTIVATE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
        WM_SYSCOMMAND, WNDPROC,
    },
};
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

unsafe fn draw_pulsing_circle(state: &WindowState, hdc_mem: HDC) {
    let pen_color = if state.hover {
        COLORREF(0x0000_FF00) // Green when hovered
    } else {
        COLORREF(0x0000_00FF) // Blue otherwise
    };
    let hpen = CreatePen(PS_SOLID, 3, pen_color);
    let old_pen = SelectObject(hdc_mem, hpen);
    let brush = CreateSolidBrush(COLORREF(0x0000_0000));
    let old_brush = SelectObject(hdc_mem, brush);
    let radius = (60.0 + (state.phase.sin() * 30.0)) as i32;
    let center = state.center();
    let _ = Ellipse(
        hdc_mem,
        center.x as i32 - radius,
        center.y as i32 - radius,
        center.x as i32 + radius,
        center.y as i32 + radius,
    );
    let _ = SelectObject(hdc_mem, old_pen);
    let _ = DeleteObject(hpen);
    let _ = SelectObject(hdc_mem, old_brush);
    let _ = DeleteObject(brush);
}

fn draw_gdi(window: &Window) {
    unsafe {
        let hwnd = match window.window_handle().unwrap().as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as _),
            _ => unimplemented!("Unsupported platform"),
        };

        let hdc_screen = GetDC(hwnd);
        let hdc_mem = CreateCompatibleDC(hdc_screen);

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

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbitmap = CreateDIBSection(
            hdc_mem,
            &raw const bmi,
            DIB_RGB_COLORS,
            &raw mut bits_ptr,
            HANDLE::default(),
            0,
        )
        .unwrap_or_default();
        let old_bmp = SelectObject(hdc_mem, hbitmap);

        if !bits_ptr.is_null() {
            let buffer_size = (width * height * 4) as usize;
            let dst = std::slice::from_raw_parts_mut(bits_ptr.cast::<u8>(), buffer_size);

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

        draw_pulsing_circle(&state, hdc_mem);

        // Blit to screen
        let _ = BitBlt(hdc_screen, 0, 0, width, height, hdc_mem, 0, 0, SRCCOPY);

        // Clean up
        SelectObject(hdc_mem, old_bmp);
        let _ = DeleteObject(hbitmap);
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(hwnd, hdc_screen);
    }
}

static mut OLD_WNDPROC: Option<WNDPROC> = None;

unsafe extern "system" fn custom_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        // Get the cursor position from lparam (screen coordinates)
        let screen_x = i32::from((lparam.0 & 0xFFFF) as i16);
        let screen_y = i32::from(((lparam.0 >> 16) & 0xFFFF) as i16);

        // Get window position to convert to window coordinates
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &raw mut rect).is_ok() {
            let window_x = screen_x - rect.left;
            let window_y = screen_y - rect.top;

            // Check if the cursor is over the circle
            let state = WINDOW_STATE.lock().unwrap();
            let cursor_pos = PhysicalPosition::new(f64::from(window_x), f64::from(window_y));

            if App::cursor_in_circle(&state, cursor_pos) {
                return LRESULT(HTTRANSPARENT as isize); // Circle is click-through
            }
            return LRESULT(HTCAPTION as isize); // Background is draggable
        }
    }

    // Intercept maximize command and double-click to go fullscreen instead
    if msg == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) == SC_MAXIMIZE as usize {
        toggle_fullscreen(hwnd);
        return LRESULT(0);
    }

    if msg == WM_NCLBUTTONDBLCLK {
        toggle_fullscreen(hwnd);
        return LRESULT(0);
    }

    // Call the original window procedure
    if let Some(old_proc) = OLD_WNDPROC {
        CallWindowProcW(old_proc, hwnd, msg, wparam, lparam)
    } else {
        LRESULT(0)
    }
}

static mut IS_FULLSCREEN: bool = false;
static mut SAVED_POSITION: (i32, i32) = (0, 0);
static mut SAVED_SIZE: (i32, i32) = (0, 0);

unsafe fn toggle_fullscreen(hwnd: HWND) {
    use windows::Win32::Foundation::RECT;

    if IS_FULLSCREEN {
        // Restore to normal size
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            SAVED_POSITION.0,
            SAVED_POSITION.1,
            SAVED_SIZE.0,
            SAVED_SIZE.1,
            SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );
        IS_FULLSCREEN = false;

        // Update state
        let mut state = WINDOW_STATE.lock().unwrap();
        state.position = PhysicalPosition::new(SAVED_POSITION.0, SAVED_POSITION.1);
        state.size = PhysicalSize::new(SAVED_SIZE.0 as u32, SAVED_SIZE.1 as u32);
        state.rescale_needed = true;
    } else {
        // Save current position and size
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &raw mut rect).is_ok() {
            SAVED_POSITION = (rect.left, rect.top);
            SAVED_SIZE = (rect.right - rect.left, rect.bottom - rect.top);
        }

        // Get full screen dimensions (including taskbar)
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);

        // Set to fullscreen
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            screen_width,
            screen_height,
            SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );
        IS_FULLSCREEN = true;

        // Update state
        let mut state = WINDOW_STATE.lock().unwrap();
        state.position = PhysicalPosition::new(0, 0);
        state.size = PhysicalSize::new(screen_width as u32, screen_height as u32);
        state.rescale_needed = true;
    }
}

fn setup_click_through(window: &Window) {
    unsafe {
        let hwnd = match window.window_handle().unwrap().as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as _),
            _ => return,
        };

        // Store the original window procedure
        let old_proc = GetWindowLongPtrW(hwnd, GWL_WNDPROC);
        OLD_WNDPROC = Some(std::mem::transmute::<isize, WNDPROC>(old_proc));

        // Set our custom window procedure
        SetWindowLongPtrW(hwnd, GWL_WNDPROC, custom_wndproc as usize as isize);

        debug!("Click-through enabled");
    }
}
