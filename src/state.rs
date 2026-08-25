use crate::overlay::hit_test_circle;
use std::sync::{
    LazyLock, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tracing::debug;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winsafe::{GetSystemMetrics, HWND, HwndPlace, POINT, SIZE, co};

pub struct GlobalState {
    pub phase: f64,
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

#[derive(Debug, Default)]
pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    pub const fn new(value: f64) -> Self {
        Self(AtomicU64::new(value.to_bits()))
    }

    pub fn load(&self, order: Ordering) -> f64 {
        f64::from_bits(self.0.load(order))
    }

    pub fn store(&self, value: f64, order: Ordering) {
        self.0.store(value.to_bits(), order);
    }
}

pub fn custom_wndproc(hwnd: HWND, msg: co::WM, wparam: usize, lparam: isize) -> isize {
    if msg == co::WM::NCHITTEST {
        // Get the cursor position from lparam (screen coordinates)
        // We can `unwrap_or_default` here because the values for an NCHITTEST message are always
        // valid as i32, even if the cursor is off-screen.
        let screen_x = i32::try_from(lparam & 0xFFFF).unwrap_or_default();
        let screen_y = i32::try_from((lparam >> 16) & 0xFFFF).unwrap_or_default();

        // Get window position to convert to window coordinates
        if let Ok(rect) = hwnd.GetWindowRect() {
            let cursor_pos = PhysicalPosition::new(
                f64::from(screen_x - rect.left),
                f64::from(screen_y - rect.top),
            );
            let center = PhysicalPosition {
                x: f64::from(rect.right - rect.left) / 2.0,
                y: f64::from(rect.bottom - rect.top) / 2.0,
            };
            let phase = {
                let state = GLOBAL_STATE.lock().unwrap();
                state.phase
            };
            return isize::from(hit_test_circle(center, phase, cursor_pos).cast_signed());
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
    old_proc.map_or(0, |old_proc| old_proc(hwnd, msg, wparam, lparam))
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
                SIZE::with(
                    state.old_size.width.cast_signed(),
                    state.old_size.height.cast_signed(),
                ),
            )
        } else {
            // Save current position and size before going fullscreen
            let rect = hwnd.GetWindowRect()?;
            state.old_position = PhysicalPosition::new(rect.left, rect.top);
            state.old_size = PhysicalSize::new(
                rect.right.abs_diff(rect.left),
                rect.bottom.abs_diff(rect.top),
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
