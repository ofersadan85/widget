use clap::Parser;
use std::sync::{Arc, atomic::AtomicU64, mpsc};
use tracing::error;
use winit::event_loop::{ControlFlow, EventLoop};

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
        // Bounded so the decoder blocks until the window consumes a frame,
        // pacing decoding to real playback speed instead of racing ahead.
        let (frame_tx, frame_rx) = mpsc::sync_channel(2);
        let fps = Arc::new(AtomicU64::new(0.0f64.to_bits()));
        let app = App::new_with_stream(size_tx, frame_rx, fps.clone());
        let mut stream = FrameStream::new(filename, size_rx, frame_tx, fps);
        std::thread::spawn(move || {
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
