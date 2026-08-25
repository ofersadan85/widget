use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    Rational, codec, format, frame, media,
    software::scaling::{context::Context as Scaler, flag::Flags},
    util::format::pixel::Pixel,
};
use std::path::PathBuf;
use std::sync::{Arc, atomic::Ordering, mpsc};
use tracing::{debug, info, trace, warn};
use winit::dpi::PhysicalSize;

use crate::{error::Result, state::AtomicF64};

/// `FFmpeg`'s internal timestamp unit for APIs that aren't tied to a stream's
/// own `time_base` (`Input::seek`, `Input::duration`), in ticks per second.
const AV_TIME_BASE: f64 = 1_000_000.0;

/// Playback control sent from the window thread to the decode thread.
#[derive(Debug, Clone, Copy)]
pub enum PlaybackCommand {
    TogglePause,
    /// Absolute target position, in seconds.
    Seek(f64),
}

/// A decoded, scaled video frame paired with its presentation timestamp (in
/// seconds), so the window never has to reach across threads for position.
pub struct DecodedFrame {
    pub frame: frame::Video,
    pub pts_seconds: f64,
}

impl Default for DecodedFrame {
    fn default() -> Self {
        Self {
            frame: frame::Video::empty(),
            pts_seconds: 0.0,
        }
    }
}

pub struct FrameStream {
    input: PathBuf,
    paused: bool,
    fps: Arc<AtomicF64>,
    duration: Arc<AtomicF64>,
    size_sync: mpsc::Receiver<PhysicalSize<u32>>,
    frame_sync: mpsc::SyncSender<DecodedFrame>,
    command_sync: mpsc::Receiver<PlaybackCommand>,
    frame_buffer_before: frame::Video,
    last_sent_pts: Option<f64>,
}

impl FrameStream {
    pub fn new(
        input: PathBuf,
        size_sync: mpsc::Receiver<PhysicalSize<u32>>,
        frame_sync: mpsc::SyncSender<DecodedFrame>,
        command_sync: mpsc::Receiver<PlaybackCommand>,
        fps: Arc<AtomicF64>,
        duration: Arc<AtomicF64>,
    ) -> Self {
        Self {
            input,
            paused: false,
            fps,
            duration,
            size_sync,
            frame_sync,
            command_sync,
            frame_buffer_before: frame::Video::empty(),
            last_sent_pts: None,
        }
    }

    #[expect(clippy::too_many_lines)]
    pub fn read_frames(&mut self) -> Result<()> {
        let mut input = format::input(&self.input)?;
        let video = input
            .streams()
            .best(media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let video_index = video.index();
        let frame_rate = video.avg_frame_rate();
        let time_base = video.time_base();
        let fps = if frame_rate.1 != 0 {
            f64::from(frame_rate.0) / f64::from(frame_rate.1)
        } else {
            0.0
        };
        self.fps.store(fps, Ordering::Relaxed);
        self.duration
            .store(input.duration() as f64 / AV_TIME_BASE, Ordering::Relaxed);
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
        loop {
            // Drain every pending command before touching the demuxer, so a
            // burst of key presses (e.g. several seeks in a row) collapses to
            // just the last one instead of decoding through each in turn.
            while let Ok(cmd) = self.command_sync.try_recv() {
                self.handle_command(
                    cmd,
                    &mut input,
                    &mut decoder,
                    &mut scaler,
                    video_index,
                    time_base,
                )?;
            }
            if self.paused {
                // Block instead of spinning: any command wakes this immediately.
                match self.command_sync.recv() {
                    Ok(cmd) => self.handle_command(
                        cmd,
                        &mut input,
                        &mut decoder,
                        &mut scaler,
                        video_index,
                        time_base,
                    )?,
                    Err(_) => return Ok(()), // window closed, sender dropped
                }
                continue;
            }

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

            // Read one packet at a time (rather than holding a `for` loop over
            // `input.packets()`) so `input` is never borrowed across an
            // iteration - that borrow would otherwise make `input.seek()`
            // impossible to call from `handle_command`. The stream index is
            // extracted into a plain `usize` immediately so nothing borrowed
            // from `input` survives past this statement.
            let next_packet = input
                .packets()
                .next()
                .map(|(stream, packet)| (stream.index(), packet));
            let packet = match next_packet {
                Some((idx, packet)) if idx == video_index => packet,
                Some(_) => {
                    // packet from another stream (e.g. audio)
                    continue;
                }
                None => {
                    // End of stream: wait here for a seek instead of exiting,
                    // so rewinding after playback finishes still works.
                    debug!("End of stream reached, waiting for a seek");
                    self.paused = true;
                    loop {
                        match self.command_sync.recv() {
                            Ok(cmd) => {
                                self.handle_command(
                                    cmd,
                                    &mut input,
                                    &mut decoder,
                                    &mut scaler,
                                    video_index,
                                    time_base,
                                )?;
                                if matches!(cmd, PlaybackCommand::Seek(_)) {
                                    break;
                                }
                            }
                            Err(_) => return Ok(()),
                        }
                    }
                    continue;
                }
            };

            decoder.send_packet(&packet)?;
            while decoder.receive_frame(&mut self.frame_buffer_before).is_ok() {
                self.scale_and_send(&mut scaler, time_base)?;
                trace!("Frame\t{i}");
                i += 1;
            }
        }
    }

    fn handle_command(
        &mut self,
        cmd: PlaybackCommand,
        input: &mut format::context::Input,
        decoder: &mut ffmpeg::decoder::Video,
        scaler: &mut Scaler,
        video_index: usize,
        time_base: Rational,
    ) -> Result<()> {
        match cmd {
            PlaybackCommand::TogglePause => self.paused = !self.paused,
            PlaybackCommand::Seek(target_secs) => {
                input.seek((target_secs * AV_TIME_BASE) as i64, ..)?;
                decoder.flush();
                self.frame_buffer_before = frame::Video::empty();
                self.last_sent_pts = None;
                self.decode_next_frame(
                    input,
                    decoder,
                    scaler,
                    video_index,
                    time_base,
                    target_secs,
                )?;
            }
        }
        Ok(())
    }

    /// Decodes and sends exactly one fresh frame, so a seek is visible right
    /// away even while paused. Silently does nothing if the seek landed at
    /// (or past) the end of the stream.
    fn decode_next_frame(
        &mut self,
        input: &mut format::context::Input,
        decoder: &mut ffmpeg::decoder::Video,
        scaler: &mut Scaler,
        video_index: usize,
        time_base: Rational,
        target_secs: f64,
    ) -> Result<()> {
        loop {
            let next_packet = input
                .packets()
                .next()
                .map(|(stream, packet)| (stream.index(), packet));
            match next_packet {
                Some((idx, packet)) if idx == video_index => {
                    decoder.send_packet(&packet)?;
                    if decoder.receive_frame(&mut self.frame_buffer_before).is_ok() {
                        let pts_seconds = self
                            .frame_buffer_before
                            .timestamp()
                            .map_or(0.0, |ts| ts as f64 * f64::from(time_base));
                        if pts_seconds < target_secs {
                            continue;
                        }
                        self.scale_and_send(scaler, time_base)?;
                        return Ok(());
                    }
                }
                Some(_) => {}
                None => return Ok(()),
            }
        }
    }

    /// Scales the most recently decoded frame into a fresh BGRA frame and
    /// sends it, tagged with its presentation timestamp in seconds.
    fn scale_and_send(&mut self, scaler: &mut Scaler, time_base: Rational) -> Result<()> {
        // Scale into a fresh, uniquely-owned frame and move it into the
        // channel. Scaler::run allocates its output when given an empty
        // frame, so this needs no reused buffer to clone before sending -
        // the clone (a full pixel-buffer copy) is skipped entirely.
        let mut output_frame = frame::Video::empty();
        scaler.run(&self.frame_buffer_before, &mut output_frame)?;
        let pts_seconds = self
            .frame_buffer_before
            .timestamp()
            .map_or(0.0, |ts| ts as f64 * f64::from(time_base));
        if self.last_sent_pts.is_some_and(|last| pts_seconds <= last) {
            return Ok(());
        }
        self.last_sent_pts = Some(pts_seconds);
        self.frame_sync.send(DecodedFrame {
            frame: output_frame,
            pts_seconds,
        })?;
        Ok(())
    }
}
