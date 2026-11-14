//! **Stage** — the one processing surface. Leaves and composites
//! present the same five verbs, so anything that processes audio nests
//! inside anything that composes it.

use crate::units::Seconds;

/// How long a stage's output keeps ringing after its input stops.
///
/// The same three cases a plugin reports to its host: output stops with
/// the input, rings for a known number of samples, or sustains itself.
/// Filters estimate the samples to fall 120 dB ([`Tail::from_pole`]);
/// composites combine their parts' tails ([`then`](Tail::then) for
/// serial, [`max`](Tail::max) for parallel).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Tail {
    /// Output stops with input (gains, memoryless curves).
    #[default]
    None,
    /// Output decays below −120 dB within this many samples.
    Samples(u32),
    /// Output may never stop (generators, feedback without a bound).
    Infinite,
}

/// The decay a [`Tail`] estimate measures to: 120 dB, as an amplitude
/// ratio.
pub const TAIL_FLOOR: f64 = 1e-6;

impl Tail {
    /// A tail of `n` samples (`0` is [`Tail::None`]).
    pub const fn samples(n: u32) -> Self {
        if n == 0 {
            Tail::None
        } else {
            Tail::Samples(n)
        }
    }

    /// The decay of a recursion whose slowest pole has radius
    /// `radius`: the samples until `radius^n` falls below
    /// [`TAIL_FLOOR`]. A radius of 1 or more never decays.
    pub fn from_pole(radius: f64) -> Self {
        let r = radius.abs();
        if !r.is_finite() || r >= 1.0 {
            return Tail::Infinite;
        }
        if r <= TAIL_FLOOR {
            return Tail::samples(1);
        }
        Self::from_count((TAIL_FLOOR.ln() / r.ln()).ceil())
    }

    /// The decay of a first-order system with time constant `seconds`
    /// at `sample_rate`: `seconds · sample_rate · ln(1/TAIL_FLOOR)`.
    pub fn from_time_constant(tau: Seconds, sample_rate: f32) -> Self {
        let seconds = tau.get();
        if seconds.is_nan() || seconds <= 0.0 {
            return Tail::None;
        }
        Self::from_count((seconds * f64::from(sample_rate) * -TAIL_FLOOR.ln()).ceil())
    }

    /// The decay of a second-order section `1 + a1·z⁻¹ + a2·z⁻²`:
    /// [`from_pole`](Self::from_pole) of its larger pole, at least the two
    /// samples its state holds.
    pub fn from_biquad(a1: f64, a2: f64) -> Self {
        let disc = a1 * a1 - 4.0 * a2;
        let radius = if disc < 0.0 {
            a2.abs().sqrt()
        } else {
            let s = disc.sqrt();
            ((-a1 + s) * 0.5).abs().max(((-a1 - s) * 0.5).abs())
        };
        Self::from_pole(radius).max(Tail::Samples(2))
    }

    fn from_count(n: f64) -> Self {
        if n >= f64::from(u32::MAX) {
            Tail::Infinite
        } else {
            Tail::samples(n as u32)
        }
    }

    /// Samples until silence: `Some(0)` for none, `None` for infinite.
    pub const fn to_samples(self) -> Option<u32> {
        match self {
            Tail::None => Some(0),
            Tail::Samples(n) => Some(n),
            Tail::Infinite => None,
        }
    }

    /// Serial composition: `next` rings on after `self` stops.
    pub fn then(self, next: Tail) -> Tail {
        match (self.to_samples(), next.to_samples()) {
            (Some(a), Some(b)) => a.checked_add(b).map_or(Tail::Infinite, Tail::samples),
            _ => Tail::Infinite,
        }
    }

    /// Parallel composition: the longer of the two.
    pub fn max(self, other: Tail) -> Tail {
        match (self.to_samples(), other.to_samples()) {
            (Some(a), Some(b)) => Tail::samples(a.max(b)),
            _ => Tail::Infinite,
        }
    }

    /// `n` more samples of ringing (a delay before or after the tail).
    pub fn plus(self, n: u32) -> Tail {
        self.then(Tail::samples(n))
    }

    /// The same tail measured at a rate `factor`× slower, rounding up
    /// (a bound may be conservative, never short).
    pub fn div_ceil(self, factor: u32) -> Tail {
        match self {
            Tail::Samples(n) => Tail::samples(n.div_ceil(factor.max(1))),
            other => other,
        }
    }
}

/// A DSP processor: prepare / reset / tick / process / latency / tail.
///
/// * `prepare` is told the rate `tick` will run at (inside an N×
///   oversampler that is N× the host rate) and caches every
///   rate-derived coefficient; setters take musical units only.
///   It may allocate; everything else is realtime.
/// * `tick` advances one sample. A generator (oscillator, LFO,
///   envelope) ignores its input; it is a [`Generator`] too, read with
///   `next()`.
/// * `process` is the block form; overrides stay bit-identical to the
///   tick loop.
/// * `latency` is group delay in samples at the prepared rate; `tail`
///   is how long output rings after input stops. Composites forward
///   both: serial sums, parallel takes the max, a 2× region divides.
pub trait Stage {
    fn prepare(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    fn tick(&mut self, x: f32) -> f32;
    fn process(&mut self, block: &mut [f32]) {
        for s in block {
            *s = self.tick(*s);
        }
    }
    fn latency(&self) -> u32 {
        0
    }
    fn tail(&self) -> Tail {
        Tail::None
    }
}

/// A [`Stage`] whose output ignores its input: an oscillator, LFO,
/// envelope or random walk. [`next`](Self::next) is its next sample,
/// so a generator is never ticked with a dummy input.
///
/// ```
/// use stave_dsp::{Adsr, AdsrConfig, Generator, Stage};
///
/// let mut env = Adsr::new();
/// env.prepare(48_000.0);
/// env.set(&AdsrConfig { attack_ms: 1.0, decay_ms: 50.0, sustain: 0.5, release_ms: 100.0 });
/// env.gate_on();
/// let level = env.next();
/// assert!(level > 0.0);
/// ```
pub trait Generator: Stage {
    /// The next sample (one [`tick`](Stage::tick); the generator's
    /// input is not read).
    #[inline]
    fn next(&mut self) -> f32 {
        self.tick(0.0)
    }
}

/// The wire: output is input, no state, no delay. The empty interior
/// of a closure-driven region ([`Oversampled2x::tick_with`]).
///
/// [`Oversampled2x::tick_with`]: crate::Oversampled2x::tick_with
impl Stage for () {
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        x
    }
}

impl<S: Stage + ?Sized> Stage for Box<S> {
    fn prepare(&mut self, sample_rate: f32) {
        (**self).prepare(sample_rate);
    }
    fn reset(&mut self) {
        (**self).reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        (**self).tick(x)
    }
    fn process(&mut self, block: &mut [f32]) {
        (**self).process(block);
    }
    fn latency(&self) -> u32 {
        (**self).latency()
    }
    fn tail(&self) -> Tail {
        (**self).tail()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::seconds;

    #[test]
    fn tails_compose_by_the_invariants() {
        let (a, b) = (Tail::Samples(10), Tail::Samples(4));
        assert_eq!(a.then(b), Tail::Samples(14));
        assert_eq!(a.then(Tail::None), a);
        assert_eq!(Tail::None.then(Tail::None), Tail::None);
        assert_eq!(a.then(Tail::Infinite), Tail::Infinite);
        assert_eq!(a.max(b), a);
        assert_eq!(a.max(Tail::Infinite), Tail::Infinite);
        assert_eq!(Tail::Samples(9).div_ceil(2), Tail::Samples(5));
        assert_eq!(Tail::Samples(u32::MAX).plus(1), Tail::Infinite);
        assert_eq!(Tail::samples(0), Tail::None);
    }

    #[test]
    fn pole_tails_fall_120_db() {
        // 0.5^n < 1e-6 first at n = 20.
        assert_eq!(Tail::from_pole(0.5), Tail::Samples(20));
        assert_eq!(Tail::from_pole(-0.5), Tail::Samples(20));
        assert_eq!(Tail::from_pole(1.0), Tail::Infinite);
        assert_eq!(Tail::from_pole(0.0), Tail::Samples(1));
        // One second of time constant at 48 kHz: 13.8 τ.
        let Tail::Samples(n) = Tail::from_time_constant(seconds(1.0), 48_000.0) else {
            panic!()
        };
        assert!((n as f64 - 48_000.0 * 13.8155).abs() < 2.0, "{n}");
        // A resonant pair: radius √a2.
        assert_eq!(Tail::from_biquad(0.0, 0.25), Tail::from_pole(0.5));
        assert_eq!(Tail::from_biquad(0.0, 0.0), Tail::Samples(2));
    }
}
