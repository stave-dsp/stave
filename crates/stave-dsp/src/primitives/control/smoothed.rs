//! Block-rate parameter smoother. The writer sets a target at any time;
//! the audio thread pulls toward it smoothly.
//!
//! Two flavours in one type:
//! - `set_target(v)` + `tick` per sample = one-pole exponential glide.
//! - `set_target(v)` + `next_block(n)` = a linear step per block, to read
//!   once per block and apply N times.

use crate::kernel::control::Source;
use crate::kernel::node::{Generator, Stage};
use crate::primitives::filters::one_pole::tau_coeff;
use crate::units::Seconds;
use crate::units::ms;

/// Default time constant. ~5 ms = no zipper at GUI rate, no audible lag.
pub const DEFAULT_TIME_MS: f32 = 5.0;

/// A parameter ramp: a control [`Source`] driven by its writer, never by
/// program material — `tick` ignores its input.
///
/// `prepare` retunes the coefficient for the new rate, keeping the time
/// constant; `reset` completes the glide — a parameter's rest state is
/// its target (zeroing would fabricate a value the writer never set).
#[derive(Clone, Copy, Debug)]
pub struct SmoothedParam {
    target: f32,
    current: f32,
    /// α for the one-pole: cur += α · (target − cur).
    alpha: f32,
    sample_rate: f32,
    time_ms: f32,
}

impl SmoothedParam {
    pub fn new(initial: f32) -> Self {
        let mut s = Self {
            target: initial,
            current: initial,
            alpha: 0.0,
            sample_rate: 48_000.0,
            time_ms: DEFAULT_TIME_MS,
        };
        s.set_time(ms(f64::from(DEFAULT_TIME_MS)));
        s
    }

    /// The glide's time constant (state kept).
    pub fn set_time(&mut self, time: Seconds) {
        let ms = time.to_ms() as f32;
        debug_assert!(ms > 0.0, "time constant must be positive");
        self.time_ms = ms;
        self.alpha = tau_coeff(Seconds::new(f64::from(ms * 0.001)), self.sample_rate);
    }

    pub fn set_target(&mut self, v: f32) {
        self.target = v;
    }

    pub fn snap_to(&mut self, v: f32) {
        self.target = v;
        self.current = v;
    }

    /// Block-rate linear step: advance toward the target by one block's
    /// worth of the time constant and return the new value, to be held
    /// for the block. Reaches the target in ~one time constant
    /// regardless of block size; never overshoots.
    pub fn next_block(&mut self, n: usize) -> f32 {
        let steps_total = (self.sample_rate * self.time_ms * 0.001).max(1.0);
        let frac = (n as f32 / steps_total).min(1.0);
        self.current += (self.target - self.current) * frac;
        if (self.current - self.target).abs() < 1e-9 {
            self.current = self.target;
        }
        self.current
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }
}

impl Stage for SmoothedParam {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_time(ms(f64::from(self.time_ms)));
    }
    fn reset(&mut self) {
        self.current = self.target;
    }
    /// One sample of glide; the input is ignored.
    #[inline]
    fn tick(&mut self, _: f32) -> f32 {
        self.current += self.alpha * (self.target - self.current);
        self.current
    }
}

impl Generator for SmoothedParam {}

impl Source for SmoothedParam {
    fn state(&self) -> f32 {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glide_and_block_step_both_arrive_in_about_one_time_constant() {
        let mut s = SmoothedParam::new(0.0);
        s.prepare(48_000.0);
        s.set_target(1.0);
        for _ in 0..240 {
            s.tick(0.0);
        }
        assert!((s.current() - (1.0 - (-1.0f32).exp())).abs() < 1e-3);
        let mut b = SmoothedParam::new(0.0);
        b.prepare(48_000.0);
        b.set_target(1.0);
        assert!(b.next_block(120) < 1.0);
        assert_eq!(b.next_block(480), 1.0, "a time constant of blocks arrives");
        b.set_target(0.0);
        b.reset();
        assert_eq!(b.state(), 0.0, "reset completes the glide");
    }
}
