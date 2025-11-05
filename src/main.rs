#![allow(non_snake_case)]

use std::cell::Cell;

use winsafe::co::{ERROR, LWA, PS, ROP, SW, VK, WM, WS, WS_EX};
use winsafe::{
    AtomStr, DispatchMessage, GetCursorPos, GetMessage, IdMenu, PostQuitMessage, RegisterClassEx,
    SetLastError, SysResult, WString, COLORREF, HBRUSH, HINSTANCE, HPEN, HWND, MSG, POINT, RECT,
    SIZE, WNDCLASSEX,
};

// Animation & interaction state
thread_local! {
    pub static PHASE: Cell<f32> = Cell::new(0.0);
    pub static HOVER: Cell<bool> = Cell::new(false);
    pub static CURSOR: Cell<POINT> = Cell::new(POINT::default());
    pub static WINDOW_RECT: Cell<RECT> = Cell::new(RECT::default());
}

// Color constants
pub const ALPHA_KEY: COLORREF = COLORREF::from_rgb(0, 0, 0); // Black as transparent color
pub const RED: COLORREF = COLORREF::from_rgb(255, 0, 0);
pub const GREEN: COLORREF = COLORREF::from_rgb(0, 255, 0);
pub const BLUE: COLORREF = COLORREF::from_rgb(0, 0, 255);

pub const WINDOW_SIZE: SIZE = SIZE { cx: 300, cy: 300 };

pub const WINDOW_POSITION: POINT = POINT { x: 100, y: 100 };
pub const CIRCLE_CENTER: POINT = POINT {
    x: WINDOW_SIZE.cx / 2,
    y: WINDOW_SIZE.cy / 2,
};

fn draw_gdi(hwnd: &HWND) -> SysResult<()> {
    let hdc_screen = hwnd.GetDC()?;
    let hdc_mem = hdc_screen.CreateCompatibleDC()?;
    let h_bitmap = hdc_screen.CreateCompatibleBitmap(WINDOW_SIZE.cx, WINDOW_SIZE.cy)?;
    let _old_bmp = hdc_mem.SelectObject(&*h_bitmap)?;

    let bg_brush = HBRUSH::CreateSolidBrush(ALPHA_KEY)?;
    (*hdc_mem).FillRect(WINDOW_RECT.get(), &*bg_brush)?;

    // Animation state - get current values
    let phase = PHASE.get();
    let radius = 60.0 + (phase.sin() * 30.0);

    // Hover highlight
    let pen_color = if HOVER.get() { GREEN } else { RED };

    // Create brush and pen for the circle
    let circle_brush = HBRUSH::CreateSolidBrush(ALPHA_KEY)?;
    let circle_pen = HPEN::CreatePen(PS::SOLID, 3, pen_color)?;
    let _old_brush = hdc_mem.SelectObject(&*circle_brush)?;
    let _old_pen = hdc_mem.SelectObject(&*circle_pen)?;

    // Draw circle (ellipse)
    let radius_i = radius as i32;

    hdc_mem.Ellipse(RECT {
        left: CIRCLE_CENTER.x - radius_i,
        top: CIRCLE_CENTER.y - radius_i,
        right: CIRCLE_CENTER.x + radius_i,
        bottom: CIRCLE_CENTER.y + radius_i,
    })?;
    hdc_mem.Ellipse(RECT {
        left: CIRCLE_CENTER.x - 1,
        top: CIRCLE_CENTER.y - 1,
        right: CIRCLE_CENTER.x + 1,
        bottom: CIRCLE_CENTER.y + 1,
    })?;

    let blue_brush = HBRUSH::CreateSolidBrush(BLUE)?;
    let _ = hdc_mem.FrameRect(
        RECT {
            left: 0,
            top: 0,
            right: WINDOW_SIZE.cx,
            bottom: WINDOW_SIZE.cy,
        },
        &*blue_brush,
    )?;

    // Copy to screen efficiently
    hdc_screen.BitBlt(
        POINT::default(),
        WINDOW_SIZE,
        &*hdc_mem,
        POINT::default(),
        ROP::SRCCOPY,
    )?;
    Ok(())
}

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

extern "system" fn wndproc(hwnd: HWND, msg: WM, wparam: usize, _lparam: isize) -> isize {
    match msg {
        WM::TIMER => {
            PHASE.with(|p| p.set(p.get() + 0.05));
            CURSOR.set(GetCursorPos().unwrap_or_default());
            WINDOW_RECT.set(hwnd.GetWindowRect().unwrap_or_default());
            HOVER.set(cursor_in_circle());
            let _ = hwnd.InvalidateRect(None, false);
        }
        WM::PAINT => {
            if let Ok(_paint_guard) = hwnd.BeginPaint() {
                let _ = draw_gdi(&hwnd);
            }
        }
        // WM::NCHITTEST => {
        //     println!("WM::NCHITTEST received");
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
        //             HTCLIENT as isize
        //         } else {
        //             dbg!("Missed circle area");
        //             HTTRANSPARENT as isize
        //         }
        //     } else {
        //         dbg!("Failed to get window rect");
        //         HTTRANSPARENT as isize
        //     }
        // }
        WM::LBUTTONDOWN => {
            println!("Circle clicked!");
        }
        WM::SETCURSOR => {
            // Always use arrow cursor to prevent busy cursor
            // let sys_cursor = hwnd.hinstance().LoadCursor(IdIdcStr::Idc(IDC::ARROW));
            // if let Ok(sys_cursor) = sys_cursor {
            //     SetCursor(hwnd, sys_cursor);
            // }
            return 1; // Indicate we handled the cursor
        }
        WM::KEYDOWN if wparam == VK::ESCAPE.raw().into() => {
            hwnd.DestroyWindow().unwrap_or_default();
        }
        WM::DESTROY => {
            let _ = hwnd.KillTimer(1);
            PostQuitMessage(0);
        }
        // _ => return hwnd.DefWindowProc(msg, wparam, lparam),
        _ => {}
    }
    0
}

pub fn create_window(title: Option<&str>, position: POINT, size: SIZE) -> SysResult<HWND> {
    let h_instance = HINSTANCE::GetModuleHandle(None)?;
    let mut class_name = WString::from_str("MyInteractiveWidget");
    let mut wc = WNDCLASSEX::default();
    wc.hInstance = h_instance;
    wc.lpfnWndProc = Some(wndproc);
    wc.set_lpszClassName(Some(&mut class_name));

    // dbg!(wc.lpszClassName());
    // dbg!(wc.lpszMenuName());
    // dbg!(&wc.hInstance);
    // dbg!(wc.style);
    // dbg!(wc.lpfnWndProc);

    SetLastError(ERROR::SUCCESS);
    // SAFETY: Called SetLastError before to clear any previous error and provided window procedure.
    let class_atom = unsafe { RegisterClassEx(&wc)? };

    // dbg!("Window class registered: ************");
    // dbg!(class_atom);
    // dbg!(wc.lpszClassName());
    // dbg!(wc.lpszMenuName());
    // dbg!(&wc.hInstance);
    // dbg!(wc.style);
    // dbg!(wc.lpfnWndProc);

    // SAFETY: Window procedure is valid and class is registered. Messages will be handled there.
    let hwnd = unsafe {
        HWND::CreateWindowEx(
            WS_EX::LAYERED | WS_EX::TOOLWINDOW | WS_EX::TOPMOST,
            AtomStr::Atom(class_atom),
            title,
            WS::POPUP,
            position,
            size,
            None,
            IdMenu::None,
            &wc.hInstance,
            None,
        )?
    };

    dbg!("Window created: ************");
    Ok(hwnd)
}

fn main() -> SysResult<()> {
    let hwnd = create_window(
        Some("Transparent Widget Test"),
        WINDOW_POSITION,
        WINDOW_SIZE,
    )?;
    hwnd.ShowWindow(SW::SHOW);
    // Set initial layered window attributes
    hwnd.SetLayeredWindowAttributes(ALPHA_KEY, 255, LWA::COLORKEY | LWA::ALPHA)?;
    // 60 FPS timer
    hwnd.SetTimer(1, 16, None)?;

    let mut msg = MSG::default();
    while GetMessage(&mut msg, None, 0, 0)? {
        // let _ = TranslateMessage(&msg);
        unsafe { DispatchMessage(&msg) };
    }
    Ok(())
}
