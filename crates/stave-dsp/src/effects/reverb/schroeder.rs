//! Schroeder's reverberator — parallel combs into series allpasses —
//! and Moorer's refinement with lowpass combs.

use crate::kernel::node::{Stage, Tail};
use crate::units::Seconds;
use crate::primitives::time::comb::{Comb, DelayAllpass};

use super::{decay_tail, loop_gain};

const MAX_COMBS: usize = 6;
const MAX_ALLPASSES: usize = 2;

/// Schroeder (1962): four combs, two allpasses (ms, coefficient).
const SCHROEDER_COMBS_MS: [f32; 4] = [29.7, 37.1, 41.1, 43.7];
const SCHROEDER_ALLPASSES: [(f32, f32); 2] = [(5.0, 0.7), (1.7, 0.7)];
/// Moorer (1979): six lowpass combs, one allpass.
const MOORER_COMBS_MS: [f32; 6] = [50.0, 56.0, 61.0, 68.0, 72.0, 78.0];
const MOORER_ALLPASSES: [(f32, f32); 1] = [(6.0, 0.7)];
/// Moorer's in-loop lowpass, as [`Comb::set_damping`]'s 0–1.
const MOORER_DAMPING: f32 = 0.3;

/// Schroeder's reverberator: parallel feedback combs, summed, through
/// series allpass diffusers. [`Schroeder::moorer`] is Moorer's version,
/// with a lowpass in every comb so highs decay faster, as in a room.
///
/// # Models
/// Schroeder, "Natural sounding artificial reverberation", JAES 10(3),
/// 1962 (four combs, two allpasses), and Moorer, "About this
/// reverberation business", CMJ 3(2), 1979 (six lowpass combs, one
/// allpass; the early-reflection taps are left to the caller). No
/// hardware claim; mono, wet only.
///
/// # Derivation
/// Each comb of `mᵢ` whole samples recirculates with
/// `gᵢ = 10^(−3·mᵢ/(T60·sample_rate))` (all combs fall 60 dB in T60);
/// Moorer's in-loop lowpass has unity DC gain, so the T60 holds at low
/// frequencies and shortens above. Built on [`Comb`] and
/// [`DelayAllpass`]; lengths in ms are rounded to whole samples at the
/// prepared rate.
///
/// # Verified
/// `crates/stave-dsp/src/effects/reverb/tests.rs::measured_t60_matches_the_setting`,
/// `crates/stave-dsp/src/effects/reverb/tests.rs::stable_at_maximum_decay` and
/// `crates/stave-dsp/src/effects/reverb/tests.rs::sample_rate_invariant` (T60 as Schroeder T30 around 1 kHz): within
/// 5 % (Schroeder: four combs leave few modes near 1 kHz, so the read is
/// noisier) and 3 % (Moorer) of the setting at 1 and 2.5 s (measured
/// −3.7 % / +1.2 % and −0.3 % / −1.8 %); stable at infinite T60; from
/// 44.1 to 192 kHz the T60 holds within 2.2 % and the level within
/// 0.07 dB.
///
/// # Cost
/// `"Schroeder (4 combs, 2 allpasses)"` and `"Schroeder::moorer (6 combs)"`
/// on the datasheet.
///
/// # Aliasing
/// None (linear).
///
/// # Valid ranges
/// T60 in `(0, ∞]` s; damping in `[0, 1)`.
#[derive(Clone, Debug)]
pub struct Schroeder {
    combs: [Comb; MAX_COMBS],
    comb_count: usize,
    allpasses: [DelayAllpass; MAX_ALLPASSES],
    allpass_count: usize,
    t60_s: f32,
    sample_rate: f32,
}

impl Default for Schroeder {
    fn default() -> Self {
        Self::new()
    }
}

impl Schroeder {
    fn build(combs_ms: &[f32], allpasses: &[(f32, f32)], damping: f32) -> Self {
        let mut s = Self {
            combs: core::array::from_fn(|i| {
                let ms = combs_ms.get(i).copied().unwrap_or(1.0);
                let delay = Seconds::new(f64::from(ms) / 1e3);
                let mut c = Comb::new(delay, delay, 0.0);
                c.set_damping(damping);
                c
            }),
            comb_count: combs_ms.len(),
            allpasses: core::array::from_fn(|i| {
                let (ms, g) = allpasses.get(i).copied().unwrap_or((1.0, 0.0));
                let delay = Seconds::new(f64::from(ms) / 1e3);
                DelayAllpass::new(delay, delay, g)
            }),
            allpass_count: allpasses.len(),
            t60_s: 1.5,
            sample_rate: 48_000.0,
        };
        s.update();
        s
    }

    /// Schroeder's four combs and two allpasses, T60 1.5 s.
    pub fn new() -> Self {
        Self::build(&SCHROEDER_COMBS_MS, &SCHROEDER_ALLPASSES, 0.0)
    }

    /// Moorer's six lowpass combs and one allpass, T60 1.5 s.
    pub fn moorer() -> Self {
        Self::build(&MOORER_COMBS_MS, &MOORER_ALLPASSES, MOORER_DAMPING)
    }

    /// The time for the tail to fall 60 dB (at low frequencies), in
    /// seconds; `f32::INFINITY` freezes it.
    pub fn set_t60(&mut self, t60: Seconds) {
        let t60_s = t60.get() as f32;
        debug_assert!(t60_s > 0.0, "T60 {t60_s} s");
        self.t60_s = t60_s;
        self.update();
    }

    /// The combs' in-loop lowpass, 0 (bright) … 1 (dark).
    pub fn set_damping(&mut self, damping: f32) {
        for c in &mut self.combs[..self.comb_count] {
            c.set_damping(damping);
        }
    }

    fn update(&mut self) {
        let (t60, rate) = (self.t60_s, self.sample_rate);
        for c in &mut self.combs[..self.comb_count] {
            c.set_feedback(loop_gain(c.delay_samples() as f32, t60, rate));
        }
    }
}

impl Stage for Schroeder {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for c in &mut self.combs[..self.comb_count] {
            c.prepare(sample_rate);
        }
        for a in &mut self.allpasses[..self.allpass_count] {
            a.prepare(sample_rate);
        }
        self.update();
    }

    fn reset(&mut self) {
        for c in &mut self.combs[..self.comb_count] {
            c.reset();
        }
        for a in &mut self.allpasses[..self.allpass_count] {
            a.reset();
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut sum = 0.0;
        for c in &mut self.combs[..self.comb_count] {
            sum += c.tick(x);
        }
        let mut y = sum / self.comb_count as f32;
        for a in &mut self.allpasses[..self.allpass_count] {
            y = a.tick(y);
        }
        y
    }

    fn tail(&self) -> Tail {
        let longest = self.combs[..self.comb_count]
            .iter()
            .map(|c| c.delay_samples())
            .max()
            .unwrap_or(0);
        let allpasses = self.allpasses[..self.allpass_count]
            .iter()
            .map(|a| a.tail())
            .fold(Tail::None, Tail::then);
        decay_tail(self.t60_s, self.sample_rate, longest as f32).then(allpasses)
    }
}
