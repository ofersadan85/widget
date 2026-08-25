use clap::Parser;

mod colors;
mod error;
mod ff;
mod overlay;
mod state;
mod window;

use crate::{error::Result, window::App};

#[derive(Parser, Debug)]
struct Args {
    /// Path to the video file to play
    file: Option<std::path::PathBuf>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let mut app = if let Some(file) = args.file {
        App::new_with_stream(file)?
    } else {
        App::new()?
    };
    app.run()?;
    Ok(())
}
