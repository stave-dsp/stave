//! **WaveNet** — the feed-forward network Neural Amp Modeler trains:
//! stacks of gated, dilated causal convolutions with a residual path and
//! a summed skip ("head") path.

use super::activation::Activation;
use super::conv::{padded, Conv, History};
use super::error::{LoadError, Weights};
use super::rate::{RateGuard, RateMismatch, RatePolicy};
use crate::kernel::node::{Stage, Tail};

/// A WaveNet (van den Oord et al. 2016, as adapted to amplifier capture
/// by Wright, Damskägg & Välimäki 2019 and Neural Amp Modeler), run one
/// sample at a time.
///
/// # Models
///
/// Whatever it was trained on: an amplifier, pedal or other device
/// captured by input/output recordings. The network makes no claim
/// about the device's internals; it is the layer `learned` because its
/// claim is the training data.
///
/// One or more *layer arrays*, each: a 1×1 rechannel of its input, then
/// layers of
///
/// ```text
/// z    = dilated_conv(x) + mixer(input)        (K taps, d apart; causal)
/// a    = act(z)  |  act(z₁)·gate(z₂)  |  blend  (plain, gated, blended)
/// x   += layer1x1(a)                          (residual)
/// head += head1x1(a)  (or a)                  (skip)
/// ```
///
/// then a convolution of the summed head path. Each array's head output
/// seeds the next array's head sum; the last array's head output times
/// `head_scale` is the sample out.
///
/// # Derivation
///
/// Trained out of tree (Neural Amp Modeler's PyTorch trainer) and loaded
/// from its `.nam` file by [`nam::parse`](super::nam::parse); the
/// convolutions are PyTorch's `Conv1d` streamed causally. Silence
/// before the first sample is the trainer's convention (`pad_start`):
/// [`reset`](Stage::reset) settles every layer's history to the state
/// infinite silence would leave (each layer's constant response, solved
/// once per reset), so the first output matches the trainer's.
///
/// # Verified
///
/// Three `.nam` files exported by Neural Amp Modeler 0.13 run against
/// the trainer's own forward pass: the standard two-array, gated net
/// (`crates/stave-dsp/tests/nam_parity.rs::wavenet_standard_matches_the_trainer`),
/// the 0.13 trainer's single-array shape with per-layer kernels and a
/// 4-tap head (`crates/stave-dsp/tests/nam_parity.rs::wavenet_packed_matches_the_trainer`),
/// and bottleneck, head 1×1, blended gating and five activations
/// (`crates/stave-dsp/tests/nam_parity.rs::wavenet_mixed_matches_the_trainer`);
/// random shapes against an independent f64 forward pass
/// (`crates/stave-dsp/tests/nam_parity.rs::random_wavenets_match_the_f64_reference`).
/// Budget: max abs error ≤ 1e-5 (measured ≤ 6e-7 against the trainer,
/// ≤ 3e-7 against the f64 reference). No allocation in `process`:
/// `crates/stave-dsp/tests/learned_no_alloc.rs::networks_process_without_allocating`.
/// `scripts/nam-reference.py` regenerates the references.
///
/// # Cost
///
/// The standard capture architecture (16 + 8 channels, 2 × 10 layers,
/// kernel 3), about 13 000 multiply-adds and 240 `tanh` a sample:
/// datasheet `"WaveNet (NAM standard)"`, about 14% of one core per
/// channel at 48 kHz; a lighter one is `"WaveNet (12 + 6 channels)"`.
/// One sample at a time, output lanes vectorised; batching frames per
/// block (weight reuse) is DSP.md P4's.
///
/// # Aliasing
///
/// None added beyond the trained network's own: it was trained at one
/// rate and its nonlinearity's aliasing at that rate is part of what it
/// learned. Running it in an [`Oversampled2x`](crate::Oversampled2x)
/// region is only correct when the region's rate is the trained rate
/// (see [`RatePolicy`]). Measured
/// (`crates/stave-dsp/tests/aliasing_budgets.rs::learned_stages_hold_their_alias_budgets`,
/// worst inharmonic energy at or below 20 kHz re the fundamental,
/// 44.1/48 kHz): the standard-shape fixture −32.2/−31.7 dB, a random
/// NAM-standard network (weights ±0.125) −72.4/−73.4, a slimmable
/// fixture −50.8/−51.1 at one channel and −39.3/−34.7 at four.
///
/// # Valid ranges
///
/// Input in the range it was trained on (NAM normalises captures to
/// about −18 dBFS RMS; peaks near ±1). Sample rate: the trained rate,
/// [`sample_rate`](Self::sample_rate); at any other the stage outputs
/// silence unless [`RatePolicy::Any`] is chosen. A WaveNet stays
/// rate-bound under [`RatePolicy::Interpolated`] (its memory is a learned
/// FIR, with no state to delay): silent, the mismatch reported
/// (`crates/stave-dsp/tests/learned_rates.rs::wavenets_stay_rate_bound`).
#[derive(Clone, Debug)]
pub struct WaveNet {
    arrays: Vec<LayerArray>,
    head_scale: f32,
    receptive_field: usize,
    rate: RateGuard,
}

/// How a layer turns its convolution output into activations.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Gating {
    /// `act(z)`, `bottleneck` channels.
    None,
    /// `act(z₁) · gate(z₂)`, z split into two halves of `bottleneck`.
    Gated(Activation),
    /// `g·act(z₁) + (1 − g)·z₁` with `g = gate(z₂)`.
    Blended(Activation),
}

/// One layer's shape, as a file describes it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayerSpec {
    pub(crate) kernel: usize,
    pub(crate) dilation: usize,
    pub(crate) activation: Activation,
    pub(crate) gating: Gating,
}

/// One layer array's shape, as a file describes it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArraySpec {
    pub(crate) input_size: usize,
    pub(crate) condition_size: usize,
    pub(crate) channels: usize,
    pub(crate) bottleneck: usize,
    pub(crate) layers: Vec<LayerSpec>,
    pub(crate) layer1x1: bool,
    /// Output channels of the head 1×1, when it is active.
    pub(crate) head1x1: Option<usize>,
    pub(crate) head_size: usize,
    pub(crate) head_kernel: usize,
    pub(crate) head_dilation: usize,
    pub(crate) head_bias: bool,
}

impl ArraySpec {
    /// Width of the summed head path inside the array.
    fn head_width(&self) -> usize {
        self.head1x1.unwrap_or(self.bottleneck)
    }
}

#[derive(Clone, Debug)]
struct Layer {
    conv: Conv,
    mixer: Conv,
    layer1x1: Option<Conv>,
    head1x1: Option<Conv>,
    activation: Activation,
    gating: Gating,
    bottleneck: usize,
    /// The layer's input, as far back as the convolution reads.
    history: History,
}

#[derive(Clone, Debug)]
struct LayerArray {
    rechannel: Conv,
    layers: Vec<Layer>,
    head_rechannel: Conv,
    head_history: History,
    /// The residual path (`channels`), the array's output.
    residual: Vec<f32>,
    /// The summed head path.
    head: Vec<f32>,
    /// The head convolution's output (`head_size`).
    head_out: Vec<f32>,
    /// Scratch: convolution output, activations, gate, 1×1 outputs.
    z: Vec<f32>,
    act: Vec<f32>,
    gate: Vec<f32>,
    tmp: Vec<f32>,
}

impl WaveNet {
    /// Build from layer-array shapes and the exporter's flat weight list
    /// (per array: rechannel, per layer conv + bias, mixer, layer 1×1 +
    /// bias, head 1×1 + bias; head convolution [+ bias]; finally
    /// `head_scale`, which overrides the config's).
    pub(crate) fn build(
        specs: &[ArraySpec],
        weights: &[f32],
        sample_rate: f32,
    ) -> Result<Self, LoadError> {
        validate(specs)?;
        let mut w = Weights::new(weights);
        let mut arrays = Vec::with_capacity(specs.len());
        let mut receptive_field = 1;
        for spec in specs {
            let rechannel = Conv::read(&mut w, spec.input_size, spec.channels, 1, 1, false);
            let mut layers = Vec::with_capacity(spec.layers.len());
            let mut widest_z = 0;
            for l in &spec.layers {
                let mid = match l.gating {
                    Gating::None => spec.bottleneck,
                    _ => 2 * spec.bottleneck,
                };
                widest_z = widest_z.max(mid);
                let conv = Conv::read(&mut w, spec.channels, mid, l.kernel, l.dilation, true);
                let mixer = Conv::read(&mut w, spec.condition_size, mid, 1, 1, false);
                let layer1x1 = spec
                    .layer1x1
                    .then(|| Conv::read(&mut w, spec.bottleneck, spec.channels, 1, 1, true));
                let head1x1 = spec
                    .head1x1
                    .map(|h| Conv::read(&mut w, spec.bottleneck, h, 1, 1, true));
                receptive_field += conv.span() - 1;
                layers.push(Layer {
                    history: History::new(spec.channels, conv.span()),
                    conv,
                    mixer,
                    layer1x1,
                    head1x1,
                    activation: l.activation.clone(),
                    gating: l.gating.clone(),
                    bottleneck: spec.bottleneck,
                });
            }
            let head_rechannel = Conv::read(
                &mut w,
                spec.head_width(),
                spec.head_size,
                spec.head_kernel,
                spec.head_dilation,
                spec.head_bias,
            );
            receptive_field += head_rechannel.span() - 1;
            arrays.push(LayerArray {
                head_history: History::new(spec.head_width(), head_rechannel.span()),
                rechannel,
                layers,
                head_rechannel,
                // Buffers a convolution writes are whole lane groups.
                residual: vec![0.0; padded(spec.channels)],
                head: vec![0.0; padded(spec.head_width())],
                head_out: vec![0.0; padded(spec.head_size)],
                z: vec![0.0; padded(widest_z)],
                act: vec![0.0; spec.bottleneck],
                gate: vec![0.0; spec.bottleneck],
                tmp: vec![0.0; padded(spec.channels.max(spec.head_width()))],
            });
        }
        let head_scale = w.one();
        w.finish()?;
        let mut net = Self {
            arrays,
            head_scale,
            receptive_field,
            rate: RateGuard::new(sample_rate),
        };
        net.reset();
        Ok(net)
    }

    /// The sample rate the network was trained at, in Hz.
    pub fn sample_rate(&self) -> f32 {
        self.rate.trained()
    }

    /// The trained and prepared rates, when the stage was prepared at a
    /// rate the network was not trained at.
    pub fn rate_mismatch(&self) -> Option<RateMismatch> {
        self.rate.mismatch()
    }

    /// What to do at a rate other than the trained one (default
    /// [`RatePolicy::Strict`]: silence).
    pub fn set_rate_policy(&mut self, policy: RatePolicy) {
        self.rate.set_policy(policy);
    }

    /// Input samples one output depends on (the current one included).
    pub fn receptive_field(&self) -> usize {
        self.receptive_field
    }

    /// Multiply-adds per sample, every convolution counted (the
    /// activations' transcendentals are extra): the figure a plugin
    /// compares sizes of one capture by.
    pub fn multiply_adds(&self) -> usize {
        self.arrays
            .iter()
            .map(|a| {
                let layers: usize = a
                    .layers
                    .iter()
                    .map(|l| {
                        l.conv.multiply_adds()
                            + l.mixer.multiply_adds()
                            + l.layer1x1.as_ref().map_or(0, Conv::multiply_adds)
                            + l.head1x1.as_ref().map_or(0, Conv::multiply_adds)
                    })
                    .sum();
                a.rechannel.multiply_adds() + layers + a.head_rechannel.multiply_adds()
            })
            .sum()
    }

    /// One step of the network; `settle` first fills every history with
    /// its current input, the state that input held forever leaves.
    fn step(&mut self, x: f32, settle: bool) -> f32 {
        let condition = [x];
        for i in 0..self.arrays.len() {
            let (done, rest) = self.arrays.split_at_mut(i);
            let (input, head_in) = match done.last() {
                None => (&condition[..], None),
                Some(prev) => (&prev.residual[..], Some(&prev.head_out[..])),
            };
            rest[0].step(input, head_in, &condition, settle);
        }
        let last = self.arrays.last().map_or(0.0, |a| a.head_out[0]);
        self.head_scale * last
    }
}

impl LayerArray {
    #[inline]
    fn step(&mut self, input: &[f32], head_in: Option<&[f32]>, condition: &[f32], settle: bool) {
        self.rechannel.apply_now(input, &mut self.residual);
        match head_in {
            Some(h) => self.head.copy_from_slice(h),
            None => self.head.fill(0.0),
        }
        for layer in &mut self.layers {
            layer.step(
                &mut self.residual,
                condition,
                &mut self.head,
                &mut self.z,
                &mut self.act,
                &mut self.gate,
                &mut self.tmp,
                settle,
            );
        }
        if settle {
            self.head_history.fill(&self.head);
        } else {
            self.head_history.push(&self.head);
        }
        self.head_rechannel
            .apply(&self.head_history, &mut self.head_out);
    }
}

impl Layer {
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        residual: &mut [f32],
        condition: &[f32],
        head: &mut [f32],
        z: &mut [f32],
        act: &mut [f32],
        gate: &mut [f32],
        tmp: &mut [f32],
        settle: bool,
    ) {
        if settle {
            self.history.fill(residual);
        } else {
            self.history.push(residual);
        }
        self.conv.apply(&self.history, z);
        self.mixer.add_now(condition, z);
        let b = self.bottleneck;
        let act = &mut act[..b];
        self.activation.apply_into(&z[..b], act);
        match &self.gating {
            Gating::None => {}
            Gating::Gated(g) => {
                g.apply_into(&z[b..2 * b], gate);
                for (a, &s) in act.iter_mut().zip(&*gate) {
                    *a *= s;
                }
            }
            Gating::Blended(g) => {
                g.apply_into(&z[b..2 * b], gate);
                for ((a, &s), &z1) in act.iter_mut().zip(&*gate).zip(&z[..b]) {
                    *a = s * *a + (1.0 - s) * z1;
                }
            }
        }
        match &self.head1x1 {
            Some(conv) => {
                conv.apply_now(act, tmp);
                add(head, &tmp[..conv.outputs]);
            }
            None => add(head, &act[..b]),
        }
        match &self.layer1x1 {
            Some(conv) => {
                conv.apply_now(act, tmp);
                add(residual, &tmp[..conv.outputs]);
            }
            None => add(residual, &act[..b]),
        }
    }
}

#[inline]
fn add(acc: &mut [f32], x: &[f32]) {
    for (a, &v) in acc.iter_mut().zip(x) {
        *a += v;
    }
}

/// The shapes chain: mono in, each array fed by the last, one sample out.
fn validate(specs: &[ArraySpec]) -> Result<(), LoadError> {
    let bad = |what: String| Err(LoadError::Config(what));
    let Some(first) = specs.first() else {
        return bad("a WaveNet needs at least one layer array".into());
    };
    if first.input_size != 1 || first.condition_size != 1 {
        return Err(LoadError::Unsupported(format!(
            "input_size {} / condition_size {} (mono networks read 1 / 1)",
            first.input_size, first.condition_size
        )));
    }
    for (i, s) in specs.iter().enumerate() {
        if s.channels == 0 || s.bottleneck == 0 || s.head_size == 0 || s.layers.is_empty() {
            return bad(format!(
                "layer array {i} has an empty dimension or no layers"
            ));
        }
        if s.head_kernel == 0 || s.head_dilation == 0 || s.head1x1 == Some(0) {
            return bad(format!(
                "layer array {i}: head kernel, dilation and 1x1 must be at least 1"
            ));
        }
        if s.condition_size != 1 {
            return Err(LoadError::Unsupported(format!(
                "layer array {i}: condition_size {} (the condition is the mono input)",
                s.condition_size
            )));
        }
        if !s.layer1x1 && s.bottleneck != s.channels {
            return bad(format!(
                "layer array {i}: without a layer 1x1 the bottleneck must equal the channels"
            ));
        }
        if i > 0 {
            let prev = &specs[i - 1];
            if s.input_size != prev.channels {
                return bad(format!(
                    "layer array {i}: input_size {} but the previous array has {} channels",
                    s.input_size, prev.channels
                ));
            }
            if s.head_width() != prev.head_size {
                return bad(format!(
                    "layer array {i}: head path {} wide but the previous array's head outputs {}",
                    s.head_width(),
                    prev.head_size
                ));
            }
        }
        for (j, l) in s.layers.iter().enumerate() {
            if l.kernel == 0 || l.dilation == 0 {
                return bad(format!(
                    "layer array {i} layer {j}: kernel and dilation must be at least 1"
                ));
            }
            let mut acts = vec![&l.activation];
            if let Gating::Gated(g) | Gating::Blended(g) = &l.gating {
                acts.push(g);
            }
            for a in acts {
                if a.channels().is_some_and(|n| n != s.bottleneck) {
                    return bad(format!(
                        "layer array {i} layer {j}: PReLU slopes do not match the bottleneck"
                    ));
                }
            }
        }
    }
    let out = specs[specs.len() - 1].head_size;
    if out != 1 {
        return Err(LoadError::Unsupported(format!(
            "{out} output channels (mono networks output 1)"
        )));
    }
    Ok(())
}

impl Stage for WaveNet {
    /// Record the rate (see [`RatePolicy`]) and settle the network.
    fn prepare(&mut self, sample_rate: f32) {
        self.rate.prepare(sample_rate);
        self.reset();
    }

    /// Settle every layer to the state infinite silence leaves.
    fn reset(&mut self) {
        self.step(0.0, true);
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        if self.rate.muted() {
            return 0.0;
        }
        self.step(x, false)
    }

    /// Output settles `receptive_field − 1` samples after input stops
    /// (to the network's response to silence, which need not be zero).
    fn tail(&self) -> Tail {
        Tail::samples(u32::try_from(self.receptive_field - 1).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests;
