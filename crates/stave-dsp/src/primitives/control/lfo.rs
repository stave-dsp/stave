//! Phase-accumulator LFO with the SHAPE BASIS (sine, triangle,
//! saw, square) and optional analog drift. Character beyond the
//! basis is COMPOSITION, not variants: run a shape through
//! [`Lag`](crate::primitives::control::lag::Lag) — a rounded triangle is
//! the Juno chorus, a fast-rise/slow-fall sine is the Univibe lamp.
//! `prepare` sets the sample rate, `set_rate` the cycle frequency,
//! `tick` advances and returns the next sample in [−1, 1].

use crate::primitives::random::Xorshift32;

/// The mathematical waveform basis. Everything vintage is a recipe
/// over these plus shaping stages — never a fifth variant here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
}

#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    sr: f32,
    phase: f32,
    inc: f32,
    shape: LfoShape,
    /// Analog clock imperfection: 0 = crystal, 1 = ±5% slow random
    /// walk on the rate. Updated once per cycle so the walk is
    /// rate-independent.
    drift: f32,
    drift_state: f32,
    rng: Xorshift32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Lfo {
    pub const fn new() -> Self {
        Self {
            sr: 48_000.0,
            phase: 0.0,
            inc: 0.0,
            shape: LfoShape::Sine,
            drift: 0.0,
            drift_state: 0.0,
            rng: Xorshift32::new(0x9E37_79B9),
        }
    }

    pub fn prepare(&mut self, sample_rate: f32, hz: f32, initial_phase_radians: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sr = sample_rate;
        self.phase = initial_phase_radians;
        self.drift_state = 0.0;
        self.set_rate(hz);
    }

    pub fn set_rate(&mut self, hz: f32) {
        let two_pi = core::f32::consts::TAU;
        self.inc = two_pi * hz.max(0.0) / self.sr;
    }

    pub fn set_shape(&mut self, shape: LfoShape) {
        self.shape = shape;
    }

    /// Analog clock imperfection, 0..1 (1 ≈ ±5% rate walk).
    pub fn set_drift(&mut self, amount: f32) {
        debug_assert!((0.0..=1.0).contains(&amount), "drift {amount}");
        self.drift = amount;
    }

    pub fn set_phase(&mut self, radians: f32) {
        self.phase = radians;
    }

    /// Current phase in [0, τ) — the display cursor reads this.
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// The shape evaluated at an arbitrary phase — the display draws
    /// one cycle by sweeping this; `tick` uses it live. Pure.
    #[inline]
    pub fn eval(shape: LfoShape, phase: f32) -> f32 {
        let two_pi = core::f32::consts::TAU;
        let t = (phase / two_pi).rem_euclid(1.0); // 0..1 cycle position
        match shape {
            LfoShape::Sine => (two_pi * t).sin(),
            // −1 at t=0, +1 at t=0.5, back to −1. Tri/saw/square
            // start at their minimum; sine at its zero-crossing.
            LfoShape::Triangle => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            LfoShape::Saw => 2.0 * t - 1.0,
            LfoShape::Square => {
                if t < 0.5 {
                    -1.0
                } else {
                    1.0
                }
            }
        }
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        // Sine keeps the raw-phase expression: existing consumers
        // (forge's ripple driver) are bit-pinned on it, and the
        // wrapped phase makes it equal to eval() mathematically.
        let y = match self.shape {
            LfoShape::Sine => self.phase.sin(),
            other => Self::eval(other, self.phase),
        };
        let inc = self.inc * (1.0 + self.drift_state);
        self.phase += inc;
        let two_pi = core::f32::consts::TAU;
        if self.phase >= two_pi {
            self.phase -= two_pi;
            // New cycle: step the drift walk (bounded, zero-mean).
            if self.drift > 0.0 {
                let x = self.rng.next_u32();
                let step = (x as f32 / u32::MAX as f32 - 0.5) * 0.04;
                self.drift_state = (self.drift_state + step * self.drift)
                    .clamp(-0.05 * self.drift, 0.05 * self.drift);
            }
        }
        y
    }
}

#[cfg(test)]
mod properties {
    extern crate alloc;

    use super::*;

    #[test]
    fn shapes_hit_their_marks() {
        let tau = core::f32::consts::TAU;
        // Triangle: min at 0, max at half-cycle, linear between.
        assert_eq!(Lfo::eval(LfoShape::Triangle, 0.0), -1.0);
        assert_eq!(Lfo::eval(LfoShape::Triangle, tau * 0.5), 1.0);
        assert!((Lfo::eval(LfoShape::Triangle, tau * 0.25)).abs() < 1e-6);
        // Saw: −1 → +1 over the cycle.
        assert_eq!(Lfo::eval(LfoShape::Saw, 0.0), -1.0);
        assert!((Lfo::eval(LfoShape::Saw, tau * 0.75) - 0.5).abs() < 1e-6);
        // Square: low half then high half.
        assert_eq!(Lfo::eval(LfoShape::Square, tau * 0.25), -1.0);
        assert_eq!(Lfo::eval(LfoShape::Square, tau * 0.75), 1.0);
        // Sine: zero at 0, +1 at quarter.
        assert!(Lfo::eval(LfoShape::Sine, 0.0).abs() < 1e-6);
        assert!((Lfo::eval(LfoShape::Sine, tau * 0.25) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn drift_is_bounded_and_zero_when_off() {
        let sr = 48_000.0;
        // Off: cycle length is exact.
        let mut clean = Lfo::new();
        clean.prepare(sr, 2.0, 0.0);
        let mut intervals = alloc::vec::Vec::new();
        let mut last_cross = None;
        let mut last = clean.tick();
        for i in 0..(sr as usize * 5) {
            let y = clean.tick();
            if last < 0.0 && y >= 0.0 {
                if let Some(prev) = last_cross {
                    intervals.push(i - prev);
                }
                last_cross = Some(i);
            }
            last = y;
        }
        // A crystal: cycle lengths cluster tightly at sr/2 Hz. (f32
        // phase accumulation quantizes crossings by a couple of
        // samples — that is not drift.)
        let expect = (sr / 2.0) as usize;
        assert!(intervals.len() >= 8);
        let min = *intervals.iter().min().unwrap() as i64;
        let max = *intervals.iter().max().unwrap() as i64;
        assert!(max - min <= 4, "clean spread {min}..{max}");
        assert!(
            (min - expect as i64).abs() <= 4,
            "clean rate near {expect}: {min}"
        );
        // On: rate walks, but never past ±5%.
        let mut drifty = Lfo::new();
        drifty.prepare(sr, 2.0, 0.0);
        drifty.set_drift(1.0);
        let mut crossings = 0;
        let mut last = drifty.tick();
        for _ in 0..(sr as usize * 30) {
            let y = drifty.tick();
            if last < 0.0 && y >= 0.0 {
                crossings += 1;
            }
            last = y;
        }
        let hz = crossings as f32 / 30.0;
        assert!(
            (hz - 2.0).abs() / 2.0 < 0.05,
            "mean rate within the walk bound: {hz}"
        );
    }
}
