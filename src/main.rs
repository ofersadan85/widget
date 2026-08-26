use clap::Parser;
use color_eyre::eyre::{Context, Result};

mod colors;
mod ff;
mod overlay;
mod state;
mod window;

use crate::window::App;

#[derive(Parser, Debug)]
struct Args {
    /// Path to the video file to play
    file: Option<std::path::PathBuf>,

    /// Automatically close the application after the video finishes
    #[clap(long)]
    autoclose: bool,
}

fn main() -> Result<()> {
    color_eyre::install().wrap_err("failed to install global error handler")?;
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let mut app = if let Some(file) = args.file {
        App::new_with_stream(&file, args.autoclose)
            .wrap_err_with(|| format!("failed to initialize app for video '{}'", file.display()))?
    } else {
        App::new().wrap_err("failed to initialize animation-only app")?
    };
    app.run().wrap_err("winit application event loop failed")?;
    Ok(())
}
