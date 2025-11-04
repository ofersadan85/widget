use std::cell::Cell;
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM, COLORREF, POINT},
        Graphics::{
            Gdi::{
                CreateCompatibleDC, CreateCompatibleBitmap, DeleteDC, DeleteObject,
                GetDC, ReleaseDC, SelectObject, BitBlt, SRCCOPY,
                CreateSolidBrush, FillRect, Ellipse,
                CreatePen, PS_SOLID,
                BeginPaint, EndPaint, PAINTSTRUCT, InvalidateRect,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
};

// Animation & interaction state
thread_local! {
    static PHASE: Cell<f32> = Cell::new(0.0);
    static HOVER: Cell<bool> = Cell::new(false);
}

fn main() {
    unsafe {
        let h_instance = GetModuleHandleW(None).unwrap();
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
            100,
            100,
            400,
            300,
            None,
            None,
            h_instance,
            None,
        ).unwrap();

        let _ = ShowWindow(hwnd, SW_SHOW);
        
        // Set initial layered window attributes
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_COLORKEY | LWA_ALPHA).unwrap();
        
        let _ = PostMessageW(hwnd, WM_PAINT, WPARAM(0), LPARAM(0));

        // 60 FPS timer
        SetTimer(hwnd, 1, 16, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER => {
                PHASE.with(|p| p.set(p.get() + 0.05));
                
                // Check current cursor position to maintain hover state even when stationary
                let mut cursor_pos = POINT { x: 0, y: 0 };
                if GetCursorPos(&mut cursor_pos).is_ok() {
                    let mut window_rect = RECT::default();
                    if GetWindowRect(hwnd, &mut window_rect).is_ok() {
                        let pt_x = cursor_pos.x - window_rect.left;
                        let pt_y = cursor_pos.y - window_rect.top;
                        
                        // Check if cursor is within window bounds and over circle
                        if pt_x >= 0 && pt_x < 400 && pt_y >= 0 && pt_y < 300 {
                            let dx = pt_x as f32 - 200.0;
                            let dy = pt_y as f32 - 150.0;
                            
                            let current_radius = PHASE.with(|p| {
                                let phase = p.get();
                                60.0 + (phase.sin() * 30.0)
                            });
                            
                            let dist2 = dx * dx + dy * dy;
                            let inside = dist2 < current_radius * current_radius;
                            
                            HOVER.with(|h| h.set(inside));
                        } else {
                            HOVER.with(|h| h.set(false));
                        }
                    }
                }
                
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
            WM_NCHITTEST => {
                let x = (lparam.0 as i16) as i32;
                let y = ((lparam.0 >> 16) as i16) as i32;

                // Convert screen coordinates to window coordinates
                let mut window_rect = RECT::default();
                if GetWindowRect(hwnd, &mut window_rect).is_ok() {
                    let pt_x = x - window_rect.left;
                    let pt_y = y - window_rect.top;

                    // Our circle is centered at (200,150)
                    let dx = pt_x as f32 - 200.0;
                    let dy = pt_y as f32 - 150.0;

                    let current_radius = PHASE.with(|p| {
                        let phase = p.get();
                        60.0 + (phase.sin() * 30.0)
                    });
                    
                    let dist2 = dx * dx + dy * dy;
                    let inside = dist2 < current_radius * current_radius;

                    if inside {
                        LRESULT(HTCLIENT as isize)
                    } else {
                        LRESULT(HTTRANSPARENT as isize)
                    }
                } else {
                    LRESULT(HTTRANSPARENT as isize)
                }
            }
            WM_MOUSEMOVE => {
                // Timer will handle hover detection, just acknowledge the message
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                // Simple response when clicked - could add beep here later
                LRESULT(0)
            }
            WM_SETCURSOR => {
                // Always use arrow cursor to prevent busy cursor
                SetCursor(LoadCursorW(None, IDC_ARROW).unwrap_or_default());
                LRESULT(1) // Indicate we handled the cursor
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

    let width = 400;
    let height = 300;

    let hbitmap = CreateCompatibleBitmap(hdc_screen, width, height);
    let old_bmp = SelectObject(hdc_mem, hbitmap);

    // Clear background with black (will be made transparent)
    let rect = RECT { left: 0, top: 0, right: width, bottom: height };
    let bg_brush = CreateSolidBrush(COLORREF(0x00000000)); // Black background
    FillRect(hdc_mem, &rect, bg_brush);
    let _ = DeleteObject(bg_brush);

    // Animation state - get current values
    let (radius, base_alpha) = PHASE.with(|p| {
        let phase = p.get();
        let r = 60.0 + (phase.sin() * 30.0);
        let a = 0.6 + 0.4 * (phase.cos() * 0.5 + 0.5); // Animate alpha too
        (r, a)
    });

    // Hover highlight
    let hover = HOVER.with(|h| h.get());

    let color_value = if hover {
        0x0000FF00 // Pure green when hovering (BGR format)
    } else {
        0x000000FF // Pure red normally (BGR format)
    };

    // Create brush and pen for the circle
    let circle_brush = CreateSolidBrush(COLORREF(color_value));
    let circle_pen = CreatePen(PS_SOLID, 2, COLORREF(color_value));
    
    let old_brush = SelectObject(hdc_mem, circle_brush);
    let old_pen = SelectObject(hdc_mem, circle_pen);

    // Draw circle (ellipse)
    let center_x = 200;
    let center_y = 150;
    let radius_i = radius as i32;
    
    let _ = Ellipse(
        hdc_mem,
        center_x - radius_i,
        center_y - radius_i,
        center_x + radius_i,
        center_y + radius_i,
    );

    // Clean up drawing objects
    SelectObject(hdc_mem, old_brush);
    SelectObject(hdc_mem, old_pen);
    let _ = DeleteObject(circle_brush);
    let _ = DeleteObject(circle_pen);

    // Copy to screen efficiently
    let _ = BitBlt(hdc_screen, 0, 0, width, height, hdc_mem, 0, 0, SRCCOPY);

    // Update transparency
    let alpha = (base_alpha * 255.0) as u8;
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0x00000000), alpha, LWA_COLORKEY);

    // Clean up
    SelectObject(hdc_mem, old_bmp);
    let _ = DeleteObject(hbitmap);
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(hwnd, hdc_screen);
}
