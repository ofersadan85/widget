use crate::{audio::AudioOutput, state::AtomicF64};
use color_eyre::eyre::{Context, Result, eyre};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    ChannelLayout, Rational, codec, format, frame, media,
    software::{
        resampling::Context as Resampler,
        scaling::{context::Context as Scaler, flag::Flags},
    },
    util::format::{
        pixel::Pixel,
        sample::{Sample, Type as SampleType},
    },
};
use std::path::PathBuf;
use std::sync::{Arc, atomic::Ordering, mpsc};
use tracing::{debug, info, trace, warn};
use winit::dpi::PhysicalSize;

/// `FFmpeg`'s internal timestamp unit for APIs that aren't tied to a stream's
/// own `time_base` (`Input::seek`, `Input::duration`), in ticks per second.
const AV_TIME_BASE: f64 = 1_000_000.0;
const OUTPUT_SAMPLE_RATE: u32 = 48_000;
const OUTPUT_CHANNELS: u16 = 2;
const OUTPUT_CHANNEL_LAYOUT: ChannelLayout = ChannelLayout::STEREO;
const OUTPUT_SAMPLE_FORMAT: Sample = Sample::I16(SampleType::Packed);

/// Playback control sent from the window thread to the decode thread.
#[derive(Debug, Clone, Copy)]
pub enum PlaybackCommand {
    TogglePause,
    /// Absolute target position, in seconds.
    Seek(f64),
    /// Player-local output gain in [0.0, 1.0].
    SetVolume(f32),
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
    autoclose: bool,
    volume: f32,
}

struct AudioStreamState {
    stream_index: usize,
    time_base: Rational,
    decoder: ffmpeg::decoder::Audio,
    resampler: Resampler,
    decoded: frame::Audio,
    resampled: frame::Audio,
}

impl FrameStream {
    pub fn new(
        input: PathBuf,
        size_sync: mpsc::Receiver<PhysicalSize<u32>>,
        frame_sync: mpsc::SyncSender<DecodedFrame>,
        command_sync: mpsc::Receiver<PlaybackCommand>,
        fps: Arc<AtomicF64>,
        duration: Arc<AtomicF64>,
        autoclose: bool,
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
            autoclose,
            volume: 1.0,
        }
    }

    #[expect(clippy::too_many_lines, clippy::cast_precision_loss)]
    pub fn read_frames(&mut self) -> Result<()> {
        let mut input = format::input(&self.input)
            .wrap_err_with(|| format!("failed to open video input '{}'", self.input.display()))?;
        let video = input
            .streams()
            .best(media::Type::Video)
            .ok_or_else(|| eyre!("no video stream found in '{}'", self.input.display()))?;
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
            ctx.set_parameters(codec_params).wrap_err_with(|| {
                format!(
                    "failed to configure hardware decoder for '{}'",
                    self.input.display()
                )
            })?;
            ctx.decoder().video().wrap_err_with(|| {
                format!(
                    "failed to create hardware video decoder for '{}'",
                    self.input.display()
                )
            })?
        } else {
            warn!("⚠️ Hardware decoder not available, using software decoder");
            let codec =
                codec::context::Context::from_parameters(codec_params).wrap_err_with(|| {
                    format!(
                        "failed to create FFmpeg decoder context for '{}'",
                        self.input.display()
                    )
                })?;
            codec.decoder().video().wrap_err_with(|| {
                format!(
                    "failed to create software video decoder for '{}'",
                    self.input.display()
                )
            })?
        };
        let mut scaler = Scaler::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::BGRA,
            1,
            1,
            Flags::BILINEAR,
        )
        .wrap_err_with(|| {
            format!(
                "failed to create scaler for decoded video {}x{} in '{}'",
                decoder.width(),
                decoder.height(),
                self.input.display()
            )
        })?;

        let mut audio = input
            .streams()
            .best(media::Type::Audio)
            .map(|stream| Self::init_audio_stream(&stream))
            .transpose()?;

        let audio_output = if audio.is_some() {
            let out = AudioOutput::new(OUTPUT_SAMPLE_RATE, OUTPUT_CHANNELS)
                .wrap_err("failed to open Windows waveOut audio output")?;
            out.set_volume(self.volume);
            Some(out)
        } else {
            None
        };

        if audio.is_none() {
            debug!(
                "No audio stream found in '{}'; running video-only",
                self.input.display()
            );
        }

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
                    audio.as_mut(),
                    audio_output.as_ref(),
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
                        audio.as_mut(),
                        audio_output.as_ref(),
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
                Some((idx, packet)) => {
                    if let (Some(audio_state), Some(audio_out)) =
                        (audio.as_mut(), audio_output.as_ref())
                        && idx == audio_state.stream_index
                    {
                        self.decode_audio_packet(audio_state, audio_out, &packet)?;
                    }
                    continue;
                }
                None => {
                    if self.autoclose {
                        return Ok(());
                    }
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
                                    audio.as_mut(),
                                    audio_output.as_ref(),
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

    #[expect(clippy::cast_possible_truncation, clippy::too_many_arguments)]
    fn handle_command(
        &mut self,
        cmd: PlaybackCommand,
        input: &mut format::context::Input,
        decoder: &mut ffmpeg::decoder::Video,
        scaler: &mut Scaler,
        video_index: usize,
        time_base: Rational,
        audio: Option<&mut AudioStreamState>,
        audio_output: Option<&AudioOutput>,
    ) -> Result<()> {
        match cmd {
            PlaybackCommand::TogglePause => {
                self.paused = !self.paused;
                if let Some(audio_out) = audio_output {
                    if self.paused {
                        audio_out.pause()?;
                    } else {
                        audio_out.resume()?;
                    }
                }
            }
            PlaybackCommand::Seek(target_secs) => {
                input
                    .seek((target_secs * AV_TIME_BASE) as i64, ..)
                    .wrap_err_with(|| {
                        format!(
                            "failed to seek '{}' to {}s",
                            self.input.display(),
                            target_secs
                        )
                    })?;
                decoder.flush();
                self.frame_buffer_before = frame::Video::empty();
                self.last_sent_pts = None;
                if let Some(audio_state) = audio {
                    audio_state.decoder.flush();
                    audio_state.resampler = Self::build_resampler(&audio_state.decoder)?;
                    audio_state.decoded = frame::Audio::empty();
                    audio_state.resampled = frame::Audio::empty();
                }
                if let Some(audio_out) = audio_output {
                    audio_out.reset()?;
                }
                self.decode_next_frame(
                    input,
                    decoder,
                    scaler,
                    video_index,
                    time_base,
                    target_secs,
                )?;
            }
            PlaybackCommand::SetVolume(volume) => {
                self.volume = volume.clamp(0.0, 1.0);
                if let Some(audio_out) = audio_output {
                    audio_out.set_volume(self.volume);
                }
            }
        }
        Ok(())
    }

    fn init_audio_stream(stream: &format::stream::Stream<'_>) -> Result<AudioStreamState> {
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let codec = codec::context::Context::from_parameters(stream.parameters())
            .wrap_err("failed to create audio decoder context")?;
        let decoder = codec
            .decoder()
            .audio()
            .wrap_err("failed to create audio decoder")?;
        let resampler = Self::build_resampler(&decoder)?;
        Ok(AudioStreamState {
            stream_index,
            time_base,
            decoder,
            resampler,
            decoded: frame::Audio::empty(),
            resampled: frame::Audio::empty(),
        })
    }

    fn build_resampler(decoder: &ffmpeg::decoder::Audio) -> Result<Resampler> {
        let mut src_layout = decoder.channel_layout();
        if src_layout.is_empty() {
            src_layout = ChannelLayout::default(i32::from(decoder.channels()));
        }
        Resampler::get(
            decoder.format(),
            src_layout,
            decoder.rate(),
            OUTPUT_SAMPLE_FORMAT,
            OUTPUT_CHANNEL_LAYOUT,
            OUTPUT_SAMPLE_RATE,
        )
        .wrap_err("failed to create audio resampler")
    }

    fn decode_audio_packet(
        &self,
        audio: &mut AudioStreamState,
        audio_output: &AudioOutput,
        packet: &ffmpeg::Packet,
    ) -> Result<()> {
        audio
            .decoder
            .send_packet(packet)
            .wrap_err("failed to send packet to audio decoder")?;
        while audio.decoder.receive_frame(&mut audio.decoded).is_ok() {
            audio.resampled = frame::Audio::empty();
            audio
                .resampler
                .run(&audio.decoded, &mut audio.resampled)
                .wrap_err("failed to resample audio frame")?;
            if audio.resampled.samples() == 0 {
                continue;
            }
            let raw = audio.resampled.plane::<i16>(0);
            if raw.is_empty() {
                continue;
            }
            #[expect(clippy::cast_precision_loss)]
            let ts_calc = |ts| ts as f64 * f64::from(audio.time_base);
            audio_output.submit(raw).wrap_err_with(|| {
                format!(
                    "failed to enqueue audio samples for '{}' at {}s",
                    self.input.display(),
                    audio.decoded.timestamp().map_or(0.0, ts_calc),
                )
            })?;
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
            #[expect(clippy::cast_precision_loss)]
            let ts_calc = |ts| ts as f64 * f64::from(time_base);
            let next_packet = input
                .packets()
                .next()
                .map(|(stream, packet)| (stream.index(), packet));
            match next_packet {
                Some((idx, packet)) if idx == video_index => {
                    decoder.send_packet(&packet)?;
                    if decoder.receive_frame(&mut self.frame_buffer_before).is_ok() {
                        let pts_seconds = self.frame_buffer_before.timestamp().map_or(0.0, ts_calc);
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
        #[expect(clippy::cast_precision_loss)]
        let ts_calc = |ts| ts as f64 * f64::from(time_base);
        // Scale into a fresh, uniquely-owned frame and move it into the
        // channel. Scaler::run allocates its output when given an empty
        // frame, so this needs no reused buffer to clone before sending -
        // the clone (a full pixel-buffer copy) is skipped entirely.
        let mut output_frame = frame::Video::empty();
        scaler
            .run(&self.frame_buffer_before, &mut output_frame)
            .wrap_err("failed to scale decoded frame to window buffer")?;
        let pts_seconds = self.frame_buffer_before.timestamp().map_or(0.0, ts_calc);
        if self.last_sent_pts.is_some_and(|last| pts_seconds <= last) {
            return Ok(());
        }
        self.last_sent_pts = Some(pts_seconds);
        self.frame_sync
            .send(DecodedFrame {
                frame: output_frame,
                pts_seconds,
            })
            .wrap_err_with(|| {
                format!(
                    "failed to enqueue decoded video frame for '{}' at {}s",
                    self.input.display(),
                    pts_seconds
                )
            })?;
        Ok(())
    }
}
