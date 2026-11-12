//! Envelope follower: a one-pole that rises at the attack rate and falls at
//! the release rate. It follows its input as given — rectify first for a
//! peak envelope, pass a gain trajectory to smooth one.
//!
//! An optional slow release runs a second state in parallel (same attack,
//! longer release) and the follower reports the larger of the two: the
//! two-stage release of an opto cell (LA-2A T4) — a fast initial drop,
//! then a long tail.

use crate::primitives::denormal::flush;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EnvelopeTimes {
    pub attack_ms: f32,
    pub release_ms: f32,
    pub slow_release_ms: Option<f32>,
}

#[derive(Copy, Clone, Debug)]
pub struct EnvelopeFollower {
    times: EnvelopeTimes,
    rate: f32,
    attack: f32,
    release: f32,
    slow_release: f32,
    state: f32,
    slow: f32,
}

impl EnvelopeFollower {
    pub const fn new(times: EnvelopeTimes) -> Self {
        Self {
            times,
            rate: 0.0,
            attack: 1.0,
            release: 1.0,
            slow_release: 1.0,
            state: 0.0,
            slow: 0.0,
        }
    }

    /// Rate-dependent coefficients from the stored times; state cleared.
    pub fn prepare(&mut self, rate: f32) {
        self.retime(rate);
        self.reset();
    }

    /// New rate, state kept: a time-constant change mid-signal must not
    /// click.
    pub fn retime(&mut self, rate: f32) {
        debug_assert!(rate > 0.0, "sample rate must be positive");
        self.rate = rate;
        self.set_times(self.times);
    }

    pub fn times(&self) -> EnvelopeTimes {
        self.times
    }

    /// State kept.
    pub fn set_times(&mut self, times: EnvelopeTimes) {
        self.set_attack_ms(times.attack_ms);
        self.set_release_ms(times.release_ms);
        self.set_slow_release_ms(times.slow_release_ms);
    }

    pub fn set_attack_ms(&mut self, ms: f32) {
        self.times.attack_ms = ms;
        self.attack = self.coefficient(ms);
    }

    pub fn set_release_ms(&mut self, ms: f32) {
        self.times.release_ms = ms;
        self.release = self.coefficient(ms);
    }

    /// `None` switches the slow stage off.
    pub fn set_slow_release_ms(&mut self, ms: Option<f32>) {
        self.times.slow_release_ms = ms;
        match ms {
            Some(ms) => self.slow_release = self.coefficient(ms),
            None => self.slow = 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.state = 0.0;
        self.slow = 0.0;
    }

    pub fn state(&self) -> f32 {
        match self.times.slow_release_ms {
            Some(_) => self.state.max(self.slow),
            None => self.state,
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let k = if x > self.state {
            self.attack
        } else {
            self.release
        };
        self.state = flush(self.state + (x - self.state) * k);
        if self.times.slow_release_ms.is_some() {
            let k = if x > self.slow {
                self.attack
            } else {
                self.slow_release
            };
            self.slow = flush(self.slow + (x - self.slow) * k);
        }
        self.state()
    }

    /// The one-pole step toward the input per sample for time constant
    /// `ms`: 1 − e^(−1/τ). τ = 0 follows instantly.
    fn coefficient(&self, ms: f32) -> f32 {
        debug_assert!(ms >= 0.0, "time constant {ms} ms");
        let tau_samples = ms * 0.001 * self.rate;
        1.0 - (-1.0 / tau_samples).exp()
    }
}

impl crate::kernel::control::Source for EnvelopeFollower {
    fn prepare(&mut self, rate: f32) {
        EnvelopeFollower::prepare(self, rate);
    }
    fn reset(&mut self) {
        EnvelopeFollower::reset(self);
    }
    fn tick(&mut self, detector: f32) -> f32 {
        EnvelopeFollower::tick(self, detector)
    }
    fn state(&self) -> f32 {
        EnvelopeFollower::state(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn follower(attack_ms: f32, release_ms: f32, slow_release_ms: Option<f32>) -> EnvelopeFollower {
        let mut f = EnvelopeFollower::new(EnvelopeTimes {
            attack_ms,
            release_ms,
            slow_release_ms,
        });
        f.prepare(48_000.0);
        f
    }

    #[test]
    fn one_time_constant_reaches_one_minus_one_over_e() {
        let mut f = follower(10.0, 10.0, None);
        let tau = 480;
        for _ in 0..tau {
            f.tick(1.0);
        }
        assert!(
            (f.state() - (1.0 - (-1.0f32).exp())).abs() < 1e-3,
            "{}",
            f.state()
        );
    }

    #[test]
    fn zero_attack_follows_instantly() {
        let mut f = follower(0.0, 50.0, None);
        assert_eq!(f.tick(0.7), 0.7);
    }

    #[test]
    fn the_slow_stage_holds_the_tail_the_fast_stage_drops() {
        let (mut single, mut dual) = (follower(1.0, 20.0, None), follower(1.0, 20.0, Some(400.0)));
        for _ in 0..4_800 {
            single.tick(1.0);
            dual.tick(1.0);
        }
        for _ in 0..4_800 {
            single.tick(0.0);
            dual.tick(0.0);
        }
        assert!(
            single.state() < 0.01,
            "e^-5 after five fast time constants: {}",
            single.state()
        );
        assert!(dual.state() > 0.7, "{}", dual.state());
    }
}
