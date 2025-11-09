use std::sync::{LazyLock, Mutex};
use windows::Win32::Foundation::{POINT, RECT};

pub static WINDOW_STATE: LazyLock<Mutex<WindowState>> =
    LazyLock::new(|| Mutex::new(WindowState::default()));

pub struct WindowState {
    pub title: String,
    pub hover: bool,
    pub phase: f32,
    pub cursor: POINT,
    pub rect: RECT,
    pub frame: ffmpeg_next::frame::Video,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            title: String::from("AmazingWidget"),
            hover: false,
            phase: 0.0,
            cursor: POINT::default(),
            rect: RECT {
                left: 100,
                top: 100,
                right: 400,
                bottom: 400,
            },
            frame: ffmpeg_next::frame::Video::empty(),
        }
    }
}

impl WindowState {
    pub fn position(&self) -> POINT {
        POINT {
            x: self.rect.left,
            y: self.rect.top,
        }
    }

    pub fn size(&self) -> POINT {
        POINT {
            x: self.rect.right - self.rect.left,
            y: self.rect.bottom - self.rect.top,
        }
    }

    pub fn center(&self) -> POINT {
        POINT {
            x: self.size().x / 2,
            y: self.size().y / 2,
        }
    }
}
