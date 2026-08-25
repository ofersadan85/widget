#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    FFmpeg(#[from] ffmpeg_next::Error),
    #[error(transparent)]
    Winsafe(#[from] winsafe::co::ERROR),
    #[error(transparent)]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error(transparent)]
    SyncFrameSend(#[from] std::sync::mpsc::SendError<crate::ff::DecodedFrame>),
    #[error(transparent)]
    Fmt(#[from] std::fmt::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
