//! **`.nam`** — Neural Amp Modeler's model file: JSON holding a format
//! `version`, an `architecture`, its `config`, a flat `weights` list,
//! `metadata` and the training `sample_rate`.
//!
//! Read from the published format (the file layout is data); written
//! from the format, not from NAM's code. Neural Amp Modeler
//! (<https://github.com/sdatkinson/neural-amp-modeler>) and its C++
//! inference core NeuralAmpModelerCore
//! (<https://github.com/sdatkinson/NeuralAmpModelerCore>) are © Steven
//! Atkinson under the MIT licence; their sources were read as the
//! format's reference, and the parity references in `tests/` come from
//! running the Python package.
//!
//! Implemented: format versions 0.5 to 0.7; architectures `WaveNet`
//! (both the legacy keys — `head_size`, `head_bias`, `gated`,
//! `kernel_size` — and 0.13-trainer keys — `head`, `kernel_sizes`,
//! `bottleneck`, `head1x1`, `layer1x1`, per-layer activations, `gating_mode`
//! with `gated` and `blended` modes) and `LSTM`. Refused with
//! [`LoadError::Unsupported`] or [`LoadError::Architecture`], never
//! silently ignored: FiLM, grouped convolutions, a condition DSP, a
//! WaveNet post-head, slimmable and packed containers
//! (`SlimmableContainer`), `ConvNet`, `Linear`, and multi-channel
//! networks.
//!
//! ```
//! use stave_dsp::learned::nam;
//! use stave_dsp::Stage;
//!
//! // A one-layer WaveNet small enough to write by hand: y = 0.5·tanh(2x).
//! let file = r#"{"version": "0.5.4", "architecture": "WaveNet", "sample_rate": 48000,
//!   "config": {"layers": [{"input_size": 1, "condition_size": 1, "channels": 1,
//!     "head_size": 1, "head_bias": false, "kernel_size": 1, "dilations": [1],
//!     "activation": "Tanh", "gated": false}], "head": null, "head_scale": 0.5},
//!   "weights": [1, 2, 0, 0, 0, 0, 1, 0.5]}"#;
//! let mut amp = nam::parse(file)?.network;
//! amp.prepare(48_000.0);
//! assert!(amp.rate_mismatch().is_none());
//! assert!((amp.tick(0.25) - 0.5 * 0.5f32.tanh()).abs() < 1e-6);
//!
//! // Prepared at a rate it was not trained at, it refuses (silence) and says why.
//! amp.prepare(44_100.0);
//! assert_eq!(amp.tick(0.25), 0.0);
//! assert_eq!(amp.rate_mismatch().unwrap().trained, 48_000.0);
//! # Ok::<(), stave_dsp::learned::LoadError>(())
//! ```

mod wavenet;

use super::error::LoadError;
use super::json::Json;
use super::lstm::Lstm;
use super::rate::{RateMismatch, RatePolicy};
use super::wavenet::WaveNet;
use crate::kernel::node::{Stage, Tail};

/// The rate Neural Amp Modeler assumes for a file without a
/// `sample_rate` (its trainer has always recorded at 48 kHz).
pub const DEFAULT_SAMPLE_RATE: f32 = 48_000.0;

/// A loaded `.nam` file.
#[derive(Clone, Debug)]
pub struct Nam {
    /// The network, ready to [`prepare`](Stage::prepare).
    pub network: Network,
    /// The file's format version (`"0.5.4"`, `"0.7.0"` …).
    pub version: String,
    /// The training rate the file records (`None` for files that
    /// predate the field: the network then reports
    /// [`DEFAULT_SAMPLE_RATE`]).
    pub sample_rate: Option<f32>,
    pub metadata: Metadata,
}

/// The `metadata` fields a plugin may show or calibrate with; absent
/// fields are `None`. Nothing here changes the network's output: a
/// plugin applies the levels itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    /// The capture's name.
    pub name: Option<String>,
    /// Who made the capture.
    pub modeled_by: Option<String>,
    /// Output loudness on NAM's standard input, dB.
    pub loudness: Option<f32>,
    /// NAM's 0–1 estimate of the capture's gain.
    pub gain: Option<f32>,
    /// The interface level the capture's input was recorded at, dBu for
    /// a full-scale sine.
    pub input_level_dbu: Option<f32>,
    /// The same for the output.
    pub output_level_dbu: Option<f32>,
}

/// A network a `.nam` file holds: one [`Stage`] whatever the
/// architecture, so a plugin holds one type and loads any file.
#[derive(Clone, Debug)]
pub enum Network {
    WaveNet(WaveNet),
    Lstm(Lstm),
}

impl Network {
    /// The sample rate the network was trained at, in Hz.
    pub fn sample_rate(&self) -> f32 {
        match self {
            Network::WaveNet(n) => n.sample_rate(),
            Network::Lstm(n) => n.sample_rate(),
        }
    }

    /// The trained and prepared rates, when they differ.
    pub fn rate_mismatch(&self) -> Option<RateMismatch> {
        match self {
            Network::WaveNet(n) => n.rate_mismatch(),
            Network::Lstm(n) => n.rate_mismatch(),
        }
    }

    /// What to do at a rate other than the trained one (default
    /// [`RatePolicy::Strict`]: silence).
    pub fn set_rate_policy(&mut self, policy: RatePolicy) {
        match self {
            Network::WaveNet(n) => n.set_rate_policy(policy),
            Network::Lstm(n) => n.set_rate_policy(policy),
        }
    }
}

impl Stage for Network {
    fn prepare(&mut self, sample_rate: f32) {
        match self {
            Network::WaveNet(n) => n.prepare(sample_rate),
            Network::Lstm(n) => n.prepare(sample_rate),
        }
    }
    fn reset(&mut self) {
        match self {
            Network::WaveNet(n) => n.reset(),
            Network::Lstm(n) => n.reset(),
        }
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        match self {
            Network::WaveNet(n) => n.tick(x),
            Network::Lstm(n) => n.tick(x),
        }
    }
    fn process(&mut self, block: &mut [f32]) {
        match self {
            Network::WaveNet(n) => n.process(block),
            Network::Lstm(n) => n.process(block),
        }
    }
    fn latency(&self) -> u32 {
        0
    }
    fn tail(&self) -> Tail {
        match self {
            Network::WaveNet(n) => n.tail(),
            Network::Lstm(n) => n.tail(),
        }
    }
}

/// Read a `.nam` file's text (the caller reads the file; this crate
/// does no I/O). Allocates: call it off the audio thread, then hand the
/// network over.
pub fn parse(text: &str) -> Result<Nam, LoadError> {
    let json = Json::parse(text).map_err(|e| LoadError::Json {
        offset: e.offset,
        reason: e.reason,
    })?;
    let version = json
        .field("version")
        .and_then(Json::as_str)
        .ok_or_else(|| LoadError::Config("missing \"version\"".into()))?
        .to_string();
    check_version(&version)?;
    let architecture = json
        .field("architecture")
        .and_then(Json::as_str)
        .ok_or_else(|| LoadError::Config("missing \"architecture\"".into()))?;
    let config = json
        .field("config")
        .ok_or_else(|| LoadError::Config("missing \"config\"".into()))?;
    let sample_rate =
        match json.field("sample_rate") {
            None => None,
            Some(v) => Some(v.as_f64().filter(|r| *r > 0.0).ok_or_else(|| {
                LoadError::Config("\"sample_rate\" must be a positive number".into())
            })? as f32),
        };
    let rate = sample_rate.unwrap_or(DEFAULT_SAMPLE_RATE);
    let weights = weights(&json)?;
    let network = match architecture {
        "WaveNet" => Network::WaveNet(wavenet::build(config, &weights, rate)?),
        "LSTM" => {
            mono(config, "LSTM")?;
            Network::Lstm(Lstm::build(
                size(config, "input_size")?,
                size(config, "hidden_size")?,
                size(config, "num_layers")?,
                &weights,
                rate,
            )?)
        }
        other => return Err(LoadError::Architecture(other.to_string())),
    };
    Ok(Nam {
        network,
        version,
        sample_rate,
        metadata: metadata(json.field("metadata")),
    })
}

/// Versions 0.5.x to 0.7.x: the range whose layout this reader
/// implements (NeuralAmpModelerCore reads the same range).
fn check_version(version: &str) -> Result<(), LoadError> {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    match (
        parts.next().flatten(),
        parts.next().flatten(),
        parts.next().flatten(),
        parts.next(),
    ) {
        (Some(0), Some(5..=7), Some(_), None) => Ok(()),
        _ => Err(LoadError::Version(version.to_string())),
    }
}

/// The flat `weights` list, as `f32`.
pub(crate) fn weights(json: &Json) -> Result<Vec<f32>, LoadError> {
    let list = json
        .field("weights")
        .and_then(Json::as_array)
        .ok_or_else(|| LoadError::Config("missing \"weights\" list".into()))?;
    list.iter()
        .map(|w| w.as_f64().map(|v| v as f32))
        .collect::<Option<Vec<f32>>>()
        .ok_or_else(|| LoadError::Config("\"weights\" holds a non-number".into()))
}

/// A whole, non-negative number field.
pub(crate) fn size(json: &Json, key: &str) -> Result<usize, LoadError> {
    json.field(key)
        .and_then(Json::as_f64)
        .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n < 1e9)
        .map(|n| n as usize)
        .ok_or_else(|| LoadError::Config(format!("\"{key}\" must be a whole number")))
}

/// Top-level `in_channels` / `out_channels`, when given, are 1.
fn mono(config: &Json, what: &str) -> Result<(), LoadError> {
    for key in ["in_channels", "out_channels"] {
        if config.field(key).is_some() && size(config, key)? != 1 {
            return Err(LoadError::Unsupported(format!(
                "{what} with {key} other than 1"
            )));
        }
    }
    Ok(())
}

fn metadata(json: Option<&Json>) -> Metadata {
    let Some(m) = json else {
        return Metadata::default();
    };
    let number = |key: &str| m.field(key).and_then(Json::as_f64).map(|v| v as f32);
    let text = |key: &str| m.field(key).and_then(Json::as_str).map(str::to_string);
    Metadata {
        name: text("name"),
        modeled_by: text("modeled_by"),
        loudness: number("loudness"),
        gain: number("gain"),
        input_level_dbu: number("input_level_dbu"),
        output_level_dbu: number("output_level_dbu"),
    }
}

#[cfg(test)]
mod tests;
