use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    codec, format, frame, media,
    software::scaling::{context::Context as Scaler, flag::Flags},
    util::format::pixel::Pixel,
};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use tracing::{debug, info, trace, warn};
use winit::dpi::PhysicalSize;

use crate::error::Result;

pub struct FrameStream {
    pub input: PathBuf,
    pub fps: Arc<AtomicU64>,
    pub size_sync: mpsc::Receiver<PhysicalSize<u32>>,
    pub frame_sync: mpsc::SyncSender<frame::Video>,
    pub frame_buffer_before: frame::Video,
}

impl FrameStream {
    pub fn new(
        input: PathBuf,
        size_sync: mpsc::Receiver<PhysicalSize<u32>>,
        frame_sync: mpsc::SyncSender<frame::Video>,
        fps: Arc<AtomicU64>,
    ) -> Self {
        Self {
            input,
            fps,
            size_sync,
            frame_sync,
            frame_buffer_before: frame::Video::empty(),
        }
    }

    pub fn read_frames(&mut self) -> Result<()> {
        let mut input = format::input(&self.input)?;
        let video = input
            .streams()
            .best(media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let video_index = video.index();
        let frame_rate = video.avg_frame_rate();
        let fps = if frame_rate.1 != 0 {
            f64::from(frame_rate.0) / f64::from(frame_rate.1)
        } else {
            0.0
        };
        self.fps.store(fps.to_bits(), Ordering::Relaxed);
        let codec_params = video.parameters();
        let mut decoder = if let Some(hw_codec) = codec::decoder::find_by_name("h264_cuvid") {
            info!("✅ Using CUDA hardware decoder: h264_cuvid");
            let mut ctx = codec::context::Context::new_with_codec(hw_codec);
            ctx.set_parameters(codec_params)?;
            ctx.decoder().video()?
        } else {
            warn!("⚠️ Hardware decoder not available, using software decoder");
            let codec = codec::context::Context::from_parameters(codec_params)?;
            codec.decoder().video()?
        };
        let mut scaler = Scaler::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::BGRA,
            1,
            1,
            Flags::BILINEAR,
        )?;
        let mut i = 0;
        for (stream, packet) in input.packets() {
            if stream.index() == video_index {
                decoder.send_packet(&packet)?;
                while decoder.receive_frame(&mut self.frame_buffer_before).is_ok() {
                    while let Ok(new_size) = self.size_sync.try_recv() {
                        let output = scaler.output();
                        if output.width != new_size.width || output.height != new_size.height {
                            // We need to recreate the scaler due to window resize
                            scaler = Scaler::get(
                                decoder.format(),
                                decoder.width(),
                                decoder.height(),
                                Pixel::BGRA,
                                new_size.width,
                                new_size.height,
                                Flags::BILINEAR,
                            )?;
                        }
                    }
                    // Scale into a fresh, uniquely-owned frame and move it into the
                    // channel. Scaler::run allocates its output when given an empty
                    // frame, so this needs no reused buffer to clone before sending -
                    // the clone (a full pixel-buffer copy) is skipped entirely.
                    let mut output_frame = frame::Video::empty();
                    scaler.run(&self.frame_buffer_before, &mut output_frame)?;
                    self.frame_sync.send(output_frame)?;
                    trace!("Frame\t{i}");
                    i += 1;
                }
            }
        }
        decoder.send_eof()?;
        debug!("End of stream reached");
        Ok(())
    }
}
