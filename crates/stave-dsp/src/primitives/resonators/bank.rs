//! The modal resonator bank.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::units::{Hz, Seconds};

/// One mode of a [`Bank`]: where it rings, how long, how loud.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Mode {
    /// The resonance.
    pub frequency: Hz,
    /// Its T60.
    pub decay: Seconds,
    /// Its peak gain (linear): a sine at `frequency` comes out this
    /// much louder.
    pub gain: f32,
}

/// A bank of tuned modes: any set of resonances, each with its own
/// frequency, T60 and level, excited together.
///
/// # Models
///
/// Modal synthesis: a resonating body as a sum of decaying sinusoidal
/// modes, each a two-pole resonator. Each mode is Mathews & Smith's
/// phasor filter (2003): the complex one-pole `z ← r·e^(jθ)·z + x`,
/// output `Im z` — frequency and decay change without transients or
/// gain jumps, and the rotation stays accurate at very high Q (a direct
/// form loses `θ` near DC). Smear's lattice of resonators is one bank;
/// Abel et al.'s modal reverberator (2015) is a very large one.
///
/// # Derivation
///
/// `r = 10^(−3/(T60·fs))`, `θ = 2πf/fs`. The `Im z` path's transfer
/// function is `r sin θ z⁻¹/(1 − 2r cos θ z⁻¹ + r² z⁻²)`; each mode's
/// output weight is its gain over that function's magnitude at `θ`
/// (computed in f64 at each [`set`](Self::set)), so a mode peaks at its
/// gain. f32 state, coefficients rounded from f64.
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/resonators/tests.rs::bank_modes_ring_as_set`:
/// each of 24 modes (40 Hz–16 kHz, T60 0.05–20 s) rings at its
/// frequency within 0.01 cent, decays within 0.5 % of its T60, and
/// peaks at its gain within 0.01 dB.
///
/// # Cost
///
/// `"Bank (32 modes)"` on the datasheet.
///
/// # Aliasing
///
/// None: linear.
///
/// # Valid ranges
///
/// Frequencies inside (0, 0.5·fs); T60 > 0. f32 rounds `r` to 6e-8, so
/// a 20 s mode at 48 kHz holds its T60 to about 0.5 %; longer drifts
/// further.
///
/// ```
/// use stave_dsp::primitives::resonators::{Bank, Mode};
/// use stave_dsp::units::{hz, seconds};
/// use stave_dsp::Stage;
///
/// // A struck bar: three inharmonic modes.
/// let mut bar = Bank::new(&[
///     Mode { frequency: hz(440.0), decay: seconds(2.0), gain: 1.0 },
///     Mode { frequency: hz(1_210.0), decay: seconds(0.9), gain: 0.5 },
///     Mode { frequency: hz(2_370.0), decay: seconds(0.4), gain: 0.25 },
/// ]);
/// bar.prepare(48_000.0);
/// let strike = bar.tick(1.0);
/// assert!(strike.is_finite());
/// ```
#[derive(Clone, Debug)]
pub struct Bank {
    modes: Vec<Mode>,
    sample_rate: f32,
    /// Per mode: `r·cos θ`, `r·sin θ`, the output weight, and the state.
    re: Vec<f32>,
    im: Vec<f32>,
    weight: Vec<f32>,
    z_re: Vec<f32>,
    z_im: Vec<f32>,
}

impl Bank {
    /// A bank of `modes` (this allocates; [`set`](Self::set) does not).
    pub fn new(modes: &[Mode]) -> Self {
        let n = modes.len();
        let mut b = Self {
            modes: modes.to_vec(),
            sample_rate: 48_000.0,
            re: vec![0.0; n],
            im: vec![0.0; n],
            weight: vec![0.0; n],
            z_re: vec![0.0; n],
            z_im: vec![0.0; n],
        };
        b.update_all();
        b
    }

    /// How many modes.
    pub fn len(&self) -> usize {
        self.modes.len()
    }

    /// Whether the bank has no modes.
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }

    /// The modes as set.
    pub fn modes(&self) -> &[Mode] {
        &self.modes
    }

    /// Retune mode `i` (realtime; its ringing carries on at the new
    /// frequency and decay without a jump). Out of range is ignored.
    pub fn set(&mut self, i: usize, mode: Mode) {
        if let Some(m) = self.modes.get_mut(i) {
            *m = mode;
            self.update(i);
        }
    }

    fn update_all(&mut self) {
        for i in 0..self.modes.len() {
            self.update(i);
        }
    }

    fn update(&mut self, i: usize) {
        let m = self.modes[i];
        let fs = f64::from(self.sample_rate);
        let theta = core::f64::consts::TAU * f64::from(m.frequency.get()).clamp(0.0, 0.5 * fs) / fs;
        let t60 = m.decay.get();
        let r = if t60 > 0.0 { 10f64.powf(-3.0 / (t60 * fs)) } else { 0.0 };
        let (s, c) = theta.sin_cos();
        self.re[i] = (r * c) as f32;
        self.im[i] = (r * s) as f32;
        // |r sin θ e^(−jθ) / (1 − 2r cos θ e^(−jθ) + r² e^(−2jθ))| at θ.
        let (s2, c2) = (2.0 * theta).sin_cos();
        let den_re = 1.0 - 2.0 * r * c * c + r * r * c2;
        let den_im = 2.0 * r * c * s - r * r * s2;
        let peak = r * s / (den_re * den_re + den_im * den_im).sqrt();
        self.weight[i] = if peak > 0.0 { (f64::from(m.gain) / peak) as f32 } else { 0.0 };
    }
}

impl Stage for Bank {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_all();
        self.reset();
    }
    fn reset(&mut self) {
        self.z_re.fill(0.0);
        self.z_im.fill(0.0);
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut y = 0.0f32;
        let lanes = self
            .z_re
            .iter_mut()
            .zip(self.z_im.iter_mut())
            .zip(self.re.iter().zip(&self.im))
            .zip(&self.weight);
        for (((zr, zi), (cr, ci)), w) in lanes {
            let r = cr * *zr - ci * *zi + x;
            let i = ci * *zr + cr * *zi;
            *zr = denormal::flush(r);
            *zi = denormal::flush(i);
            y += w * i;
        }
        y
    }
    /// The longest mode's T60, doubled (60 → 120 dB).
    fn tail(&self) -> Tail {
        let longest = self.modes.iter().map(|m| m.decay.get()).fold(0.0f64, f64::max);
        if self.modes.is_empty() {
            Tail::None
        } else {
            Tail::from_time_constant(longest / 6.907_755, self.sample_rate)
        }
    }
}
