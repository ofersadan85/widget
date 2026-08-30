use crate::overlay::hit_test_circle;
use std::sync::{
    LazyLock, Mutex,
    atomic::{AtomicU64, Ordering},
};
use winit::dpi::PhysicalPosition;
use winsafe::{HWND, co};

pub struct GlobalState {
    pub phase: f64,
    pub old_proc: Option<winsafe::WNDPROC>,
}

pub static GLOBAL_STATE: LazyLock<Mutex<GlobalState>> = LazyLock::new(|| {
    Mutex::new(GlobalState {
        phase: 0.0,
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

    // Copy the original proc out of the global state before invoking it.
    // Avoid holding the mutex while calling back into Win32 because that proc can
    // synchronously dispatch additional window messages and re-enter this callback.
    let old_proc = {
        let state = GLOBAL_STATE.lock().unwrap();
        state.old_proc
    };
    old_proc.map_or(0, |old_proc| old_proc(hwnd, msg, wparam, lparam))
}
