//! Phase-accumulator LFO over the shared [`Waveform`] basis, with
//! optional analog drift. Character beyond the basis is composition,
//! not variants: run a shape through [`Lag`](crate::Lag) — a rounded
//! triangle is the classic ensemble chorus, a fast-rise/slow-fall sine
//! is a photocell lamp's throb.
//!
//! A generator [`Stage`]: `set_hz` sets the cycle frequency, `tick`
//! (input ignored) returns the next sample in [−1, 1]. Phase is in
//! cycles, `0 ≤ t < 1`, as everywhere in stave-dsp.

use crate::kernel::node::{Stage, Tail};
use crate::primitives::osc::Waveform;
use crate::primitives::random::Xorshift32;

#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    sample_rate: f32,
    hz: f32,
    /// Cycles, `0 ≤ phase < 1`.
    phase: f32,
    inc: f32,
    waveform: Waveform,
    drift: f32,
    drift_state: f32,
    rng: Xorshift32,
}

impl Default for Lfo {
    /// 1 Hz sine.
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl Lfo {
    pub fn new(hz: f32) -> Self {
        let mut lfo = Self {
            sample_rate: 48_000.0,
            hz,
            phase: 0.0,
            inc: 0.0,
            waveform: Waveform::Sine,
            drift: 0.0,
            drift_state: 0.0,
            rng: Xorshift32::new(0x9E37_79B9),
        };
        lfo.set_hz(hz);
        lfo
    }

    pub fn set_hz(&mut self, hz: f32) {
        self.hz = hz;
        self.inc = hz.max(0.0) / self.sample_rate;
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.waveform = waveform;
    }

    /// Cycle-to-cycle rate wander, 0 (off) … 1 (±5 %).
    pub fn set_drift(&mut self, amount: f32) {
        debug_assert!((0.0..=1.0).contains(&amount), "drift {amount}");
        self.drift = amount;
    }

    /// Phase in cycles.
    pub fn set_phase(&mut self, cycles: f32) {
        self.phase = cycles.rem_euclid(1.0);
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }
}

impl Stage for Lfo {
    /// Re-derive the increment; restart at phase 0 with no drift.
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_hz(self.hz);
        self.reset();
    }
    fn reset(&mut self) {
        self.phase = 0.0;
        self.drift_state = 0.0;
    }
    #[inline]
    fn tick(&mut self, _: f32) -> f32 {
        let y = self.waveform.eval(self.phase);
        self.phase += self.inc * (1.0 + self.drift_state);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            if self.drift > 0.0 {
                let x = self.rng.next_u32();
                let step = (x as f32 / u32::MAX as f32 - 0.5) * 0.04;
                self.drift_state = (self.drift_state + step * self.drift)
                    .clamp(-0.05 * self.drift, 0.05 * self.drift);
            }
        }
        y
    }
    /// A generator: it never stops.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    #[test]
    fn shapes_hit_their_marks() {
        assert_eq!(Waveform::Triangle.eval(0.0), -1.0);
        assert_eq!(Waveform::Triangle.eval(0.5), 1.0);
        assert!(Waveform::Triangle.eval(0.25).abs() < 1e-6);
        assert_eq!(Waveform::Saw.eval(0.0), -1.0);
        assert!((Waveform::Saw.eval(0.75) - 0.5).abs() < 1e-6);
        // In phase with the sine: positive for the first half cycle.
        assert_eq!(Waveform::Square.eval(0.25), 1.0);
        assert_eq!(Waveform::Square.eval(0.75), -1.0);
        assert!(Waveform::Sine.eval(0.0).abs() < 1e-6);
        assert!((Waveform::Sine.eval(0.25) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn drift_is_bounded_and_zero_when_off() {
        let sr = 48_000.0;
        let mut clean = Lfo::new(2.0);
        clean.prepare(sr);
        let mut intervals = Vec::new();
        let mut last_cross = None;
        let mut last = clean.tick(0.0);
        for i in 0..(sr as usize * 5) {
            let y = clean.tick(0.0);
            if last < 0.0 && y >= 0.0 {
                if let Some(prev) = last_cross {
                    intervals.push(i - prev);
                }
                last_cross = Some(i);
            }
            last = y;
        }
        let expect = (sr / 2.0) as usize;
        assert!(intervals.len() >= 8);
        let min = *intervals.iter().min().unwrap() as i64;
        let max = *intervals.iter().max().unwrap() as i64;
        assert!(max - min <= 4, "clean spread {min}..{max}");
        assert!(
            (min - expect as i64).abs() <= 4,
            "clean rate near {expect}: {min}"
        );
        let mut drifty = Lfo::new(2.0);
        drifty.prepare(sr);
        drifty.set_drift(1.0);
        let mut crossings = 0;
        let mut last = drifty.tick(0.0);
        for _ in 0..(sr as usize * 30) {
            let y = drifty.tick(0.0);
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
