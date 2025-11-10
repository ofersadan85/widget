use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    codec, format, frame, media,
    software::scaling::{context::Context as Scaler, flag::Flags},
    util::format::pixel,
};
use tracing::{debug, info, trace, warn};

use crate::error::Result;
use crate::state::{FRAME_SYNC, WINDOW_STATE};

pub struct FrameStream {
    pub input: format::context::Input,
    pub video_index: usize,
    pub decoder: codec::decoder::Video,
    pub scaler: Scaler,
    pub fps: i32,
}

impl FrameStream {
    pub fn new(input: &str) -> Result<Self> {
        let input = format::input(&input)?;
        let video = input
            .streams()
            .best(media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let video_index = video.index();
        let fps = video.avg_frame_rate().0;
        let codec_params = video.parameters();
        let decoder = if let Some(hw_codec) = codec::decoder::find_by_name("h264_cuvid") {
            info!("✅ Using CUDA hardware decoder: h264_cuvid");
            let mut ctx = codec::context::Context::new_with_codec(hw_codec);
            ctx.set_parameters(codec_params)?;
            ctx.decoder().video()?
        } else {
            warn!("⚠️ Hardware decoder not available, using software decoder");
            let codec = codec::context::Context::from_parameters(codec_params)?;
            codec.decoder().video()?
        };

        // Get initial window size for scaling
        let window_size = {
            let state = WINDOW_STATE.lock().unwrap();
            (state.size.width, state.size.height)
        };

        let scaler = Scaler::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            pixel::Pixel::BGRA,
            window_size.0,
            window_size.1,
            Flags::BILINEAR,
        )?;
        Ok(Self {
            input,
            video_index,
            decoder,
            scaler,
            fps,
        })
    }

    pub fn read_frames(&mut self) -> Result<()> {
        let mut frame = frame::Video::empty();
        let mut i = 0;
        for (stream, packet) in self.input.packets() {
            if stream.index() == self.video_index {
                self.decoder.send_packet(&packet)?;
                while self.decoder.receive_frame(&mut frame).is_ok() {
                    // Check if we need to recreate the scaler due to window resize
                    let rescale_needed = {
                        let state = WINDOW_STATE.lock().unwrap();
                        state.rescale_needed
                    };

                    if rescale_needed {
                        let (new_width, new_height) = {
                            let mut state = WINDOW_STATE.lock().unwrap();
                            state.rescale_needed = false;
                            (state.size.width, state.size.height)
                        };

                        debug!(
                            "Recreating scaler for new size: {}x{}",
                            new_width, new_height
                        );
                        self.scaler = Scaler::get(
                            self.decoder.format(),
                            self.decoder.width(),
                            self.decoder.height(),
                            pixel::Pixel::BGRA,
                            new_width,
                            new_height,
                            Flags::BILINEAR,
                        )?;
                    }

                    // Always create a fresh output frame for scaling
                    let mut rgb_frame = frame::Video::empty();
                    self.scaler.run(&frame, &mut rgb_frame)?;

                    let mut state = FRAME_SYNC.wait(WINDOW_STATE.lock().unwrap()).unwrap();
                    state.frame.clone_from(&rgb_frame);
                    trace!("Frame\t{i}");
                    i += 1;
                }
            }
        }
        self.decoder.send_eof()?;
        debug!("End of stream reached");
        Ok(())
    }
}
