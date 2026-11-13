//! The Sallen–Key lowpass with a saturating amplifier, zero-delay.

use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::zdf::{self, Knobs};
use crate::primitives::solve::NewtonStats;
use crate::units::{Db, Hz};

/// The two-pole Sallen–Key lowpass (equal resistors, equal capacitors)
/// whose gain stage saturates: resonance is the stage's gain, drive how
/// hard it is pushed.
///
/// # Models
/// The unity-component Sallen–Key lowpass of R. P. Sallen and E. L. Key
/// (*A practical method of designing RC active filters*, 1955): the first
/// capacitor bootstrapped from the amplifier's output, the second to
/// ground, the amplifier of gain `K = 1 + 2·resonance` saturating as
/// `tanh` at ±1 (the drive's full scale). Small-signal it is
/// `K / (s² + (3 − K)·s + 1)`: `Q = 1/(3 − K)`, self-oscillation at
/// `K = 3` (resonance 1), the level held by the amplifier's saturation.
/// The output is the amplifier's divided by `K·drive`, so the passband
/// sits at unity. A structure, no claim on a particular unit (the
/// MS-20's later revision and many synth filters are Sallen–Keys).
///
/// # Derivation
/// The circuit's two capacitor voltages, `q₁′ = x − 2q₁ − 2y + q₂`,
/// `q₂′ = q₁ + y − q₂`, `y = tanh(K·q₂)` (time in units of `1/ω₀`),
/// integrated by the trapezoidal rule with zero-delay feedback,
/// prewarped at the cutoff; Newton on both states each sample. f64 state.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/filters/sallen_key/tests.rs::cutoff_is_exact_across_the_range`:
/// the small-signal response within 0.01 dB and 0.1° of the prewarped
/// prototype at half, one and one and a half times the cutoff,
/// 50 Hz–16 kHz, resonance 0–0.8 (measured 7e-6 dB); its siblings hold
/// self-oscillation (within 0.5 % of the cutoff at 1.01),
/// the drive's harmonics, rate invariance and aliasing.
///
/// # Cost
/// Datasheet `"SallenKey"` at 48 kHz.
///
/// # Aliasing
/// The amplifier's `tanh` clips hard when driven: run it at 4× —
/// `Oversampled2x::front(Oversampled2x::nested(..))`, 84 samples of
/// latency. A stepped tone (500 Hz–12 kHz, half scale, +12 dB drive,
/// cutoff 8 kHz): audible inharmonic energy 95.9 dB down at 44.1 kHz,
/// 100.6 dB at 48 kHz (budgets 93 and 98,
/// `crates/stave-dsp/tests/aliasing_budgets.rs::nonlinear_stages_hold_their_alias_budgets`); 45.5 and 47.0 dB at 2×.
///
/// # Valid ranges
/// Cutoff 1 Hz–0.49·fs (held there), resonance 0–1.5, drive −24 to
/// +36 dB. Solves are capped and counted ([`stats`](Self::stats)).
/// The cutoff may move every sample, at audio rate: swept 50 Hz–15 kHz
/// at 1.7 kHz it stays bounded and converged
/// (`crates/stave-dsp/src/primitives/filters/sallen_key/tests.rs::stays_bounded_under_audio_rate_modulation`).
#[derive(Clone, Debug)]
pub struct SallenKey {
    knobs: Knobs,
    g: f64,
    s: [f64; 2],
    q: [f64; 2],
    stats: NewtonStats,
}

impl SallenKey {
    /// A lowpass at `cutoff` (its pole frequency, where it sings once
    /// `resonance` reaches 1), drive 0 dB.
    pub fn new(cutoff: Hz, resonance: f32) -> Self {
        let knobs = Knobs::new(cutoff, resonance);
        Self {
            g: knobs.warped(),
            knobs,
            s: [0.0; 2],
            q: [0.0; 2],
            stats: NewtonStats::default(),
        }
    }

    /// Retune, state kept; cheap enough per sample (one `tan`).
    pub fn set_cutoff(&mut self, cutoff: Hz) {
        self.knobs.cutoff = cutoff;
        self.g = self.knobs.warped();
    }

    /// 0 is a Q of 0.5 (unity gain), 1 the onset of self-oscillation
    /// (gain 3).
    pub fn set_resonance(&mut self, resonance: f32) {
        self.knobs.resonance = resonance;
    }

    /// Gain into the filter (0 dB: the amplifier clips at full scale);
    /// the output is divided by it.
    pub fn set_drive(&mut self, drive: Db) {
        self.knobs.set_drive(drive);
    }

    /// Newton's work and cap hits since `prepare`.
    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    #[inline]
    fn slopes(x: f64, gain: f64, q: &[f64; 2]) -> ([f64; 2], f64, f64) {
        let y = zdf::tanh(gain * q[1]);
        let f = [x - 2.0 * q[0] - 2.0 * y + q[1], q[0] + y - q[1]];
        (f, y, gain * (1.0 - y * y))
    }

    /// One sample in f64.
    pub fn tick_f64(&mut self, input: f64) -> f64 {
        let (g, d) = (self.g, self.knobs.drive);
        let gain = 1.0 + 2.0 * self.knobs.resonance();
        let x = d * input;
        let s = self.s;
        let mut q = self.q;
        let (mut iterations, mut converged) = (0, false);
        let (mut f, mut y);
        loop {
            let (slopes, out, c) = Self::slopes(x, gain, &q);
            (f, y) = (slopes, out);
            let mut r = [q[0] - s[0] - g * f[0], q[1] - s[1] - g * f[1]];
            if zdf::small(&r) {
                converged = true;
                break;
            }
            if iterations == zdf::MAX_ITERATIONS {
                break;
            }
            iterations += 1;
            let mut j = [
                [1.0 + 2.0 * g, -g * (1.0 - 2.0 * c)],
                [-g, 1.0 - g * (c - 1.0)],
            ];
            zdf::solve(&mut j, &mut r);
            zdf::update(&mut q, &r);
        }
        zdf::record(&mut self.stats, iterations, converged);
        self.s = [q[0] + g * f[0], q[1] + g * f[1]];
        self.q = q;
        y / (gain * d)
    }
}

impl Stage for SallenKey {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.knobs.sample_rate = sample_rate;
        self.g = self.knobs.warped();
        self.reset();
        self.stats = NewtonStats::default();
    }
    fn reset(&mut self) {
        self.s = [0.0; 2];
        self.q = [0.0; 2];
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }
    /// The linearised poles, `s² + (2 − 2·resonance)·s + 1 = 0`.
    fn tail(&self) -> Tail {
        let r = self.knobs.resonance();
        zdf::linear_tail(&[1.0, 2.0 - 2.0 * r, 1.0], self.g)
    }
}

#[cfg(test)]
mod tests;
