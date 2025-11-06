use std::cell::Cell;
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection,
            CreatePen, CreateSolidBrush, DeleteDC, DeleteObject, Ellipse, EndPaint, FillRect,
            GetDC, InvalidateRect, ReleaseDC, SelectObject, SetDIBits, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, PAINTSTRUCT, PS_SOLID, SRCCOPY,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, MOD_CONTROL, VIRTUAL_KEY, VK_0,
                VK_A, VK_D, VK_ESCAPE, VK_LBUTTON, VK_S, VK_W,
            },
            WindowsAndMessaging::*,
        },
    },
};

// Animation & interaction state
thread_local! {
    static PHASE: Cell<f32> = Cell::new(0.0);
    static HOVER: Cell<bool> = Cell::new(false);
    static CURSOR: Cell<POINT> = Cell::new(POINT::default());
    static WINDOW_RECT: Cell<RECT> = Cell::new(RECT::default());
}

struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    const fn w(&self) -> i32 {
        self.width
    }

    const fn h(&self) -> i32 {
        self.height
    }
}

const WINDOW_SIZE: Size = Size {
    width: 400,
    height: 300,
};

const WINDOW_POSITION: POINT = POINT { x: 100, y: 100 };

const CIRCLE_CENTER: POINT = POINT {
    x: WINDOW_SIZE.w() / 2,
    y: WINDOW_SIZE.h() / 2,
};

fn main() -> windows::core::Result<()> {
    unsafe {
        let h_instance = GetModuleHandleW(None)?;
        let class_name = w!("InteractiveWidget");

        let wc = WNDCLASSW {
            hInstance: h_instance.into(),
            lpszClassName: class_name,
            lpfnWndProc: Some(wndproc),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST, // no WS_EX_TRANSPARENT now
            class_name,
            w!("Interactive Transparent Widget"),
            WS_POPUP,
            WINDOW_POSITION.x,
            WINDOW_POSITION.y,
            WINDOW_SIZE.w(),
            WINDOW_SIZE.h(),
            None,
            None,
            h_instance,
            None,
        )?;

        RegisterHotKey(hwnd, 999, MOD_CONTROL, VK_0.0 as u32)?;

        let _ = ShowWindow(hwnd, SW_SHOW);

        // Set initial layered window attributes
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 127, LWA_COLORKEY | LWA_ALPHA).unwrap();

        let _ = PostMessageW(hwnd, WM_PAINT, WPARAM(0), LPARAM(0));

        // 60 FPS timer
        SetTimer(hwnd, 1, 16, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            // let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

// fn cursor_in_rect(rect: &RECT) -> bool {
//     let pt = CURSOR.with(|c| c.get());
//     (rect.left..rect.right).contains(&pt.x) && (rect.top..rect.bottom).contains(&pt.y)
// }

// fn cursor_in_window() -> bool {
//     let rect = WINDOW_RECT.with(|wr| wr.get());
//     cursor_in_rect(&rect)
// }

const fn relative_position(pt: &POINT, rect: &RECT) -> POINT {
    POINT {
        x: pt.x - rect.left,
        y: pt.y - rect.top,
    }
}

fn cursor_in_circle() -> bool {
    let pt = relative_position(&CURSOR.get(), &WINDOW_RECT.get());
    let dx = (pt.x - CIRCLE_CENTER.x) as f32;
    let dy = (pt.y - CIRCLE_CENTER.y) as f32;
    let radius = PHASE.with(|phase| {
        let phase = phase.get();
        60.0 + (phase.sin() * 30.0)
    });
    dx * dx + dy * dy < radius * radius
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER => {
                PHASE.with(|p| p.set(p.get() + 0.05));
                CURSOR.with(|c| {
                    let mut cursor_pos = POINT::default();
                    if GetCursorPos(&mut cursor_pos).is_ok() {
                        c.set(cursor_pos);
                    }
                });
                WINDOW_RECT.with(|wr| {
                    let mut window_rect = RECT::default();
                    if GetWindowRect(hwnd, &mut window_rect).is_ok() {
                        wr.set(window_rect);
                    }
                });
                HOVER.set(cursor_in_circle());
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _hdc = BeginPaint(hwnd, &mut ps);
                draw_gdi(hwnd);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            // WM_NCHITTEST => {
            //     println!("WM_NCHITTEST received");
            //     let cursor_pos = POINT {
            //         x: (lparam.0 as i16) as i32,
            //         y: ((lparam.0 >> 16) as i16) as i32,
            //     };

            //     // Convert screen coordinates to window coordinates
            //     let mut window_rect = RECT::default();
            //     if GetWindowRect(hwnd, &mut window_rect).is_ok() {
            //         let cursor_pos = relative_position(&cursor_pos, &window_rect);
            //         if is_in_circle(&cursor_pos) {
            //             dbg!("Hit circle area");
            //             LRESULT(HTCLIENT as isize)
            //         } else {
            //             dbg!("Missed circle area");
            //             LRESULT(HTTRANSPARENT as isize)
            //         }
            //     } else {
            //         dbg!("Failed to get window rect");
            //         LRESULT(HTTRANSPARENT as isize)
            //     }
            // }
            WM_MOUSEMOVE => {
                // Timer will handle hover detection, just acknowledge the message

                // Handle dragging if needed
                if GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 {
                    let current_pos = CURSOR.get();
                    let new_x = current_pos.x - (WINDOW_SIZE.w() / 2);
                    let new_y = current_pos.y - (WINDOW_SIZE.h() / 2);
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOP,
                        new_x,
                        new_y,
                        WINDOW_SIZE.w(),
                        WINDOW_SIZE.h(),
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                // Simple response when clicked - could add beep here later
                println!("Circle clicked!");
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
                println!("Hotkey pressed!");
                LRESULT(0)
            }
            WM_KEYDOWN => handle_keys(hwnd, VIRTUAL_KEY(wparam.0 as u16), lparam),
            WM_DESTROY => {
                let _ = UnregisterHotKey(hwnd, 999);
                let _ = KillTimer(hwnd, 1);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn handle_keys(hwnd: HWND, key: VIRTUAL_KEY, lparam: LPARAM) -> LRESULT {
    let mut movement = POINT::default();
    match key {
        VK_ESCAPE => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        VK_W => movement.y -= 10,
        VK_S => movement.y += 10,
        VK_A => movement.x -= 10,
        VK_D => movement.x += 10,
        _ => {}
    }
    if movement != POINT::default() {
        let current = WINDOW_RECT.get();
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOP,
                current.left + movement.x,
                current.top + movement.y,
                WINDOW_SIZE.w(),
                WINDOW_SIZE.h(),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }
    LRESULT(0)
}

unsafe fn draw_gdi(hwnd: HWND) {
    let hdc_screen = GetDC(hwnd);
    let hdc_mem = CreateCompatibleDC(hdc_screen);

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: WINDOW_SIZE.w(),
            biHeight: -WINDOW_SIZE.h(),
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
        &bmi,
        DIB_RGB_COLORS,
        &mut bits_ptr,
        HANDLE::default(),
        0,
    )
    .unwrap_or_default();
    let old_bmp = SelectObject(hdc_mem, hbitmap);

    if !bits_ptr.is_null() {
        let buffer_size = (WINDOW_SIZE.w() * WINDOW_SIZE.h() * 4) as usize;
        let dst = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, buffer_size);

        // Fill with arbitrary image data (BGRA)
        let red = ((PHASE.get() % 1.0) * 255.0) as u8;
        for y in 0..WINDOW_SIZE.h() {
            for x in 0..WINDOW_SIZE.w() {
                let i = ((y * WINDOW_SIZE.w() + x) * 4) as usize;
                dst[i + 0] = (x % 255) as u8; // Blue
                dst[i + 1] = (y % 255) as u8; // Green
                dst[i + 2] = red;
                // dst[i + 3] = 255; // Alpha (ignored in SRCCOPY)
            }
        }
    }

    let pen_color = if HOVER.get() {
        COLORREF(0x00FF00) // Green when hovered
    } else {
        COLORREF(0x0000FF) // Blue otherwise
    };
    let hpen = CreatePen(PS_SOLID, 3, pen_color);
    let old_pen = SelectObject(hdc_mem, hpen);
    let brush = CreateSolidBrush(COLORREF(0x00000000));
    let old_brush = SelectObject(hdc_mem, brush);
    let radius = PHASE.with(|phase| {
        let phase = phase.get();
        60.0 + (phase.sin() * 30.0)
    }) as i32;
    let _ = Ellipse(
        hdc_mem,
        CIRCLE_CENTER.x - radius,
        CIRCLE_CENTER.y - radius,
        CIRCLE_CENTER.x + radius,
        CIRCLE_CENTER.y + radius,
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
        WINDOW_SIZE.w(),
        WINDOW_SIZE.h(),
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
