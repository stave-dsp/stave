//! The audio device: list devices, open the chosen output (and input)
//! at the chosen rate and buffer, and run the plugin in the output
//! callback. cpal is the layer (CoreAudio, WASAPI, ALSA; PipeWire and
//! JACK through their ALSA plugins); ASIO on Windows is cpal's `asio`
//! feature, which needs Steinberg's SDK at build time and is not built.
//!
//! Input and output are separate streams (separate devices, even): the
//! input callback writes interleaved samples into a [`Fifo`] the output
//! callback reads. An empty FIFO reads silence; a full one drops.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig, SupportedBufferSize};

use crate::session::Instance;
use crate::settings::{Device, Settings};

/// The most frames the plugin runs at once (a longer device buffer runs
/// in pieces), and the most channels a side serves.
pub(crate) const MAX_CHUNK: usize = 4096;
const MAX_CHANNELS: usize = 8;

/// The rates and buffer sizes the settings offer.
pub(crate) const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
pub(crate) const BUFFERS: [u32; 7] = [32, 64, 128, 256, 512, 1024, 2048];

/// The devices on this machine.
#[derive(Clone, Debug, Default)]
pub(crate) struct Devices {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
}

pub(crate) fn devices() -> Devices {
    let host = cpal::default_host();
    Devices {
        outputs: host.output_devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default(),
        inputs: host.input_devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default(),
        default_output: host.default_output_device().and_then(|d| d.name().ok()),
        default_input: host.default_input_device().and_then(|d| d.name().ok()),
    }
}

/// A running device: dropping it stops the callbacks.
pub(crate) struct Running {
    _output: Stream,
    _input: Option<Stream>,
    pub output: String,
    pub input: Option<String>,
    pub sample_rate: u32,
    /// The fixed buffer asked of the device, if one was.
    pub buffer: Option<u32>,
}

fn find(mut devices: impl Iterator<Item = cpal::Device>, name: &str) -> Option<cpal::Device> {
    devices.find(|d| d.name().is_ok_and(|n| n == name))
}

/// The output device `choice` names, or the default (a named device
/// that is gone falls back to it).
fn output_device(host: &cpal::Host, choice: &Device) -> Option<cpal::Device> {
    match choice {
        Device::Named(name) => host.output_devices().ok().and_then(|d| find(d, name)),
        Device::Default => None,
    }
    .or_else(|| host.default_output_device())
}

fn input_device(host: &cpal::Host, choice: &Device) -> Option<cpal::Device> {
    match choice {
        Device::Named(name) => host.input_devices().ok().and_then(|d| find(d, name)),
        Device::Default => None,
    }
    .or_else(|| host.default_input_device())
}

/// A configuration of `ranges` at `rate` (0: `fallback`'s), f32 before
/// i16, at least `channels` wide when one is.
fn choose(
    ranges: Vec<cpal::SupportedStreamConfigRange>,
    rate: u32,
    fallback: Option<u32>,
    channels: usize,
) -> Option<cpal::SupportedStreamConfig> {
    let rate = if rate > 0 { rate } else { fallback? };
    let usable = |c: &&cpal::SupportedStreamConfigRange| {
        matches!(c.sample_format(), SampleFormat::F32 | SampleFormat::I16)
            && c.min_sample_rate().0 <= rate
            && rate <= c.max_sample_rate().0
    };
    let rank = |c: &cpal::SupportedStreamConfigRange| {
        (usize::from(c.channels()) >= channels, c.sample_format() == SampleFormat::F32, std::cmp::Reverse(c.channels()))
    };
    ranges.iter().filter(usable).max_by_key(|c| rank(c)).cloned().map(|c| c.with_sample_rate(cpal::SampleRate(rate)))
}

/// The rates `RATES` lists that the chosen output device runs.
pub(crate) fn rates(settings: &Settings) -> Vec<u32> {
    let host = cpal::default_host();
    let Some(device) = output_device(&host, &settings.output) else { return Vec::new() };
    let ranges: Vec<_> = device.supported_output_configs().map(|c| c.collect()).unwrap_or_default();
    RATES.iter().copied().filter(|&r| ranges.iter().any(|c| c.min_sample_rate().0 <= r && r <= c.max_sample_rate().0)).collect()
}

fn buffer_size(supported: &SupportedBufferSize, frames: u32) -> cpal::BufferSize {
    match *supported {
        SupportedBufferSize::Range { min, max } if frames > 0 && (min..=max).contains(&frames) => cpal::BufferSize::Fixed(frames),
        _ => cpal::BufferSize::Default,
    }
}

/// Open the devices `settings` choose and run `instance` in them.
pub(crate) fn start(instance: Arc<dyn Instance>, settings: &Settings) -> Result<Running, String> {
    let host = cpal::default_host();
    let device = output_device(&host, &settings.output).ok_or("no audio output device")?;
    let output_name = device.name().unwrap_or_else(|_| "output".into());
    let (plugin_in, plugin_out) = instance.channels();
    let default_rate = device.default_output_config().ok().map(|c| c.sample_rate().0);
    let ranges: Vec<_> = device.supported_output_configs().map_err(|e| format!("{output_name}: {e}"))?.collect();
    let supported = choose(ranges.clone(), settings.sample_rate, default_rate, plugin_out)
        .or_else(|| choose(ranges, 0, default_rate, plugin_out))
        .ok_or_else(|| format!("{output_name}: no f32 or i16 configuration"))?;
    let rate = supported.sample_rate().0;
    let format = supported.sample_format();
    let mut config: StreamConfig = supported.config();
    config.buffer_size = buffer_size(supported.buffer_size(), settings.buffer);
    let buffer = match config.buffer_size {
        cpal::BufferSize::Fixed(n) => Some(n),
        cpal::BufferSize::Default => None,
    };
    instance.prepare(rate as f32, MAX_CHUNK);

    // The input side, when asked for and the plugin has inputs.
    let mut input = None;
    let mut fifo = None;
    let mut input_channels = 0;
    if settings.input_on && plugin_in > 0 {
        if let Some(dev) = input_device(&host, &settings.input) {
            let name = dev.name().unwrap_or_else(|_| "input".into());
            let ranges: Vec<_> = dev.supported_input_configs().map(|c| c.collect()).unwrap_or_default();
            match choose(ranges, rate, None, plugin_in) {
                Some(cfg) => {
                    let format = cfg.sample_format();
                    let mut in_config: StreamConfig = cfg.config();
                    in_config.buffer_size = buffer_size(cfg.buffer_size(), settings.buffer);
                    input_channels = usize::from(in_config.channels);
                    let queue = Arc::new(Fifo::new(rate as usize / 4 * input_channels.max(1)));
                    let stream = match format {
                        SampleFormat::I16 => input_stream::<i16>(&dev, &in_config, queue.clone()),
                        _ => input_stream::<f32>(&dev, &in_config, queue.clone()),
                    };
                    match stream {
                        Ok(s) => {
                            input = Some((s, name));
                            fifo = Some(queue);
                        }
                        Err(e) => eprintln!("audio input {name}: {e}"),
                    }
                }
                None => eprintln!("audio input {name}: does not run at {rate} Hz"),
            }
        }
    }

    let callback = Callback {
        instance,
        fifo,
        device_in: input_channels,
        device_out: usize::from(config.channels),
        plugin_in: plugin_in.min(MAX_CHANNELS),
        plugin_out: plugin_out.min(MAX_CHANNELS),
        planar_in: vec![0.0; plugin_in.min(MAX_CHANNELS) * MAX_CHUNK],
        planar_out: vec![0.0; plugin_out.clamp(1, MAX_CHANNELS) * MAX_CHUNK],
        frame: vec![0.0; input_channels.max(1)],
    };
    let output = match format {
        SampleFormat::I16 => output_stream::<i16>(&device, &config, callback),
        _ => output_stream::<f32>(&device, &config, callback),
    }
    .map_err(|e| format!("{output_name}: {e}"))?;
    output.play().map_err(|e| format!("{output_name}: {e}"))?;
    let input = match input {
        Some((stream, name)) => match stream.play() {
            Ok(()) => Some((stream, name)),
            Err(e) => {
                eprintln!("audio input {name}: {e}");
                None
            }
        },
        None => None,
    };
    let (input, input_name) = match input {
        Some((s, n)) => (Some(s), Some(n)),
        None => (None, None),
    };
    Ok(Running { _output: output, _input: input, output: output_name, input: input_name, sample_rate: rate, buffer })
}

fn output_stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut callback: Callback,
) -> Result<Stream, cpal::BuildStreamError> {
    device.build_output_stream(config, move |data: &mut [T], _| callback.fill(data), |e| eprintln!("audio output: {e}"), None)
}

fn input_stream<T: SizedSample>(device: &cpal::Device, config: &StreamConfig, fifo: Arc<Fifo>) -> Result<Stream, cpal::BuildStreamError>
where
    f32: FromSample<T>,
{
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            for &s in data {
                fifo.push(f32::from_sample(s));
            }
        },
        |e| eprintln!("audio input: {e}"),
        None,
    )
}

/// What the output callback owns: the instance, the input FIFO, and
/// planar scratch sized before the stream starts (the callback never
/// allocates).
struct Callback {
    instance: Arc<dyn Instance>,
    fifo: Option<Arc<Fifo>>,
    device_in: usize,
    device_out: usize,
    plugin_in: usize,
    plugin_out: usize,
    planar_in: Vec<f32>,
    planar_out: Vec<f32>,
    /// One input frame, interleaved.
    frame: Vec<f32>,
}

impl Callback {
    fn fill<T: Sample + FromSample<f32>>(&mut self, data: &mut [T]) {
        let channels = self.device_out.max(1);
        for chunk in data.chunks_mut(MAX_CHUNK * channels) {
            let frames = chunk.len() / channels;
            self.read_input(frames);
            let mut inputs = [core::ptr::null_mut::<f32>(); MAX_CHANNELS];
            let mut outputs = [core::ptr::null_mut::<f32>(); MAX_CHANNELS];
            for (c, p) in inputs.iter_mut().enumerate().take(self.plugin_in) {
                *p = self.planar_in[c * MAX_CHUNK..].as_mut_ptr();
            }
            for (c, p) in outputs.iter_mut().enumerate().take(self.plugin_out) {
                *p = self.planar_out[c * MAX_CHUNK..].as_mut_ptr();
            }
            // SAFETY: each pointer starts its own MAX_CHUNK-long slice of
            // the scratch (`frames` ≤ MAX_CHUNK); inputs and outputs are
            // separate vectors.
            unsafe { self.instance.process(&inputs[..self.plugin_in], &outputs[..self.plugin_out], frames) };
            for (f, out) in chunk.chunks_mut(channels).enumerate() {
                for (c, s) in out.iter_mut().enumerate() {
                    // A mono plugin feeds both sides of a stereo device;
                    // channels past the plugin's are silent.
                    let source = match self.plugin_out {
                        0 => None,
                        1 if c < 2 => Some(0),
                        n if c < n => Some(c),
                        _ => None,
                    };
                    let x = source.map_or(0.0, |src| self.planar_out[src * MAX_CHUNK + f]);
                    *s = T::from_sample(x);
                }
            }
        }
    }

    /// `frames` of input into the planar scratch: device channel `c` to
    /// plugin channel `c` (a mono device feeds every plugin channel);
    /// silence without an input or when the FIFO runs dry.
    fn read_input(&mut self, frames: usize) {
        let Some(fifo) = &self.fifo else {
            self.planar_in.iter_mut().for_each(|s| *s = 0.0);
            return;
        };
        for f in 0..frames {
            let whole = fifo.len() >= self.device_in;
            for s in self.frame.iter_mut().take(self.device_in) {
                *s = if whole { fifo.pop().unwrap_or(0.0) } else { 0.0 };
            }
            for c in 0..self.plugin_in {
                let src = if self.device_in == 1 { 0 } else { c };
                self.planar_in[c * MAX_CHUNK + f] = if src < self.device_in { self.frame[src] } else { 0.0 };
            }
        }
    }
}

/// A lock-free single-producer single-consumer FIFO of samples.
pub(crate) struct Fifo {
    slots: Box<[AtomicU32]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl Fifo {
    pub(crate) fn new(capacity: usize) -> Self {
        Fifo { slots: (0..capacity.max(1)).map(|_| AtomicU32::new(0)).collect(), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    pub(crate) fn len(&self) -> usize {
        self.tail.load(Ordering::Acquire).wrapping_sub(self.head.load(Ordering::Acquire))
    }

    /// False when full (the sample is dropped).
    pub(crate) fn push(&self, x: f32) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail.wrapping_sub(self.head.load(Ordering::Acquire)) >= self.slots.len() {
            return false;
        }
        self.slots[tail % self.slots.len()].store(x.to_bits(), Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    pub(crate) fn pop(&self) -> Option<f32> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let x = f32::from_bits(self.slots[head % self.slots.len()].load(Ordering::Relaxed));
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fifo_keeps_order_and_drops_when_full() {
        let fifo = Fifo::new(3);
        assert!(fifo.push(1.0) && fifo.push(2.0) && fifo.push(3.0));
        assert!(!fifo.push(4.0));
        assert_eq!((fifo.len(), fifo.pop(), fifo.pop()), (3, Some(1.0), Some(2.0)));
        assert!(fifo.push(5.0));
        assert_eq!((fifo.pop(), fifo.pop(), fifo.pop()), (Some(3.0), Some(5.0), None));
    }
}
