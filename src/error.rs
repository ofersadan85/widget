#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    FFmpeg(#[from] ffmpeg_next::Error),
    #[error("Windows error: {0}")]
    Windows(String),
    #[error(transparent)]
    EventLoop(#[from] winit::error::EventLoopError),
}

impl From<winsafe::co::ERROR> for Error {
    fn from(e: winsafe::co::ERROR) -> Self {
        Error::Windows(format!("Error code: {}", e.raw()))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
