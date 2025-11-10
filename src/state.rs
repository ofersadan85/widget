use std::sync::{Condvar, LazyLock, Mutex};
use winit::dpi::{PhysicalPosition, PhysicalSize};

pub static FRAME_SYNC: LazyLock<Condvar> = LazyLock::new(Condvar::new);

pub static WINDOW_STATE: LazyLock<Mutex<WindowState>> =
    LazyLock::new(|| Mutex::new(WindowState::default()));

pub struct WindowState {
    pub title: String,
    pub hover: bool,
    pub phase: f32,
    pub cursor: PhysicalPosition<f64>,
    pub position: PhysicalPosition<i32>,
    pub size: PhysicalSize<u32>,
    pub frame: ffmpeg_next::frame::Video,
    pub fps: i32,
    pub rescale_needed: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            title: String::from("AmazingWidget"),
            hover: false,
            phase: 0.0,
            cursor: PhysicalPosition::new(0.0, 0.0),
            position: PhysicalPosition::new(900, 100),
            size: PhysicalSize::new(400, 300),
            frame: ffmpeg_next::frame::Video::empty(),
            fps: 30,
            rescale_needed: false,
        }
    }
}

impl WindowState {
    pub fn center(&self) -> PhysicalPosition<f64> {
        PhysicalPosition {
            x: f64::from(self.size.width) / 2.0,
            y: f64::from(self.size.height) / 2.0,
        }
    }
}
