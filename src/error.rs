#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    FFmpeg(#[from] ffmpeg_next::Error),
    #[error(transparent)]
    Winsafe(#[from] winsafe::co::ERROR),
    // #[error(transparent)]
    // WindowHandleError(#[from] winit::window::WindowHandleError),
    #[error(transparent)]
    EventLoop(#[from] winit::error::EventLoopError),
}

pub type Result<T> = std::result::Result<T, Error>;
