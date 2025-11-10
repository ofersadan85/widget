#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    FFmpeg(#[from] ffmpeg_next::Error),
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
    #[error(transparent)]
    EventLoop(#[from] winit::error::EventLoopError),
}

pub type Result<T> = std::result::Result<T, Error>;
