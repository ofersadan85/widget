use std::cell::Cell;
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen,
            CreateSolidBrush, DeleteDC, DeleteObject, Ellipse, EndPaint, FillRect, GetDC,
            InvalidateRect, ReleaseDC, SelectObject, PAINTSTRUCT, PS_SOLID, SRCCOPY,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{Input::KeyboardAndMouse::VK_ESCAPE, WindowsAndMessaging::*},
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

        let _ = ShowWindow(hwnd, SW_SHOW);

        // Set initial layered window attributes
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_COLORKEY | LWA_ALPHA).unwrap();

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
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(hwnd, 1);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

unsafe fn draw_gdi(hwnd: HWND) {
    let hdc_screen = GetDC(hwnd);
    let hdc_mem = CreateCompatibleDC(hdc_screen);

    let hbitmap = CreateCompatibleBitmap(hdc_screen, WINDOW_SIZE.w(), WINDOW_SIZE.h());
    let old_bmp = SelectObject(hdc_mem, hbitmap);

    let bg_brush = CreateSolidBrush(COLORREF(0x00000000)); // Black background
    FillRect(hdc_mem, &WINDOW_RECT.get(), bg_brush);
    let _ = DeleteObject(bg_brush);

    // Animation state - get current values
    let (radius, base_alpha) = PHASE.with(|p| {
        let phase = p.get();
        let r = 60.0 + (phase.sin() * 30.0);
        let a = 0.6 + 0.4 * (phase.cos() * 0.5 + 0.5); // Animate alpha too
        (r, a)
    });

    // Hover highlight
    let red = 0x000000FF; // Red in BGR format
    let green = 0x0000FF00; // Green in BGR format
    let color_value = if HOVER.get() { green } else { red };

    // Create brush and pen for the circle
    // let fill = COLORREF(0x00FFFFFF); // White fill
    let fill = COLORREF(0x00000000); // Transparent fill
    let circle_brush = CreateSolidBrush(fill);
    let circle_pen = CreatePen(PS_SOLID, 3, COLORREF(color_value));

    let old_brush = SelectObject(hdc_mem, circle_brush);
    let old_pen = SelectObject(hdc_mem, circle_pen);

    // Draw circle (ellipse)
    let radius_i = radius as i32;

    let _ = Ellipse(
        hdc_mem,
        CIRCLE_CENTER.x - radius_i,
        CIRCLE_CENTER.y - radius_i,
        CIRCLE_CENTER.x + radius_i,
        CIRCLE_CENTER.y + radius_i,
    );

    // Clean up drawing objects
    SelectObject(hdc_mem, old_brush);
    SelectObject(hdc_mem, old_pen);
    let _ = DeleteObject(circle_brush);
    let _ = DeleteObject(circle_pen);

    // Copy to screen efficiently
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

    // Update transparency
    let alpha = (base_alpha * 255.0) as u8;
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0x00000000), alpha, LWA_COLORKEY);

    // Clean up
    SelectObject(hdc_mem, old_bmp);
    let _ = DeleteObject(hbitmap);
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(hwnd, hdc_screen);
}
