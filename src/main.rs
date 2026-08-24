use std::sync::mpsc;
use tracing::error;
use winit::event_loop::{ControlFlow, EventLoop};
use clap::Parser;

mod colors;
mod error;
mod ff;
mod state;
mod window;

use crate::{error::Result, ff::FrameStream, window::App};

#[derive(Parser, Debug)]
struct Args {
    /// Path to the video file to play
    file: Option<std::path::PathBuf>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    ffmpeg_next::init()?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let args = Args::parse();
    let mut app = if let Some(filename) = args.file {
        let (size_tx, size_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let app = App::new_with_stream(size_tx, frame_rx);
        let mut stream = FrameStream::new(filename, size_rx, frame_tx);
        std::thread::spawn(move || {
            // app.fps = stream.fps; // TODO: Need to somehow sync FPS during read_frames
            if let Err(e) = stream.read_frames() {
                error!("Error in FFmpeg thread: {e}");
            }
        });
        app
    } else {
        App::new()
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
