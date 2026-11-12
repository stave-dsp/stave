//! The audio device: pick a CPAL output configuration and run an
//! [`Engine`] in its callback. All realtime behavior lives in the
//! engine; this file is device plumbing only.

mod engine;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig, SupportedStreamConfig};
use stave_host::config::{InvalidSampleRate, SampleRate};
use stave_host::instance::Instance;

pub use engine::{Engine, Shared, MAX_EVENTS};

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no default audio output device")]
    NoOutputDevice,
    #[error("no f32 output configuration on the default device")]
    NoF32Config,
    #[error("could not list the device's configurations")]
    Configs(#[from] cpal::SupportedStreamConfigsError),
    #[error("the device reports an unusable rate")]
    DeviceRate(#[from] InvalidSampleRate),
    #[error("could not build the output stream")]
    Build(#[from] cpal::BuildStreamError),
    #[error("could not start the output stream")]
    Play(#[from] cpal::PlayStreamError),
}

/// A running output stream; dropping it stops audio and drops the
/// plugin it owns (on the dropping thread).
pub struct AudioStream {
    _stream: Stream,
}

pub struct DeviceProfile {
    pub device_name: String,
    pub sample_rate: SampleRate,
}

/// The default device and the rate it will actually run near `hint`.
pub fn pick_device_profile(hint: SampleRate) -> Result<DeviceProfile, AudioError> {
    let device = default_device()?;
    let cfg = choose_config(&device, hint.hz() as f32)?;
    Ok(DeviceProfile {
        device_name: device.name().unwrap_or_else(|_| "?".into()),
        sample_rate: SampleRate::new(f64::from(cfg.sample_rate().0))?,
    })
}

/// Start `plugin` on the default output device.
pub fn start<P: Instance + Send + 'static>(
    plugin: P,
    shared: Shared,
) -> Result<AudioStream, AudioError> {
    let device = default_device()?;
    let log = shared.log.clone();
    log.info(format!(
        "audio device: {}",
        device.name().unwrap_or_else(|_| "?".into())
    ));
    let rate = plugin.layout().sample_rate;
    let supported = choose_config(&device, rate as f32)?;
    let channels = usize::from(supported.channels());
    let config: StreamConfig = supported.into();
    if (f64::from(config.sample_rate.0) - rate).abs() > 0.5 {
        log.warn(format!(
            "plugin runs at {rate} Hz but the device at {} Hz: pitch will be wrong",
            config.sample_rate.0
        ));
    }
    log.info(format!(
        "stream: {} Hz, {channels} ch, buffer={:?}",
        config.sample_rate.0, config.buffer_size
    ));
    let mut engine = Engine::new(plugin, shared);
    let err_log = log.clone();
    let stream = device.build_output_stream(
        &config,
        move |buf: &mut [f32], _| engine.fill(buf, channels),
        move |e| err_log.error(format!("stream error: {e}")),
        None,
    )?;
    stream.play()?;
    Ok(AudioStream { _stream: stream })
}

fn default_device() -> Result<cpal::Device, AudioError> {
    cpal::default_host()
        .default_output_device()
        .ok_or(AudioError::NoOutputDevice)
}

/// An f32 configuration running `hint_sr` if the device can, else the
/// device's fastest f32 rate.
fn choose_config(device: &cpal::Device, hint_sr: f32) -> Result<SupportedStreamConfig, AudioError> {
    let target = hint_sr as u32;
    let f32s = || {
        device
            .supported_output_configs()
            .map(|it| it.filter(|c| c.sample_format() == SampleFormat::F32))
    };
    if let Some(c) =
        f32s()?.find(|c| c.min_sample_rate().0 <= target && target <= c.max_sample_rate().0)
    {
        return Ok(c.with_sample_rate(cpal::SampleRate(target)));
    }
    f32s()?
        .next()
        .map(|c| {
            let max = c.max_sample_rate();
            c.with_sample_rate(max)
        })
        .ok_or(AudioError::NoF32Config)
}
