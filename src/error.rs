#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    FFmpeg(#[from] ffmpeg_next::Error),
    #[error(transparent)]
    WinError(#[from] windows::core::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
