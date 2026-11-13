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
//! with `gated` and `blended` modes) and `LSTM`; slimmable networks
//! (Atkinson 2025, "Slimmable NAM"): a WaveNet trained for channel
//! slicing (`"slimmable": {"method": "slice_channels_uniform", …}`, what
//! the 0.13 trainer exports) and a `SlimmableContainer` of whole
//! submodels (any architecture above, selected by `max_value` as
//! NeuralAmpModelerCore's `NAM/container.cpp` does: a [`Container`]),
//! both offered as [`Nam::sizes`]. Refused with
//! [`LoadError::Unsupported`] or [`LoadError::Architecture`], never
//! silently ignored: FiLM, grouped convolutions, a condition DSP, a
//! WaveNet post-head, packed WaveNets, a container inside a container
//! or a slimmable WaveNet inside one, a container field other than
//! `submodels`, `max_value` and `model`, `ConvNet`, `Linear`, and
//! multi-channel networks.
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
//!
//! # Choosing a size
//!
//! A slimmable file runs at full size by default ([`Nam::network`]);
//! [`Nam::sizes`] lists every size it offers, smallest first, with its
//! cost, and [`Nam::network_at`] (or [`Nam::slimmed`], NAM's own 0–1
//! slider) hands over a smaller one. Every size was built by [`parse`],
//! so choosing one only clones; swap it in off the audio thread.
//!
//! A `SlimmableContainer` also switches in place: its network is a
//! [`Network::Container`] holding every submodel, and
//! [`Network::set_size`] picks one on the audio thread without
//! allocating.
//!
//! ```
//! use stave_dsp::learned::nam;
//! use stave_dsp::Stage;
//!
//! // y = s·tanh(2x), a one-layer WaveNet scaled by `s`.
//! let wavenet = |s: f32| format!(r#"{{"version": "0.7.0", "architecture": "WaveNet",
//!   "sample_rate": 48000, "config": {{"layers": [{{"input_size": 1, "condition_size": 1,
//!     "channels": 1, "head_size": 1, "head_bias": false, "kernel_size": 1,
//!     "dilations": [1], "activation": "Tanh", "gated": false}}], "head": null,
//!     "head_scale": {s}}}, "weights": [1, 2, 0, 0, 0, 0, 1, {s}]}}"#);
//! let file = format!(r#"{{"version": "0.7.0", "architecture": "SlimmableContainer",
//!   "config": {{"submodels": [{{"max_value": 0.5, "model": {}}},
//!                            {{"max_value": 1.0, "model": {}}}]}}}}"#,
//!   wavenet(0.25), wavenet(0.5));
//! let mut amp = nam::parse(&file)?.network;
//! assert_eq!(amp.breakpoints(), &[0.5]);
//! amp.prepare(48_000.0);
//! let t = 0.5f32.tanh();
//! assert!((amp.tick(0.25) - 0.5 * t).abs() < 1e-6); // full size by default
//! amp.set_size(0.4); // below 0.5: the first submodel (no allocation)
//! assert!((amp.tick(0.25) - 0.25 * t).abs() < 1e-6);
//! amp.set_size(0.5); // a breakpoint selects the next submodel
//! assert!((amp.tick(0.25) - 0.5 * t).abs() < 1e-6);
//! # Ok::<(), stave_dsp::learned::LoadError>(())
//! ```
//!
//! ```
//! use stave_dsp::learned::nam;
//! use stave_dsp::Stage;
//!
//! // A two-channel WaveNet trained to also run on one channel.
//! let file = r#"{"version": "0.7.0", "architecture": "WaveNet", "sample_rate": 48000,
//!   "config": {"layers": [{"input_size": 1, "condition_size": 1, "channels": 2,
//!     "head": {"out_channels": 1, "kernel_size": 1, "bias": false},
//!     "kernel_sizes": [1], "dilations": [1], "activation": "Tanh", "gated": false,
//!     "slimmable": {"method": "slice_channels_uniform",
//!                   "kwargs": {"allowed_channels": [1, 2]}}}],
//!     "head": null, "head_scale": 1.0},
//!   "weights": [1, 1,  1, 0, 0, 1, 0, 0,  1, 1,  0, 0, 0, 0, 0, 0,  0.5, 0.25,  1]}"#;
//! let amp = nam::parse(file)?;
//! let sizes = amp.sizes();
//! assert_eq!(sizes.len(), 2);
//! assert_eq!((sizes[0].channels, sizes[1].channels), (Some(1), Some(2)));
//! assert!(sizes[0].multiply_adds < sizes[1].multiply_adds);
//!
//! // Full size: 0.5·tanh(2x) + 0.25·tanh(2x); one channel keeps the first term.
//! let (mut full, mut slim) = (amp.network.clone(), amp.network_at(0));
//! full.prepare(48_000.0);
//! slim.prepare(48_000.0);
//! let t = 0.5f32.tanh();
//! assert!((full.tick(0.25) - 0.75 * t).abs() < 1e-6);
//! assert!((slim.tick(0.25) - 0.5 * t).abs() < 1e-6);
//! // NAM's slider: index ⌊ratio · sizes⌋, so anything under 0.5 is one channel.
//! assert_eq!(amp.slimmed(0.3).multiply_adds(), sizes[0].multiply_adds);
//! # Ok::<(), stave_dsp::learned::LoadError>(())
//! ```

mod container;
mod wavenet;

pub use container::Container;

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
    /// [`sizes`](Self::sizes).
    sizes: Vec<Size>,
    /// One network per size, in the same order.
    networks: Vec<Network>,
}

/// One size a `.nam` file can run at: a width of a slimmable WaveNet,
/// a submodel of a `SlimmableContainer`, or a plain file's one network.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Size {
    /// The lowest value of NAM's 0–1 size slider that selects this size
    /// (`i / sizes` for a slimmable WaveNet; for a container's submodel
    /// the previous submodel's `max_value`, rounded up to `f32`, and 0
    /// for the first): [`Nam::slimmed`] takes the same values.
    pub ratio: f32,
    /// Multiply-adds per sample ([`Network::multiply_adds`]): the cost
    /// sizes compare by.
    pub multiply_adds: usize,
    /// Channels per layer, for a width of a slimmable WaveNet (`None`
    /// for a container's submodel or a plain file).
    pub channels: Option<usize>,
}

impl Nam {
    /// Every size the file offers in slider order, the last being the
    /// default [`network`](Self::network). A plain file offers one; a
    /// slimmable WaveNet its `allowed_channels` in order; a container
    /// its submodels in the file's (ascending `max_value`) order.
    pub fn sizes(&self) -> &[Size] {
        &self.sizes
    }

    /// The network at [`sizes`](Self::sizes)`()[index]`, unprepared
    /// (allocates a clone: call off the audio thread). An index past the
    /// end is the full size. A container's submodel comes alone, not in
    /// a [`Container`].
    pub fn network_at(&self, index: usize) -> Network {
        self.networks[index.min(self.networks.len() - 1)].clone()
    }

    /// The network NAM's size slider selects at `ratio` (0–1): for a
    /// slimmable WaveNet `sizes[min(⌊ratio · n⌋, n − 1)]`, the trainer's
    /// own index rule for channel slicing; for a container the submodel
    /// [`Container::size_for`] picks, alone.
    pub fn slimmed(&self, ratio: f32) -> Network {
        if let Network::Container(c) = &self.network {
            return self.network_at(c.size_for(f64::from(ratio)));
        }
        let n = self.networks.len();
        let i = (ratio.clamp(0.0, 1.0) * n as f32).floor() as usize;
        self.network_at(i.min(n - 1))
    }
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
///
/// # Models
///
/// Whatever the capture was: a [`WaveNet`] or an [`Lstm`] trained on a
/// device's input/output recordings, chosen by the file's
/// `architecture`, or a [`Container`] of such submodels at several
/// sizes.
///
/// # Derivation
///
/// Trained out of tree by Neural Amp Modeler's PyTorch trainer; the
/// reader is written from the published `.nam` layout (0.5–0.7, legacy
/// and 0.13 keys) by [`parse`].
///
/// # Verified
///
/// Each architecture against the trainer's own forward pass on files it
/// exported (`crates/stave-dsp/tests/nam_parity.rs::wavenet_standard_matches_the_trainer`,
/// `crates/stave-dsp/tests/nam_parity.rs::lstm_matches_the_trainer`);
/// legacy keys read the same network
/// (`crates/stave-dsp/tests/nam_parity.rs::legacy_keys_read_the_same_network`);
/// every width of two slimmable WaveNets against the trainer slimmed to
/// it (`crates/stave-dsp/tests/nam_parity.rs::slimmable_widths_match_the_trainer`)
/// and every submodel of a `SlimmableContainer`, selected by
/// `max_value`
/// (`crates/stave-dsp/tests/nam_parity.rs::container_submodels_match_the_trainer`).
/// Budget: max abs error ≤ 1e-5 (measured ≤ 6e-7 on the plain files).
///
/// # Cost
///
/// The network it holds, plus one match per sample: see [`WaveNet`]
/// (`"WaveNet (NAM standard)"`) and [`Lstm`] (`"LSTM (1 x 16)"`) on the
/// datasheet; [`multiply_adds`](Self::multiply_adds) compares the sizes
/// of a slimmable file. A container runs its chosen submodel and keeps
/// every other resident.
///
/// # Aliasing
///
/// None added beyond what the network learned at its trained rate,
/// which is not little: `crates/stave-dsp/tests/aliasing_budgets.rs`
/// (`learned_stages_hold_their_alias_budgets`) records each fixture's
/// and each standard size's. A host at half the trained rate runs it in
/// an [`Oversampled2x`](crate::Oversampled2x) region.
///
/// # Valid ranges
///
/// Input in the range it was trained on (NAM normalises captures to
/// about −18 dBFS RMS). Sample rate: the trained rate,
/// [`sample_rate`](Self::sample_rate); at any other it outputs silence
/// and [`rate_mismatch`](Self::rate_mismatch) says why, unless
/// [`RatePolicy::Any`] is chosen, or [`RatePolicy::Interpolated`] for an
/// LSTM (half to sixteen times the trained rate). Features the loader
/// refuses (FiLM, grouped convolutions, condition DSP, post-head,
/// packed WaveNets, unknown container fields) fail at [`parse`] by
/// name. A container's size switch is a hard cut (see [`Container`]).
#[derive(Clone, Debug)]
pub enum Network {
    WaveNet(WaveNet),
    Lstm(Lstm),
    /// A `SlimmableContainer`: every submodel, one running.
    Container(Container),
}

impl Network {
    /// The sample rate the network was trained at, in Hz.
    pub fn sample_rate(&self) -> f32 {
        match self {
            Network::WaveNet(n) => n.sample_rate(),
            Network::Lstm(n) => n.sample_rate(),
            Network::Container(n) => n.sample_rate(),
        }
    }

    /// The trained and prepared rates, when they differ.
    pub fn rate_mismatch(&self) -> Option<RateMismatch> {
        match self {
            Network::WaveNet(n) => n.rate_mismatch(),
            Network::Lstm(n) => n.rate_mismatch(),
            Network::Container(n) => n.rate_mismatch(),
        }
    }

    /// Multiply-adds per sample (see [`WaveNet::multiply_adds`],
    /// [`Lstm::multiply_adds`]; a container's running submodel's).
    pub fn multiply_adds(&self) -> usize {
        match self {
            Network::WaveNet(n) => n.multiply_adds(),
            Network::Lstm(n) => n.multiply_adds(),
            Network::Container(n) => n.multiply_adds(),
        }
    }

    /// What to do at a rate other than the trained one (default
    /// [`RatePolicy::Strict`]: silence). [`RatePolicy::Interpolated`]
    /// runs an LSTM at any rate and leaves a WaveNet strict; it
    /// allocates, so call it off the audio thread.
    pub fn set_rate_policy(&mut self, policy: RatePolicy) {
        match self {
            Network::WaveNet(n) => n.set_rate_policy(policy),
            Network::Lstm(n) => n.set_rate_policy(policy),
            Network::Container(n) => n.set_rate_policy(policy),
        }
    }

    /// NAM's size value (0–1, `SetSlimmableSize`): a [`Container`]
    /// switches to the submodel it selects ([`Container::set_size`];
    /// real-time safe). Any other network has one size here and ignores
    /// it: a slimmable WaveNet's widths are separate networks
    /// ([`Nam::slimmed`]).
    pub fn set_size(&mut self, value: f64) {
        if let Network::Container(c) = self {
            c.set_size(value);
        }
    }

    /// The size values at which [`set_size`](Self::set_size) changes
    /// the network ([`Container::breakpoints`]); empty for any other.
    pub fn breakpoints(&self) -> &[f64] {
        match self {
            Network::Container(c) => c.breakpoints(),
            _ => &[],
        }
    }
}

impl Stage for Network {
    fn prepare(&mut self, sample_rate: f32) {
        match self {
            Network::WaveNet(n) => n.prepare(sample_rate),
            Network::Lstm(n) => n.prepare(sample_rate),
            Network::Container(n) => n.prepare(sample_rate),
        }
    }
    fn reset(&mut self) {
        match self {
            Network::WaveNet(n) => n.reset(),
            Network::Lstm(n) => n.reset(),
            Network::Container(n) => n.reset(),
        }
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        match self {
            Network::WaveNet(n) => n.tick(x),
            Network::Lstm(n) => n.tick(x),
            Network::Container(n) => n.tick(x),
        }
    }
    fn process(&mut self, block: &mut [f32]) {
        match self {
            Network::WaveNet(n) => n.process(block),
            Network::Lstm(n) => n.process(block),
            Network::Container(n) => n.process(block),
        }
    }
    fn latency(&self) -> u32 {
        0
    }
    fn tail(&self) -> Tail {
        match self {
            Network::WaveNet(n) => n.tail(),
            Network::Lstm(n) => n.tail(),
            Network::Container(n) => n.tail(),
        }
    }
}

/// Read a `.nam` file's text (the caller reads the file; this crate
/// does no I/O). Allocates: call it off the audio thread, then hand the
/// network over. A slimmable file builds every size it offers here, so
/// [`Nam::network_at`] only clones; a container's network holds every
/// submodel, so [`Network::set_size`] only switches.
pub fn parse(text: &str) -> Result<Nam, LoadError> {
    let json = Json::parse(text).map_err(|e| LoadError::Json {
        offset: e.offset,
        reason: e.reason,
    })?;
    let file = model(&json, false, DEFAULT_SAMPLE_RATE)?;
    let count = file.networks.len();
    let sizes = file
        .networks
        .iter()
        .enumerate()
        .map(|(i, (channels, n))| Size {
            ratio: match (&file.max_values, i) {
                (None, _) => i as f32 / count as f32,
                (Some(_), 0) => 0.0,
                (Some(m), _) => at_least(m[i - 1]),
            },
            multiply_adds: n.multiply_adds(),
            channels: *channels,
        })
        .collect();
    let networks: Vec<Network> = file.networks.into_iter().map(|(_, n)| n).collect();
    let network = match file.max_values {
        Some(max_values) => Network::Container(Container::new(networks.clone(), max_values)),
        None => networks[count - 1].clone(),
    };
    Ok(Nam {
        network,
        version: file.version,
        sample_rate: file.sample_rate,
        metadata: metadata(json.field("metadata")),
        sizes,
        networks,
    })
}

/// The least `f32` not below `v`, so that a ratio read back as `f64`
/// still selects what `v` does.
fn at_least(v: f64) -> f32 {
    let r = v as f32;
    if f64::from(r) < v {
        r.next_up()
    } else {
        r
    }
}

/// One `.nam` document: its version, recorded rate, and the networks it
/// offers in slider order (never empty; one for a plain file).
struct File {
    version: String,
    sample_rate: Option<f32>,
    networks: Vec<(Option<usize>, Network)>,
    /// A container's `max_value`s, one per network.
    max_values: Option<Vec<f64>>,
}

/// A container's submodels in file order, their `max_value`s, and the
/// rate they agree on.
type Submodels = (Vec<Network>, Vec<f64>, Option<f32>);

/// One document; `rate` is the training rate assumed when it records
/// none (NAM's default, or a container's shared rate for a submodel).
fn model(json: &Json, nested: bool, rate: f32) -> Result<File, LoadError> {
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
    let sample_rate = sample_rate(json)?;
    if architecture == "SlimmableContainer" {
        if nested {
            return Err(LoadError::Unsupported(
                "a SlimmableContainer inside a SlimmableContainer".into(),
            ));
        }
        let (networks, max_values, sample_rate) = container(json, config, sample_rate)?;
        return Ok(File {
            version,
            sample_rate,
            networks: networks.into_iter().map(|n| (None, n)).collect(),
            max_values: Some(max_values),
        });
    }
    let rate = sample_rate.unwrap_or(rate);
    let weights = weights(json)?;
    let networks = match architecture {
        "WaveNet" => wavenet::build(config, &weights, rate)?
            .into_iter()
            .map(|(c, n)| (c, Network::WaveNet(n)))
            .collect(),
        "LSTM" => {
            mono(config, "LSTM")?;
            vec![(
                None,
                Network::Lstm(Lstm::build(
                    size(config, "input_size")?,
                    size(config, "hidden_size")?,
                    size(config, "num_layers")?,
                    &weights,
                    rate,
                )?),
            )]
        }
        other => return Err(LoadError::Architecture(other.to_string())),
    };
    Ok(File {
        version,
        sample_rate,
        networks,
        max_values: None,
    })
}

/// A document's `sample_rate`, when it records one.
fn sample_rate(json: &Json) -> Result<Option<f32>, LoadError> {
    match json.field("sample_rate") {
        None => Ok(None),
        Some(v) => {
            Ok(Some(v.as_f64().filter(|r| *r > 0.0).ok_or_else(|| {
                LoadError::Config("\"sample_rate\" must be a positive number".into())
            })? as f32))
        }
    }
}

/// A `SlimmableContainer` (NeuralAmpModelerCore's `NAM/container.cpp`):
/// `config.submodels`, a non-empty list of `{"max_value": number,
/// "model": <a whole .nam document>}`, strictly ascending by
/// `max_value`, the last at least 1. Every rate recorded (the
/// container's and each submodel's) must agree; a submodel recording
/// none runs at the agreed one. Any other field is refused by name.
fn container(json: &Json, config: &Json, own_rate: Option<f32>) -> Result<Submodels, LoadError> {
    if json.field("weights").is_some() {
        return Err(LoadError::Config(
            "a SlimmableContainer holds no weights of its own".into(),
        ));
    }
    if let Some(key) = config.keys().find(|k| *k != "submodels") {
        return Err(LoadError::Unsupported(format!(
            "SlimmableContainer config field \"{key}\""
        )));
    }
    let submodels = config
        .field("submodels")
        .and_then(Json::as_array)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            LoadError::Config("a SlimmableContainer needs a non-empty \"submodels\" list".into())
        })?;
    // The cheap checks first: shape, ordering, rates.
    let mut inners = Vec::with_capacity(submodels.len());
    let mut max_values: Vec<f64> = Vec::with_capacity(submodels.len());
    let mut agreed = own_rate.map(|r| (r, "the container".to_string()));
    for (i, entry) in submodels.iter().enumerate() {
        let path = submodel_path(i);
        if !matches!(entry, Json::Object(_)) {
            return Err(LoadError::Config(format!("{path} must be an object")));
        }
        if let Some(key) = entry.keys().find(|k| !["max_value", "model"].contains(k)) {
            return Err(LoadError::Unsupported(format!(
                "{path}: field \"{key}\" (a submodel holds \"max_value\" and \"model\")"
            )));
        }
        let max_value = entry
            .field("max_value")
            .and_then(Json::as_f64)
            .ok_or_else(|| LoadError::Config(format!("{path} needs a numeric \"max_value\"")))?;
        if let Some(&previous) = max_values.last() {
            if max_value <= previous {
                return Err(LoadError::Config(format!(
                    "{path}: \"max_value\" {max_value} is not above the previous submodel's \
                     {previous} (submodels must be sorted strictly ascending)"
                )));
            }
        }
        max_values.push(max_value);
        let inner = entry
            .field("model")
            .ok_or_else(|| LoadError::Config(format!("{path} has no \"model\"")))?;
        if let Some(r) = sample_rate(inner).map_err(|e| in_submodel(i, e))? {
            match &agreed {
                Some((a, from)) if *a != r => {
                    return Err(LoadError::Config(format!(
                        "{path}.model was trained at {r} Hz, {from} at {a} Hz"
                    )));
                }
                Some(_) => {}
                None => agreed = Some((r, format!("{path}.model"))),
            }
        }
        inners.push(inner);
    }
    let last = max_values[max_values.len() - 1];
    if last < 1.0 {
        return Err(LoadError::Config(format!(
            "the last SlimmableContainer submodel's \"max_value\" is {last}; it must be at \
             least 1 so every size value selects a submodel"
        )));
    }
    let rate = agreed.map(|(r, _)| r);
    let mut networks = Vec::with_capacity(inners.len());
    for (i, inner) in inners.into_iter().enumerate() {
        let file = model(inner, true, rate.unwrap_or(DEFAULT_SAMPLE_RATE))
            .map_err(|e| in_submodel(i, e))?;
        let mut sizes = file.networks.into_iter();
        let (Some((_, network)), None) = (sizes.next(), sizes.next()) else {
            return Err(LoadError::Unsupported(format!(
                "{}.model: a slimmable network inside a container",
                submodel_path(i)
            )));
        };
        networks.push(network);
    }
    Ok((networks, max_values, rate))
}

/// Where submodel `i` sits in a container file.
fn submodel_path(i: usize) -> String {
    format!("SlimmableContainer config.submodels[{i}]")
}

/// A submodel's error, saying where the submodel is.
fn in_submodel(i: usize, e: LoadError) -> LoadError {
    let at = |what: String| format!("{}.model: {what}", submodel_path(i));
    match e {
        LoadError::Version(v) => LoadError::Config(at(format!("unsupported file version {v}"))),
        LoadError::Architecture(a) => LoadError::Architecture(at(a)),
        LoadError::Config(w) => LoadError::Config(at(w)),
        LoadError::Unsupported(w) => LoadError::Unsupported(at(w)),
        other => other,
    }
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
