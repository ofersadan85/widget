#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    FFmpeg(#[from] ffmpeg_next::Error),
    #[error(transparent)]
    Winsafe(#[from] winsafe::co::ERROR),
    #[error(transparent)]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error(transparent)]
    SyncSend(#[from] std::sync::mpsc::SendError<ffmpeg_next::frame::Video>),
}

pub type Result<T> = std::result::Result<T, Error>;
