use crate::{keyboard::Volume, state::AtomicF32};
use color_eyre::eyre::{Result, eyre};
use cpal::{
    Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, atomic::Ordering},
};

const TARGET_BUFFER_MS: u32 = 200;

struct SharedAudio {
    queue: Mutex<VecDeque<i16>>,
    volume: AtomicF32,
}

pub struct AudioOutput {
    stream: Stream,
    shared: Arc<SharedAudio>,
    target_samples: usize,
}

impl AudioOutput {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| eyre!("no default audio output device available"))?;

        let (sample_format, stream_config) = select_output_config(&device, sample_rate, channels)?;
        let target_samples = usize::try_from(
            (u64::from(stream_config.sample_rate)
                * u64::from(stream_config.channels)
                * u64::from(TARGET_BUFFER_MS))
                / 1_000,
        )
        .map_err(|_| eyre!("target audio buffer size does not fit usize"))?;

        let shared = Arc::new(SharedAudio {
            queue: Mutex::new(VecDeque::with_capacity(target_samples)),
            volume: AtomicF32::new(1.0),
        });

        let error_callback = |err| {
            tracing::error!("cpal output stream error: {err}");
        };

        let stream = build_stream(
            &device,
            stream_config,
            sample_format,
            &shared,
            error_callback,
        )?;
        stream.play()?;

        Ok(Self {
            stream,
            shared,
            target_samples,
        })
    }

    pub fn set_volume(&self, volume: Volume) {
        let volume = volume.get().clamp(0.0, 1.0);
        self.shared.volume.store(volume, Ordering::Relaxed);
    }

    pub fn submit(&self, pcm: &[i16]) -> Result<()> {
        if pcm.is_empty() {
            return Ok(());
        }
        let mut queue = self
            .shared
            .queue
            .lock()
            .map_err(|_| eyre!("audio queue mutex poisoned"))?;

        if queue.len().saturating_add(pcm.len()) > self.target_samples {
            let overflow = queue
                .len()
                .saturating_add(pcm.len())
                .saturating_sub(self.target_samples);
            queue.drain(..overflow);
        }
        queue.extend(pcm.iter().copied());
        drop(queue);
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.stream.pause()?;
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        self.stream.play()?;
        Ok(())
    }

    pub fn reset(&self) -> Result<()> {
        self.shared
            .queue
            .lock()
            .map_err(|_| eyre!("audio queue mutex poisoned"))?
            .clear();
        Ok(())
    }
}

fn select_output_config(
    device: &Device,
    sample_rate: u32,
    channels: u16,
) -> Result<(SampleFormat, StreamConfig)> {
    let supported = device.supported_output_configs()?;
    let mut selected = None;

    for cfg_range in supported {
        if cfg_range.channels() == channels
            && cfg_range.min_sample_rate() <= sample_rate
            && sample_rate <= cfg_range.max_sample_rate()
        {
            let preferred = cfg_range.with_sample_rate(sample_rate);
            let score = match cfg_range.sample_format() {
                SampleFormat::I16 => 0,
                SampleFormat::F32 => 1,
                SampleFormat::U16 => 2,
                _ => 10,
            };
            if selected.as_ref().is_none_or(|(s, _, _)| score < *s) {
                selected = Some((score, cfg_range.sample_format(), preferred.config()));
            }
        }
    }

    selected.map(|(_, fmt, cfg)| (fmt, cfg)).ok_or_else(|| {
        eyre!("no compatible audio output config for {channels} channels @ {sample_rate} Hz")
    })
}

fn build_stream(
    device: &Device,
    config: StreamConfig,
    sample_format: SampleFormat,
    shared: &Arc<SharedAudio>,
    error_callback: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream> {
    match sample_format {
        SampleFormat::I16 => build_stream_typed::<i16>(device, config, shared, error_callback),
        SampleFormat::F32 => build_stream_typed::<f32>(device, config, shared, error_callback),
        SampleFormat::U16 => build_stream_typed::<u16>(device, config, shared, error_callback),
        _ => Err(eyre!(
            "unsupported sample format from cpal: {sample_format:?}"
        )),
    }
}

fn build_stream_typed<T>(
    device: &Device,
    config: StreamConfig,
    shared: &Arc<SharedAudio>,
    error_callback: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let callback_shared = Arc::clone(shared);
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| render_audio_frame(data, &callback_shared),
        error_callback,
        None,
    )?;
    Ok(stream)
}

fn render_audio_frame<T>(out: &mut [T], shared: &SharedAudio)
where
    T: SizedSample + FromSample<f32>,
{
    let volume = shared.volume.load(Ordering::Relaxed);
    if let Ok(mut queue) = shared.queue.lock() {
        for sample in out.iter_mut() {
            let src = queue.pop_front().unwrap_or_default();
            let normalized = (f32::from(src) / f32::from(i16::MAX)) * volume;
            *sample = T::from_sample_(normalized.clamp(-1.0, 1.0));
        }
    } else {
        for sample in out.iter_mut() {
            *sample = T::from_sample_(0.0);
        }
    }
}
