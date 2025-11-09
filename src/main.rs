use tracing::{debug, error};
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreatePen, CreateSolidBrush,
            DeleteDC, DeleteObject, Ellipse, EndPaint, GetDC, InvalidateRect, ReleaseDC,
            SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, PAINTSTRUCT,
            PS_SOLID, SRCCOPY,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, MOD_CONTROL, VIRTUAL_KEY, VK_0,
                VK_A, VK_D, VK_ESCAPE, VK_LBUTTON, VK_S, VK_W,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
                GetMessageW, GetWindowRect, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage,
                RegisterClassW, SetCursor, SetForegroundWindow, SetLayeredWindowAttributes,
                SetTimer, SetWindowPos, ShowWindow, TranslateMessage, HWND_TOP, IDC_ARROW,
                LWA_ALPHA, LWA_COLORKEY, MSG, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOW, WM_DESTROY,
                WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR,
                WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
};

mod error;
mod ff;
mod state;
use error::Result;
use ff::FrameStream;
use state::{WindowState, FRAME_SYNC, WINDOW_STATE};

const HOTKEY_ID: i32 = 999;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    ffmpeg_next::init()?;

    std::thread::spawn(move || {
        if let Err(e) = FrameStream::new("sample-5.mp4").and_then(|mut s| {
            WINDOW_STATE.lock().unwrap().fps = s.fps;
            s.read_frames()
        }) {
            error!("Error in FFmpeg thread: {}", e);
        }
    });

    unsafe {
        let h_instance = GetModuleHandleW(None)?;
        let class_name = w!("InteractiveWidget");

        let wc = WNDCLASSW {
            hInstance: h_instance.into(),
            lpszClassName: class_name,
            lpfnWndProc: Some(wndproc),
            ..Default::default()
        };
        RegisterClassW(&raw const wc);

        let (hwnd, frame_ms) = {
            let state = WINDOW_STATE.lock().unwrap();
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST, // no WS_EX_TRANSPARENT now
                class_name,
                w!("Interactive Transparent Widget"),
                WS_POPUP,
                state.position().x,
                state.position().y,
                state.size().x,
                state.size().y,
                None,
                None,
                h_instance,
                None,
            )?;
            (hwnd, (1000 / state.fps) as u32)
        };

        RegisterHotKey(hwnd, HOTKEY_ID, MOD_CONTROL, u32::from(VK_0.0))?;

        let _ = ShowWindow(hwnd, SW_SHOW);

        // Set initial layered window attributes
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 127, LWA_COLORKEY | LWA_ALPHA)?;

        let _ = PostMessageW(hwnd, WM_PAINT, WPARAM(0), LPARAM(0));

        debug!("Setting timer with interval {frame_ms} ms");
        SetTimer(hwnd, 1, frame_ms, None);

        let mut msg = MSG::default();
        while GetMessageW(&raw mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
    }
    Ok(())
}

const fn relative_position(pt: POINT, rect: &RECT) -> POINT {
    POINT {
        x: pt.x - rect.left,
        y: pt.y - rect.top,
    }
}

fn cursor_in_circle(state: &WindowState) -> bool {
    let pt = relative_position(state.cursor, &state.rect);
    let dx = (pt.x - state.center().x) as f32;
    let dy = (pt.y - state.center().y) as f32;
    let radius = 60.0 + (state.phase.sin() * 30.0);
    dx * dx + dy * dy < radius * radius
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER => {
                let mut state = WINDOW_STATE.lock().unwrap();
                state.phase += 0.05;
                let mut cursor_pos = POINT::default();
                if GetCursorPos(&raw mut cursor_pos).is_ok() {
                    state.cursor = cursor_pos;
                }
                let mut window_rect = RECT::default();
                if GetWindowRect(hwnd, &raw mut window_rect).is_ok() {
                    state.rect = window_rect;
                }
                state.hover = cursor_in_circle(&state);
                FRAME_SYNC.notify_one();
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            WM_PAINT => {
                debug!("WM_PAINT received");
                let mut ps = PAINTSTRUCT::default();
                let _hdc = BeginPaint(hwnd, &raw mut ps);
                draw_gdi(hwnd);
                let _ = EndPaint(hwnd, &raw const ps);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0 {
                    let state = WINDOW_STATE.lock().unwrap();
                    let new_x = state.cursor.x - (state.size().x / 2);
                    let new_y = state.cursor.y - (state.size().y / 2);
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOP,
                        new_x,
                        new_y,
                        state.size().x,
                        state.size().y,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                // Simple response when clicked - could add beep here later
                debug!("Circle clicked!");
                LRESULT(0)
            }
            WM_SETCURSOR => {
                // Always use arrow cursor to prevent busy cursor
                SetCursor(LoadCursorW(None, IDC_ARROW).unwrap_or_default());
                LRESULT(1) // Indicate we handled the cursor
            }
            WM_HOTKEY => {
                // Focus the window when hotkey is pressed
                let _ = SetForegroundWindow(hwnd);
                debug!("Hotkey pressed!");
                LRESULT(0)
            }
            WM_KEYDOWN => handle_keys(hwnd, VIRTUAL_KEY(wparam.0 as u16), lparam),
            WM_DESTROY => {
                let _ = UnregisterHotKey(hwnd, HOTKEY_ID);
                let _ = KillTimer(hwnd, 1);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn handle_keys(hwnd: HWND, key: VIRTUAL_KEY, lparam: LPARAM) -> LRESULT {
    let mut handled = true;
    let mut movement = POINT::default();
    match key {
        VK_ESCAPE => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        VK_W => movement.y -= 10,
        VK_S => movement.y += 10,
        VK_A => movement.x -= 10,
        VK_D => movement.x += 10,
        _ => {
            handled = false;
        }
    }
    if movement != POINT::default() {
        let current = WINDOW_STATE.lock().unwrap();
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOP,
                current.rect.left + movement.x,
                current.rect.top + movement.y,
                current.size().x,
                current.size().y,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }
    if handled {
        LRESULT(0)
    } else {
        unsafe { DefWindowProcW(hwnd, WM_KEYDOWN, WPARAM(key.0 as usize), lparam) }
    }
}

unsafe fn draw_gdi(hwnd: HWND) {
    let hdc_screen = GetDC(hwnd);
    let hdc_mem = CreateCompatibleDC(hdc_screen);

    let state = WINDOW_STATE.lock().unwrap();
    let _ = SetWindowPos(
        hwnd,
        HWND_TOP,
        state.rect.left,
        state.rect.top,
        state.size().x.max(state.frame.width() as i32),
        state.size().y.max(state.frame.height() as i32),
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    debug!(
        "Drawing at position ({}, {}), size ({}, {})",
        state.position().x,
        state.position().y,
        state.size().x,
        state.size().y
    );

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: state.size().x,
            biHeight: -state.size().y, // top-down
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
        let buffer_size = (state.size().x * state.size().y * 4) as usize;
        let dst = std::slice::from_raw_parts_mut(bits_ptr.cast::<u8>(), buffer_size);

        // Fill with arbitrary image data (BGRA)
        dbg!(buffer_size);
        if state.frame.is_empty() {
            debug!("No frame data available");
            let red = ((state.phase % 1.0) * 255.0) as u8;
            for y in 0..state.size().y {
                for x in 0..state.size().x {
                    let i = ((y * state.size().x + x) * 4) as usize;
                    dst[i] = (x % 255) as u8; // Blue
                    dst[i + 1] = (y % 255) as u8; // Green
                    dst[i + 2] = red;
                    // dst[i + 3] = 255; // Alpha (ignored in SRCCOPY)
                }
            }
        } else {
            debug!("{}", state.frame.data(0).len());
            dst.copy_from_slice(&state.frame.data(0)[..buffer_size]);
        }
    }

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
    let _ = Ellipse(
        hdc_mem,
        state.center().x - radius,
        state.center().y - radius,
        state.center().x + radius,
        state.center().y + radius,
    );
    let _ = SelectObject(hdc_mem, old_pen);
    let _ = DeleteObject(hpen);
    let _ = SelectObject(hdc_mem, old_brush);
    let _ = DeleteObject(brush);

    // Blit to screen
    let _ = BitBlt(
        hdc_screen,
        0,
        0,
        state.size().x,
        state.size().y,
        hdc_mem,
        0,
        0,
        SRCCOPY,
    );

    // Clean up
    SelectObject(hdc_mem, old_bmp);
    let _ = DeleteObject(hbitmap);
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(hwnd, hdc_screen);
}
