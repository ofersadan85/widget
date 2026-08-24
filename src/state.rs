use std::sync::{LazyLock, Mutex};
use tracing::debug;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winsafe::{GetSystemMetrics, HWND, HwndPlace, POINT, SIZE, co};

pub struct GlobalState {
    pub phase: f32,
    pub is_fullscreen: bool,
    pub old_position: PhysicalPosition<i32>,
    pub old_size: PhysicalSize<u32>,
    pub old_proc: Option<winsafe::WNDPROC>,
}

pub static GLOBAL_STATE: LazyLock<Mutex<GlobalState>> = LazyLock::new(|| {
    Mutex::new(GlobalState {
        phase: 0.0,
        is_fullscreen: false,
        old_position: PhysicalPosition::new(0, 0),
        old_size: PhysicalSize::new(800, 600),
        old_proc: None,
    })
});

pub fn custom_wndproc(hwnd: HWND, msg: co::WM, wparam: usize, lparam: isize) -> isize {
    if msg == co::WM::NCHITTEST {
        // Get the cursor position from lparam (screen coordinates)
        let screen_x = i32::from((lparam & 0xFFFF) as i16);
        let screen_y = i32::from(((lparam >> 16) & 0xFFFF) as i16);

        // Get window position to convert to window coordinates
        if let Ok(rect) = hwnd.GetWindowRect() {
            let window_x = screen_x - rect.left;
            let window_y = screen_y - rect.top;

            // Check if the cursor is over the circle
            let cursor_pos = PhysicalPosition::new(f64::from(window_x), f64::from(window_y));
            let rect = hwnd.GetWindowRect().unwrap_or_default();
            let center = PhysicalPosition {
                x: f64::from(rect.right - rect.left) / 2.0,
                y: f64::from(rect.bottom - rect.top) / 2.0,
            };
            let phase = {
                let state = GLOBAL_STATE.lock().unwrap();
                state.phase
            };
            if is_cursor_in_circle(center, phase, cursor_pos) {
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

    // Copy the original proc out of the global state before invoking it.
    // Avoid holding the mutex while calling back into Win32 because that proc can
    // synchronously dispatch additional window messages and re-enter this callback.
    let old_proc = {
        let state = GLOBAL_STATE.lock().unwrap();
        state.old_proc
    };

    if let Some(old_proc) = old_proc {
        old_proc(hwnd, msg, wparam, lparam)
    } else {
        0
    }
}

pub fn toggle_fullscreen(hwnd: &HWND) -> crate::Result<()> {
    let mut state = GLOBAL_STATE.lock().unwrap();
    let (point, size) = {
        // The mutex needs to be dropped before going across `SetWindowPos`
        // because the Moved/Resized events will try to lock it again.
        if state.is_fullscreen {
            // Restore to old position and size
            (
                POINT::with(state.old_position.x, state.old_position.y),
                SIZE::with(state.old_size.width as i32, state.old_size.height as i32),
            )
        } else {
            // Save current position and size before going fullscreen
            let rect = hwnd.GetWindowRect()?;
            state.old_position = PhysicalPosition::new(rect.left, rect.top);
            state.old_size = PhysicalSize::new(
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32,
            );
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
    drop(state); // Drop the mutex before calling SetWindowPos

    debug!(
        "Setting window position to ({}, {}), size ({}x{})",
        point.x, point.y, size.cx, size.cy
    );
    let resize_flags: co::SWP = co::SWP::FRAMECHANGED | co::SWP::NOACTIVATE;
    hwnd.SetWindowPos(HwndPlace::None, point, size, resize_flags)?;
    {
        let mut state = GLOBAL_STATE.lock().unwrap();
        state.is_fullscreen = !state.is_fullscreen;
    }
    Ok(())
}

pub fn is_cursor_in_circle(
    center: PhysicalPosition<f64>,
    phase: f32,
    cursor_pos: PhysicalPosition<f64>,
) -> bool {
    let dx = cursor_pos.x - center.x;
    let dy = cursor_pos.y - center.y;
    let radius = 60.0 + (f64::from(phase.sin()) * 30.0);
    dx * dx + dy * dy < radius * radius
}
