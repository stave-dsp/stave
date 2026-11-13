//! The Korg35 (MS-20) lowpass, zero-delay, its resonance saturating.

use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::zdf::{self, Knobs};
use crate::primitives::solve::NewtonStats;
use crate::units::{Db, Hz};

/// The Korg35 lowpass of the MS-20: two one-pole lowpasses with the
/// output's highpass fed back between them through a saturator.
///
/// # Models
/// Vadim Zavalishin's lowpass Sallen–Key filter (*The Art of VA Filter
/// Design*, rev. 2.1.0, §5.8) — the structure the MS-20's Korg35 filter is
/// modelled with: a lowpass into a second lowpass whose input also
/// receives `tanh(k·hp(y))`, the highpass of the output at the same
/// cutoff, with `k = 2·resonance`. Small-signal it is
/// `1 / (s² + (2 − k)·s + 1)`: self-oscillation at `k = 2`
/// (resonance 1), its level held by the saturator, as the MS-20's
/// diodes hold it. A structure: no claim on the Korg35 chip's internals.
///
/// # Derivation
/// The three one-poles, integrated by the trapezoidal rule (Zavalishin's
/// topology-preserving transform) with zero-delay feedback, prewarped at
/// the cutoff. The first lowpass and the highpass's lowpass resolve in
/// closed form, so Newton runs on the output alone. f64 state.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/filters/korg35/tests.rs::cutoff_is_exact_across_the_range`:
/// the small-signal response within 0.01 dB and 0.1° of the prewarped
/// prototype at half, one and one and a half times the cutoff,
/// 50 Hz–16 kHz, resonance 0–0.8 (measured 7e-6 dB); its siblings hold
/// self-oscillation (within 0.5 % of the cutoff at 1.01),
/// the drive's harmonics, rate invariance and aliasing.
///
/// # Cost
/// Datasheet `"Korg35"` at 48 kHz.
///
/// # Aliasing
/// Only the feedback saturates, behind two poles: 2× is enough —
/// [`Oversampled2x::front`](crate::Oversampled2x::front), 66 samples of
/// latency. A stepped tone (500 Hz–12 kHz, half scale, +12 dB drive,
/// cutoff 8 kHz): audible inharmonic energy 79.6 dB down at 44.1 kHz,
/// 82.9 dB at 48 kHz (budgets 77 and 80,
/// `crates/stave-dsp/tests/aliasing_budgets.rs::nonlinear_stages_hold_their_alias_budgets`). At resonance 0 nothing saturates.
///
/// # Valid ranges
/// Cutoff 1 Hz–0.49·fs (held there), resonance 0–1.5 (held there: past
/// it the implicit step can lose its unique solution), drive −24 to
/// +36 dB. Solves are capped and counted ([`stats`](Self::stats)).
/// The cutoff may move every sample, at audio rate: swept 50 Hz–15 kHz
/// at 1.7 kHz it stays bounded and converged
/// (`crates/stave-dsp/src/primitives/filters/korg35/tests.rs::stays_bounded_under_audio_rate_modulation`).
#[derive(Clone, Debug)]
pub struct Korg35 {
    knobs: Knobs,
    g: f64,
    /// The trapezoidal states: first lowpass, second lowpass, the
    /// highpass's lowpass.
    s: [f64; 3],
    y: f64,
    stats: NewtonStats,
}

/// The largest resonance: `k = 3`, where the step's slope stays positive.
const MAX_RESONANCE: f64 = 1.5;

impl Korg35 {
    /// A lowpass at `cutoff` (where it sings once `resonance` reaches 1),
    /// drive 0 dB.
    pub fn new(cutoff: Hz, resonance: f32) -> Self {
        let knobs = Knobs::new(cutoff, resonance);
        Self {
            g: knobs.warped(),
            knobs,
            s: [0.0; 3],
            y: 0.0,
            stats: NewtonStats::default(),
        }
    }

    /// Retune, state kept; cheap enough per sample (one `tan`).
    pub fn set_cutoff(&mut self, cutoff: Hz) {
        self.knobs.cutoff = cutoff;
        self.g = self.knobs.warped();
    }

    /// 0 is two plain poles, 1 the onset of self-oscillation; held
    /// inside 0–1.5.
    pub fn set_resonance(&mut self, resonance: f32) {
        self.knobs.resonance = resonance;
    }

    /// Gain into the filter (0 dB: the feedback saturates at full
    /// scale); the output is divided by it.
    pub fn set_drive(&mut self, drive: Db) {
        self.knobs.set_drive(drive);
    }

    /// Newton's work and cap hits since `prepare`.
    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    fn feedback(&self) -> f64 {
        2.0 * self.knobs.resonance().min(MAX_RESONANCE)
    }

    /// One sample in f64.
    pub fn tick_f64(&mut self, input: f64) -> f64 {
        let (g, d) = (self.g, self.knobs.drive);
        let k = self.feedback();
        let [s1, s2, s3] = self.s;
        let a = (s1 + g * d * input) / (1.0 + g);
        // hp(y) = y − (s₃ + g·y)/(1 + g), linear in y.
        let hp = |y: f64| (y - s3) / (1.0 + g);
        let mut y = self.y;
        let (mut iterations, mut converged) = (0, false);
        loop {
            let t = zdf::tanh(k * hp(y));
            let r = y - s2 - g * (a + t - y);
            if r.abs() < zdf::TOLERANCE {
                converged = true;
                break;
            }
            if iterations == zdf::MAX_ITERATIONS {
                break;
            }
            iterations += 1;
            let slope = 1.0 + g - g * k * (1.0 - t * t) / (1.0 + g);
            zdf::update(core::array::from_mut(&mut y), &[r / slope]);
        }
        zdf::record(&mut self.stats, iterations, converged);
        let l3 = (s3 + g * y) / (1.0 + g);
        self.s = [2.0 * a - s1, 2.0 * y - s2, 2.0 * l3 - s3];
        self.y = y;
        y / d
    }
}

impl Stage for Korg35 {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.knobs.sample_rate = sample_rate;
        self.g = self.knobs.warped();
        self.reset();
        self.stats = NewtonStats::default();
    }
    fn reset(&mut self) {
        self.s = [0.0; 3];
        self.y = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }
    /// The linearised poles, `(s² + (2 − k)·s + 1)·(s + 1) = 0` (the
    /// first lowpass's pole cancels in the response but not in the state).
    fn tail(&self) -> Tail {
        let b = 2.0 - self.feedback();
        zdf::linear_tail(&[1.0, 1.0 + b, 1.0 + b, 1.0], self.g)
    }
}

#[cfg(test)]
mod tests;
