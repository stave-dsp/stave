//! Envelope follower: a one-pole that rises at the attack rate and falls at
//! the release rate. It follows its input as given — rectify first for a
//! peak envelope, pass a gain trajectory to smooth one.
//!
//! An optional slow release runs a second state in parallel (same attack,
//! longer release) and the follower reports the larger of the two: the
//! two-stage release of an opto cell (LA-2A T4) — a fast initial drop,
//! then a long tail.

use crate::kernel::control::Source;
use crate::kernel::denormal::flush;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::one_pole::tau_coeff;
use crate::units::{ms, Seconds};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EnvelopeTimes {
    pub attack_ms: f32,
    pub release_ms: f32,
    pub slow_release_ms: Option<f32>,
}

#[derive(Copy, Clone, Debug)]
pub struct EnvelopeFollower {
    times: EnvelopeTimes,
    sample_rate: f32,
    attack: f32,
    release: f32,
    slow_release: f32,
    state: f32,
    slow: f32,
}

impl EnvelopeFollower {
    /// A follower with `times`, runnable at 48 kHz before `prepare`.
    pub fn new(times: EnvelopeTimes) -> Self {
        let mut f = Self {
            times,
            sample_rate: 48_000.0,
            attack: 1.0,
            release: 1.0,
            slow_release: 1.0,
            state: 0.0,
            slow: 0.0,
        };
        f.set_times(times);
        f
    }

    pub fn times(&self) -> EnvelopeTimes {
        self.times
    }

    /// State kept.
    pub fn set_times(&mut self, times: EnvelopeTimes) {
        self.set_attack(ms(f64::from(times.attack_ms)));
        self.set_release(ms(f64::from(times.release_ms)));
        self.set_slow_release(times.slow_release_ms.map(|t| ms(f64::from(t))));
    }

    pub fn set_attack(&mut self, attack: Seconds) {
        let ms = attack.to_ms() as f32;
        self.times.attack_ms = ms;
        self.attack = self.coefficient(ms);
    }

    pub fn set_release(&mut self, release: Seconds) {
        let ms = release.to_ms() as f32;
        self.times.release_ms = ms;
        self.release = self.coefficient(ms);
    }

    /// `None` switches the slow stage off.
    pub fn set_slow_release(&mut self, slow_release: Option<Seconds>) {
        let ms = slow_release.map(|t| t.to_ms() as f32);
        self.times.slow_release_ms = ms;
        match ms {
            Some(ms) => self.slow_release = self.coefficient(ms),
            None => self.slow = 0.0,
        }
    }

    /// The one-pole step toward the input per sample for time constant
    /// `ms`. Zero follows instantly.
    fn coefficient(&self, ms: f32) -> f32 {
        debug_assert!(ms >= 0.0, "time constant {ms} ms");
        tau_coeff(Seconds::new(f64::from(ms * 0.001)), self.sample_rate)
    }
}

impl Stage for EnvelopeFollower {
    /// Rate-dependent coefficients from the stored times; state cleared.
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_times(self.times);
        self.reset();
    }

    fn reset(&mut self) {
        self.state = 0.0;
        self.slow = 0.0;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
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

    /// The slowest release, to −120 dB.
    fn tail(&self) -> Tail {
        let release = Tail::from_pole(f64::from(1.0 - self.release));
        match self.times.slow_release_ms {
            Some(_) => release.max(Tail::from_pole(f64::from(1.0 - self.slow_release))),
            None => release,
        }
    }
}

impl Source for EnvelopeFollower {
    fn state(&self) -> f32 {
        match self.times.slow_release_ms {
            Some(_) => self.state.max(self.slow),
            None => self.state,
        }
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
